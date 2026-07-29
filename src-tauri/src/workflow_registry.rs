//! Workflow registry client: pulls the remote workflow registry (git clone
//! --depth 1) into a local cache under `data_dir/registry/` and serves index /
//! workflow reads plus downloads into `data_dir/workflows/`.
//!
//! Pull strategy: clone into a staging dir `remote-<ts>`, then swap it into
//! `current` via renames; on any failure the previous `current` cache is kept
//! and reused (offline tolerance).

use crate::context::AppContext;
use crate::fs_ops::{copy_dir_recursive, ensure_dir, path_to_string, remove_entry};
use crate::skill_ops::normalize_github_url;
use crate::workflow::{Workflow, WORKFLOW_FILE, WORKFLOW_README, workflows_dir};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteWorkflowSummary {
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

#[derive(Debug, Deserialize)]
struct RegistryIndex {
    #[serde(default)]
    workflows: Vec<RemoteWorkflowSummary>,
}

pub fn fetch_index(
    ctx: &AppContext,
    registry_url: &str,
) -> Result<Vec<RemoteWorkflowSummary>, String> {
    let source = normalize_github_url(registry_url)?;
    fetch_index_from_source(ctx, &source)
}

/// 只读本地缓存的注册表索引，不触发拉取（批次 4 追加，cache-first 语义用）。
/// 无可用缓存（文件缺失或内容损坏）时返回 None，调用方应回退 fetch_index。
pub fn read_cached_index(ctx: &AppContext) -> Option<Vec<RemoteWorkflowSummary>> {
    let text = fs::read_to_string(current_dir(ctx).join("index.json")).ok()?;
    parse_index(ctx, &text).ok()
}

pub fn fetch_workflow(
    ctx: &AppContext,
    registry_url: &str,
    path: &str,
) -> Result<(Workflow, Option<String>), String> {
    let source = normalize_github_url(registry_url)?;
    fetch_workflow_from_source(ctx, &source, path)
}

/// Download a registry workflow into `data_dir/workflows/<slug>/`; the slug is
/// authoritative from the workflow.yaml content, not from the index path.
/// An existing installation with the same slug is replaced. Returns the slug.
pub fn download_to_installed(
    ctx: &AppContext,
    registry_url: &str,
    path: &str,
) -> Result<String, String> {
    let source = normalize_github_url(registry_url)?;
    download_to_installed_from_source(ctx, &source, path)
}

// --- core implementations -------------------------------------------------
// The `*_from_source` variants take the clone source verbatim (no GitHub-only
// normalization). Besides being called by the public API after normalization,
// they double as the test hook: unit tests pass local fixture git repositories
// so the full fetch/download flow runs without network access.

fn fetch_index_from_source(
    ctx: &AppContext,
    source: &str,
) -> Result<Vec<RemoteWorkflowSummary>, String> {
    refresh_cache(ctx, source)?;
    read_current_index(ctx)
}

fn fetch_workflow_from_source(
    ctx: &AppContext,
    source: &str,
    path: &str,
) -> Result<(Workflow, Option<String>), String> {
    refresh_cache(ctx, source)?;
    read_current_workflow(ctx, path)
}

fn download_to_installed_from_source(
    ctx: &AppContext,
    source: &str,
    path: &str,
) -> Result<String, String> {
    let (workflow, _readme) = fetch_workflow_from_source(ctx, source, path)?;
    let slug = workflow.slug;
    if !is_safe_slug(&slug) {
        return Err(format!(
            "Refusing to install workflow with unsafe slug '{slug}': must match [a-z0-9-]+"
        ));
    }

    let source_dir = current_dir(ctx).join(guard_registry_path(path)?);
    let target_root = workflows_dir(ctx);
    ensure_dir(&target_root)?;
    let target = target_root.join(&slug);
    if target.exists() {
        remove_entry(&target)?;
    }
    copy_dir_recursive(&source_dir, &target)?;
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

    let status = Command::new("git")
        .args(["clone", "--depth", "1", source])
        .arg(&staging)
        .status()
        .map_err(|error| format!("Unable to clone {source}: {error}"))?;
    if !status.success() {
        let _ = fs::remove_dir_all(&staging);
        if current.is_dir() {
            // Pull failed: keep the previous cache and continue with it.
            return Ok(());
        }
        return Err(format!(
            "Unable to clone {source}: git exited with {status}"
        ));
    }

    swap_current(&root, &staging, &current, stamp)
}

/// Promote `staging` to `current`. Directory replacement is not atomic on all
/// platforms, so the old cache is renamed aside first and restored on failure.
fn swap_current(
    root: &Path,
    staging: &Path,
    current: &Path,
    stamp: i64,
) -> Result<(), String> {
    if !current.exists() {
        return fs::rename(staging, current).map_err(|error| {
            format!(
                "Unable to store registry cache at {}: {error}",
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
            "Unable to set aside old registry cache {}: {error}",
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
            Err(format!("Unable to promote new registry cache: {error}"))
        }
    }
}

fn read_current_index(ctx: &AppContext) -> Result<Vec<RemoteWorkflowSummary>, String> {
    let file = current_dir(ctx).join("index.json");
    let text = fs::read_to_string(&file).map_err(|error| {
        format!(
            "Unable to read registry index at {}: {error}",
            path_to_string(&file)
        )
    })?;
    parse_index(ctx, &text).map_err(|error| {
        format!(
            "Unable to parse registry index at {}: {error}",
            path_to_string(&file)
        )
    })
}

/// fetch_index 与 read_cached_index 共用的装配逻辑：解析 index.json 文本，
/// 并对照 `data_dir/workflows/` 计算各条目的 installed 标记。
fn parse_index(
    ctx: &AppContext,
    text: &str,
) -> Result<Vec<RemoteWorkflowSummary>, serde_json::Error> {
    let index: RegistryIndex = serde_json::from_str(text)?;
    let installed_root = workflows_dir(ctx);
    Ok(index
        .workflows
        .into_iter()
        .map(|mut summary| {
            summary.installed =
                is_safe_slug(&summary.slug) && installed_root.join(&summary.slug).is_dir();
            summary
        })
        .collect())
}

fn read_current_workflow(
    ctx: &AppContext,
    path: &str,
) -> Result<(Workflow, Option<String>), String> {
    let dir = current_dir(ctx).join(guard_registry_path(path)?);
    let file = dir.join(WORKFLOW_FILE);
    let text = fs::read_to_string(&file).map_err(|error| {
        format!(
            "Unable to read workflow at {}: {error}",
            path_to_string(&file)
        )
    })?;
    let workflow = Workflow::from_yaml(&text)
        .map_err(|error| format!("{}: {error}", path_to_string(&file)))?;
    let readme = fs::read_to_string(dir.join(WORKFLOW_README)).ok();
    Ok((workflow, readme))
}

fn registry_root(ctx: &AppContext) -> PathBuf {
    ctx.data_dir().join("registry")
}

fn current_dir(ctx: &AppContext) -> PathBuf {
    registry_root(ctx).join("current")
}

/// Registry entry paths are repo-relative (e.g. `flows/beta-flow`); reject
/// anything that could escape the cache directory.
fn guard_registry_path(path: &str) -> Result<&str, String> {
    if path.trim().is_empty() {
        return Err("Registry path must not be empty".to_string());
    }
    let candidate = Path::new(path);
    if candidate
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
    {
        Ok(path)
    } else {
        Err(format!(
            "Invalid registry path '{path}': only relative path segments are allowed"
        ))
    }
}

fn is_safe_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ctx(temp: &tempfile::TempDir) -> AppContext {
        AppContext::new(temp.path().join("data"), temp.path().join("home"))
    }

    fn git(repo: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(repo)
            .status()
            .expect("git must run");
        assert!(status.success(), "git {args:?} failed with {status}");
    }

    fn commit_fixture(repo: &Path, message: &str) {
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

    const ALPHA_YAML: &str = "name: Alpha 流程\n\
        slug: alpha-flow\n\
        version: 0.1.0\n\
        description: 测试 alpha\n\
        author: tester\n\
        tags:\n  - alpha\n\
        icon: spark\n\
        groups:\n  - id: g1\n    name: 组一\n\
        steps:\n  - name: 步骤一\n    group: g1\n    skills:\n      - placeholder: 待补充\n";

    const BETA_YAML: &str = "name: Beta\nslug: beta-flow\nversion: 0.2.0\ndescription: 测试 beta\n";

    fn write_fixture_content(repo: &Path) {
        fs::write(
            repo.join("index.json"),
            concat!(
                "{\"version\":1,\"workflows\":[",
                "{\"slug\":\"alpha-flow\",\"name\":\"Alpha 流程\",\"version\":\"0.1.0\",",
                "\"description\":\"测试 alpha\",\"author\":\"tester\",\"tags\":[\"alpha\"],",
                "\"icon\":\"spark\",\"path\":\"alpha-flow\"},",
                "{\"slug\":\"beta-flow\",\"name\":\"Beta\",\"version\":\"0.2.0\",",
                "\"description\":\"测试 beta\",\"path\":\"flows/beta-flow\"}",
                "]}"
            ),
        )
        .expect("write index");
        let alpha = repo.join("alpha-flow");
        fs::create_dir_all(&alpha).expect("alpha dir");
        fs::write(alpha.join(WORKFLOW_FILE), ALPHA_YAML).expect("alpha yaml");
        fs::write(alpha.join(WORKFLOW_README), "# Alpha README").expect("alpha readme");
        let beta = repo.join("flows").join("beta-flow");
        fs::create_dir_all(&beta).expect("beta dir");
        fs::write(beta.join(WORKFLOW_FILE), BETA_YAML).expect("beta yaml");
    }

    /// Build a local git repository holding a registry fixture (index.json +
    /// workflow subdirectories). Returned TempDir keeps the repo alive.
    fn fixture_repo() -> tempfile::TempDir {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).expect("repo dir");
        git(&repo, &["init"]);
        write_fixture_content(&repo);
        commit_fixture(&repo, "fixture v1");
        temp
    }

    fn repo_source(fixture: &tempfile::TempDir) -> String {
        path_to_string(&fixture.path().join("repo"))
    }

    #[test]
    fn read_cached_index_serves_cache_without_pulling() {
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        // 无缓存 → None
        assert_eq!(read_cached_index(&ctx), None);

        // 手工铺缓存（不经 git clone）：current/index.json + 一个已安装工作流目录
        let current = ctx.data_dir().join("registry").join("current");
        fs::create_dir_all(&current).expect("cache dir");
        fs::write(
            current.join("index.json"),
            concat!(
                "{\"version\":1,\"workflows\":[",
                "{\"slug\":\"alpha-flow\",\"name\":\"Alpha\",\"version\":\"0.1.0\",",
                "\"description\":\"a\",\"path\":\"alpha-flow\"},",
                "{\"slug\":\"beta-flow\",\"name\":\"Beta\",\"version\":\"0.2.0\",",
                "\"description\":\"b\",\"path\":\"flows/beta-flow\"}",
                "]}"
            ),
        )
        .expect("write cached index");
        fs::create_dir_all(workflows_dir(&ctx).join("alpha-flow")).expect("installed dir");

        let cached = read_cached_index(&ctx).expect("cached index");
        assert_eq!(cached.len(), 2);
        assert_eq!(cached[0].slug, "alpha-flow");
        assert!(cached[0].installed);
        assert_eq!(cached[1].slug, "beta-flow");
        assert!(!cached[1].installed);

        // 缓存损坏 → None（调用方回退 fetch_index）
        fs::write(current.join("index.json"), "{ not valid json").expect("corrupt");
        assert_eq!(read_cached_index(&ctx), None);
    }

    #[test]
    fn full_flow_fetch_index_workflow_and_download() {
        let fixture = fixture_repo();
        let source = repo_source(&fixture);
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);

        let index = fetch_index_from_source(&ctx, &source).expect("fetch index");
        assert_eq!(index.len(), 2);
        assert_eq!(index[0].slug, "alpha-flow");
        assert_eq!(index[0].name, "Alpha 流程");
        assert_eq!(index[0].version, "0.1.0");
        assert_eq!(index[0].author.as_deref(), Some("tester"));
        assert_eq!(index[0].tags, vec!["alpha".to_string()]);
        assert_eq!(index[0].icon.as_deref(), Some("spark"));
        assert_eq!(index[0].path, "alpha-flow");
        assert_eq!(index[1].slug, "beta-flow");
        assert_eq!(index[1].path, "flows/beta-flow");
        assert_eq!(index[1].author, None);
        assert!(index[1].tags.is_empty());
        assert!(!index[0].installed);
        assert!(!index[1].installed);

        let (alpha, alpha_readme) =
            fetch_workflow_from_source(&ctx, &source, "alpha-flow").expect("fetch alpha");
        assert_eq!(alpha.slug, "alpha-flow");
        assert_eq!(alpha.groups.len(), 1);
        assert_eq!(alpha.steps.len(), 1);
        assert_eq!(alpha_readme.as_deref(), Some("# Alpha README"));

        // Nested registry path (Normal/Normal) is accepted.
        let (beta, beta_readme) =
            fetch_workflow_from_source(&ctx, &source, "flows/beta-flow").expect("fetch beta");
        assert_eq!(beta.slug, "beta-flow");
        assert!(beta.steps.is_empty());
        assert_eq!(beta_readme, None);

        let slug = download_to_installed_from_source(&ctx, &source, "alpha-flow")
            .expect("download alpha");
        assert_eq!(slug, "alpha-flow");
        let installed_dir = workflows_dir(&ctx).join("alpha-flow");
        assert!(installed_dir.join(WORKFLOW_FILE).is_file());
        assert!(installed_dir.join(WORKFLOW_README).is_file());

        // Installed comparison: downloaded workflow flips to installed=true.
        let index = fetch_index_from_source(&ctx, &source).expect("fetch index again");
        assert!(index[0].installed);
        assert!(!index[1].installed);

        // Slug comes from workflow.yaml, not from the index path layout.
        let slug = download_to_installed_from_source(&ctx, &source, "flows/beta-flow")
            .expect("download beta");
        assert_eq!(slug, "beta-flow");
        assert!(workflows_dir(&ctx)
            .join("beta-flow")
            .join(WORKFLOW_FILE)
            .is_file());
    }

    #[test]
    fn download_replaces_existing_installation() {
        let fixture = fixture_repo();
        let source = repo_source(&fixture);
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);

        download_to_installed_from_source(&ctx, &source, "alpha-flow").expect("first download");
        let stale = workflows_dir(&ctx).join("alpha-flow").join("stale.txt");
        fs::write(&stale, "stale").expect("write stale marker");

        download_to_installed_from_source(&ctx, &source, "alpha-flow").expect("re-download");
        assert!(!stale.exists());
        assert!(workflows_dir(&ctx)
            .join("alpha-flow")
            .join(WORKFLOW_FILE)
            .is_file());
    }

    #[test]
    fn refresh_replaces_cache_with_new_registry_content() {
        let fixture = fixture_repo();
        let source = repo_source(&fixture);
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);

        let index = fetch_index_from_source(&ctx, &source).expect("v1 index");
        assert_eq!(index.len(), 2);

        // Publish a v2 of the fixture registry: alpha removed, gamma added.
        let repo = fixture.path().join("repo");
        fs::write(
            repo.join("index.json"),
            concat!(
                "{\"version\":2,\"workflows\":[",
                "{\"slug\":\"gamma-flow\",\"name\":\"Gamma\",\"version\":\"1.0.0\",",
                "\"description\":\"测试 gamma\",\"path\":\"gamma-flow\"}",
                "]}"
            ),
        )
        .expect("write v2 index");
        let gamma = repo.join("gamma-flow");
        fs::create_dir_all(&gamma).expect("gamma dir");
        fs::write(
            gamma.join(WORKFLOW_FILE),
            "name: Gamma\nslug: gamma-flow\nversion: 1.0.0\ndescription: 测试 gamma\n",
        )
        .expect("gamma yaml");
        commit_fixture(&repo, "fixture v2");

        let index = fetch_index_from_source(&ctx, &source).expect("v2 index");
        assert_eq!(index.len(), 1);
        assert_eq!(index[0].slug, "gamma-flow");

        // No leftover staging/backup dirs after successful swaps.
        let registry = ctx.data_dir().join("registry");
        let leftovers: Vec<String> = fs::read_dir(&registry)
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

        // Origin disappears: clone fails, but the old cache must still serve.
        let missing = path_to_string(&fixture.path().join("does-not-exist"));
        let index = fetch_index_from_source(&ctx, &missing).expect("fallback to cache");
        assert_eq!(index.len(), 2);
        assert_eq!(index[0].slug, "alpha-flow");

        // Without any cache, a failed clone is an error.
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
            error.contains("Unable to parse registry index"),
            "error: {error}"
        );
    }

    #[test]
    fn public_api_rejects_non_github_registry_urls_before_cloning() {
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);

        for url in [
            "https://gitlab.com/owner/repo.git",
            "https://example.com/registry",
            "",
        ] {
            let error = fetch_index(&ctx, url).expect_err("non-GitHub url must fail");
            assert!(error.contains("GitHub"), "url '{url}' error: {error}");
            let error = fetch_workflow(&ctx, url, "alpha-flow").expect_err("must fail");
            assert!(error.contains("GitHub"), "url '{url}' error: {error}");
            let error = download_to_installed(&ctx, url, "alpha-flow").expect_err("must fail");
            assert!(error.contains("GitHub"), "url '{url}' error: {error}");
        }

        assert_eq!(
            normalize_github_url("Pgooone/oh-my-skills-workflows").expect("normalize"),
            "https://github.com/Pgooone/oh-my-skills-workflows.git"
        );
        assert_eq!(
            normalize_github_url("https://github.com/Pgooone/oh-my-skills-workflows.git")
                .expect("normalize"),
            "https://github.com/Pgooone/oh-my-skills-workflows.git"
        );
    }

    #[test]
    fn registry_path_guard_rejects_traversal_and_absolute_paths() {
        let fixture = fixture_repo();
        let source = repo_source(&fixture);
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);

        for path in ["..", "../outside", "/abs", "a/../../b", "", "."] {
            let error = fetch_workflow_from_source(&ctx, &source, path)
                .expect_err("path must be rejected");
            assert!(
                error.contains("Invalid registry path") || error.contains("must not be empty"),
                "path '{path}' error: {error}"
            );
        }
    }

    #[test]
    fn download_rejects_unsafe_workflow_slug() {
        let temp = tempfile::tempdir().expect("fixture temp");
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).expect("repo dir");
        git(&repo, &["init"]);
        fs::write(
            repo.join("index.json"),
            concat!(
                "{\"version\":1,\"workflows\":[",
                "{\"slug\":\"evil-flow\",\"name\":\"Evil\",\"version\":\"1.0.0\",",
                "\"description\":\"evil\",\"path\":\"evil-flow\"}",
                "]}"
            ),
        )
        .expect("write index");
        let evil = repo.join("evil-flow");
        fs::create_dir_all(&evil).expect("evil dir");
        // The yaml slug would escape the workflows directory if used verbatim.
        fs::write(
            evil.join(WORKFLOW_FILE),
            "name: Evil\nslug: ../Escape\nversion: 1.0.0\ndescription: evil\n",
        )
        .expect("evil yaml");
        commit_fixture(&repo, "evil fixture");

        let ctx_temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&ctx_temp);
        let error = download_to_installed_from_source(&ctx, &path_to_string(&repo), "evil-flow")
            .expect_err("unsafe slug must fail");
        assert!(error.contains("unsafe slug"), "error: {error}");
        assert!(!workflows_dir(&ctx).exists());
    }

    #[test]
    fn installed_flag_ignores_malformed_index_slugs() {
        let temp = tempfile::tempdir().expect("fixture temp");
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).expect("repo dir");
        git(&repo, &["init"]);
        fs::write(
            repo.join("index.json"),
            concat!(
                "{\"version\":1,\"workflows\":[",
                "{\"slug\":\"../settings\",\"name\":\"Bad\",\"version\":\"1.0.0\",",
                "\"description\":\"bad\",\"path\":\"bad\"}",
                "]}"
            ),
        )
        .expect("write index");
        commit_fixture(&repo, "malicious slug fixture");

        let ctx_temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&ctx_temp);
        let index =
            fetch_index_from_source(&ctx, &path_to_string(&repo)).expect("index still parses");
        assert_eq!(index.len(), 1);
        assert!(!index[0].installed);
    }
}
