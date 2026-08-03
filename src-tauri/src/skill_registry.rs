//! Skill registry client: pulls the remote skill registry (git clone --depth 1)
//! into a local cache under `data_dir/skill-registry/` and serves index reads,
//! downloads into the central library (`settings.library_path/<slug>/`), and
//! hash-based batch update checks for registry-tracked skills.
//!
//! 结构有意镜像 workflow_registry（DD §5.5.2，不泛化既有）：staging → swap →
//! 离线回退的缓存模式、仅 Normal 段的路径安检、[a-z0-9-]+ slug 安检。差异面：
//! 下载落点为中心库且写 `~/.agents/.skill-lock.json` 条目（同 slug 异源冲突
//! 拒绝门-L3、source/sourceUrl 恒写归一化 https 形态门-L5、byte-verbatim 直拷
//! 门-L6）；更新检查为批量一次 clone 的 hash 两态（本地被改与远端更新同形
//! updateAvailable=true，DD §5.5.5 设计例外门-F-11）。

use crate::context::AppContext;
use crate::fs_ops::{copy_dir_recursive, ensure_dir, hash_dir, path_to_string, remove_entry};
use crate::models::{SkillLockEntry, SkillLockFile};
use crate::skill_ops::normalize_github_url;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

// 常量唯一来源在 settings.rs（C1 随 skill_registry_url 设置项引入）；此处
// re-export 保持 `skill_registry::OFFICIAL_SKILL_REGISTRY_URL` 可访问（DD §5.5.1）。
pub use crate::settings::OFFICIAL_SKILL_REGISTRY_URL;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSkillSummary {
    pub slug: String,
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub icon: Option<String>,
    pub path: String,
    #[serde(default)]
    pub installed: bool,
}

/// check_updates 结果项（DD §8.4 wire 形状 {slug, updateAvailable, remoteVersion}）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RegistrySkillUpdate {
    pub slug: String,
    pub update_available: bool,
    pub remote_version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SkillRegistryIndex {
    #[serde(default)]
    skills: Vec<RemoteSkillSummary>,
}

pub fn fetch_index(
    ctx: &AppContext,
    registry_url: &str,
) -> Result<Vec<RemoteSkillSummary>, String> {
    let source = normalize_github_url(registry_url)?;
    fetch_index_from_source(ctx, &source)
}

/// 只读本地缓存的注册表索引，不触发拉取（cache-first 语义用，镜像
/// workflow_registry::read_cached_index）。无可用缓存（文件缺失/内容损坏/
/// settings 不可读）时返回 None，调用方应回退 fetch_index。
pub fn read_cached_index(ctx: &AppContext) -> Option<Vec<RemoteSkillSummary>> {
    let text = fs::read_to_string(current_dir(ctx).join("index.json")).ok()?;
    parse_index(ctx, &text).ok()
}

/// 下载注册表 skill 到中心库 `library_path/<slug>/` 并写 lock 条目；slug 以
/// index 条目为准。同 slug 异源（lock 记录来源归一化后不等）→ 冲突拒绝，不做
/// 静默换源（换源走「先删后下」两步，前端引导）。返回 slug。
pub fn download_skill(ctx: &AppContext, registry_url: &str, path: &str) -> Result<String, String> {
    let source = normalize_github_url(registry_url)?;
    download_skill_from_source(ctx, &source, path)
}

/// 批量更新检查（DD §5.5.5 设计例外：避免逐 skill 整库 clone N 次）：筛 lock 中
/// 来源归一化等于 settings.skillRegistryUrl 的条目 → fetch_index 一次 → 逐条目
/// hash_dir(library/<slug>) vs hash_dir(current/{path})。
pub fn check_updates(ctx: &AppContext) -> Result<Vec<RegistrySkillUpdate>, String> {
    let registry_url = skill_registry_url(ctx)?;
    let tracked: Vec<String> = read_lock_entries(ctx)?
        .into_iter()
        .filter(|(slug, entry)| {
            is_safe_slug(slug)
                && entry
                    .source_url
                    .as_deref()
                    .is_some_and(|url| crate::github_auth::is_official_repo(url, &registry_url))
        })
        .map(|(slug, _entry)| slug)
        .collect();
    // 无当前注册表跟踪条目 → 不拉取直接空（离线友好）。
    if tracked.is_empty() {
        return Ok(Vec::new());
    }

    let summaries = fetch_index(ctx, &registry_url)?;
    let library_root = library_root(ctx)?;
    let mut updates = Vec::new();
    for slug in tracked {
        // 门-F-12 镜像：注册表已无该条目 → 不再检查。
        let Some(entry) = summaries.iter().find(|item| item.slug == slug) else {
            continue;
        };
        let Ok(guarded) = guard_registry_path(&entry.path) else {
            continue;
        };
        // 本地目录缺失/不可读、缓存目录不可读 → 跳过（无判定证据）。
        let (Ok(local_hash), Ok(remote_hash)) = (
            hash_dir(&library_root.join(&slug)),
            hash_dir(&current_dir(ctx).join(guarded)),
        ) else {
            continue;
        };
        updates.push(RegistrySkillUpdate {
            slug,
            update_available: local_hash != remote_hash,
            remote_version: Some(entry.version.clone()),
        });
    }
    Ok(updates)
}

