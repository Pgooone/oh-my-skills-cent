//! D7 路径白名单（PathJail）。
//!
//! Web 化后路径来自 HTTP 请求，所有接受路径参数的 endpoint 必须先把路径 jail
//! 在「中心库 + 已注册 agent skills 目录 + 项目 roots + 数据目录 + ~/.agents」内，
//! 否则例如 `remove_skill_entries` 就是任意文件删除 API。
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

    /// expand_home → 组件规范化（拒绝 `..`）→ 必须位于某允许根之下。
    /// 通过时返回规范化后的路径。
    pub fn check(&self, raw: &str) -> Result<PathBuf, String> {
        let expanded = expand_with_home(raw, &self.home_dir);
        if expanded
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(format!("Path must not contain '..': {raw}"));
        }
        let normalized = normalize_components(&expanded);
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
        };
        (temp, ctx, settings)
    }

    #[test]
    fn allows_paths_under_registered_roots() {
        let (_temp, ctx, settings) = fixture();
        let jail = PathJail::new(&ctx, &settings);

        let library_entry = path_to_string(&ctx.home_dir().join("library").join("skills").join("demo"));
        assert!(jail.check(&library_entry).is_ok());

        // agent global root（~/.claude/skills 展开自 fake home）
        assert!(jail.check("~/.claude/skills/demo").is_ok());

        // project folder 下的 agent project root
        let project_root = path_to_string(
            &ctx.home_dir()
                .join("projects")
                .join("demo")
                .join(".claude")
                .join("skills")
                .join("demo"),
        );
        assert!(jail.check(&project_root).is_ok());

        // custom root / data_dir / ~/.agents
        assert!(jail.check("~/extra-skills/demo").is_ok());
        let plan = path_to_string(&ctx.data_dir().join("plans").join("p1.json"));
        assert!(jail.check(&plan).is_ok());
        assert!(jail.check("~/.agents/.skill-lock.json").is_ok());
    }

    #[test]
    fn rejects_parent_dir_components() {
        let (_temp, ctx, settings) = fixture();
        let jail = PathJail::new(&ctx, &settings);

        let traversal = format!(
            "{}/../escape",
            path_to_string(&ctx.home_dir().join("library").join("skills"))
        );
        let err = jail.check(&traversal).expect_err("should reject");
        assert!(err.contains(".."), "{err}");

        assert!(jail.check("~/.claude/skills/../../etc").is_err());
    }

    #[test]
    fn rejects_paths_outside_whitelist() {
        let (_temp, ctx, settings) = fixture();
        let jail = PathJail::new(&ctx, &settings);

        // home 本身不在允许根集内（只有具体 skills 根在）
        let outside = path_to_string(&ctx.home_dir().join("Documents"));
        assert!(jail.check(&outside).is_err());
        assert!(jail.check("/etc/passwd").is_err());

        // project folder 本身也不是允许根（只有其下 agent project roots 是）
        let folder = path_to_string(&ctx.home_dir().join("projects").join("demo"));
        assert!(jail.check(&folder).is_err());
    }

    #[test]
    fn normalize_removes_dot_components() {
        let path = PathBuf::from("/a/./b/./c");
        assert_eq!(normalize_components(&path), PathBuf::from("/a/b/c"));
    }
}
