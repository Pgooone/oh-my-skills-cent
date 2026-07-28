use crate::models::{
    AgentTarget, ApplyResult, InstallationRef, InventorySnapshot, ProjectWorkspaceCandidate,
    ScanOptions, Settings, SkillContent, SkillLockEntry, SkillRef, SkillUpdateCheck, SyncPlan,
    SyncReplacement,
};
use crate::{app_context, fs_ops, registry, scanner, settings, skill_ops, sync_plan};
use std::collections::BTreeMap;
use tauri::AppHandle;

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Result<Settings, String> {
    let ctx = app_context(&app)?;
    settings::load_settings(&ctx)
}

#[tauri::command]
pub fn save_settings(app: AppHandle, settings: Settings) -> Result<Settings, String> {
    let ctx = app_context(&app)?;
    settings::save_settings(&ctx, &settings)?;
    settings::load_settings(&ctx)
}

#[tauri::command]
pub fn scan_inventory(
    app: AppHandle,
    options: Option<ScanOptions>,
) -> Result<InventorySnapshot, String> {
    let ctx = app_context(&app)?;
    let snapshot = scanner::scan(
        &ctx,
        options.unwrap_or(ScanOptions {
            include_orphaned: false,
        }),
    )?;
    scanner::write_library_index(&ctx, &snapshot)?;
    scanner::write_inventory_cache(&ctx, &snapshot)?;
    Ok(snapshot)
}

#[tauri::command]
pub fn read_inventory_cache(app: AppHandle) -> Result<Option<InventorySnapshot>, String> {
    let ctx = app_context(&app)?;
    scanner::read_inventory_cache(&ctx)
}

#[tauri::command]
pub fn discover_project_workspaces(
    app: AppHandle,
    base_path: String,
) -> Result<Vec<ProjectWorkspaceCandidate>, String> {
    let ctx = app_context(&app)?;
    let settings = settings::load_settings(&ctx)?;
    registry::discover_project_workspaces(&base_path, &settings)
}

#[tauri::command]
pub fn read_skill_content(skill_ref: SkillRef) -> Result<SkillContent, String> {
    scanner::read_skill_content(skill_ref)
}

#[tauri::command]
pub fn read_skill_lock() -> Result<BTreeMap<String, SkillLockEntry>, String> {
    skill_ops::read_skill_lock()
}

#[tauri::command]
pub fn open_path(path: String) -> Result<(), String> {
    let path = fs_ops::expand_home(&path);
    // Broken symlinks and missing entries fail Path::exists() (metadata follows
    // the link). Fall back to the nearest existing ancestor so users can still
    // open the skills root in Finder/Explorer and clean up the entry.
    let open_target = if path.exists() {
        path
    } else {
        existing_ancestor(&path).ok_or_else(|| {
            format!("Path does not exist: {}", fs_ops::path_to_string(&path))
        })?
    };

    tauri_plugin_opener::open_path(&open_target, None::<&str>).map_err(|error| {
        format!(
            "Unable to open {}: {error}",
            fs_ops::path_to_string(&open_target)
        )
    })
}

fn existing_ancestor(path: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut current = path.parent()?;
    loop {
        if current.as_os_str().is_empty() {
            return None;
        }
        if current.exists() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

/// Delete skill installation entries (directories or symlinks) from disk.
/// Only paths whose parent directory is named `skills` are accepted, so the
/// command cannot wipe arbitrary folders.
#[tauri::command]
pub fn remove_skill_entries(
    paths: Vec<String>,
) -> Result<skill_ops::RemoveSkillEntriesResult, String> {
    skill_ops::remove_skill_entries(paths)
}

#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    if !url.starts_with("https://github.com/") {
        return Err("Only GitHub URLs can be opened from this view".to_string());
    }
    tauri_plugin_opener::open_url(&url, None::<&str>)
        .map_err(|error| format!("Unable to open {url}: {error}"))
}

#[tauri::command]
pub fn check_skills_sh_update(
    app: AppHandle,
    slug: String,
    entry_path: String,
    source_url: String,
    skill_path: Option<String>,
) -> Result<SkillUpdateCheck, String> {
    let ctx = app_context(&app)?;
    skill_ops::check_skills_sh_update(&ctx, slug, entry_path, source_url, skill_path)
}

#[tauri::command]
pub fn update_skills_sh_skill(
    app: AppHandle,
    slug: String,
    entry_path: String,
    source_url: String,
    skill_path: Option<String>,
) -> Result<SkillUpdateCheck, String> {
    let ctx = app_context(&app)?;
    skill_ops::update_skills_sh_skill(&ctx, slug, entry_path, source_url, skill_path)
}

#[tauri::command]
pub fn preview_adopt(app: AppHandle, source: InstallationRef) -> Result<SyncPlan, String> {
    let ctx = app_context(&app)?;
    sync_plan::preview_adopt(&ctx, source)
}

#[tauri::command]
pub fn preview_sync(
    app: AppHandle,
    skill_id: String,
    targets: Vec<AgentTarget>,
    replacements: Option<Vec<SyncReplacement>>,
) -> Result<SyncPlan, String> {
    let ctx = app_context(&app)?;
    sync_plan::preview_sync(&ctx, skill_id, targets, replacements.unwrap_or_default())
}

#[tauri::command]
pub fn preview_sync_from_installation(
    app: AppHandle,
    source: InstallationRef,
    targets: Vec<AgentTarget>,
    replacements: Option<Vec<SyncReplacement>>,
) -> Result<SyncPlan, String> {
    let ctx = app_context(&app)?;
    sync_plan::preview_sync_from_installation(&ctx, source, targets, replacements.unwrap_or_default())
}

#[tauri::command]
pub fn preview_quick_migration(
    app: AppHandle,
    source: InstallationRef,
    targets: Vec<AgentTarget>,
    method: String,
) -> Result<SyncPlan, String> {
    let ctx = app_context(&app)?;
    sync_plan::preview_quick_migration(&ctx, source, targets, method)
}

#[tauri::command]
pub fn preview_batch_sync(
    app: AppHandle,
    sources: Vec<InstallationRef>,
    targets: Vec<AgentTarget>,
    replacements: Option<Vec<SyncReplacement>>,
) -> Result<SyncPlan, String> {
    let ctx = app_context(&app)?;
    sync_plan::preview_batch_sync(&ctx, sources, targets, replacements.unwrap_or_default())
}

#[tauri::command]
pub fn preview_batch_quick_migration(
    app: AppHandle,
    sources: Vec<InstallationRef>,
    targets: Vec<AgentTarget>,
    method: String,
) -> Result<SyncPlan, String> {
    let ctx = app_context(&app)?;
    sync_plan::preview_batch_quick_migration(&ctx, sources, targets, method)
}

#[tauri::command]
pub fn apply_sync_plan(app: AppHandle, plan_id: String) -> Result<ApplyResult, String> {
    let ctx = app_context(&app)?;
    sync_plan::apply_plan(&ctx, plan_id)
}