/// 执行更新（备份 `backups/skill-registry-updates/{UTC ts}/{slug}` → 删 → 重建
/// → lock.updatedAt 刷新）。跨源拒绝：lock 来源与当前注册表不归一化相等时不用
/// 注册表缓存覆盖他人来源的安装。内容已一致时幂等返回，不产备份。
pub fn apply_update(ctx: &AppContext, slug: &str) -> Result<(), String> {
    if !is_safe_slug(slug) {
        return Err(format!(
            "Invalid skill slug '{slug}': must be non-empty and match [a-z0-9-]+"
        ));
    }
    let registry_url = skill_registry_url(ctx)?;
    let mut skills = read_lock_entries(ctx)?;
    let Some(entry) = skills.get(slug) else {
        return Err(format!(
            "Skill '{slug}' has no lock entry; nothing to update from"
        ));
    };
    let tracked = entry
        .source_url
        .as_deref()
        .is_some_and(|url| crate::github_auth::is_official_repo(url, &registry_url));
    if !tracked {
        return Err(format!(
            "Skill '{slug}' is not tracked from the current skill registry; refusing to update"
        ));
    }

    let summaries = fetch_index(ctx, &registry_url)?;
    let Some(index_entry) = summaries.iter().find(|item| item.slug == slug) else {
        return Err(format!(
            "Skill '{slug}' is not present in the current skill registry; unable to update"
        ));
    };
    let local_dir = library_root(ctx)?.join(slug);
    let remote_dir = current_dir(ctx).join(guard_registry_path(&index_entry.path)?);

    // 幂等：内容已一致 → 不产备份不动文件。
    let local_hash = hash_dir(&local_dir)
        .map_err(|error| format!("Unable to hash local skill '{slug}': {error}"))?;
    let remote_hash = hash_dir(&remote_dir)?;
    if local_hash == remote_hash {
        return Ok(());
    }

    let stamp = Utc::now().format("%Y%m%dT%H%M%SZ");
    let backup = ctx
        .data_dir()
        .join("backups")
        .join("skill-registry-updates")
        .join(stamp.to_string())
        .join(slug);
    copy_dir_recursive(&local_dir, &backup)?;
    remove_entry(&local_dir)?;
    copy_dir_recursive(&remote_dir, &local_dir)?;

    // lock.updatedAt 刷新（同一份读-改-写；其余字段不动）。
    if let Some(entry) = skills.get_mut(slug) {
        entry.updated_at = Some(Utc::now().to_rfc3339());
    }
    write_lock_entries(ctx, &skills)
}

// --- core implementations -------------------------------------------------
// `*_from_source` 变体逐字使用 clone 来源（不经 GitHub-only 归一化），与
// workflow_registry 同模式：除公开 API 归一化后调用外，兼作单测钩子（本地
// fixture git 仓库当来源，全链路零网络）。

fn fetch_index_from_source(
    ctx: &AppContext,
    source: &str,
) -> Result<Vec<RemoteSkillSummary>, String> {
    refresh_cache(ctx, source)?;
    read_current_index(ctx)
}

fn download_skill_from_source(
    ctx: &AppContext,
    source: &str,
    path: &str,
) -> Result<String, String> {
    // 复核归一化（门-L5）：写 lock 的来源恒为归一化 https 形态；逐字来源
    // （本地 fixture 路径）在此拒绝——下载链路没有合法的非归一化来源。
    let locked_source = normalize_github_url(source)?;
    let summaries = fetch_index_from_source(ctx, source)?;
    let entry = summaries
        .iter()
        .find(|item| item.path == path)
        .ok_or_else(|| {
            format!("Skill path '{path}' is not present in the skill registry index")
        })?;
    let slug = entry.slug.clone();
    if !is_safe_slug(&slug) {
        return Err(format!(
            "Refusing to install skill with unsafe slug '{slug}': must match [a-z0-9-]+"
        ));
    }

    // 冲突拒绝（门-L3）：同 slug 既有 lock 条目且来源归一化后不等 → Err。
    let mut skills = read_lock_entries(ctx)?;
    if let Some(existing) = skills.get(&slug) {
        let same_source = existing
            .source_url
            .as_deref()
            .is_some_and(|url| crate::github_auth::is_official_repo(url, &locked_source));
        if !same_source {
            return Err(format!(
                "Skill '{slug}' conflicts with an existing installation from a different source; remove it first"
            ));
        }
    }

    // byte-verbatim 直拷（门-L6）：current/{path} → library/<slug>/，
    // 不增删改任何文件（hash 语义前提）。
    let source_dir = current_dir(ctx).join(guard_registry_path(&entry.path)?);
    let library_root = library_root(ctx)?;
    ensure_dir(&library_root)?;
    let target = library_root.join(&slug);
    if target.exists() {
        remove_entry(&target)?;
    }
    copy_dir_recursive(&source_dir, &target)?;

    // lock 读-改-写：五字段，installedAt 现在、updatedAt None。
    skills.insert(
        slug.clone(),
        SkillLockEntry {
            source: Some(locked_source.clone()),
            source_type: Some("github".to_string()),
            source_url: Some(locked_source),
            skill_path: Some(entry.path.clone()),
            installed_at: Some(Utc::now().to_rfc3339()),
            updated_at: None,
        },
    );
    write_lock_entries(ctx, &skills)?;
    Ok(slug)
}

