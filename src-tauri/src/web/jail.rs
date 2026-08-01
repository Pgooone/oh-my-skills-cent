//! D7 路径白名单（PathJail），按 D7-R1 分两个校验级别。
//!
//! Web 化后路径来自 HTTP 请求，所有接受路径参数的 endpoint 必须先过 jail，
//! 否则例如 `remove_skill_entries` 就是任意文件删除 API。
//!
//! - `check_write`（Tier 2 文件变更类，严格）：路径必须落在允许根集内——
//!   中心库 + 已注册 agent skills 目录 + 项目 roots + 数据目录 + ~/.agents。
//!   适用：remove_skill_entries / update_skills_sh_skill / apply_sync_plan /
//!   check_skills_sh_update。
//! - `check_browse`（Tier 1 注册/浏览类，宽松）：home 子树（含 home 本身）+
//!   允许根集 + Windows 盘符顶层一层（盘符根及其直接子目录，便于跨盘选项目
//!   目录）。适用：list_dir / discover_project_workspaces.basePath /
//!   save_settings.libraryPath——这类操作的本质是注册新位置，严格 jail 会把
//!   合法功能 403 掉，其直接效果仅是读目录列表或写 settings.json。
//!
//! 允许根集在启动时计算，并在 save_settings 成功后由 `AppState::refresh_jail` 刷新。

use crate::context::AppContext;
use crate::fs_ops::path_to_string;
use crate::models::Settings;
use crate::registry;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone)]
pub struct PathJail {
    allowed_roots: Vec<PathBuf>,
    home_dir: PathBuf,
}

impl PathJail {
    /// 从当前 settings 展开允许根集（见模块文档）。`~` 以 `ctx.home_dir()` 展开，
    /// 与生产环境 `fs_ops::expand_home`（进程 home）一致，同时让测试可用 fake home。
    pub fn new(ctx: &AppContext, settings: &Settings) -> Self {
        let home_dir = ctx.home_dir().to_path_buf();
        let mut roots = Vec::new();

        // 中心库
        push_expanded(&mut roots, &settings.library_path, &home_dir);

        // 各 agent 的 global skills 目录
        for agent in registry::known_agents() {
            for root in &agent.global_roots {
                push_expanded(&mut roots, root, &home_dir);
            }
        }

        // project_folders 下各 agent 的 project skills 目录
        for folder in &settings.project_folders {
            let base = expand_with_home(folder, &home_dir);
            for agent in registry::known_agents() {
                for relative in &agent.project_roots {
                    push_normalized(&mut roots, base.join(relative));
                }
            }
        }

        // 自定义根（其 path 本身即 skills 根）
        for custom in &settings.custom_roots {
            push_expanded(&mut roots, &custom.path, &home_dir);
        }

        // 数据目录（plans / backups / updates checkout）
        push_normalized(&mut roots, ctx.data_dir().to_path_buf());

        // ~/.agents（skill lock / skills.sh 更新目标）
        push_normalized(&mut roots, home_dir.join(".agents"));

        Self {
            allowed_roots: roots,
            home_dir,
        }
    }

    /// Tier 2 严格校验（文件变更类）：expand_home → 组件规范化（拒绝 `..`）
    /// → 必须位于某允许根之下。通过时返回规范化后的路径。
    pub fn check_write(&self, raw: &str) -> Result<PathBuf, String> {
        let normalized = self.normalize(raw)?;
        if self
            .allowed_roots
            .iter()
            .any(|root| path_starts_with(&normalized, root))
        {
            Ok(normalized)
        } else {
            Err(format!(
                "Path is outside the allowed roots: {}",
                path_to_string(&normalized)
            ))
        }
    }

    /// Tier 1 宽松校验（D7-R1，注册/浏览类）：home 子树（含 home 本身）+
    /// 允许根集 + Windows 盘符顶层一层。通过时返回规范化后的路径。
    pub fn check_browse(&self, raw: &str) -> Result<PathBuf, String> {
        let normalized = self.normalize(raw)?;
        if path_starts_with(&normalized, &self.home_dir)
            || self
                .allowed_roots
                .iter()
                .any(|root| path_starts_with(&normalized, root))
            || is_drive_top_level(&normalized)
        {
            Ok(normalized)
        } else {
            Err(format!(
                "Path is outside the browsable area: {}",
                path_to_string(&normalized)
            ))
        }
    }

