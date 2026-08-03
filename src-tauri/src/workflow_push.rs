//! M5 workflow-push：注册表一键推送与 fork 贡献链路（DD §5）。
//!
//! 写侧契约（NFR-7，同步进注册表 README「写侧契约」一节）：包目录 `{path}/`
//! 且 path 恒 = slug；index.json 根 `{"version":1,"workflows":[...]}`（skill
//! 注册表为 `"skills"`）；条目 8 字段 slug/name/version/description/author/
//! tags/icon/path（author/icon 缺省省略、tags 缺省 []）；默认分支 main；
//! 写侧以 pretty JSON 整体重写 index.json（serde_json 未启 preserve_order，
//! 键序按字母序）。工作流条目取自 workflow.yaml 同名字段；skill 条目字段
//! 映射见 `skill_entry_from_dir`。
//!
//! 三态 wire 契约（复审 AC-02 钉死）：contribute_* 的 NoToken/NeedFork/Ready
//! 统一走 Ok 载荷（`{"status": ...}`，前端按 status 分支），Err 通道只给真
//! 错误。push rejected（non-fast-forward）不自动合并 → 固定语义「远端已更
//! 新，请重试」。
//!
//! git 调用纪律（R10）：全部经 git_ops 原语（本模块零 Command::new）。clone
//! 用 clone_repo_verbatim（workflow_registry 生产路径同款先例）：GitHub-only
//! 与 userinfo 把关在上游边界（settings 保存校验门-L4；fork_clone_url 构造
//! 恒 GitHub 形态），本地 fixture 仓库因此可直接驱动全链路真 git 零外网。

use crate::context::AppContext;
use crate::fs_ops::{copy_dir_recursive, ensure_dir, path_to_string, remove_entry};
use crate::models::Settings;
use crate::skill_ops::normalize_github_url;
use crate::workflow::{workflows_dir, Workflow, WORKFLOW_FILE};
use crate::{git_ops, github_auth, scanner, settings};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// 写侧契约：index 条目与 upsert
// ---------------------------------------------------------------------------

/// index.json 条目（8 字段契约，DD §5.1）。author/icon 为 None 时键省略、
/// tags 缺省空数组。序列化键序按字母序（serde_json BTreeMap，见模块头注释）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IndexEntry {
    pub slug: String,
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub path: String,
}

impl IndexEntry {
    /// 工作流条目：取自 workflow.yaml 同名字段；path 恒 = slug（DD §5.1）。
    fn from_workflow(workflow: &Workflow) -> IndexEntry {
        IndexEntry {
            slug: workflow.slug.clone(),
            name: workflow.name.clone(),
            version: workflow.version.clone(),
            description: workflow.description.clone(),
            author: workflow.author.clone(),
            tags: workflow.tags.clone(),
            icon: workflow.icon.clone(),
            path: workflow.slug.clone(),
        }
    }
}

/// skill 条目字段映射（DD §5.1 钉死）：slug=目录名（调用方给的规范 slug）、
/// name=frontmatter.name（缺省回退 slug）、description=frontmatter.description
/// （缺省空串）、version=metadata.version（缺省 "0.1.0"）、author/tags/icon=
/// metadata 对应键（tags 逗号分隔、缺省 []；author/icon 缺省省略）。
fn skill_entry_from_dir(dir: &Path, slug: &str) -> Result<IndexEntry, String> {
    let file = dir.join("SKILL.md");
    let text = fs::read_to_string(&file).map_err(|error| {
        format!(
            "Unable to read SKILL.md at {}: {error}",
            path_to_string(&file)
        )
    })?;
    let (frontmatter, _body) = scanner::parse_skill_markdown(&text);
    let frontmatter = frontmatter.ok_or_else(|| {
        format!(
            "SKILL.md at {} is missing valid frontmatter",
            path_to_string(&file)
        )
    })?;
    let metadata = &frontmatter.metadata;
    let meta_value = |key: &str| {
        metadata
            .get(key)
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
    };
    Ok(IndexEntry {
        slug: slug.to_string(),
        name: frontmatter
            .name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(slug)
            .to_string(),
        version: meta_value("version").unwrap_or("0.1.0").to_string(),
        description: frontmatter.description.unwrap_or_default(),
        author: meta_value("author").map(str::to_string),
        tags: meta_value("tags")
            .map(|raw| {
                raw.split(',')
                    .map(str::trim)
                    .filter(|item| !item.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        icon: meta_value("icon").map(str::to_string),
        path: slug.to_string(),
    })
}

/// 注册表 index 写侧（DD §5.1）：缺文件则建 `{"version":1}` 根、缺数组则建
/// 空数组；按 slug upsert（原位替换保序，未命中追加）；pretty JSON 整体重写。
/// 根非 object / array_key 对应值非数组 → Err（坏索引不擅自修复）。
pub fn upsert_index_entry(
    index_path: &Path,
    array_key: &str,
    entry: &IndexEntry,
) -> Result<(), String> {
    let mut root = if index_path.exists() {
        let text = fs::read_to_string(index_path).map_err(|error| {
            format!(
                "Unable to read registry index at {}: {error}",
                path_to_string(index_path)
            )
        })?;
        serde_json::from_str::<Value>(&text).map_err(|error| {
            format!(
                "Unable to parse registry index at {}: {error}",
                path_to_string(index_path)
            )
        })?
    } else {
        json!({ "version": 1 })
    };
    let object = root.as_object_mut().ok_or_else(|| {
        format!(
            "Registry index at {} is not a JSON object",
            path_to_string(index_path)
        )
    })?;
    if !object.contains_key("version") {
        object.insert("version".to_string(), json!(1));
    }
    if !object.contains_key(array_key) {
        object.insert(array_key.to_string(), json!([]));
    }
    let array = object
        .get_mut(array_key)
        .and_then(Value::as_array_mut)
        .ok_or_else(|| {
            format!(
                "Registry index key '{array_key}' at {} is not an array",
                path_to_string(index_path)
            )
        })?;

    let entry_value = serde_json::to_value(entry)
        .map_err(|error| format!("Unable to serialize index entry for '{}': {error}", entry.slug))?;
    match array
        .iter()
        .position(|item| item.get("slug").and_then(Value::as_str) == Some(entry.slug.as_str()))
    {
        Some(index) => array[index] = entry_value,
        None => array.push(entry_value),
    }

    if let Some(parent) = index_path.parent() {
        ensure_dir(parent)?;
    }
    let text = serde_json::to_string_pretty(&root)
        .map_err(|error| format!("Unable to serialize registry index: {error}"))?;
    fs::write(index_path, text).map_err(|error| {
        format!(
            "Unable to write registry index at {}: {error}",
            path_to_string(index_path)
        )
    })
}

// ---------------------------------------------------------------------------
// 返回体（wire 形态）
// ---------------------------------------------------------------------------

/// `push_workflow_to_registry` 返回体（DD §5.2：{commitHash, registryUrl}）。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PushResult {
    pub commit_hash: String,
    pub registry_url: String,
}

/// 贡献三态统一走 Ok 载荷（wire 形态钉死，复审 AC-02）：`{"status":"noToken"}`
/// / `{"status":"needFork","forkPageUrl":...}` / `{"status":"ready","compareUrl":
/// ...,"branch":...}`；前端按 status 字段分支，Err 通道只给真错误。
/// 注意 enum 容器上 rename_all 只改变体名；variant 内字段须 rename_all_fields。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "status")]
pub enum ContributeOutcome {
    NoToken,
    NeedFork { fork_page_url: String },
    Ready { compare_url: String, branch: String },
}

/// C7 访客上传复用（DD §8.3）：push 分支成功后的回执。gh 建 PR 属 C7 范围；
/// branch_url = 官方仓内分支 compare 页（base 恒 main），PR 创建失败时的降级出口。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UploadOutcome {
    pub branch: String,
    pub branch_url: String,
}

/// 注册表种类（workflow / skill）：官方地址、index 数组键、条目构建的分派点。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RegistryKind {
    Workflow,
    Skill,
}

