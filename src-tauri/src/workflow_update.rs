//! M3 workflow-update：工作流来源元数据（SourceMeta）与三态更新检查（DD §3）。
//!
//! 元数据落点 `data_dir/workflow-sources/{slug}.json`——目录外，禁入
//! `workflows/<slug>/`（会被 hash_dir 计入内容哈希）。判定以三方内容哈希为
//! 核心：本地安装目录 vs source 快照 vs 注册表缓存（`registry/current/{path}`，
//! 公开约定）。换注册表后旧来源条目归 Local 不再检查（门-F-12 语义，已知
//! 取舍：不按 source.registryUrl 分组多拉）。

use crate::context::AppContext;
use crate::fs_ops::{copy_dir_recursive, ensure_dir, hash_dir, path_to_string};
use crate::workflow::workflows_dir;
use crate::workflow_registry::{RemoteWorkflowSummary, download_to_installed, fetch_index};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Component, Path, PathBuf};

/// `data_dir/workflow-sources/{slug}.json`：一次注册表下载的来源快照。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SourceMeta {
    pub registry_url: String,
    pub path: String,
    pub content_hash: String,
    pub installed_at: String,
}

/// 三态（+Modified 细分）serde 形态：internally tagged，`kind` 判别字段，
/// 前端按 kind 分支（local / upToDate / updateAvailable / modified）。
/// 注意 enum 容器上 rename_all 只改变体名；variant 内字段须 rename_all_fields。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "kind")]
pub enum WorkflowUpdateState {
    Local,
    UpToDate,
    UpdateAvailable { remote_version: String },
    Modified {
        remote_changed: bool,
        remote_version: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowUpdateStatus {
    pub slug: String,
    pub state: WorkflowUpdateState,
    pub local_version: Option<String>,
    pub remote_version: Option<String>,
}

/// 下载/更新成功后记录来源快照；contentHash = 安装目录当前内容哈希。
pub fn record_source(
    ctx: &AppContext,
    slug: &str,
    registry_url: &str,
    path: &str,
) -> Result<(), String> {
    if !is_valid_slug(slug) {
        return Err(format!(
            "Invalid workflow slug '{slug}': must be non-empty and match [a-z0-9-]+"
        ));
    }
    let content_hash = hash_dir(&workflows_dir(ctx).join(slug))?;
    let meta = SourceMeta {
        registry_url: registry_url.to_string(),
        path: path.to_string(),
        content_hash,
        installed_at: Utc::now().to_rfc3339(),
    };
    ensure_dir(&sources_dir(ctx))?;
    let file = source_path(ctx, slug);
    let text = serde_json::to_string_pretty(&meta)
        .map_err(|error| format!("Unable to serialize source metadata for '{slug}': {error}"))?;
    fs::write(&file, text).map_err(|error| {
        format!(
            "Unable to write source metadata at {}: {error}",
            path_to_string(&file)
        )
    })
}

/// 读取来源快照；缺失/损坏/非法 slug 一律 None（按无 source 处理）。
pub fn read_source(ctx: &AppContext, slug: &str) -> Option<SourceMeta> {
    if !is_valid_slug(slug) {
        return None;
    }
    let text = fs::read_to_string(source_path(ctx, slug)).ok()?;
    serde_json::from_str(&text).ok()
}

/// 全量检查：拉取索引一次（内部已含缓存刷新与离线回退），惰性清理孤儿
/// source，逐已安装工作流判定（含无 source 的本地工作流 → Local）。
pub fn check_all(ctx: &AppContext) -> Result<Vec<WorkflowUpdateStatus>, String> {
    let registry_url = workflow_registry_url(ctx)?;
    let summaries = fetch_index(ctx, &registry_url)?;
    cleanup_orphan_sources(ctx);
    let installed = crate::workflow::list_installed(ctx)?;
    Ok(installed
        .into_iter()
        .map(|item| check_one(ctx, &item.slug, &summaries))
        .collect())
}

/// 单个判定。summaries 由调用方提供（check_all 拉一次后逐条复用）。
pub fn check_one(
    ctx: &AppContext,
    slug: &str,
    summaries: &[RemoteWorkflowSummary],
) -> WorkflowUpdateStatus {
    let local_version = crate::workflow::load(ctx, slug)
        .ok()
        .map(|workflow| workflow.version);
    let entry = summaries.iter().find(|item| item.slug == slug);
    let entry_version = entry.map(|item| item.version.clone());

    let status = |state: WorkflowUpdateState, remote_version: Option<String>| {
        WorkflowUpdateStatus {
            slug: slug.to_string(),
            state,
            local_version: local_version.clone(),
            remote_version,
        }
    };

    let Some(source) = read_source(ctx, slug) else {
        return status(WorkflowUpdateState::Local, None);
    };

    // 孤儿清理（惰性，免改 workflow.rs::delete）：source 在而 workflows/<slug>
    // 不在 → 删 source 文件。
    let local_dir = workflows_dir(ctx).join(slug);
    if !local_dir.is_dir() {
        let _ = fs::remove_file(source_path(ctx, slug));
        return status(WorkflowUpdateState::Local, None);
    }

    // 本地内容不可读：保守归 Local，不诱导覆盖式更新。
    let Some(local_hash) = hash_dir(&local_dir).ok() else {
        return status(WorkflowUpdateState::Local, None);
    };

    if local_hash != source.content_hash {
        // 本地被改：再比远端填 remote_changed（缓存不可比 → 无证据 → false）。
        let remote_changed = entry
            .and_then(|item| remote_content_hash(ctx, &item.path))
            .is_some_and(|remote_hash| remote_hash != source.content_hash);
        return status(
            WorkflowUpdateState::Modified {
                remote_changed,
                remote_version: entry_version.clone(),
            },
            entry_version,
        );
    }

    // 门-F-12：注册表无条目 → Local（换注册表后旧来源不再检查）。
    let Some(entry) = entry else {
        return status(WorkflowUpdateState::Local, None);
    };
    let remote_version = entry.version.clone();
    if local_version.as_deref() != Some(entry.version.as_str()) {
        return status(
            WorkflowUpdateState::UpdateAvailable {
                remote_version: remote_version.clone(),
            },
            Some(remote_version),
        );
    }
    // version 相等但缓存内容变了（调研 c9 兜底）。
    match remote_content_hash(ctx, &entry.path) {
        Some(remote_hash) if remote_hash != source.content_hash => status(
            WorkflowUpdateState::UpdateAvailable {
                remote_version: remote_version.clone(),
            },
            Some(remote_version),
        ),
        _ => status(WorkflowUpdateState::UpToDate, Some(remote_version)),
    }
}

/// 执行更新：Modified 未确认拒绝 → 备份 `backups/workflow-updates/{UTC ts}/
/// {slug}` → 复用 download_to_installed → record_source 重写。返回更新后状态。
pub fn apply_update(
    ctx: &AppContext,
    slug: &str,
    confirm_modified: bool,
) -> Result<WorkflowUpdateStatus, String> {
    if !is_valid_slug(slug) {
        return Err(format!(
            "Invalid workflow slug '{slug}': must be non-empty and match [a-z0-9-]+"
        ));
    }
    if read_source(ctx, slug).is_none() {
        return Err(format!(
            "Workflow '{slug}' has no registry source metadata; nothing to update from"
        ));
    }
    let registry_url = workflow_registry_url(ctx)?;
    let summaries = fetch_index(ctx, &registry_url)?;
    let before = check_one(ctx, slug, &summaries);
    match &before.state {
        WorkflowUpdateState::Local => {
            return Err(format!(
                "Workflow '{slug}' is not present in the current registry; nothing to update"
            ));
        }
        // 幂等：已是最新直接返回，不产备份不动文件。
        WorkflowUpdateState::UpToDate => return Ok(before),
        WorkflowUpdateState::Modified { .. } if !confirm_modified => {
            return Err(format!(
                "Workflow '{slug}' has local modifications; confirm overwrite to apply the update"
            ));
        }
        _ => {}
    }
    let entry = summaries
        .iter()
        .find(|item| item.slug == slug)
        .ok_or_else(|| {
            format!("Workflow '{slug}' is not present in the current registry; unable to update")
        })?;

    // 先备份再覆盖：download_to_installed 会整目录替换安装。
    let stamp = Utc::now().format("%Y%m%dT%H%M%SZ");
    let backup = ctx
        .data_dir()
        .join("backups")
        .join("workflow-updates")
        .join(stamp.to_string())
        .join(slug);
    copy_dir_recursive(&workflows_dir(ctx).join(slug), &backup)?;

    let installed_slug = download_to_installed(ctx, &registry_url, &entry.path)?;
    record_source(ctx, &installed_slug, &registry_url, &entry.path)?;
    Ok(check_one(ctx, &installed_slug, &summaries))
}

fn sources_dir(ctx: &AppContext) -> PathBuf {
    ctx.data_dir().join("workflow-sources")
}

fn source_path(ctx: &AppContext, slug: &str) -> PathBuf {
    sources_dir(ctx).join(format!("{slug}.json"))
}

/// 孤儿清理（check_all 扫描入口，与 check_one 内分支同规则）：source 在而
/// workflows/<slug> 不在 → 删 source 文件。尽力而为，失败不阻断检查。
fn cleanup_orphan_sources(ctx: &AppContext) {
    let Ok(entries) = fs::read_dir(sources_dir(ctx)) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Some(slug) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if !workflows_dir(ctx).join(slug).is_dir() {
            let _ = fs::remove_file(&path);
        }
    }
}

/// 注册表缓存内容哈希（`data_dir/registry/current/{path}`，公开约定）。
/// 缓存缺失/路径非法/不可读 → None（无证据判定远端变化，调用方按保守处理）。
fn remote_content_hash(ctx: &AppContext, path: &str) -> Option<String> {
    let candidate = Path::new(path);
    if path.is_empty()
        || !candidate
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return None;
    }
    hash_dir(
        &ctx.data_dir()
            .join("registry")
            .join("current")
            .join(candidate),
    )
    .ok()
}