    /// 两级共用的前置处理：expand_home → 拒绝 `..` → 组件规范化。
    fn normalize(&self, raw: &str) -> Result<PathBuf, String> {
        let expanded = expand_with_home(raw, &self.home_dir);
        if expanded
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(format!("Path must not contain '..': {raw}"));
        }
        Ok(normalize_components(&expanded))
    }
}

/// Windows 盘符顶层一层：`X:\`（盘符根）或 `X:\<一个目录名>`（直接子目录）。
/// 只放行浏览/注册，深层路径仍需位于 home 子树或允许根之下。
#[cfg(windows)]
fn is_drive_top_level(path: &Path) -> bool {
    let mut components = path.components();
    if !matches!(components.next(), Some(Component::Prefix(_))) {
        return false;
    }
    if !matches!(components.next(), Some(Component::RootDir)) {
        return false;
    }
    match components.next() {
        // 盘符根本身（如 D:\）
        None => true,
        // 盘符直接子目录（如 D:\Projects）
        Some(Component::Normal(_)) => components.next().is_none(),
        _ => false,
    }
}

#[cfg(not(windows))]
fn is_drive_top_level(_path: &Path) -> bool {
    false
}

/// 与 `fs_ops::expand_home` 同形，但 home 显式传入（jail 内部一致性）。
fn expand_with_home(path: &str, home: &Path) -> PathBuf {
    if path == "~" {
        return home.to_path_buf();
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return home.join(rest);
    }
    if let Some(rest) = path.strip_prefix("~\\") {
        return home.join(rest);
    }
    PathBuf::from(path)
}

fn push_expanded(roots: &mut Vec<PathBuf>, raw: &str, home: &Path) {
    let expanded = expand_with_home(raw, home);
    push_normalized(roots, expanded);
}

fn push_normalized(roots: &mut Vec<PathBuf>, path: PathBuf) {
    let normalized = normalize_components(&path);
    if !roots.contains(&normalized) {
        roots.push(normalized);
    }
}

/// 去掉 `.` 组件（不解析符号链接，不要求路径存在）。
fn normalize_components(path: &Path) -> PathBuf {
    path.components()
        .filter(|component| !matches!(component, Component::CurDir))
        .collect()
}

#[cfg(windows)]
fn path_identity(path: &Path) -> String {
    path_to_string(path).replace('\\', "/").to_ascii_lowercase()
}

#[cfg(not(windows))]
fn path_identity(path: &Path) -> String {
    path_to_string(path)
}