impl RegistryKind {
    /// C7 contribute_upload 的 kind 参数（"workflow" | "skill"）解析。
    // C7 调用面（readonly-mode 卡接入后激活，本卡仅测试触达）。
    #[allow(dead_code)]
    pub(crate) fn parse(value: &str) -> Option<RegistryKind> {
        match value {
            "workflow" => Some(RegistryKind::Workflow),
            "skill" => Some(RegistryKind::Skill),
            _ => None,
        }
    }

    fn noun(self) -> &'static str {
        match self {
            RegistryKind::Workflow => "workflow",
            RegistryKind::Skill => "skill",
        }
    }

    fn array_key(self) -> &'static str {
        match self {
            RegistryKind::Workflow => "workflows",
            RegistryKind::Skill => "skills",
        }
    }

    // C7 调用面（contribute_to_official 经此取官方地址）。
    #[allow(dead_code)]
    fn official_url(self) -> &'static str {
        match self {
            RegistryKind::Workflow => settings::OFFICIAL_WORKFLOW_REGISTRY_URL,
            RegistryKind::Skill => settings::OFFICIAL_SKILL_REGISTRY_URL,
        }
    }

    /// 配置的注册表地址（空值回填官方缺省，与 load_settings 兜底同规则）。
    fn configured_url(self, app_settings: &Settings) -> String {
        let (value, official) = match self {
            RegistryKind::Workflow => (
                &app_settings.workflow_registry_url,
                settings::OFFICIAL_WORKFLOW_REGISTRY_URL,
            ),
            RegistryKind::Skill => (
                &app_settings.skill_registry_url,
                settings::OFFICIAL_SKILL_REGISTRY_URL,
            ),
        };
        value
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty())
            .unwrap_or(official)
            .to_string()
    }

    /// 中心库已安装内容 → (staged 目录, index 条目)。workflow 经 load 校验
    /// slug 与 yaml 一致性；skill 校验 [a-z0-9-]+ 后定位 `library_path/<slug>/`。
    fn staged_and_entry(
        self,
        ctx: &AppContext,
        app_settings: &Settings,
        slug: &str,
    ) -> Result<(PathBuf, IndexEntry), String> {
        match self {
            RegistryKind::Workflow => {
                let workflow = crate::workflow::load(ctx, slug)?;
                ensure_yaml_slug_matches(slug, &workflow.slug, "directory")?;
                Ok((
                    workflows_dir(ctx).join(slug),
                    IndexEntry::from_workflow(&workflow),
                ))
            }
            RegistryKind::Skill => {
                if !is_valid_slug(slug) {
                    return Err(format!(
                        "Invalid skill slug '{slug}': must be non-empty and match [a-z0-9-]+"
                    ));
                }
                let dir = PathBuf::from(&app_settings.library_path).join(slug);
                if !dir.is_dir() {
                    return Err(format!(
                        "Skill '{slug}' is not installed in the central library: {}",
                        path_to_string(&dir)
                    ));
                }
                let entry = skill_entry_from_dir(&dir, slug)?;
                Ok((dir, entry))
            }
        }
    }

    /// C7 staging 目录 → index 条目（内容校验 C7 已做，此处重建条目并复核
    /// workflow yaml 与 slug 一致性，防包目录与 index 条目分裂）。
    // C7 调用面（contribute_to_official 经此重建条目）。
    #[allow(dead_code)]
    fn entry_from_staged(self, staged_dir: &Path, slug: &str) -> Result<IndexEntry, String> {
        match self {
            RegistryKind::Workflow => {
                let file = staged_dir.join(WORKFLOW_FILE);
                let text = fs::read_to_string(&file).map_err(|error| {
                    format!(
                        "Unable to read workflow at {}: {error}",
                        path_to_string(&file)
                    )
                })?;
                let workflow = Workflow::from_yaml(&text)?;
                if let Err(errors) = workflow.validate() {
                    return Err(format!(
                        "Staged workflow failed validation: {}",
                        errors.join("; ")
                    ));
                }
                ensure_yaml_slug_matches(slug, &workflow.slug, "staged directory")?;
                Ok(IndexEntry::from_workflow(&workflow))
            }
            RegistryKind::Skill => skill_entry_from_dir(staged_dir, slug),
        }
    }
}

/// yaml 内 slug 与目录/参数 slug 不一致 → 拒绝（否则推送会把注册表写成包
/// 目录与 index 条目 slug 分裂的状态）。
fn ensure_yaml_slug_matches(expected: &str, actual: &str, location: &str) -> Result<(), String> {
    if expected == actual {
        return Ok(());
    }
    Err(format!(
        "Workflow slug mismatch: {location} '{expected}' contains yaml slug '{actual}'; \
         fix the install before publishing"
    ))
}

// ---------------------------------------------------------------------------
// 共享写侧管线：clone →（可选建分支）→ 拷包 → upsert → commit → push
// ---------------------------------------------------------------------------

/// 一次推送作业的全部输入。`source_url` 生产恒 GitHub 形态（上游边界把关），
/// 单测注入本地 bare fixture 路径（clone_repo_verbatim 先例，零外网全链路）。
struct PushJob<'a> {
    /// 临时目录标签（push / contrib / upload）。
    tag: &'a str,
    source_url: &'a str,
    /// Some → clone 后 `checkout -b` 该分支。
    branch: Option<&'a str>,
    /// 待发布包目录（内容逐字节拷贝到 `repo/<slug>/`）。
    staged_dir: &'a Path,
    entry: &'a IndexEntry,
    array_key: &'a str,
    commit_message: &'a str,
    /// "HEAD"（直推默认分支 main）或分支名。
    push_refspec: &'a str,
    identity_fallback: &'a str,
    token: Option<&'a str>,
}

/// 管线执行：临时根 `data_dir/tmp/{tag}-{slug}-{ms}/` 内 clone（R4 临时目录
/// 全在 data_dir），成功/失败两路都清理临时目录（C4 同款约定）。返回 commit hash。
fn stage_commit_push(ctx: &AppContext, job: &PushJob) -> Result<String, String> {
    let temp_root = ctx.data_dir().join("tmp").join(format!(
        "{}-{}-{}",
        job.tag,
        job.entry.slug,
        Utc::now().timestamp_millis()
    ));
    let result = (|| {
        let repo = temp_root.join("repo");
        git_ops::clone_repo_verbatim(job.source_url, &repo, job.token)?;
        if let Some(branch) = job.branch {
            git_ops::create_branch(&repo, branch)?;
        }
        let package = repo.join(&job.entry.slug);
        if package.exists() {
            remove_entry(&package)?;
        }
        copy_dir_recursive(job.staged_dir, &package)?;
        upsert_index_entry(&repo.join("index.json"), job.array_key, job.entry)?;
        let identity = git_ops::detect_identity(&repo, job.identity_fallback);
        let hash = git_ops::commit_all(&repo, job.commit_message, &identity)?;
        push_refspec(&repo, "origin", job.push_refspec, job.token)?;
        Ok(hash)
    })();
    if temp_root.exists() {
        let _ = remove_entry(&temp_root);
    }
    result
}

/// push 封装：non-fast-forward（远端已更新）→ 固定语义 Err「远端已更新，
/// 请重试」（不自动合并，DD §5.3）；其余错误原样上抛（git_ops::run 已脱敏）。
fn push_refspec(repo: &Path, remote: &str, refspec: &str, token: Option<&str>) -> Result<(), String> {
    git_ops::push(repo, remote, refspec, token).map_err(|error| {
        let lowered = error.to_lowercase();
        if lowered.contains("rejected")
            || lowered.contains("non-fast-forward")
            || lowered.contains("fetch first")
        {
            format!(
                "Push rejected: the remote registry has been updated, \
                 please retry（远端已更新，请重试）: {error}"
            )
        } else {
            error
        }
    })
}