/// 与 workflow_registry::is_safe_slug 同规则（原函数私有，按 DD §7 先例
/// 模块内同规则拷贝）。
fn is_valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// load_settings 已保证空值回填官方缺省；此处兜底仅为避免解包 panic。
/// （与 commands.rs / web routes 的私有助手同规则。）
fn workflow_registry_url(ctx: &AppContext) -> Result<String, String> {
    Ok(crate::settings::load_settings(ctx)?
        .workflow_registry_url
        .filter(|url| !url.trim().is_empty())
        .unwrap_or_else(|| crate::settings::OFFICIAL_WORKFLOW_REGISTRY_URL.to_string()))
}

/// 测试共享 fixture（本模块单测与 web 端点 oneshot 复用）。造数据复刻真实
/// 流程：本地 fixture 注册表 git 仓库 → 真 clone 铺缓存 → 下载安装 → 检查
/// → 更新；公开 API（fetch_index/download_to_installed）经 settings 指向
/// 必然 clone 失败的 GitHub 形态 URL，走生产级离线回退读预置缓存。
#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use crate::fs_ops::remove_entry;
    use crate::workflow::WORKFLOW_FILE;
    use std::process::Command;

    /// settings 指向此 URL：GitHub 形态（过 normalize）但仓库不存在，
    /// 有无网络 clone 都失败 → fetch_index / download_to_installed 离线
    /// 回退到预 clone 的缓存。结果确定性，速度取决于有无网络（秒级以内）。
    pub(crate) const UNCLONEABLE_URL: &str =
        "https://github.com/oms-fixture/nonexistent-repo-000.git";

    pub(crate) const ALPHA_YAML_V1: &str =
        "name: Alpha 流程\nslug: alpha-flow\nversion: 0.1.0\ndescription: 测试 alpha\n";
    pub(crate) const ALPHA_README_V1: &str = "# Alpha v1\n";
    pub(crate) const ALPHA_YAML_V2: &str =
        "name: Alpha 流程\nslug: alpha-flow\nversion: 0.2.0\ndescription: 测试 alpha v2\n";
    pub(crate) const ALPHA_README_V2: &str = "# Alpha v2\n";

    pub(crate) fn git(repo: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(repo)
            .status()
            .expect("git must run");
        assert!(status.success(), "git {args:?} failed with {status}");
    }

    pub(crate) fn commit_fixture(repo: &Path, message: &str) {
        git(repo, &["add", "-A"]);
        git(
            repo,
            &[
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=test",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-m",
                message,
            ],
        );
    }

    /// 覆写 fixture 仓库内容：index.json（单条目 alpha-flow）+ alpha-flow/
    /// 工作流目录。发布后须 commit_fixture 再刷新缓存才生效。
    pub(crate) fn write_alpha(repo: &Path, yaml: &str, readme: &str, index_version: &str) {
        fs::write(
            repo.join("index.json"),
            format!(
                "{{\"version\":1,\"workflows\":[{{\"slug\":\"alpha-flow\",\"name\":\"Alpha 流程\",\"version\":\"{index_version}\",\"description\":\"测试 alpha\",\"path\":\"alpha-flow\"}}]}}"
            ),
        )
        .expect("write index");
        let alpha = repo.join("alpha-flow");
        fs::create_dir_all(&alpha).expect("alpha dir");
        fs::write(alpha.join(WORKFLOW_FILE), yaml).expect("alpha yaml");
        fs::write(alpha.join("README.md"), readme).expect("alpha readme");
    }

    /// 本地 fixture 注册表仓库（v1 已提交）。TempDir 须由调用方持有存活。
    pub(crate) fn fixture_repo() -> tempfile::TempDir {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).expect("repo dir");
        git(&repo, &["init"]);
        write_alpha(&repo, ALPHA_YAML_V1, ALPHA_README_V1, "0.1.0");
        commit_fixture(&repo, "alpha v1");
        temp
    }

    pub(crate) fn repo_source(fixture: &tempfile::TempDir) -> String {
        path_to_string(&fixture.path().join("repo"))
    }

    /// 把 fixture 仓库真 clone 进注册表缓存（复刻 refresh_cache 的产物形态：
    /// `registry/current` = clone 结果；重 clone 前先移除旧缓存）。
    pub(crate) fn refresh_cache_from_fixture(ctx: &AppContext, source: &str) {
        let current = ctx.data_dir().join("registry").join("current");
        ensure_dir(current.parent().expect("registry root")).expect("registry root");
        if current.exists() {
            remove_entry(&current).expect("remove old cache");
        }
        crate::git_ops::clone_repo_verbatim(source, &current, None).expect("clone fixture");
    }

    /// 复刻 download_to_installed_from_source 的后段：缓存已就绪 → 读
    /// workflow.yaml 取 slug → 整目录拷入 workflows/（前段 refresh 由
    /// refresh_cache_from_fixture 完成）。
    pub(crate) fn download_from_cache(ctx: &AppContext, path: &str) -> String {
        let source_dir = ctx.data_dir().join("registry").join("current").join(path);
        let text =
            fs::read_to_string(source_dir.join(WORKFLOW_FILE)).expect("read cached yaml");
        let workflow = crate::workflow::Workflow::from_yaml(&text).expect("parse cached yaml");
        let target = workflows_dir(ctx).join(&workflow.slug);
        ensure_dir(&workflows_dir(ctx)).expect("workflows dir");
        if target.exists() {
            remove_entry(&target).expect("remove old install");
        }
        copy_dir_recursive(&source_dir, &target).expect("copy install");
        workflow.slug
    }

    /// 真实流程一段式：clone fixture → 下载安装 → record_source（被测函数）。
    pub(crate) fn install_alpha(ctx: &AppContext, source: &str) {
        refresh_cache_from_fixture(ctx, source);
        let slug = download_from_cache(ctx, "alpha-flow");
        record_source(ctx, &slug, source, "alpha-flow").expect("record source");
    }

    /// settings 指向 UNCLONEABLE_URL（核心 save_settings 不做 URL 校验，
    /// 校验在 save_settings_with_merge）。
    pub(crate) fn point_registry_at_uncloneable_url(ctx: &AppContext) {
        let mut settings = crate::settings::default_settings(ctx).expect("default settings");
        settings.workflow_registry_url = Some(UNCLONEABLE_URL.to_string());
        crate::settings::save_settings(ctx, &settings).expect("save settings");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs_ops::remove_entry;
    use crate::workflow::WORKFLOW_FILE;
    use test_support::*;

    fn test_ctx(temp: &tempfile::TempDir) -> AppContext {
        AppContext::new(temp.path().join("data"), temp.path().join("home"))
    }

    fn summaries_from_cache(ctx: &AppContext) -> Vec<RemoteWorkflowSummary> {
        crate::workflow_registry::read_cached_index(ctx).expect("cached index")
    }

    fn local_readme(ctx: &AppContext) -> PathBuf {
        workflows_dir(ctx).join("alpha-flow").join("README.md")
    }

    fn local_hash(ctx: &AppContext) -> String {
        hash_dir(&workflows_dir(ctx).join("alpha-flow")).expect("local hash")
    }

    fn cache_hash(ctx: &AppContext) -> String {
        hash_dir(
            &ctx.data_dir()
                .join("registry")
                .join("current")
                .join("alpha-flow"),
        )
        .expect("cache hash")
    }

    fn single_backup_dir(ctx: &AppContext) -> PathBuf {
        let root = ctx.data_dir().join("backups").join("workflow-updates");
        let stamps: Vec<_> = fs::read_dir(&root)
            .expect("backup root")
            .filter_map(Result::ok)
            .collect();
        assert_eq!(stamps.len(), 1, "expected exactly one backup stamp");
        stamps[0].path().join("alpha-flow")
    }

    // -- 三态判定矩阵（7 case）-----------------------------------------------

    #[test]
    fn check_one_local_without_source() {
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        // 有安装目录但无 source：即便注册表有同名条目也归 Local。
        let dir = workflows_dir(&ctx).join("alpha-flow");
        ensure_dir(&dir).expect("install dir");
        fs::write(dir.join(WORKFLOW_FILE), ALPHA_YAML_V1).expect("yaml");

        let status = check_one(&ctx, "alpha-flow", &summaries_from_cache_or_empty(&ctx));
        assert_eq!(status.state, WorkflowUpdateState::Local);
        assert_eq!(status.local_version.as_deref(), Some("0.1.0"));
        assert_eq!(status.remote_version, None);
    }

    fn summaries_from_cache_or_empty(ctx: &AppContext) -> Vec<RemoteWorkflowSummary> {
        crate::workflow_registry::read_cached_index(ctx).unwrap_or_default()
    }

    #[test]
    fn check_one_local_when_registry_entry_missing() {
        let fixture = fixture_repo();
        let source = repo_source(&fixture);
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        install_alpha(&ctx, &source);

        // 门-F-12：有 source 但注册表（summaries）无条目 → Local。
        let status = check_one(&ctx, "alpha-flow", &[]);
        assert_eq!(status.state, WorkflowUpdateState::Local);
        assert_eq!(status.local_version.as_deref(), Some("0.1.0"));
        assert_eq!(status.remote_version, None);
    }

    #[test]
    fn check_one_up_to_date() {
        let fixture = fixture_repo();
        let source = repo_source(&fixture);
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        install_alpha(&ctx, &source);

        let status = check_one(&ctx, "alpha-flow", &summaries_from_cache(&ctx));
        assert_eq!(status.state, WorkflowUpdateState::UpToDate);
        assert_eq!(status.local_version.as_deref(), Some("0.1.0"));
        assert_eq!(status.remote_version.as_deref(), Some("0.1.0"));
    }

    #[test]
    fn check_one_update_available_on_version_bump() {
        let fixture = fixture_repo();
        let source = repo_source(&fixture);
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        install_alpha(&ctx, &source);

        // 发布 v2（version bump）并刷新缓存。
        let repo = fixture.path().join("repo");
        write_alpha(&repo, ALPHA_YAML_V2, ALPHA_README_V2, "0.2.0");
        commit_fixture(&repo, "alpha v2");
        refresh_cache_from_fixture(&ctx, &source);

        let status = check_one(&ctx, "alpha-flow", &summaries_from_cache(&ctx));
        assert_eq!(
            status.state,
            WorkflowUpdateState::UpdateAvailable {
                remote_version: "0.2.0".to_string()
            }
        );
        assert_eq!(status.local_version.as_deref(), Some("0.1.0"));
        assert_eq!(status.remote_version.as_deref(), Some("0.2.0"));
    }

    #[test]
    fn check_one_update_available_on_same_version_content_change() {
        let fixture = fixture_repo();
        let source = repo_source(&fixture);
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        install_alpha(&ctx, &source);

        // 同 version 静默改内容（调研 c9 兜底路径）。
        let repo = fixture.path().join("repo");
        write_alpha(&repo, ALPHA_YAML_V1, "# Alpha v1.1\n", "0.1.0");
        commit_fixture(&repo, "alpha silent content change");
        refresh_cache_from_fixture(&ctx, &source);

        let status = check_one(&ctx, "alpha-flow", &summaries_from_cache(&ctx));
        assert_eq!(
            status.state,
            WorkflowUpdateState::UpdateAvailable {
                remote_version: "0.1.0".to_string()
            }
        );
    }

    #[test]
    fn check_one_modified_with_remote_unchanged() {
        let fixture = fixture_repo();
        let source = repo_source(&fixture);
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        install_alpha(&ctx, &source);

        fs::write(local_readme(&ctx), "# 本地改动\n").expect("local edit");

        let status = check_one(&ctx, "alpha-flow", &summaries_from_cache(&ctx));
        assert_eq!(
            status.state,
            WorkflowUpdateState::Modified {
                remote_changed: false,
                remote_version: Some("0.1.0".to_string())
            }
        );
        assert_eq!(status.local_version.as_deref(), Some("0.1.0"));
    }

    #[test]
    fn check_one_modified_with_remote_changed() {
        let fixture = fixture_repo();
        let source = repo_source(&fixture);
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        install_alpha(&ctx, &source);

        // 本地改动 + 远端同 version 内容变 → remote_changed = true。
        fs::write(local_readme(&ctx), "# 本地改动\n").expect("local edit");
        let repo = fixture.path().join("repo");
        write_alpha(&repo, ALPHA_YAML_V1, "# Alpha v1.1\n", "0.1.0");
        commit_fixture(&repo, "alpha silent content change");
        refresh_cache_from_fixture(&ctx, &source);

        let status = check_one(&ctx, "alpha-flow", &summaries_from_cache(&ctx));
        assert_eq!(
            status.state,
            WorkflowUpdateState::Modified {
                remote_changed: true,
                remote_version: Some("0.1.0".to_string())
            }
        );
    }

    // -- 孤儿 source 惰性清理 --------------------------------------------------

    #[test]
    fn orphan_source_removed_lazily() {
        let fixture = fixture_repo();
        let source = repo_source(&fixture);
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        install_alpha(&ctx, &source);
        let source_file = ctx.data_dir().join("workflow-sources").join("alpha-flow.json");
        assert!(source_file.is_file());

        // 卸载工作流（删安装目录）→ source 成孤儿；check_one 惰性清理。
        remove_entry(&workflows_dir(&ctx).join("alpha-flow")).expect("uninstall");
        let status = check_one(&ctx, "alpha-flow", &summaries_from_cache(&ctx));
        assert_eq!(status.state, WorkflowUpdateState::Local);
        assert!(!source_file.exists());

        // check_all 的 sources 目录扫描同一规则。
        install_alpha(&ctx, &source);
        assert!(source_file.is_file());
        remove_entry(&workflows_dir(&ctx).join("alpha-flow")).expect("uninstall");
        cleanup_orphan_sources(&ctx);
        assert!(!source_file.exists());
    }

    // -- record_source 写读往返 ------------------------------------------------

    #[test]
    fn record_source_write_read_roundtrip() {
        let fixture = fixture_repo();
        let source = repo_source(&fixture);
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);

        // 未安装 → Err（hash_dir 失败）；非法 slug → Err；缺失 → None。
        assert!(record_source(&ctx, "alpha-flow", &source, "alpha-flow").is_err());
        assert!(record_source(&ctx, "../evil", &source, "alpha-flow").is_err());
        assert_eq!(read_source(&ctx, "alpha-flow"), None);

        refresh_cache_from_fixture(&ctx, &source);
        let slug = download_from_cache(&ctx, "alpha-flow");
        record_source(&ctx, &slug, &source, "alpha-flow").expect("record");

        let meta = read_source(&ctx, &slug).expect("read back");
        assert_eq!(meta.registry_url, source);
        assert_eq!(meta.path, "alpha-flow");
        assert_eq!(meta.content_hash, local_hash(&ctx));
        assert!(!meta.installed_at.is_empty());

        // 损坏的 source 文件 → None（按无 source 处理）。
        fs::write(
            ctx.data_dir().join("workflow-sources").join("alpha-flow.json"),
            "{ not valid json",
        )
        .expect("corrupt");
        assert_eq!(read_source(&ctx, "alpha-flow"), None);
    }

    // -- check_all（真实离线回退链路）-----------------------------------------

    #[test]
    fn check_all_marks_tracked_and_local_only_workflows() {
        let fixture = fixture_repo();
        let source = repo_source(&fixture);
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        install_alpha(&ctx, &source);
        point_registry_at_uncloneable_url(&ctx);

        // 手写一个无 source 的本地工作流。
        let local_dir = workflows_dir(&ctx).join("local-only");
        ensure_dir(&local_dir).expect("local dir");
        fs::write(
            local_dir.join(WORKFLOW_FILE),
            "name: Local\nslug: local-only\nversion: 0.0.1\ndescription: 本地流程\n",
        )
        .expect("local yaml");

        // fetch_index 真实执行：clone 失败 → 离线回退预置缓存。
        let statuses = check_all(&ctx).expect("check all");
        assert_eq!(statuses.len(), 2);
        assert_eq!(statuses[0].slug, "alpha-flow");
        assert_eq!(statuses[0].state, WorkflowUpdateState::UpToDate);
        assert_eq!(statuses[1].slug, "local-only");
        assert_eq!(statuses[1].state, WorkflowUpdateState::Local);
    }

    // -- apply_update ----------------------------------------------------------

    #[test]
    fn apply_update_rejects_unconfirmed_modified() {
        let fixture = fixture_repo();
        let source = repo_source(&fixture);
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        install_alpha(&ctx, &source);
        point_registry_at_uncloneable_url(&ctx);

        fs::write(local_readme(&ctx), "# 本地改动\n").expect("local edit");
        let modified_hash = local_hash(&ctx);
        let source_file = ctx.data_dir().join("workflow-sources").join("alpha-flow.json");
        let source_before = fs::read_to_string(&source_file).expect("source before");

        let error = apply_update(&ctx, "alpha-flow", false).expect_err("must reject");
        assert!(error.contains("local modifications"), "error: {error}");

        // 拒绝后零副作用：安装/source 不变，无备份产生。
        assert_eq!(local_hash(&ctx), modified_hash);
        assert_eq!(
            fs::read_to_string(&source_file).expect("source after"),
            source_before
        );
        assert!(!ctx.data_dir().join("backups").join("workflow-updates").exists());
    }

    #[test]
    fn apply_update_backs_up_then_updates_and_records() {
        let fixture = fixture_repo();
        let source = repo_source(&fixture);
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        install_alpha(&ctx, &source);
        point_registry_at_uncloneable_url(&ctx);
        let pre_hash = local_hash(&ctx);

        // 发布 v2 并刷新缓存（模拟索引已在网络可用时刷新；更新执行本身
        // 仍走离线回退——clone 必失败，读已刷新缓存）。
        let repo = fixture.path().join("repo");
        write_alpha(&repo, ALPHA_YAML_V2, ALPHA_README_V2, "0.2.0");
        commit_fixture(&repo, "alpha v2");
        refresh_cache_from_fixture(&ctx, &source);

        let status = apply_update(&ctx, "alpha-flow", false).expect("update");
        assert_eq!(status.state, WorkflowUpdateState::UpToDate);
        assert_eq!(status.local_version.as_deref(), Some("0.2.0"));
        assert_eq!(status.remote_version.as_deref(), Some("0.2.0"));

        // 更新后本地 hash == 注册表缓存 hash；source 重写为新 hash。
        assert_eq!(local_hash(&ctx), cache_hash(&ctx));
        let meta = read_source(&ctx, "alpha-flow").expect("source");
        assert_eq!(meta.content_hash, local_hash(&ctx));
        assert_eq!(meta.path, "alpha-flow");

        // 备份产生且内容等于更新前。
        let backup = single_backup_dir(&ctx);
        assert_eq!(hash_dir(&backup).expect("backup hash"), pre_hash);
    }

    #[test]
    fn apply_update_with_confirm_overwrites_local_modifications() {
        let fixture = fixture_repo();
        let source = repo_source(&fixture);
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        install_alpha(&ctx, &source);
        point_registry_at_uncloneable_url(&ctx);

        // 本地改动 + 远端同 version 内容变（Modified + remote_changed）。
        fs::write(local_readme(&ctx), "# 本地改动\n").expect("local edit");
        let modified_hash = local_hash(&ctx);
        let repo = fixture.path().join("repo");
        write_alpha(&repo, ALPHA_YAML_V1, "# Alpha v1.1\n", "0.1.0");
        commit_fixture(&repo, "alpha silent content change");
        refresh_cache_from_fixture(&ctx, &source);

        let status = apply_update(&ctx, "alpha-flow", true).expect("update with confirm");
        assert_eq!(status.state, WorkflowUpdateState::UpToDate);

        // 本地改动被远端内容覆盖（安装 == 缓存，逐字节一致）。
        assert_eq!(local_hash(&ctx), cache_hash(&ctx));
        assert_eq!(
            fs::read_to_string(local_readme(&ctx)).expect("readme"),
            fs::read_to_string(
                ctx.data_dir()
                    .join("registry")
                    .join("current")
                    .join("alpha-flow")
                    .join("README.md")
            )
            .expect("cached readme")
        );

        // 备份内容 = 覆盖前的本地（含本地改动）。
        let backup = single_backup_dir(&ctx);
        assert_eq!(hash_dir(&backup).expect("backup hash"), modified_hash);
        assert_eq!(
            fs::read_to_string(backup.join("README.md")).expect("backup readme"),
            "# 本地改动\n"
        );
    }

    #[test]
    fn apply_update_rejects_local_and_sourceless_workflows() {
        let fixture = fixture_repo();
        let source = repo_source(&fixture);
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        install_alpha(&ctx, &source);
        point_registry_at_uncloneable_url(&ctx);

        // 无 source → Err（不进入 fetch/备份）。
        let error = apply_update(&ctx, "never-installed", false).expect_err("must fail");
        assert!(error.contains("no registry source metadata"), "error: {error}");

        // 非法 slug → Err。
        for slug in ["..", "../settings", "a/b", "", "UPPER"] {
            let error = apply_update(&ctx, slug, false).expect_err("bad slug must fail");
            assert!(error.contains("Invalid workflow slug"), "slug '{slug}': {error}");
        }

        // UpToDate 幂等：不产备份直接返回。
        let status = apply_update(&ctx, "alpha-flow", false).expect("idempotent");
        assert_eq!(status.state, WorkflowUpdateState::UpToDate);
        assert!(!ctx.data_dir().join("backups").join("workflow-updates").exists());
    }
}