fn refresh_cache(ctx: &AppContext, source: &str) -> Result<(), String> {
    let root = registry_root(ctx);
    let current = root.join("current");
    let stamp = Utc::now().timestamp_millis();
    let staging = root.join(format!("remote-{stamp}"));
    ensure_dir(&root)?;
    if staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }

    let token = crate::github_auth::resolve_token(ctx);
    if let Err(error) = crate::git_ops::clone_repo_verbatim(source, &staging, token.as_deref()) {
        let _ = fs::remove_dir_all(&staging);
        if current.is_dir() {
            // Pull failed: keep the previous cache and continue with it.
            return Ok(());
        }
        return Err(format!("Unable to clone {source}: {error}"));
    }

    swap_current(&root, &staging, &current, stamp)
}

/// Promote `staging` to `current`. Directory replacement is not atomic on all
/// platforms, so the old cache is renamed aside first and restored on failure.
fn swap_current(root: &Path, staging: &Path, current: &Path, stamp: i64) -> Result<(), String> {
    if !current.exists() {
        return fs::rename(staging, current).map_err(|error| {
            format!(
                "Unable to store skill registry cache at {}: {error}",
                path_to_string(current)
            )
        });
    }

    let backup = root.join(format!("backup-{stamp}"));
    if backup.exists() {
        let _ = fs::remove_dir_all(&backup);
    }
    fs::rename(current, &backup).map_err(|error| {
        format!(
            "Unable to set aside old skill registry cache {}: {error}",
            path_to_string(current)
        )
    })?;
    match fs::rename(staging, current) {
        Ok(()) => {
            let _ = fs::remove_dir_all(&backup);
            Ok(())
        }
        Err(error) => {
            let _ = fs::rename(&backup, current);
            Err(format!("Unable to promote new skill registry cache: {error}"))
        }
    }
}

fn read_current_index(ctx: &AppContext) -> Result<Vec<RemoteSkillSummary>, String> {
    let file = current_dir(ctx).join("index.json");
    let text = fs::read_to_string(&file).map_err(|error| {
        format!(
            "Unable to read skill registry index at {}: {error}",
            path_to_string(&file)
        )
    })?;
    parse_index(ctx, &text).map_err(|error| {
        format!(
            "Unable to parse skill registry index at {}: {error}",
            path_to_string(&file)
        )
    })
}

/// fetch_index 与 read_cached_index 共用的装配逻辑：解析 index.json 文本，
/// 并对照中心库 `library_path/<slug>` 现算各条目的 installed 标记。
fn parse_index(ctx: &AppContext, text: &str) -> Result<Vec<RemoteSkillSummary>, String> {
    let index: SkillRegistryIndex =
        serde_json::from_str(text).map_err(|error| format!("{error}"))?;
    let installed_root = library_root(ctx)?;
    Ok(index
        .skills
        .into_iter()
        .map(|mut summary| {
            summary.installed =
                is_safe_slug(&summary.slug) && installed_root.join(&summary.slug).is_dir();
            summary
        })
        .collect())
}

fn registry_root(ctx: &AppContext) -> PathBuf {
    ctx.data_dir().join("skill-registry")
}

fn current_dir(ctx: &AppContext) -> PathBuf {
    registry_root(ctx).join("current")
}

/// 中心库根（settings.library_path；load_settings 已保证非空回填）。
fn library_root(ctx: &AppContext) -> Result<PathBuf, String> {
    Ok(PathBuf::from(
        crate::settings::load_settings(ctx)?.library_path,
    ))
}

/// load_settings 已保证空值回填官方缺省；此处兜底仅为避免解包 panic。
/// （与 commands.rs / web routes / workflow_update 的私有助手同规则。）
fn skill_registry_url(ctx: &AppContext) -> Result<String, String> {
    Ok(crate::settings::load_settings(ctx)?
        .skill_registry_url
        .filter(|url| !url.trim().is_empty())
        .unwrap_or_else(|| OFFICIAL_SKILL_REGISTRY_URL.to_string()))
}

/// skill lock 落点 `~/.agents/.skill-lock.json`。`~` 以 ctx.home_dir() 展开——
/// 生产两壳 ctx 均由 fs_ops::home_dir() 构造（tauri app_context / oms-web
/// from_env），与 skill_ops::read_skill_lock 的 expand_home（进程 home）生产
/// 恒等（jail.rs::PathJail::new 同款先例）；测试 thus 可用 fake home，避免多
/// 模块并发重定向进程 HOME 环境变量的竞态与真实 lock 文件污染。
fn lock_path(ctx: &AppContext) -> PathBuf {
    ctx.home_dir().join(".agents").join(".skill-lock.json")
}

/// 读 lock 条目（与 skill_ops::read_skill_lock 同语义：文件缺失 → 空表；
/// 内容损坏 → Err，fail-closed 不静默覆盖）。
fn read_lock_entries(ctx: &AppContext) -> Result<BTreeMap<String, SkillLockEntry>, String> {
    let path = lock_path(ctx);
    let Ok(text) = fs::read_to_string(&path) else {
        return Ok(BTreeMap::new());
    };
    let lock = serde_json::from_str::<SkillLockFile>(&text).map_err(|error| {
        format!(
            "Unable to parse skill lock {}: {error}",
            path_to_string(&path)
        )
    })?;
    Ok(lock.skills)
}