/// fork 探测：GitHub 形态走 git_ops::ls_remote（normalize 把关）；其余形态
/// （生产不可达——fork_clone_url 构造恒 GitHub；fixture 测试 seam）经
/// base_command 逐字执行，防交互 env 与 stderr 脱敏原语一致（R10）。
fn probe_remote(url: &str, token: Option<&str>) -> Result<String, String> {
    if normalize_github_url(url).is_ok() {
        return git_ops::ls_remote(url, token);
    }
    let mut cmd = git_ops::base_command();
    git_ops::with_auth(&mut cmd, token);
    cmd.arg("ls-remote").arg(url);
    git_ops::run(&mut cmd, token)
}

// ---------------------------------------------------------------------------
// 一键推送（自有注册表）
// ---------------------------------------------------------------------------

/// 推送已安装工作流到**自有**注册表（DD §5.3）：官方地址 → Err 引导贡献；
/// clone → 拷包 → upsert index → commit → `push origin HEAD` → commit hash。
pub fn push_workflow_to_registry(ctx: &AppContext, slug: &str) -> Result<PushResult, String> {
    // load 内含 [a-z0-9-]+ slug 校验；坏 slug 在此即拒（先于注册表判定）。
    let workflow = crate::workflow::load(ctx, slug)?;
    ensure_yaml_slug_matches(slug, &workflow.slug, "directory")?;
    let app_settings = settings::load_settings(ctx)?;
    let registry_url = RegistryKind::Workflow.configured_url(&app_settings);
    if github_auth::is_official_repo(&registry_url, settings::OFFICIAL_WORKFLOW_REGISTRY_URL) {
        return Err(format!(
            "The official registry does not accept direct pushes; use contribute_workflow \
             to propose '{slug}' via fork + PR（官方注册表不接受直推，请走贡献流程）"
        ));
    }
    let token = github_auth::resolve_token(ctx);
    let username = app_settings
        .github_username
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let entry = IndexEntry::from_workflow(&workflow);
    let commit_message = format!("Update workflow {slug}");
    let commit_hash = stage_commit_push(
        ctx,
        &PushJob {
            tag: "push",
            source_url: &registry_url,
            branch: None,
            staged_dir: &workflows_dir(ctx).join(slug),
            entry: &entry,
            array_key: RegistryKind::Workflow.array_key(),
            commit_message: &commit_message,
            push_refspec: "HEAD",
            identity_fallback: username.as_deref().unwrap_or("oms"),
            token: token.as_deref(),
        },
    )?;
    Ok(PushResult {
        commit_hash,
        registry_url,
    })
}

// ---------------------------------------------------------------------------
// fork 贡献（contribute_workflow / contribute_skill）
// ---------------------------------------------------------------------------

pub fn contribute_workflow(ctx: &AppContext, slug: &str) -> Result<ContributeOutcome, String> {
    contribute(ctx, RegistryKind::Workflow, slug)
}

pub fn contribute_skill(ctx: &AppContext, slug: &str) -> Result<ContributeOutcome, String> {
    contribute(ctx, RegistryKind::Skill, slug)
}

/// 贡献主流程（DD §5.3）：零 token 降级 NoToken（门-F-02，Ok 载荷）→
/// githubUsername 缺 → Err 引导设置 → 本地内容校验 → fork 链路。
fn contribute(ctx: &AppContext, kind: RegistryKind, slug: &str) -> Result<ContributeOutcome, String> {
    let Some(token) = github_auth::resolve_token(ctx) else {
        return Ok(ContributeOutcome::NoToken);
    };
    let app_settings = settings::load_settings(ctx)?;
    let username = app_settings
        .github_username
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            "GitHub username is required to contribute; set it in Settings\
             （请先在设置中填写 GitHub 用户名）"
                .to_string()
        })?;
    let registry_url = kind.configured_url(&app_settings);
    let (owner, repo) = github_auth::parse_owner_repo(&registry_url)?;
    let (staged_dir, entry) = kind.staged_and_entry(ctx, &app_settings, slug)?;
    let fork_url = github_auth::fork_clone_url(username, &repo);
    contribute_via_fork(
        ctx,
        &ContributeJob {
            kind,
            slug,
            staged_dir: &staged_dir,
            entry: &entry,
            owner: &owner,
            repo: &repo,
            username,
            token: &token,
            fork_url: &fork_url,
        },
    )
}

/// fork 链路执行器的全部输入。`fork_url` 生产由 fork_clone_url 构造（恒
/// GitHub 形态）；单测注入本地 bare fixture 驱动全链路真 git 零外网。
struct ContributeJob<'a> {
    kind: RegistryKind,
    slug: &'a str,
    staged_dir: &'a Path,
    entry: &'a IndexEntry,
    owner: &'a str,
    repo: &'a str,
    username: &'a str,
    token: &'a str,
    fork_url: &'a str,
}

/// ls_remote 探测 fork（失败/空 → NeedFork）→ clone fork → 分支
/// `contrib/{slug}`（已存在则 `-{UTCts}`）→ 写入+upsert → commit → push →
/// compare_url（base 恒 main，title/body 预填）→ Ready。
fn contribute_via_fork(
    ctx: &AppContext,
    job: &ContributeJob,
) -> Result<ContributeOutcome, String> {
    let refs = match probe_remote(job.fork_url, Some(job.token)) {
        Ok(output) if !output.trim().is_empty() => output,
        _ => {
            return Ok(ContributeOutcome::NeedFork {
                fork_page_url: github_auth::fork_page_url(job.owner, job.repo),
            })
        }
    };
    let base_branch = format!("contrib/{}", job.slug);
    let branch = if refs
        .lines()
        .any(|line| line.ends_with(&format!("refs/heads/{base_branch}")))
    {
        format!("{base_branch}-{}", utc_stamp())
    } else {
        base_branch
    };
    let title = format!("Add {} {}", job.kind.noun(), job.slug);
    let body = contribute_body(job.slug);
    stage_commit_push(
        ctx,
        &PushJob {
            tag: "contrib",
            source_url: job.fork_url,
            branch: Some(&branch),
            staged_dir: job.staged_dir,
            entry: job.entry,
            array_key: job.kind.array_key(),
            commit_message: &title,
            push_refspec: &branch,
            identity_fallback: job.username,
            token: Some(job.token),
        },
    )?;
    Ok(ContributeOutcome::Ready {
        compare_url: github_auth::compare_url(
            job.owner,
            job.repo,
            job.username,
            &branch,
            &title,
            &body,
        ),
        branch,
    })
}

/// PR body 预填的 checklist 模板（compare_url body 参数，DD §5.3）。
fn contribute_body(slug: &str) -> String {
    format!(
        "## 贡献自测清单\n\n\
         - [ ] `{slug}` 包目录与 index 条目 slug 一致\n\
         - [ ] index 条目 8 字段完整（slug/name/version/description/author/tags/icon/path）\n\
         - [ ] 基于默认分支 main 的最新内容\n"
    )
}

// ---------------------------------------------------------------------------
// 官方仓上传分支（C7 访客上传复用，DD §8.3）
// ---------------------------------------------------------------------------

/// M7 复用入口：slug 先过 [a-z0-9-]+（进分支名）→ bot token（env 优先，未配
/// → Err「站点未开放贡献」）→ 条目重建与 slug 复核 → clone 官方本仓 →
/// `upload/{slug}-{UTCts}` 分支 → push。gh 建 PR 属 C7 范围，本函数只到
/// push 分支并返回分支名与分支 compare 页 URL。
// C7 contribute_upload 调用面（readonly-mode 卡接入后激活，本卡仅测试触达）。
#[allow(dead_code)]
pub(crate) fn contribute_to_official(
    ctx: &AppContext,
    kind: RegistryKind,
    staged_dir: &Path,
    slug: &str,
) -> Result<UploadOutcome, String> {
    if !is_valid_slug(slug) {
        return Err(format!(
            "Invalid {} slug '{slug}': must be non-empty and match [a-z0-9-]+",
            kind.noun()
        ));
    }
    let token = github_auth::resolve_token(ctx).ok_or_else(|| {
        "Contributions are not enabled on this site（站点未开放贡献）".to_string()
    })?;
    let entry = kind.entry_from_staged(staged_dir, slug)?;
    let official = kind.official_url();
    let (owner, repo) = github_auth::parse_owner_repo(official)?;
    upload_to_official(
        ctx,
        &UploadJob {
            kind,
            slug,
            staged_dir,
            entry: &entry,
            clone_url: official,
            owner: &owner,
            repo: &repo,
            token: &token,
        },
    )
}