/// `path` 是否等于 `root` 或位于其下（Windows 下大小写不敏感）。
fn path_starts_with(path: &Path, root: &Path) -> bool {
    let path_id = path_identity(path);
    let root_id = path_identity(root);
    let root_id = root_id.trim_end_matches('/');
    path_id == root_id || path_id.starts_with(&format!("{root_id}/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::CustomRoot;

    fn fixture() -> (tempfile::TempDir, AppContext, Settings) {
        let temp = tempfile::tempdir().expect("temp");
        let home = temp.path().join("home");
        let data = temp.path().join("data");
        let ctx = AppContext::new(data, home.clone());
        let settings = Settings {
            library_path: path_to_string(&home.join("library").join("skills")),
            project_folders: vec![path_to_string(&home.join("projects").join("demo"))],
            custom_roots: vec![CustomRoot {
                id: "extra".to_string(),
                label: "Extra".to_string(),
                path: "~/extra-skills".to_string(),
            }],
            show_raw_paths: false,
            language: "zh-CN".to_string(),
            workflow_registry_url: None,
            github_token: None,
            github_username: None,
            skill_registry_url: None,
            clear_github_token: false,
        };
        (temp, ctx, settings)
    }

    #[test]
    fn allows_paths_under_registered_roots() {
        let (_temp, ctx, settings) = fixture();
        let jail = PathJail::new(&ctx, &settings);

        let library_entry = path_to_string(&ctx.home_dir().join("library").join("skills").join("demo"));
        assert!(jail.check_write(&library_entry).is_ok());

        // agent global root（~/.claude/skills 展开自 fake home）
        assert!(jail.check_write("~/.claude/skills/demo").is_ok());

        // project folder 下的 agent project root
        let project_root = path_to_string(
            &ctx.home_dir()
                .join("projects")
                .join("demo")
                .join(".claude")
                .join("skills")
                .join("demo"),
        );
        assert!(jail.check_write(&project_root).is_ok());

        // custom root / data_dir / ~/.agents
        assert!(jail.check_write("~/extra-skills/demo").is_ok());
        let plan = path_to_string(&ctx.data_dir().join("plans").join("p1.json"));
        assert!(jail.check_write(&plan).is_ok());
        assert!(jail.check_write("~/.agents/.skill-lock.json").is_ok());
    }

    #[test]
    fn rejects_parent_dir_components() {
        let (_temp, ctx, settings) = fixture();
        let jail = PathJail::new(&ctx, &settings);

        let traversal = format!(
            "{}/../escape",
            path_to_string(&ctx.home_dir().join("library").join("skills"))
        );
        let err = jail.check_write(&traversal).expect_err("should reject");
        assert!(err.contains(".."), "{err}");

        assert!(jail.check_write("~/.claude/skills/../../etc").is_err());
        // 浏览级同样拒绝 `..`
        assert!(jail.check_browse(&traversal).is_err());
    }

    #[test]
    fn write_tier_rejects_paths_outside_whitelist() {
        let (_temp, ctx, settings) = fixture();
        let jail = PathJail::new(&ctx, &settings);

        // home 本身不在允许根集内（只有具体 skills 根在）
        let outside = path_to_string(&ctx.home_dir().join("Documents"));
        assert!(jail.check_write(&outside).is_err());
        assert!(jail.check_write("/etc/passwd").is_err());

        // project folder 本身也不是允许根（只有其下 agent project roots 是）
        let folder = path_to_string(&ctx.home_dir().join("projects").join("demo"));
        assert!(jail.check_write(&folder).is_err());
    }

    // -- D7-R1：check_browse 宽松级 -------------------------------------------

    #[test]
    fn browse_allows_home_subtree_including_unregistered_paths() {
        let (_temp, ctx, settings) = fixture();
        let jail = PathJail::new(&ctx, &settings);

        // home 本身与任意未注册子路径都放行（写级会拒绝）
        let home = path_to_string(ctx.home_dir());
        assert!(jail.check_browse(&home).is_ok());
        let unregistered = path_to_string(&ctx.home_dir().join("brand-new").join("nested").join("dir"));
        assert!(jail.check_browse(&unregistered).is_ok());
        assert!(jail.check_write(&unregistered).is_err());

        // `~` 展开同样走 home 子树
        assert!(jail.check_browse("~/somewhere/new").is_ok());
    }

    #[test]
    fn browse_allows_registered_roots() {
        let (_temp, ctx, settings) = fixture();
        let jail = PathJail::new(&ctx, &settings);

        assert!(jail.check_browse("~/.claude/skills/demo").is_ok());
        let plan = path_to_string(&ctx.data_dir().join("plans").join("p1.json"));
        assert!(jail.check_browse(&plan).is_ok());
    }

    #[test]
    fn browse_rejects_paths_outside_home_and_roots() {
        let (_temp, ctx, settings) = fixture();
        let jail = PathJail::new(&ctx, &settings);

        // 无盘符前缀的绝对路径不可能命中 Windows 盘符规则，跨平台稳定拒绝
        assert!(jail.check_browse("/etc/passwd").is_err());
        assert!(jail.check_browse("/definitely/outside/home").is_err());
    }

    #[cfg(windows)]
    #[test]
    fn browse_allows_drive_top_level_one_layer_on_windows() {
        let (_temp, ctx, settings) = fixture();
        let jail = PathJail::new(&ctx, &settings);

        // 盘符根与直接子目录放行
        assert!(jail.check_browse("D:\\").is_ok());
        assert!(jail.check_browse("D:/Projects").is_ok());
        // 第二层起拒绝（除非落在 home 子树或允许根下）
        assert!(jail.check_browse("D:\\Projects\\myrepo").is_err());
        // 盘符相对路径（无 RootDir）不放行
        assert!(jail.check_browse("D:Projects").is_err());
    }

    #[test]
    fn normalize_removes_dot_components() {
        let path = PathBuf::from("/a/./b/./c");
        assert_eq!(normalize_components(&path), PathBuf::from("/a/b/c"));
    }
}