fn write_lock_entries(
    ctx: &AppContext,
    skills: &BTreeMap<String, SkillLockEntry>,
) -> Result<(), String> {
    let path = lock_path(ctx);
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    let lock = SkillLockFile {
        skills: skills.clone(),
    };
    let text = serde_json::to_string_pretty(&lock)
        .map_err(|error| format!("Unable to serialize skill lock: {error}"))?;
    fs::write(&path, text).map_err(|error| {
        format!(
            "Unable to write skill lock {}: {error}",
            path_to_string(&path)
        )
    })
}

/// Registry entry paths are repo-relative (e.g. `skills/beta-skill`); reject
/// anything that could escape the cache directory.
/// （与 workflow_registry::guard_registry_path 同规则拷贝——原函数私有，DD §5.5.2。）
fn guard_registry_path(path: &str) -> Result<&str, String> {
    if path.trim().is_empty() {
        return Err("Skill registry path must not be empty".to_string());
    }
    let candidate = Path::new(path);
    if candidate
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
    {
        Ok(path)
    } else {
        Err(format!(
            "Invalid skill registry path '{path}': only relative path segments are allowed"
        ))
    }
}

/// （与 workflow_registry::is_safe_slug 同规则拷贝，DD §5.5.2。）
fn is_safe_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// 测试共享 fixture（本模块单测与 web 端点 oneshot 复用）。造数据复刻真实
/// 流程：本地 fixture skill 注册表 git 仓库 → 真 clone 铺缓存 → 公开 API
/// （settings 指向必然 clone 失败的 GitHub 形态 URL，走生产级离线回退读预置
/// 缓存）下载 → 检查 → 更新；禁预置期望终态。
#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use std::process::Command;

    /// settings 指向此 URL：GitHub 形态（过 normalize）但仓库不存在，
    /// 有无网络 clone 都失败 → 离线回退到预 clone 的缓存。结果确定性，
    /// 速度取决于有无网络（秒级以内）。
    pub(crate) const UNCLONEABLE_URL: &str =
        "https://github.com/oms-fixture/nonexistent-skills-repo-000.git";

    pub(crate) const ALPHA_SKILL_MD_V1: &str =
        "---\nname: alpha-skill\ndescription: 测试 alpha\n---\n# Alpha\n";
    pub(crate) const ALPHA_SKILL_MD_V2: &str =
        "---\nname: alpha-skill\ndescription: 测试 alpha v2\n---\n# Alpha v2\n";
    pub(crate) const ALPHA_NOTES_V2: &str = "# Alpha v2 notes\n";
    const ALPHA_SCRIPT: &str = "#!/bin/sh\necho alpha\n";
    const BETA_SKILL_MD: &str = "---\nname: beta-skill\ndescription: 测试 beta\n---\n# Beta\n";

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

    /// 本地 fixture skill 注册表仓库（v1 已提交：index + alpha 嵌套路径目录
    /// + beta 顶层目录）。TempDir 须由调用方持有存活。
    pub(crate) fn fixture_repo() -> tempfile::TempDir {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).expect("repo dir");
        git(&repo, &["init"]);
        write_v1(&repo);
        commit_fixture(&repo, "fixture v1");
        temp
    }

    pub(crate) fn repo_source(fixture: &tempfile::TempDir) -> String {
        path_to_string(&fixture.path().join("repo"))
    }

    /// 写 v1：index（alpha 全字段嵌套路径 + beta 最小字段顶层路径）+ 两目录。
    pub(crate) fn write_v1(repo: &Path) {
        write_content(repo, ALPHA_SKILL_MD_V1, "0.1.0", false);
    }

    /// 发布 alpha v2：SKILL.md 改 + 新增 NOTES.md + index version bump。
    pub(crate) fn publish_alpha_v2(repo: &Path) {
        write_content(repo, ALPHA_SKILL_MD_V2, "0.2.0", true);
    }

    fn write_content(repo: &Path, alpha_md: &str, alpha_version: &str, with_notes: bool) {
        fs::write(
            repo.join("index.json"),
            format!(
                concat!(
                    "{{\"version\":1,\"skills\":[",
                    "{{\"slug\":\"alpha-skill\",\"name\":\"Alpha Skill\",\"version\":\"{}\",",
                    "\"description\":\"测试 alpha\",\"author\":\"tester\",\"tags\":[\"alpha\"],",
                    "\"icon\":\"spark\",\"path\":\"skills/alpha-skill\"}},",
                    "{{\"slug\":\"beta-skill\",\"name\":\"Beta Skill\",\"version\":\"0.5.0\",",
                    "\"description\":\"测试 beta\",\"path\":\"beta-skill\"}}",
                    "]}}"
                ),
                alpha_version
            ),
        )
        .expect("write index");
        let alpha = repo.join("skills").join("alpha-skill");
        fs::create_dir_all(alpha.join("scripts")).expect("alpha dirs");
        fs::write(alpha.join("SKILL.md"), alpha_md).expect("alpha SKILL.md");
        fs::write(alpha.join("scripts").join("run.sh"), ALPHA_SCRIPT).expect("alpha script");
        if with_notes {
            fs::write(alpha.join("NOTES.md"), ALPHA_NOTES_V2).expect("alpha notes");
        }
        let beta = repo.join("beta-skill");
        fs::create_dir_all(&beta).expect("beta dir");
        fs::write(beta.join("SKILL.md"), BETA_SKILL_MD).expect("beta SKILL.md");
    }

    /// 把 fixture 仓库真 clone 进 skill 注册表缓存（复刻 refresh_cache 的
    /// 产物形态：`skill-registry/current` = clone 结果；重 clone 前先移除旧缓存）。
    pub(crate) fn seed_cache_from_fixture(ctx: &AppContext, source: &str) {
        let current = current_dir(ctx);
        ensure_dir(&registry_root(ctx)).expect("registry root");
        if current.exists() {
            remove_entry(&current).expect("remove old cache");
        }
        crate::git_ops::clone_repo_verbatim(source, &current, None).expect("clone fixture");
    }

    /// settings 指向 UNCLONEABLE_URL（核心 save_settings 不做 URL 校验，
    /// 校验在 save_settings_with_merge）。
    pub(crate) fn point_registry_at_uncloneable_url(ctx: &AppContext) {
        let mut settings = crate::settings::default_settings(ctx).expect("default settings");
        settings.skill_registry_url = Some(UNCLONEABLE_URL.to_string());
        crate::settings::save_settings(ctx, &settings).expect("save settings");
    }

    /// 真实流程一段式：seed 缓存 → settings 指向不可 clone URL → 公开
    /// download_skill（离线回退链路）安装 alpha-skill。
    pub(crate) fn install_alpha_via_public_api(ctx: &AppContext, source: &str) -> String {
        seed_cache_from_fixture(ctx, source);
        point_registry_at_uncloneable_url(ctx);
        download_skill(ctx, UNCLONEABLE_URL, "skills/alpha-skill").expect("download alpha")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_support::*;

    fn test_ctx(temp: &tempfile::TempDir) -> AppContext {
        AppContext::new(temp.path().join("data"), temp.path().join("home"))
    }

    fn library_skill(ctx: &AppContext, slug: &str) -> PathBuf {
        library_root(ctx).expect("library root").join(slug)
    }

    fn cache_alpha_dir(ctx: &AppContext) -> PathBuf {
        current_dir(ctx).join("skills").join("alpha-skill")
    }

    fn alpha_lock_entry(ctx: &AppContext) -> SkillLockEntry {
        read_lock_entries(ctx)
            .expect("lock")
            .get("alpha-skill")
            .cloned()
            .expect("alpha lock entry")
    }

    #[test]
    fn read_cached_index_serves_cache_without_pulling() {
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        assert_eq!(read_cached_index(&ctx), None);

        // 手工铺缓存（不经 git clone）：current/index.json + 一个中心库已装 skill
        let current = current_dir(&ctx);
        fs::create_dir_all(&current).expect("cache dir");
        fs::write(
            current.join("index.json"),
            concat!(
                "{\"version\":1,\"skills\":[",
                "{\"slug\":\"alpha-skill\",\"name\":\"Alpha\",\"version\":\"0.1.0\",",
                "\"description\":\"a\",\"path\":\"skills/alpha-skill\"},",
                "{\"slug\":\"beta-skill\",\"name\":\"Beta\",\"version\":\"0.2.0\",",
                "\"description\":\"b\",\"path\":\"beta-skill\"}",
                "]}"
            ),
        )
        .expect("write cached index");
        let installed = library_root(&ctx).expect("library").join("alpha-skill");
        fs::create_dir_all(&installed).expect("installed dir");

        let cached = read_cached_index(&ctx).expect("cached index");
        assert_eq!(cached.len(), 2);
        assert_eq!(cached[0].slug, "alpha-skill");
        assert!(cached[0].installed);
        assert_eq!(cached[1].slug, "beta-skill");
        assert!(!cached[1].installed);

        // 缓存损坏 → None（调用方回退 fetch_index）
        fs::write(current.join("index.json"), "{ not valid json").expect("corrupt");
        assert_eq!(read_cached_index(&ctx), None);
    }

    #[test]
    fn fetch_index_swaps_cache_and_leaves_no_residue() {
        let fixture = fixture_repo();
        let source = repo_source(&fixture);
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);

        let index = fetch_index_from_source(&ctx, &source).expect("v1 index");
        assert_eq!(index.len(), 2);
        assert_eq!(index[0].slug, "alpha-skill");
        assert_eq!(index[0].name, "Alpha Skill");
        assert_eq!(index[0].version, "0.1.0");
        assert_eq!(index[0].author.as_deref(), Some("tester"));
        assert_eq!(index[0].tags, vec!["alpha".to_string()]);
        assert_eq!(index[0].icon.as_deref(), Some("spark"));
        assert_eq!(index[0].path, "skills/alpha-skill");
        assert!(!index[0].installed);
        assert_eq!(index[1].slug, "beta-skill");
        assert_eq!(index[1].path, "beta-skill");
        assert_eq!(index[1].author, None);
        assert!(index[1].tags.is_empty());

        // 发布 alpha v2 并重新拉取：缓存换血（真 clone + swap）。
        let repo = fixture.path().join("repo");
        publish_alpha_v2(&repo);
        commit_fixture(&repo, "alpha v2");
        let index = fetch_index_from_source(&ctx, &source).expect("v2 index");
        assert_eq!(index[0].version, "0.2.0");
        assert_eq!(
            fs::read_to_string(cache_alpha_dir(&ctx).join("SKILL.md")).expect("v2 SKILL.md"),
            ALPHA_SKILL_MD_V2
        );

        // 成功 swap 后无 staging/backup 残留。
        let leftovers: Vec<String> = fs::read_dir(registry_root(&ctx))
            .expect("registry dir")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|name| name != "current")
            .collect();
        assert!(leftovers.is_empty(), "leftovers: {leftovers:?}");
    }

    #[test]
    fn failed_pull_keeps_and_reuses_old_cache() {
        let fixture = fixture_repo();
        let source = repo_source(&fixture);
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);

        let index = fetch_index_from_source(&ctx, &source).expect("seed cache");
        assert_eq!(index.len(), 2);

        // 来源消失：clone 失败，旧缓存继续服务（离线回退）。
        let missing = path_to_string(&fixture.path().join("does-not-exist"));
        let index = fetch_index_from_source(&ctx, &missing).expect("fallback to cache");
        assert_eq!(index.len(), 2);
        assert_eq!(index[0].slug, "alpha-skill");

        // 无任何缓存时 clone 失败即错误。
        let empty_temp = tempfile::tempdir().expect("temp dir");
        let empty_ctx = test_ctx(&empty_temp);
        let error =
            fetch_index_from_source(&empty_ctx, &missing).expect_err("no cache must fail");
        assert!(error.contains("Unable to clone"), "error: {error}");
    }

    #[test]
    fn malformed_index_returns_error_instead_of_panicking() {
        let temp = tempfile::tempdir().expect("fixture temp");
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).expect("repo dir");
        git(&repo, &["init"]);
        fs::write(repo.join("index.json"), "{ not valid json").expect("bad index");
        commit_fixture(&repo, "broken index");

        let ctx_temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&ctx_temp);
        let error = fetch_index_from_source(&ctx, &path_to_string(&repo))
            .expect_err("broken index must fail");
        assert!(
            error.contains("Unable to parse skill registry index"),
            "error: {error}"
        );
    }

    #[test]
    fn download_full_flow_byte_verbatim_and_lock_fields() {
        let fixture = fixture_repo();
        let source = repo_source(&fixture);
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        seed_cache_from_fixture(&ctx, &source);
        point_registry_at_uncloneable_url(&ctx);

        // 以 ssh 形态 URL 下载（归一化后与 UNCLONEABLE_URL 同仓库）：
        // clone 必失败 → 离线回退读缓存；lock 必须写归一化 https 形态（门-L5）。
        let slug = download_skill(
            &ctx,
            "git@github.com:oms-fixture/nonexistent-skills-repo-000",
            "skills/alpha-skill",
        )
        .expect("download alpha");
        assert_eq!(slug, "alpha-skill");

        // byte-verbatim（门-L6）：下载完成立即 hash_dir 比对缓存目录相等。
        let installed = library_skill(&ctx, "alpha-skill");
        assert!(installed.join("SKILL.md").is_file());
        assert!(installed.join("scripts").join("run.sh").is_file());
        assert_eq!(
            hash_dir(&installed).expect("installed hash"),
            hash_dir(&cache_alpha_dir(&ctx)).expect("cache hash"),
            "安装目录必须与注册表缓存逐字节一致"
        );

        // lock 五字段全对，source/sourceUrl 为归一化 https 形态。
        let entry = alpha_lock_entry(&ctx);
        assert_eq!(entry.source.as_deref(), Some(UNCLONEABLE_URL));
        assert_eq!(entry.source_url.as_deref(), Some(UNCLONEABLE_URL));
        assert_eq!(entry.source_type.as_deref(), Some("github"));
        assert_eq!(entry.skill_path.as_deref(), Some("skills/alpha-skill"));
        assert!(entry.installed_at.is_some());
        assert_eq!(entry.updated_at, None);

        // installed 现算：再次拉取（离线回退）→ alpha 翻转 installed=true。
        let index = fetch_index(&ctx, UNCLONEABLE_URL).expect("fetch index again");
        assert!(index[0].installed);
        assert!(!index[1].installed);
    }

    #[test]
    fn download_rejects_foreign_source_conflict_and_allows_redownload() {
        let fixture = fixture_repo();
        let source = repo_source(&fixture);
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        install_alpha_via_public_api(&ctx, &source);
        let lock_file = ctx.home_dir().join(".agents").join(".skill-lock.json");
        let lock_before = fs::read_to_string(&lock_file).expect("lock before");

        // 同 slug 异源（另一 GitHub 仓库，clone 必失败 → 离线回退同一缓存，
        // 冲突检查先于拷贝生效）→ 拒绝，lock 零副作用。
        let error = download_skill(
            &ctx,
            "https://github.com/someone-else/other-registry.git",
            "skills/alpha-skill",
        )
        .expect_err("foreign source must be rejected");
        assert!(
            error.contains("conflicts with an existing installation"),
            "error: {error}"
        );
        assert_eq!(
            fs::read_to_string(&lock_file).expect("lock after"),
            lock_before
        );
        assert!(library_skill(&ctx, "alpha-skill").join("SKILL.md").is_file());

        // 同源重下载（刷新安装）放行；updatedAt 仍为 None。
        let slug =
            download_skill(&ctx, UNCLONEABLE_URL, "skills/alpha-skill").expect("re-download");
        assert_eq!(slug, "alpha-skill");
        assert_eq!(alpha_lock_entry(&ctx).updated_at, None);
    }

    #[test]
    fn download_rejects_unsafe_slug_and_traversing_registry_path() {
        let temp = tempfile::tempdir().expect("fixture temp");
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).expect("repo dir");
        git(&repo, &["init"]);
        fs::write(
            repo.join("index.json"),
            concat!(
                "{\"version\":1,\"skills\":[",
                "{\"slug\":\"../escape\",\"name\":\"Evil\",\"version\":\"1.0.0\",",
                "\"description\":\"evil\",\"path\":\"evil\"},",
                "{\"slug\":\"trav-skill\",\"name\":\"Trav\",\"version\":\"1.0.0\",",
                "\"description\":\"trav\",\"path\":\"../outside\"}",
                "]}"
            ),
        )
        .expect("write index");
        let evil = repo.join("evil");
        fs::create_dir_all(&evil).expect("evil dir");
        fs::write(evil.join("SKILL.md"), "---\nname: evil\n---\n").expect("evil SKILL.md");
        commit_fixture(&repo, "evil fixture");

        let ctx_temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&ctx_temp);
        seed_cache_from_fixture(&ctx, &path_to_string(&repo));
        point_registry_at_uncloneable_url(&ctx);

        // slug 含穿越段 → slug 安检拒绝（先于任何拷贝）。
        let error = download_skill(&ctx, UNCLONEABLE_URL, "evil").expect_err("unsafe slug");
        assert!(error.contains("unsafe slug"), "error: {error}");

        // index path 穿越出缓存 → guard 拒绝。
        let error = download_skill(&ctx, UNCLONEABLE_URL, "../outside").expect_err("traversal");
        assert!(error.contains("Invalid skill registry path"), "error: {error}");

        // 未在 index 中的路径 → 条目查找拒绝。
        let error = download_skill(&ctx, UNCLONEABLE_URL, "no/such-skill").expect_err("missing");
        assert!(
            error.contains("is not present in the skill registry index"),
            "error: {error}"
        );

        assert!(read_lock_entries(&ctx).expect("lock").is_empty());
        assert!(!library_root(&ctx).expect("library").join("trav-skill").exists());
    }

    #[test]
    fn path_and_slug_guards_match_registry_rules() {
        for path in ["..", "../outside", "/abs", "a/../../b", "", "."] {
            assert!(guard_registry_path(path).is_err(), "path '{path}'");
        }
        for path in ["alpha-skill", "skills/alpha-skill", "a/b/c"] {
            assert!(guard_registry_path(path).is_ok(), "path '{path}'");
        }
        for slug in ["", "UPPER", "a/b", "../x", "has space", "under_score"] {
            assert!(!is_safe_slug(slug), "slug '{slug}'");
        }
        for slug in ["a", "alpha-skill", "s0-many-hyphens-123"] {
            assert!(is_safe_slug(slug), "slug '{slug}'");
        }
    }

    #[test]
    fn check_updates_covers_current_available_and_local_drift() {
        let fixture = fixture_repo();
        let source = repo_source(&fixture);
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);

        install_alpha_via_public_api(&ctx, &source);
        download_skill(&ctx, UNCLONEABLE_URL, "beta-skill").expect("download beta");

        // 手工加两条 lock：异源条目（应被筛掉）+ 同源但注册表无条目（门-F-12
        // 镜像，应跳过）。
        let mut lock = read_lock_entries(&ctx).expect("lock");
        let foreign = SkillLockEntry {
            source: Some("https://github.com/other/repo.git".to_string()),
            source_type: Some("github".to_string()),
            source_url: Some("https://github.com/other/repo.git".to_string()),
            skill_path: Some("foreign-skill".to_string()),
            installed_at: Some("2026-01-01T00:00:00Z".to_string()),
            updated_at: None,
        };
        lock.insert("foreign-skill".to_string(), foreign.clone());
        lock.insert(
            "ghost-skill".to_string(),
            SkillLockEntry {
                skill_path: Some("ghost-skill".to_string()),
                source_url: Some(UNCLONEABLE_URL.to_string()),
                ..foreign
            },
        );
        write_lock_entries(&ctx, &lock).expect("write lock");

        // 全量 current（异源与幽灵条目均不出现；结果按 slug 序——BTreeMap）。
        let updates = check_updates(&ctx).expect("check");
        assert_eq!(updates.len(), 2);
        assert_eq!(updates[0].slug, "alpha-skill");
        assert!(!updates[0].update_available);
        assert_eq!(updates[0].remote_version.as_deref(), Some("0.1.0"));
        assert_eq!(updates[1].slug, "beta-skill");
        assert!(!updates[1].update_available);
        assert_eq!(updates[1].remote_version.as_deref(), Some("0.5.0"));

        // 发布 alpha v2 并刷新缓存 → available（remoteVersion 反映 index）。
        let repo = fixture.path().join("repo");
        publish_alpha_v2(&repo);
        commit_fixture(&repo, "alpha v2");
        seed_cache_from_fixture(&ctx, &source);
        let updates = check_updates(&ctx).expect("check v2");
        assert!(updates[0].update_available);
        assert_eq!(updates[0].remote_version.as_deref(), Some("0.2.0"));
        assert!(!updates[1].update_available);

        // 本地被改：hash 漂移 → 与「有更新」同形（updateAvailable=true，
        // DD §5.5.5 hash 两态语义不做三态区分）。
        fs::write(
            library_skill(&ctx, "beta-skill").join("SKILL.md"),
            "# 本地改动\n",
        )
        .expect("local edit");
        let updates = check_updates(&ctx).expect("check drift");
        assert!(updates[1].update_available);
        assert_eq!(updates[1].remote_version.as_deref(), Some("0.5.0"));
    }

    #[test]
    fn check_updates_skips_missing_local_dirs() {
        let fixture = fixture_repo();
        let source = repo_source(&fixture);
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        install_alpha_via_public_api(&ctx, &source);

        remove_entry(&library_skill(&ctx, "alpha-skill")).expect("remove local");
        assert_eq!(check_updates(&ctx).expect("check"), Vec::new());
    }

    #[test]
    fn apply_update_backs_up_then_updates_and_refreshes_lock() {
        let fixture = fixture_repo();
        let source = repo_source(&fixture);
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        install_alpha_via_public_api(&ctx, &source);
        let pre_hash = hash_dir(&library_skill(&ctx, "alpha-skill")).expect("pre hash");
        let before = alpha_lock_entry(&ctx);
        assert_eq!(before.updated_at, None);

        // 发布 v2 并刷新缓存（更新执行本身仍走离线回退：clone 必失败，
        // 读已刷新缓存）。
        let repo = fixture.path().join("repo");
        publish_alpha_v2(&repo);
        commit_fixture(&repo, "alpha v2");
        seed_cache_from_fixture(&ctx, &source);

        apply_update(&ctx, "alpha-skill").expect("apply update");

        // 更新后本地 hash == 注册表缓存 hash（byte-verbatim 同下载约束）。
        assert_eq!(
            hash_dir(&library_skill(&ctx, "alpha-skill")).expect("post hash"),
            hash_dir(&cache_alpha_dir(&ctx)).expect("cache hash")
        );
        assert_eq!(
            fs::read_to_string(library_skill(&ctx, "alpha-skill").join("NOTES.md"))
                .expect("notes"),
            ALPHA_NOTES_V2
        );

        // 备份产生且内容等于更新前。
        let backup_root = ctx.data_dir().join("backups").join("skill-registry-updates");
        let stamps: Vec<_> = fs::read_dir(&backup_root)
            .expect("backup root")
            .filter_map(Result::ok)
            .collect();
        assert_eq!(stamps.len(), 1, "expected exactly one backup stamp");
        let backup = stamps[0].path().join("alpha-skill");
        assert_eq!(hash_dir(&backup).expect("backup hash"), pre_hash);

        // lock.updatedAt 刷新，其余字段不动。
        let after = alpha_lock_entry(&ctx);
        assert!(after.updated_at.is_some());
        assert_eq!(after.source_url, before.source_url);
        assert_eq!(after.skill_path, before.skill_path);
        assert_eq!(after.installed_at, before.installed_at);
    }

    #[test]
    fn apply_update_guards_reject_bad_slugs_unknown_and_foreign_entries() {
        let fixture = fixture_repo();
        let source = repo_source(&fixture);
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        install_alpha_via_public_api(&ctx, &source);

        for slug in ["..", "../settings", "a/b", "", "UPPER"] {
            let error = apply_update(&ctx, slug).expect_err("bad slug must fail");
            assert!(error.contains("Invalid skill slug"), "slug '{slug}': {error}");
        }

        let error = apply_update(&ctx, "never-installed").expect_err("no lock entry");
        assert!(error.contains("no lock entry"), "error: {error}");

        // 异源条目 → 跨源拒绝（不用注册表缓存覆盖他人来源的安装）。
        let mut lock = read_lock_entries(&ctx).expect("lock");
        lock
            .get_mut("alpha-skill")
            .expect("entry")
            .source_url = Some("https://github.com/other/repo.git".to_string());
        write_lock_entries(&ctx, &lock).expect("write lock");
        let error = apply_update(&ctx, "alpha-skill").expect_err("foreign source");
        assert!(
            error.contains("not tracked from the current skill registry"),
            "error: {error}"
        );

        // 恢复同源 → 已是最新幂等：Ok 且不产备份。
        let mut lock = read_lock_entries(&ctx).expect("lock");
        lock
            .get_mut("alpha-skill")
            .expect("entry")
            .source_url = Some(UNCLONEABLE_URL.to_string());
        write_lock_entries(&ctx, &lock).expect("restore lock");
        apply_update(&ctx, "alpha-skill").expect("idempotent");
        assert!(!ctx
            .data_dir()
            .join("backups")
            .join("skill-registry-updates")
            .exists());
    }
}
