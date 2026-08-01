use crate::context::AppContext;
use crate::fs_ops::{ensure_dir, path_to_string};
use crate::models::{CustomRoot, RedactedSettings, Settings};
use std::fs;
use std::path::PathBuf;

pub const OFFICIAL_WORKFLOW_REGISTRY_URL: &str =
    "https://github.com/Pgooone/oh-my-skills-workflows.git";
pub const OFFICIAL_SKILL_REGISTRY_URL: &str =
    "https://github.com/Pgooone/oh-my-skills-skills.git";

pub fn app_data_dir(ctx: &AppContext) -> Result<PathBuf, String> {
    Ok(ctx.data_dir().to_path_buf())
}

pub fn settings_path(ctx: &AppContext) -> Result<PathBuf, String> {
    Ok(app_data_dir(ctx)?.join("settings.json"))
}

pub fn default_settings(ctx: &AppContext) -> Result<Settings, String> {
    let library_path = ctx.home_dir().join(".oh-my-skills").join("skills");
    Ok(Settings {
        library_path: path_to_string(&library_path),
        project_folders: Vec::new(),
        custom_roots: Vec::<CustomRoot>::new(),
        show_raw_paths: false,
        language: "zh-CN".to_string(),
        workflow_registry_url: Some(OFFICIAL_WORKFLOW_REGISTRY_URL.to_string()),
        github_token: None,
        github_username: None,
        skill_registry_url: Some(OFFICIAL_SKILL_REGISTRY_URL.to_string()),
        clear_github_token: false,
    })
}

pub fn load_settings(ctx: &AppContext) -> Result<Settings, String> {
    let default = default_settings(ctx)?;
    let path = settings_path(ctx)?;
    if !path.exists() {
        ensure_dir(path.parent().ok_or("Settings path has no parent")?)?;
        save_settings(ctx, &default)?;
        return Ok(default);
    }

    let text = fs::read_to_string(&path).map_err(|error| {
        format!(
            "Unable to read settings at {}: {error}",
            path_to_string(&path)
        )
    })?;
    let mut settings: Settings = serde_json::from_str(&text).map_err(|error| {
        format!(
            "Unable to parse settings at {}: {error}",
            path_to_string(&path)
        )
    })?;

    if settings.library_path.trim().is_empty() {
        settings.library_path = default.library_path;
    }
    if settings.language.trim().is_empty() {
        settings.language = default.language;
    }
    if settings
        .workflow_registry_url
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .is_empty()
    {
        settings.workflow_registry_url = default.workflow_registry_url;
    }
    if settings
        .skill_registry_url
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .is_empty()
    {
        settings.skill_registry_url = default.skill_registry_url;
    }

    Ok(settings)
}

pub fn save_settings(ctx: &AppContext, settings: &Settings) -> Result<(), String> {
    let path = settings_path(ctx)?;
    ensure_dir(path.parent().ok_or("Settings path has no parent")?)?;
    ensure_dir(PathBuf::from(&settings.library_path).as_path())?;
    let text = serde_json::to_string_pretty(settings)
        .map_err(|error| format!("Unable to serialize settings: {error}"))?;
    fs::write(&path, text).map_err(|error| {
        format!(
            "Unable to write settings at {}: {error}",
            path_to_string(&path)
        )
    })?;
    // token 明文落盘（Q4）的配套缓解：unix 下把 settings.json 收敛到 0600，
    // 覆盖首次创建与已存在文件 chmod 两情形（门-F8/F-05）。
    // Windows 无对应语义，依赖用户 profile 目录 ACL 保护。
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).map_err(|error| {
            format!(
                "Unable to set permissions on settings at {}: {error}",
                path_to_string(&path)
            )
        })?;
    }
    Ok(())
}

/// token 合并三分支（门-token-F3 wire 契约单点，双壳薄转发到这里）：
/// - `clear == true` → 清除（优先级高于替换）；
/// - `incoming` 为 None（json null/缺省）或空白串 → 保持 current 不动；
/// - `incoming` 为 Some(非空) → 替换。
pub fn merge_token(
    current: Option<String>,
    incoming: Option<String>,
    clear: bool,
) -> Option<String> {
    if clear {
        return None;
    }
    match incoming {
        Some(value) if !value.trim().is_empty() => Some(value),
        _ => current,
    }
}