/// 官方仓上传作业。`clone_url` 生产 = 官方常量；单测注入本地 bare fixture，
/// owner/repo 仍传官方真值以钉死 branch_url 形态。
// C7 调用面（contribute_to_official 的工作函数）。
#[allow(dead_code)]
struct UploadJob<'a> {
    kind: RegistryKind,
    slug: &'a str,
    staged_dir: &'a Path,
    entry: &'a IndexEntry,
    clone_url: &'a str,
    owner: &'a str,
    repo: &'a str,
    token: &'a str,
}

// C7 调用面（contribute_to_official 的工作函数）。
#[allow(dead_code)]
fn upload_to_official(ctx: &AppContext, job: &UploadJob) -> Result<UploadOutcome, String> {
    let branch = format!("upload/{}-{}", job.slug, utc_stamp());
    let title = format!("Add {} {}", job.kind.noun(), job.slug);
    stage_commit_push(
        ctx,
        &PushJob {
            tag: "upload",
            source_url: job.clone_url,
            branch: Some(&branch),
            staged_dir: job.staged_dir,
            entry: job.entry,
            array_key: job.kind.array_key(),
            commit_message: &title,
            push_refspec: &branch,
            identity_fallback: "oms-bot",
            token: Some(job.token),
        },
    )?;
    // 官方仓内分支的 compare 页（base 恒 main，契约门-F-10）；同仓分支 compare
    // 无 user 前缀，与 fork 形态（github_auth::compare_url）刻意区分。
    Ok(UploadOutcome {
        branch_url: format!(
            "https://github.com/{}/{}/compare/main...{}?expand=1",
            job.owner, job.repo, branch
        ),
        branch,
    })
}

// ---------------------------------------------------------------------------
// 共享小工具
// ---------------------------------------------------------------------------

