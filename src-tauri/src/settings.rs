use crate::context::AppContext;
use crate::fs_ops::{ensure_dir, path_to_string};
use crate::models::{CustomRoot, Settings};
use std::fs;
use std::path::PathBuf;

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
    })
}