/// 保存入口（双壳共用）：先校验两个 *RegistryUrl（门-L4/门-token-F6），再按
/// merge_token 三分支合并 token，落盘后重载返回（含空值回填）。
pub fn save_settings_with_merge(
    ctx: &AppContext,
    incoming: &Settings,
) -> Result<Settings, String> {
    validate_registry_url("workflowRegistryUrl", incoming.workflow_registry_url.as_deref())?;
    validate_registry_url("skillRegistryUrl", incoming.skill_registry_url.as_deref())?;

    let current = load_settings(ctx)?;
    let mut next = incoming.clone();
    next.github_token = merge_token(
        current.github_token,
        incoming.github_token.clone(),
        incoming.clear_github_token,
    );
    save_settings(ctx, &next)?;
    load_settings(ctx)
}

/// 注册表 URL 安全校验：留空合法（load 时回填官方缺省）；非空必须过
/// GitHub-only 归一化——userinfo 与非 GitHub 来源一并拒绝。
fn validate_registry_url(field: &str, url: Option<&str>) -> Result<(), String> {
    let Some(url) = url.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    crate::skill_ops::normalize_github_url(url)
        .map(|_| ())
        .map_err(|error| format!("Invalid {field}: {error}"))
}

/// 出参裁剪（门-token-F1）：克隆后置空 token，附加 hasGithubToken。
/// 两个壳的 get_settings / save_settings 返回值一律经这里。
pub fn redacted(settings: &Settings) -> RedactedSettings {
    let has_github_token = settings
        .github_token
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    let mut stripped = settings.clone();
    stripped.github_token = None;
    RedactedSettings {
        settings: stripped,
        has_github_token,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::AppContext;

    fn fixture() -> (tempfile::TempDir, AppContext) {
        let temp = tempfile::tempdir().expect("temp");
        let ctx = AppContext::new(temp.path().join("data"), temp.path().join("home"));
        (temp, ctx)
    }

    // -- merge_token 三分支 ---------------------------------------------------

    #[test]
    fn merge_token_keeps_current_when_incoming_absent_or_blank() {
        let current = Some("ghp_old".to_string());
        assert_eq!(
            merge_token(current.clone(), None, false),
            Some("ghp_old".to_string())
        );
        assert_eq!(
            merge_token(current.clone(), Some("   ".to_string()), false),
            Some("ghp_old".to_string())
        );
    }

    #[test]
    fn merge_token_replaces_with_non_empty() {
        assert_eq!(
            merge_token(Some("ghp_old".to_string()), Some("ghp_new".to_string()), false),
            Some("ghp_new".to_string())
        );
        assert_eq!(
            merge_token(None, Some("ghp_new".to_string()), false),
            Some("ghp_new".to_string())
        );
    }

    #[test]
    fn merge_token_clear_wins_over_replacement() {
        assert_eq!(
            merge_token(Some("ghp_old".to_string()), Some("ghp_new".to_string()), true),
            None
        );
        assert_eq!(merge_token(Some("ghp_old".to_string()), None, true), None);
    }

    // -- save_settings_with_merge --------------------------------------------

    #[test]
    fn save_with_merge_keeps_token_when_unrelated_fields_change() {
        let (_temp, ctx) = fixture();
        let mut initial = default_settings(&ctx).expect("defaults");
        initial.github_token = Some("ghp_secret".to_string());
        save_settings_with_merge(&ctx, &initial).expect("save with token");
        let disk = fs::read_to_string(settings_path(&ctx).expect("path")).expect("disk");
        assert!(disk.contains("ghp_secret"), "token 正常落盘（R3）");

        // 模拟双壳回传：裁剪后的 settings（无 token）改无关字段再保存。
        let mut echo = load_settings(&ctx).expect("load");
        echo.github_token = None;
        echo.language = "en".to_string();
        let saved = save_settings_with_merge(&ctx, &echo).expect("save echo");
        assert_eq!(saved.github_token.as_deref(), Some("ghp_secret"));
        assert_eq!(saved.language, "en");
        let disk = fs::read_to_string(settings_path(&ctx).expect("path")).expect("disk");
        assert!(disk.contains("ghp_secret"), "改无关设置 token 不动");
    }

    #[test]
    fn save_with_merge_clear_drops_token_from_disk() {
        let (_temp, ctx) = fixture();
        let mut initial = default_settings(&ctx).expect("defaults");
        initial.github_token = Some("ghp_secret".to_string());
        save_settings_with_merge(&ctx, &initial).expect("save with token");

        let mut clearing = load_settings(&ctx).expect("load");
        clearing.github_token = None;
        clearing.clear_github_token = true;
        let saved = save_settings_with_merge(&ctx, &clearing).expect("clear");
        assert_eq!(saved.github_token, None);
        let disk = fs::read_to_string(settings_path(&ctx).expect("path")).expect("disk");
        assert!(!disk.contains("githubToken"), "显式清除后落盘无 token 键");
    }

    #[test]
    fn save_with_merge_rejects_userinfo_and_non_github_registry_urls() {
        let (_temp, ctx) = fixture();
        let base = default_settings(&ctx).expect("defaults");
        for (field, value) in [
            ("workflow", "https://user:pw@github.com/owner/repo.git"),
            ("skill", "https://user:pw@github.com/owner/repo.git"),
            ("workflow", "https://gitlab.com/owner/repo.git"),
            ("skill", "https://gitlab.com/owner/repo.git"),
        ] {
            let mut settings = base.clone();
            if field == "workflow" {
                settings.workflow_registry_url = Some(value.to_string());
            } else {
                settings.skill_registry_url = Some(value.to_string());
            }
            let error = save_settings_with_merge(&ctx, &settings)
                .expect_err("userinfo/非 GitHub 必须拒绝");
            assert!(error.contains("RegistryUrl"), "{field}={value}: {error}");
            assert!(!settings_path(&ctx).expect("path").exists(), "拒绝前不落盘");
        }
    }

    #[test]
    fn save_with_merge_accepts_blank_and_canonical_registry_urls() {
        let (_temp, ctx) = fixture();
        let mut settings = default_settings(&ctx).expect("defaults");
        settings.workflow_registry_url = Some(String::new());
        settings.skill_registry_url = Some("   ".to_string());
        let saved = save_settings_with_merge(&ctx, &settings).expect("blank ok");
        assert_eq!(
            saved.workflow_registry_url.as_deref(),
            Some(OFFICIAL_WORKFLOW_REGISTRY_URL)
        );
        assert_eq!(
            saved.skill_registry_url.as_deref(),
            Some(OFFICIAL_SKILL_REGISTRY_URL)
        );
    }

    #[test]
    fn load_settings_backfills_skill_registry_url() {
        let (_temp, ctx) = fixture();
        // 首次创建即含官方缺省。
        let loaded = load_settings(&ctx).expect("first load");
        assert_eq!(
            loaded.skill_registry_url.as_deref(),
            Some(OFFICIAL_SKILL_REGISTRY_URL)
        );

        // 手写空值文件 → 回填。
        let mut settings = loaded.clone();
        settings.skill_registry_url = Some(String::new());
        save_settings(&ctx, &settings).expect("save blank");
        let loaded = load_settings(&ctx).expect("reload");
        assert_eq!(
            loaded.skill_registry_url.as_deref(),
            Some(OFFICIAL_SKILL_REGISTRY_URL)
        );
    }

    // -- redacted（出参裁剪） --------------------------------------------------

    #[test]
    fn redacted_strips_token_key_and_reports_flag() {
        let (_temp, ctx) = fixture();
        let mut settings = default_settings(&ctx).expect("defaults");
        settings.github_token = Some("ghp_secret".to_string());

        let json = serde_json::to_value(redacted(&settings)).expect("json");
        let object = json.as_object().expect("object");
        assert!(!object.contains_key("githubToken"), "响应不得含 githubToken 键");
        assert_eq!(json["hasGithubToken"].as_bool(), Some(true));

        settings.github_token = None;
        let json = serde_json::to_value(redacted(&settings)).expect("json");
        let object = json.as_object().expect("object");
        assert!(!object.contains_key("githubToken"));
        assert_eq!(json["hasGithubToken"].as_bool(), Some(false));
    }

    // -- unix 0600（门-F8/F-05） ------------------------------------------------

    #[cfg(unix)]
    #[test]
    fn save_settings_enforces_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let (_temp, ctx) = fixture();
        let settings = default_settings(&ctx).expect("defaults");
        save_settings(&ctx, &settings).expect("save");
        let path = settings_path(&ctx).expect("path");
        let mode = fs::metadata(&path).expect("metadata").permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "新建即 0600");

        // 已存在文件被放宽后再次保存 → 重新收敛 0600。
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("chmod 644");
        save_settings(&ctx, &settings).expect("save again");
        let mode = fs::metadata(&path).expect("metadata").permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "已存在文件 chmod 收敛");
    }
}