/// 与 workflow_update::is_valid_slug 同规则（[a-z0-9-]+，模块内同规则拷贝，
/// DD §7 先例）。
fn is_valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// UTC 时间戳（分支冲突后缀 / upload 分支名，与 workflow_update 备份目录同形态）。
fn utc_stamp() -> String {
    Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

/// 测试共享 fixture（本模块单测与 web 端点 oneshot 复用）。造数据复刻真实
/// 流程：本地 bare 注册表仓库（main 分支，含一条 other-flow 既有条目）→
/// 真 clone → 写入/upsert → commit → push → 对端 git log 与 clone 回来逐字段
/// 校验。fixture 搭建复用 workflow_update::test_support 的 git/commit_fixture
/// （本模块测试同样零 Command::new）。
#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use crate::workflow_update::test_support::{commit_fixture, git};

    /// 本地 bare 注册表仓库（main 分支）：index.json 含一条 other-flow 既有
    /// 条目 + other-flow/ 包目录 + README。TempDir 须由调用方持有存活。
    pub(crate) fn bare_registry_repo() -> tempfile::TempDir {
        let temp = tempfile::tempdir().expect("temp dir");
        let bare = temp.path().join("registry.git");
        fs::create_dir_all(&bare).expect("bare dir");
        git(&bare, &["init", "--bare", "-b", "main"]);
        let work = temp.path().join("seed");
        git(temp.path(), &["clone", "registry.git", "seed"]);
        fs::create_dir_all(work.join("other-flow")).expect("other dir");
        fs::write(work.join("other-flow").join(WORKFLOW_FILE), OTHER_FLOW_YAML)
            .expect("other yaml");
        fs::write(work.join("index.json"), OTHER_INDEX_JSON).expect("index");
        fs::write(work.join("README.md"), "# fixture registry\n").expect("readme");
        commit_fixture(&work, "seed registry");
        git(&work, &["push", "origin", "main"]);
        temp
    }

    /// bare 仓库的 clone 来源（本地路径形态，clone_repo_verbatim 直接可用）。
    pub(crate) fn bare_url(fixture: &tempfile::TempDir) -> String {
        path_to_string(&fixture.path().join("registry.git"))
    }

    /// fixture 内 seed 工作克隆（预置分支等追加操作用）。
    pub(crate) fn seed_work(fixture: &tempfile::TempDir) -> PathBuf {
        fixture.path().join("seed")
    }

    pub(crate) const OTHER_FLOW_YAML: &str =
        "name: Other 流程\nslug: other-flow\nversion: 0.1.0\ndescription: 既有条目\n";
    pub(crate) const OTHER_INDEX_JSON: &str = concat!(
        "{\n",
        "  \"version\": 1,\n",
        "  \"workflows\": [\n",
        "    {\n",
        "      \"slug\": \"other-flow\",\n",
        "      \"name\": \"Other 流程\",\n",
        "      \"version\": \"0.1.0\",\n",
        "      \"description\": \"既有条目\",\n",
        "      \"path\": \"other-flow\"\n",
        "    }\n",
        "  ]\n",
        "}"
    );

    /// 8 字段全齐的 alpha 工作流（author/tags/icon 均设值）。
    pub(crate) const ALPHA_YAML: &str = "name: Alpha 流程\n\
         slug: alpha-flow\n\
         version: 0.1.0\n\
         description: 测试 alpha\n\
         author: tester\n\
         tags:\n  - alpha\n  - fixture\n\
         icon: beaker\n";
    pub(crate) const ALPHA_YAML_V2: &str = "name: Alpha 流程\n\
         slug: alpha-flow\n\
         version: 0.2.0\n\
         description: 测试 alpha v2\n\
         author: tester\n\
         tags:\n  - alpha\n  - fixture\n\
         icon: beaker\n";
    pub(crate) const ALPHA_README: &str = "# Alpha\n";

    pub(crate) fn install_alpha_workflow(ctx: &AppContext) {
        install_workflow(ctx, "alpha-flow", ALPHA_YAML);
    }

    pub(crate) fn install_workflow(ctx: &AppContext, slug: &str, yaml: &str) {
        let dir = workflows_dir(ctx).join(slug);
        ensure_dir(&dir).expect("workflow dir");
        fs::write(dir.join(WORKFLOW_FILE), yaml).expect("yaml");
        fs::write(dir.join("README.md"), ALPHA_README).expect("readme");
    }

    /// metadata 全齐的 skill；最小 skill（无 metadata，缺省映射实证）。
    pub(crate) const ALPHA_SKILL_MD: &str = "---\n\
         name: alpha-skill\n\
         description: 测试 skill\n\
         metadata:\n\
         \x20 version: 1.2.3\n\
         \x20 author: tester\n\
         \x20 tags: alpha, fixture\n\
         \x20 icon: flask\n\
         ---\n\nbody\n";
    pub(crate) const MINIMAL_SKILL_MD: &str =
        "---\ndescription: 最小 skill\n---\n\nbody\n";

    /// 在默认中心库（home/.oh-my-skills/skills）安装 skill。
    pub(crate) fn install_skill(ctx: &AppContext, slug: &str, skill_md: &str) {
        let dir = ctx
            .home_dir()
            .join(".oh-my-skills")
            .join("skills")
            .join(slug);
        ensure_dir(&dir).expect("skill dir");
        fs::write(dir.join("SKILL.md"), skill_md).expect("skill md");
    }

    /// 落盘 github_token + github_username（经核心 save_settings——不做 URL
    /// 校验，本地 fixture 路径因此也可写入 *RegistryUrl）。
    pub(crate) fn provision_identity(ctx: &AppContext) {
        let mut app_settings = settings::default_settings(ctx).expect("defaults");
        app_settings.github_token = Some("ghp_test_token".to_string());
        app_settings.github_username = Some("alice".to_string());
        settings::save_settings(ctx, &app_settings).expect("save identity");
    }

    pub(crate) fn point_workflow_registry_at(ctx: &AppContext, url: &str) {
        let mut app_settings = settings::load_settings(ctx).expect("settings");
        app_settings.workflow_registry_url = Some(url.to_string());
        settings::save_settings(ctx, &app_settings).expect("save registry url");
    }

    pub(crate) fn point_skill_registry_at(ctx: &AppContext, url: &str) {
        let mut app_settings = settings::load_settings(ctx).expect("settings");
        app_settings.skill_registry_url = Some(url.to_string());
        settings::save_settings(ctx, &app_settings).expect("save registry url");
    }

    /// 读 git 输出（经 git_ops 原语，本模块测试同样零 Command::new）。
    pub(crate) fn git_output(dir: &Path, args: &[&str]) -> String {
        let mut cmd = git_ops::base_command();
        cmd.arg("-C").arg(dir).args(args);
        git_ops::run(&mut cmd, None).expect("git output")
    }

    /// 断言用：data_dir/tmp 无本模块临时目录残留（目录不存在或为空）。
    pub(crate) fn assert_tmp_clean(ctx: &AppContext) {
        let tmp = ctx.data_dir().join("tmp");
        if !tmp.exists() {
            return;
        }
        let remaining: Vec<_> = fs::read_dir(&tmp)
            .expect("read tmp")
            .filter_map(Result::ok)
            .collect();
        assert!(remaining.is_empty(), "tmp/ 残留: {remaining:?}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github_auth::basic_auth_value;
    use crate::workflow_update::test_support::{commit_fixture, git};
    use std::sync::Mutex;
    use test_support::*;

    // resolve_token 读进程级环境变量；串行化需要操纵 OMS_GITHUB_TOKEN 的用例。
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn test_ctx(temp: &tempfile::TempDir) -> AppContext {
        AppContext::new(temp.path().join("data"), temp.path().join("home"))
    }

    fn alpha_entry() -> IndexEntry {
        IndexEntry {
            slug: "alpha-flow".to_string(),
            name: "Alpha 流程".to_string(),
            version: "0.1.0".to_string(),
            description: "测试 alpha".to_string(),
            author: Some("tester".to_string()),
            tags: vec!["alpha".to_string(), "fixture".to_string()],
            icon: Some("beaker".to_string()),
            path: "alpha-flow".to_string(),
        }
    }

    /// clone bare 回来（默认分支）读 index.json。
    fn cloned_index(fixture: &tempfile::TempDir, dest_name: &str) -> serde_json::Value {
        let dest = fixture.path().join(dest_name);
        git_ops::clone_repo_verbatim(&bare_url(fixture), &dest, None).expect("clone back");
        serde_json::from_str(&fs::read_to_string(dest.join("index.json")).expect("index"))
            .expect("index json")
    }

    // -- upsert_index_entry（纯函数组，无 git）--------------------------------

    #[test]
    fn upsert_creates_missing_file_with_exact_contract_shape() {
        let temp = tempfile::tempdir().expect("temp dir");
        let index = temp.path().join("index.json");
        upsert_index_entry(&index, "workflows", &alpha_entry()).expect("upsert");

        // 键序按字母序（serde_json BTreeMap，模块头注释已声明）；author/icon
        // 设值时出现在 description/name 邻位。
        let expected = concat!(
            "{\n",
            "  \"version\": 1,\n",
            "  \"workflows\": [\n",
            "    {\n",
            "      \"author\": \"tester\",\n",
            "      \"description\": \"测试 alpha\",\n",
            "      \"icon\": \"beaker\",\n",
            "      \"name\": \"Alpha 流程\",\n",
            "      \"path\": \"alpha-flow\",\n",
            "      \"slug\": \"alpha-flow\",\n",
            "      \"tags\": [\n",
            "        \"alpha\",\n",
            "        \"fixture\"\n",
            "      ],\n",
            "      \"version\": \"0.1.0\"\n",
            "    }\n",
            "  ]\n",
            "}"
        );
        assert_eq!(fs::read_to_string(&index).expect("read"), expected);
    }

    #[test]
    fn upsert_omits_author_and_icon_when_absent() {
        let temp = tempfile::tempdir().expect("temp dir");
        let index = temp.path().join("index.json");
        let entry = IndexEntry {
            author: None,
            tags: Vec::new(),
            icon: None,
            ..alpha_entry()
        };
        upsert_index_entry(&index, "skills", &entry).expect("upsert");

        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&index).expect("read")).expect("json");
        let item = &value["skills"][0];
        let object = item.as_object().expect("object");
        // 8 字段契约：6 个必备键 + author/icon 省略、tags 空数组保留。
        assert_eq!(object.len(), 6);
        assert!(!object.contains_key("author"));
        assert!(!object.contains_key("icon"));
        assert_eq!(item["tags"], json!([]));
        assert_eq!(item["slug"], json!("alpha-flow"));
        assert_eq!(item["path"], json!("alpha-flow"));
    }

    #[test]
    fn upsert_creates_missing_array_and_version_preserving_other_keys() {
        let temp = tempfile::tempdir().expect("temp dir");
        let index = temp.path().join("index.json");
        fs::write(&index, "{\n  \"note\": \"hand-written\"\n}").expect("seed");
        upsert_index_entry(&index, "workflows", &alpha_entry()).expect("upsert");

        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&index).expect("read")).expect("json");
        assert_eq!(value["version"], json!(1), "缺 version 回填 1");
        assert_eq!(value["note"], json!("hand-written"), "既有键保留");
        assert_eq!(value["workflows"].as_array().expect("array").len(), 1);
    }

    #[test]
    fn upsert_replaces_by_slug_in_place_and_appends_in_order() {
        let temp = tempfile::tempdir().expect("temp dir");
        let index = temp.path().join("index.json");
        let entry = |slug: &str, version: &str| IndexEntry {
            slug: slug.to_string(),
            version: version.to_string(),
            path: slug.to_string(),
            ..alpha_entry()
        };
        upsert_index_entry(&index, "workflows", &entry("a-flow", "0.1.0")).expect("a");
        upsert_index_entry(&index, "workflows", &entry("b-flow", "0.1.0")).expect("b");
        upsert_index_entry(&index, "workflows", &entry("c-flow", "0.1.0")).expect("c");

        // 更新中间条目：原位替换，数组顺序不变。
        upsert_index_entry(&index, "workflows", &entry("b-flow", "0.2.0")).expect("update b");
        // 新条目：尾部追加。
        upsert_index_entry(&index, "workflows", &entry("d-flow", "0.1.0")).expect("append d");

        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&index).expect("read")).expect("json");
        let array = value["workflows"].as_array().expect("array");
        let slugs: Vec<&str> = array
            .iter()
            .map(|item| item["slug"].as_str().expect("slug"))
            .collect();
        assert_eq!(slugs, vec!["a-flow", "b-flow", "c-flow", "d-flow"], "数组保序");
        assert_eq!(array[1]["version"], json!("0.2.0"), "b 原位更新");
    }

    #[test]
    fn upsert_rejects_corrupt_root_and_non_array_key() {
        let temp = tempfile::tempdir().expect("temp dir");
        let index = temp.path().join("index.json");

        fs::write(&index, "[1, 2]").expect("root array");
        let error = upsert_index_entry(&index, "workflows", &alpha_entry())
            .expect_err("non-object root must fail");
        assert!(error.contains("not a JSON object"), "error: {error}");

        fs::write(&index, "{\"workflows\": {\"oops\": true}}").expect("non-array key");
        let error = upsert_index_entry(&index, "workflows", &alpha_entry())
            .expect_err("non-array key must fail");
        assert!(error.contains("is not an array"), "error: {error}");

        fs::write(&index, "{ not valid json").expect("broken json");
        assert!(upsert_index_entry(&index, "workflows", &alpha_entry()).is_err());
    }

    #[test]
    fn skill_entry_mapping_full_metadata_and_defaults() {
        let temp = tempfile::tempdir().expect("temp dir");

        // 全量 metadata：version/author/tags/icon 取自 metadata 键。
        let full = temp.path().join("alpha-skill");
        ensure_dir(&full).expect("dir");
        fs::write(full.join("SKILL.md"), ALPHA_SKILL_MD).expect("md");
        let entry = skill_entry_from_dir(&full, "alpha-skill").expect("entry");
        assert_eq!(
            entry,
            IndexEntry {
                slug: "alpha-skill".to_string(),
                name: "alpha-skill".to_string(),
                version: "1.2.3".to_string(),
                description: "测试 skill".to_string(),
                author: Some("tester".to_string()),
                tags: vec!["alpha".to_string(), "fixture".to_string()],
                icon: Some("flask".to_string()),
                path: "alpha-skill".to_string(),
            },
            "skill 条目字段映射（DD §5.1）"
        );

        // 缺省映射：无 metadata → version "0.1.0"、tags []、author/icon 省略；
        // 无 frontmatter name → 回退目录名。
        let minimal = temp.path().join("minimal-skill");
        ensure_dir(&minimal).expect("dir");
        fs::write(minimal.join("SKILL.md"), MINIMAL_SKILL_MD).expect("md");
        let entry = skill_entry_from_dir(&minimal, "minimal-skill").expect("entry");
        assert_eq!(
            entry,
            IndexEntry {
                slug: "minimal-skill".to_string(),
                name: "minimal-skill".to_string(),
                version: "0.1.0".to_string(),
                description: "最小 skill".to_string(),
                author: None,
                tags: Vec::new(),
                icon: None,
                path: "minimal-skill".to_string(),
            },
            "缺省映射（version 0.1.0 / name 回退 slug / author&icon 省略）"
        );

        // 无 frontmatter / 无 SKILL.md → Err。
        let broken = temp.path().join("broken-skill");
        ensure_dir(&broken).expect("dir");
        fs::write(broken.join("SKILL.md"), "no frontmatter here\n").expect("md");
        assert!(skill_entry_from_dir(&broken, "broken-skill").is_err());
        assert!(skill_entry_from_dir(&temp.path().join("missing"), "missing").is_err());
    }

    // -- 一键推送 -------------------------------------------------------------

    #[test]
    fn push_rejects_official_registry_with_contribute_guidance() {
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        install_alpha_workflow(&ctx);
        // 默认 settings：workflow_registry_url = 官方地址 → 拒绝并引导贡献。
        let error = push_workflow_to_registry(&ctx, "alpha-flow").expect_err("official must fail");
        assert!(error.contains("contribute_workflow"), "error: {error}");
        assert!(error.contains("贡献"), "error: {error}");
        assert_tmp_clean(&ctx);
    }

    #[test]
    fn push_full_chain_over_local_bare_registry() {
        let fixture = bare_registry_repo();
        let bare = fixture.path().join("registry.git");
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        install_alpha_workflow(&ctx);
        point_workflow_registry_at(&ctx, &bare_url(&fixture));

        // 首推：clone → 写入 → upsert → commit → push 全链路真 git。
        let result = push_workflow_to_registry(&ctx, "alpha-flow").expect("push");
        assert_eq!(result.registry_url, bare_url(&fixture));
        assert_eq!(result.commit_hash.len(), 40, "commit hash: {}", result.commit_hash);
        assert!(
            result.commit_hash.bytes().all(|b| b.is_ascii_hexdigit()),
            "hash 形态: {}",
            result.commit_hash
        );
        assert_tmp_clean(&ctx);

        // 对端 git log 见新 commit；clone 回来逐字段校验 index 与包目录。
        let subjects = git_output(&bare, &["log", "--format=%s", "main"]);
        assert!(
            subjects.lines().any(|line| line == "Update workflow alpha-flow"),
            "log: {subjects}"
        );
        let index = cloned_index(&fixture, "verify-1");
        let array = index["workflows"].as_array().expect("array");
        assert_eq!(array.len(), 2, "既有 other-flow 保留 + alpha 追加");
        assert_eq!(array[0]["slug"], json!("other-flow"), "既有条目原位不动");
        let alpha = &array[1];
        assert_eq!(alpha["slug"], json!("alpha-flow"));
        assert_eq!(alpha["name"], json!("Alpha 流程"));
        assert_eq!(alpha["version"], json!("0.1.0"));
        assert_eq!(alpha["description"], json!("测试 alpha"));
        assert_eq!(alpha["author"], json!("tester"));
        assert_eq!(alpha["tags"], json!(["alpha", "fixture"]));
        assert_eq!(alpha["icon"], json!("beaker"));
        assert_eq!(alpha["path"], json!("alpha-flow"));
        let verify_yaml = fs::read(fixture.path().join("verify-1/alpha-flow/workflow.yaml"))
            .expect("verify yaml");
        assert_eq!(verify_yaml, ALPHA_YAML.as_bytes(), "包目录逐字节");
        let verify_readme = fs::read(fixture.path().join("verify-1/alpha-flow/README.md"))
            .expect("verify readme");
        assert_eq!(verify_readme, ALPHA_README.as_bytes());

        // 二推（版本升级）：upsert 原位更新，数组仍两条、alpha 保持第二位。
        install_workflow(&ctx, "alpha-flow", ALPHA_YAML_V2);
        let result = push_workflow_to_registry(&ctx, "alpha-flow").expect("push v2");
        assert_eq!(
            git_output(&bare, &["rev-parse", "main"]).trim(),
            result.commit_hash,
            "对端 main 指向新 commit"
        );
        assert_eq!(
            git_output(&bare, &["rev-list", "--count", "main"]).trim(),
            "3",
            "seed + 两次推送共 3 个 commit"
        );
        let index = cloned_index(&fixture, "verify-2");
        let array = index["workflows"].as_array().expect("array");
        assert_eq!(array.len(), 2);
        assert_eq!(array[0]["slug"], json!("other-flow"));
        assert_eq!(array[1]["slug"], json!("alpha-flow"));
        assert_eq!(array[1]["version"], json!("0.2.0"));
        assert_tmp_clean(&ctx);
    }

    #[test]
    fn push_rejected_maps_to_retry_message() {
        let fixture = bare_registry_repo();
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        install_alpha_workflow(&ctx);

        // 我的 staging clone（基于旧 main）。
        let staging = temp.path().join("staging");
        git_ops::clone_repo_verbatim(&bare_url(&fixture), &staging, None).expect("clone");

        // 竞品并发：另一个 clone 提交并推 main（在 fixture 目录内操作，
        // bare 相对路径 registry.git 才解析得到）。
        git(fixture.path(), &["clone", "registry.git", "competitor"]);
        let competitor = fixture.path().join("competitor");
        fs::write(competitor.join("README.md"), "# competitor\n").expect("competitor edit");
        commit_fixture(&competitor, "competitor update");
        git(&competitor, &["push", "origin", "main"]);

        // 我的 staging 提交后 push → non-fast-forward → 「远端已更新，请重试」。
        let entry = alpha_entry();
        copy_dir_recursive(&workflows_dir(&ctx).join("alpha-flow"), &staging.join("alpha-flow"))
            .expect("copy");
        upsert_index_entry(&staging.join("index.json"), "workflows", &entry).expect("upsert");
        let identity = git_ops::detect_identity(&staging, "oms");
        git_ops::commit_all(&staging, "Update workflow alpha-flow", &identity).expect("commit");
        let error = push_refspec(&staging, "origin", "HEAD", None).expect_err("must be rejected");
        assert!(error.contains("远端已更新，请重试"), "error: {error}");
    }

    #[test]
    fn errors_never_leak_token_in_either_form() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let token = "ghp_leak_probe";
        std::env::set_var("OMS_GITHUB_TOKEN", token);
        let encoded = basic_auth_value(token);

        let fixture = bare_registry_repo();
        let temp = tempfile::tempdir().expect("temp dir");
        let staging = temp.path().join("staging");
        git_ops::clone_repo_verbatim(&bare_url(&fixture), &staging, Some(token)).expect("clone");
        // 删除对端 → push 必然失败（错误串完整走 git_ops 脱敏链）。
        remove_entry(&fixture.path().join("registry.git")).expect("remove bare");
        let error = push_refspec(&staging, "origin", "HEAD", Some(token))
            .expect_err("push to removed remote must fail");

        std::env::remove_var("OMS_GITHUB_TOKEN");
        assert!(!error.contains(token), "token 本体不得入错误串: {error}");
        assert!(!error.contains(&encoded), "base64 形态不得入错误串: {error}");
    }

    // -- fork 贡献三态 ---------------------------------------------------------

    #[test]
    fn contribute_no_token_returns_structured_outcome() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        std::env::set_var("OMS_GITHUB_TOKEN", "");
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        let outcome = contribute_workflow(&ctx, "alpha-flow").expect("no token is Ok");
        std::env::remove_var("OMS_GITHUB_TOKEN");

        assert_eq!(outcome, ContributeOutcome::NoToken);
        // wire 形态钉死：{"status":"noToken"}，无多余键。
        let wire = serde_json::to_value(&outcome).expect("wire");
        assert_eq!(wire, json!({ "status": "noToken" }));
    }

    #[test]
    fn contribute_need_fork_when_probe_fails() {
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        provision_identity(&ctx);
        install_alpha_workflow(&ctx);
        // 必然不可达的 fork（有无网络 ls-remote 都失败 → NeedFork，结果确定性）。
        point_workflow_registry_at(
            &ctx,
            "https://github.com/oms-fixture/nonexistent-workflows-000.git",
        );

        let outcome = contribute_workflow(&ctx, "alpha-flow").expect("need fork is Ok");
        assert_eq!(
            outcome,
            ContributeOutcome::NeedFork {
                fork_page_url: "https://github.com/oms-fixture/nonexistent-workflows-000/fork"
                    .to_string()
            }
        );
        let wire = serde_json::to_value(&outcome).expect("wire");
        assert_eq!(
            wire,
            json!({
                "status": "needFork",
                "forkPageUrl": "https://github.com/oms-fixture/nonexistent-workflows-000/fork"
            })
        );
    }

    #[test]
    fn contribute_requires_github_username() {
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        // 仅配 token 不配 username → Err 引导设置（真错误走 Err 通道）。
        let mut app_settings = settings::default_settings(&ctx).expect("defaults");
        app_settings.github_token = Some("ghp_test_token".to_string());
        settings::save_settings(&ctx, &app_settings).expect("save");
        let error = contribute_workflow(&ctx, "alpha-flow").expect_err("username required");
        assert!(error.contains("GitHub username"), "error: {error}");
    }

    #[test]
    fn contribute_ready_full_chain_over_local_fork() {
        let fixture = bare_registry_repo();
        let bare = fixture.path().join("registry.git");
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        provision_identity(&ctx);
        install_alpha_workflow(&ctx);
        let app_settings = settings::load_settings(&ctx).expect("settings");
        let (staged_dir, entry) =
            RegistryKind::Workflow.staged_and_entry(&ctx, &app_settings, "alpha-flow").expect("staged");

        let outcome = contribute_via_fork(
            &ctx,
            &ContributeJob {
                kind: RegistryKind::Workflow,
                slug: "alpha-flow",
                staged_dir: &staged_dir,
                entry: &entry,
                owner: "oms-fixture",
                repo: "workflows",
                username: "alice",
                token: "ghp_test_token",
                fork_url: &bare_url(&fixture),
            },
        )
        .expect("ready is Ok");

        // compare_url 逐字段断言：owner/repo、base 恒 main、fork 用户、分支、title。
        let ContributeOutcome::Ready { compare_url, branch } = outcome else {
            panic!("expected Ready");
        };
        assert_eq!(branch, "contrib/alpha-flow");
        assert!(
            compare_url.starts_with(
                "https://github.com/oms-fixture/workflows/compare/main...alice:contrib/alpha-flow?expand=1&title=Add%20workflow%20alpha-flow&body="
            ),
            "compare_url: {compare_url}"
        );
        assert!(compare_url.contains("%E8%B4%A1%E7%8C%AE"), "body 预填中文模板: {compare_url}");
        let wire = serde_json::to_value(&ContributeOutcome::Ready {
            compare_url: compare_url.clone(),
            branch: branch.clone(),
        })
        .expect("wire");
        assert_eq!(wire["status"], json!("ready"));
        assert_eq!(wire["compareUrl"], json!(compare_url));
        assert_eq!(wire["branch"], json!("contrib/alpha-flow"));

        // 对端：fork 仓出现 contrib/alpha-flow 分支；clone 回来逐字段校验
        //（--depth 1 浅克隆只含默认分支，整分支克隆验证用 --branch 全量克隆）。
        let refs = git_output(&bare, &["for-each-ref", "--format=%(refname)"]);
        assert!(
            refs.lines().any(|line| line == "refs/heads/contrib/alpha-flow"),
            "refs: {refs}"
        );
        let verify = temp.path().join("verify");
        git(
            temp.path(),
            &["clone", "--branch", "contrib/alpha-flow", &bare_url(&fixture), "verify"],
        );
        let index: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(verify.join("index.json")).expect("index"),
        )
        .expect("index json");
        let array = index["workflows"].as_array().expect("array");
        assert_eq!(array.len(), 2);
        assert_eq!(array[0]["slug"], json!("other-flow"), "fork 既有内容保留");
        assert_eq!(array[1]["slug"], json!("alpha-flow"));
        assert_eq!(array[1]["version"], json!("0.1.0"));
        assert_eq!(
            fs::read(verify.join("alpha-flow/workflow.yaml")).expect("yaml"),
            ALPHA_YAML.as_bytes()
        );
        assert_tmp_clean(&ctx);
    }

    #[test]
    fn contribute_ready_branch_conflict_appends_utc_timestamp() {
        let fixture = bare_registry_repo();
        let bare = fixture.path().join("registry.git");
        // fork 上预置同名分支 contrib/alpha-flow。
        let seed = seed_work(&fixture);
        git(&seed, &["checkout", "-b", "contrib/alpha-flow"]);
        git(&seed, &["push", "origin", "contrib/alpha-flow"]);

        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        provision_identity(&ctx);
        install_alpha_workflow(&ctx);
        let app_settings = settings::load_settings(&ctx).expect("settings");
        let (staged_dir, entry) =
            RegistryKind::Workflow.staged_and_entry(&ctx, &app_settings, "alpha-flow").expect("staged");

        let outcome = contribute_via_fork(
            &ctx,
            &ContributeJob {
                kind: RegistryKind::Workflow,
                slug: "alpha-flow",
                staged_dir: &staged_dir,
                entry: &entry,
                owner: "oms-fixture",
                repo: "workflows",
                username: "alice",
                token: "ghp_test_token",
                fork_url: &bare_url(&fixture),
            },
        )
        .expect("ready is Ok");

        let ContributeOutcome::Ready { compare_url, branch } = outcome else {
            panic!("expected Ready");
        };
        assert!(
            branch.starts_with("contrib/alpha-flow-") && branch.len() > "contrib/alpha-flow-".len(),
            "冲突分支加 UTC 时间戳: {branch}"
        );
        assert!(
            compare_url.contains(&format!("main...alice:{branch}")),
            "compare_url 含带时间戳分支: {compare_url}"
        );
        let refs = git_output(&bare, &["for-each-ref", "--format=%(refname)"]);
        assert!(
            refs.lines()
                .any(|line| line == &format!("refs/heads/{branch}")),
            "refs: {refs}"
        );
        assert_tmp_clean(&ctx);
    }

    #[test]
    fn contribute_skill_full_chain_maps_entry_fields() {
        let fixture = bare_registry_repo();
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        provision_identity(&ctx);
        install_skill(&ctx, "alpha-skill", ALPHA_SKILL_MD);
        let app_settings = settings::load_settings(&ctx).expect("settings");
        let (staged_dir, entry) =
            RegistryKind::Skill.staged_and_entry(&ctx, &app_settings, "alpha-skill").expect("staged");

        let outcome = contribute_via_fork(
            &ctx,
            &ContributeJob {
                kind: RegistryKind::Skill,
                slug: "alpha-skill",
                staged_dir: &staged_dir,
                entry: &entry,
                owner: "oms-fixture",
                repo: "skills",
                username: "alice",
                token: "ghp_test_token",
                fork_url: &bare_url(&fixture),
            },
        )
        .expect("ready is Ok");
        let ContributeOutcome::Ready { compare_url, branch } = outcome else {
            panic!("expected Ready");
        };
        assert_eq!(branch, "contrib/alpha-skill");
        assert!(
            compare_url.starts_with(
                "https://github.com/oms-fixture/skills/compare/main...alice:contrib/alpha-skill?expand=1&title=Add%20skill%20alpha-skill&body="
            ),
            "compare_url: {compare_url}"
        );

        // skills 数组挂在既有 index 上（workflows 键原样保留）；整分支克隆验证。
        let verify = temp.path().join("verify");
        git(
            temp.path(),
            &["clone", "--branch", "contrib/alpha-skill", &bare_url(&fixture), "verify"],
        );
        let index: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(verify.join("index.json")).expect("index"),
        )
        .expect("index json");
        assert_eq!(
            index["workflows"][0]["slug"],
            json!("other-flow"),
            "既有 workflows 键保留"
        );
        let skills = index["skills"].as_array().expect("skills array");
        assert_eq!(skills.len(), 1);
        let skill = &skills[0];
        assert_eq!(skill["slug"], json!("alpha-skill"));
        assert_eq!(skill["name"], json!("alpha-skill"));
        assert_eq!(skill["version"], json!("1.2.3"), "version 取自 metadata");
        assert_eq!(skill["description"], json!("测试 skill"));
        assert_eq!(skill["author"], json!("tester"));
        assert_eq!(skill["tags"], json!(["alpha", "fixture"]), "tags 逗号分隔解析");
        assert_eq!(skill["icon"], json!("flask"));
        assert_eq!(skill["path"], json!("alpha-skill"));
        assert_eq!(
            fs::read(verify.join("alpha-skill/SKILL.md")).expect("skill md"),
            ALPHA_SKILL_MD.as_bytes()
        );
        assert_tmp_clean(&ctx);
    }

    #[test]
    fn contribute_rejects_bad_slug_and_uninstalled_content() {
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        provision_identity(&ctx);

        for slug in ["..", "../settings", "a/b", "", "UPPER"] {
            assert!(
                contribute_workflow(&ctx, slug).is_err(),
                "workflow slug '{slug}' must fail"
            );
            assert!(
                contribute_skill(&ctx, slug).is_err(),
                "skill slug '{slug}' must fail"
            );
        }
        // 未安装内容 → Err（真错误走 Err 通道，不进三态载荷）。
        assert!(contribute_workflow(&ctx, "not-installed").is_err());
        assert!(contribute_skill(&ctx, "not-installed").is_err());
    }

    // -- 官方仓上传分支（C7 复用入口）-----------------------------------------

    #[test]
    fn contribute_to_official_requires_token_and_valid_slug() {
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        let staged = temp.path().join("staged");
        ensure_dir(&staged).expect("staged");
        fs::write(staged.join(WORKFLOW_FILE), ALPHA_YAML).expect("yaml");

        // 无 token（env 置空 + settings 无）→ Err「站点未开放贡献」。
        {
            let _guard = ENV_LOCK.lock().expect("env lock");
            std::env::set_var("OMS_GITHUB_TOKEN", "");
            let error =
                contribute_to_official(&ctx, RegistryKind::Workflow, &staged, "alpha-flow")
                    .expect_err("no token must fail");
            std::env::remove_var("OMS_GITHUB_TOKEN");
            assert!(error.contains("站点未开放贡献"), "error: {error}");
        }

        // 有 token 但坏 slug → Err（先于任何 git 调用）。
        provision_identity(&ctx);
        for slug in ["..", "../evil", "a/b", "", "UPPER"] {
            let error = contribute_to_official(&ctx, RegistryKind::Workflow, &staged, slug)
                .expect_err("bad slug must fail");
            assert!(error.contains("Invalid workflow slug"), "slug '{slug}': {error}");
        }

        // staging yaml 与 slug 参数不一致 → Err（防包目录与 index 条目分裂）。
        let error = contribute_to_official(&ctx, RegistryKind::Workflow, &staged, "other-name")
            .expect_err("mismatch must fail");
        assert!(error.contains("slug mismatch"), "error: {error}");
    }

    #[test]
    fn contribute_to_official_full_chain_over_local_bare() {
        let fixture = bare_registry_repo();
        let bare = fixture.path().join("registry.git");
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        // C7 形态的 staging 目录（模拟访客上传解包产物）。
        let staged = temp.path().join("upload-staging");
        ensure_dir(&staged).expect("staged");
        fs::write(staged.join(WORKFLOW_FILE), ALPHA_YAML).expect("yaml");
        fs::write(staged.join("README.md"), ALPHA_README).expect("readme");
        let entry = RegistryKind::Workflow
            .entry_from_staged(&staged, "alpha-flow")
            .expect("entry");

        let outcome = upload_to_official(
            &ctx,
            &UploadJob {
                kind: RegistryKind::Workflow,
                slug: "alpha-flow",
                staged_dir: &staged,
                entry: &entry,
                clone_url: &bare_url(&fixture),
                owner: "Pgooone",
                repo: "oh-my-skills-workflows",
                token: "ghp_official_bot",
            },
        )
        .expect("upload");

        assert!(
            outcome.branch.starts_with("upload/alpha-flow-"),
            "upload/{{slug}}-{{UTCts}} 分支命名: {}",
            outcome.branch
        );
        // 分支 compare 页：官方仓内分支（无 user 前缀）、base 恒 main。
        assert_eq!(
            outcome.branch_url,
            format!(
                "https://github.com/Pgooone/oh-my-skills-workflows/compare/main...{}?expand=1",
                outcome.branch
            )
        );

        // 对端：分支存在；clone 回来校验 index 与包内容（--branch 全量克隆）。
        let refs = git_output(&bare, &["for-each-ref", "--format=%(refname)"]);
        assert!(
            refs.lines()
                .any(|line| line == &format!("refs/heads/{}", outcome.branch)),
            "refs: {refs}"
        );
        let verify = temp.path().join("verify");
        git(
            temp.path(),
            &["clone", "--branch", &outcome.branch, &bare_url(&fixture), "verify"],
        );
        let index: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(verify.join("index.json")).expect("index"),
        )
        .expect("index json");
        let array = index["workflows"].as_array().expect("array");
        assert_eq!(array.len(), 2);
        assert_eq!(array[1]["slug"], json!("alpha-flow"));
        assert_eq!(array[1]["name"], json!("Alpha 流程"));
        assert_eq!(
            fs::read(verify.join("alpha-flow/workflow.yaml")).expect("yaml"),
            ALPHA_YAML.as_bytes()
        );
        assert_tmp_clean(&ctx);
    }

    // -- wire 形态钉死 ----------------------------------------------------------

    #[test]
    fn wire_forms_are_pinned() {
        let push = PushResult {
            commit_hash: "abc123".to_string(),
            registry_url: "https://github.com/acme/workflows.git".to_string(),
        };
        assert_eq!(
            serde_json::to_value(&push).expect("json"),
            json!({
                "commitHash": "abc123",
                "registryUrl": "https://github.com/acme/workflows.git"
            })
        );

        let upload = UploadOutcome {
            branch: "upload/alpha-flow-20260803T000000Z".to_string(),
            branch_url: "https://github.com/Pgooone/oh-my-skills-workflows/compare/main...upload/alpha-flow-20260803T000000Z?expand=1".to_string(),
        };
        assert_eq!(
            serde_json::to_value(&upload).expect("json"),
            json!({
                "branch": "upload/alpha-flow-20260803T000000Z",
                "branchUrl": "https://github.com/Pgooone/oh-my-skills-workflows/compare/main...upload/alpha-flow-20260803T000000Z?expand=1"
            })
        );

        assert_eq!(RegistryKind::parse("workflow"), Some(RegistryKind::Workflow));
        assert_eq!(RegistryKind::parse("skill"), Some(RegistryKind::Skill));
        assert_eq!(RegistryKind::parse("other"), None);
    }
}
