use crate::models::{
    AgentTarget, ApplyResult, InstallationRef, InventorySnapshot, ProjectWorkspaceCandidate,
    RedactedSettings, ScanOptions, Settings, SkillContent, SkillLockEntry, SkillRef,
    SkillUpdateCheck, SyncPlan, SyncReplacement,
};
use crate::{app_context, fs_ops, registry, scanner, settings, skill_ops, sync_plan};
use std::collections::BTreeMap;
use tauri::AppHandle;

// 出参裁剪（门-token-F1）：凡返回 Settings 的 command 一律经 settings::redacted，
// githubToken 键不出现在响应里，前端凭 hasGithubToken 判断是否已配置。

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Result<RedactedSettings, String> {
    let ctx = app_context(&app)?;
    let loaded = settings::load_settings(&ctx)?;
    Ok(settings::redacted(&loaded))
}

#[tauri::command]
pub fn save_settings(app: AppHandle, settings: Settings) -> Result<RedactedSettings, String> {
    let ctx = app_context(&app)?;
    // 入参校验（两个 *RegistryUrl 拒绝 userinfo/非 GitHub）与 token 三分支合并
    // 单点在核心 save_settings_with_merge。
    let saved = settings::save_settings_with_merge(&ctx, &settings)?;
    Ok(settings::redacted(&saved))
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

// ---------------------------------------------------------------------------
// Round 2 workflows-api：7 个 workflow command 薄转发（NFR-2），apply 复用既有。
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_installed_workflows(
    app: AppHandle,
) -> Result<Vec<crate::workflow::InstalledWorkflow>, String> {
    let ctx = app_context(&app)?;
    crate::workflow::list_installed(&ctx)
}

#[tauri::command]
pub fn list_remote_workflows(
    app: AppHandle,
    refresh: Option<bool>,
) -> Result<Vec<crate::workflow_registry::RemoteWorkflowSummary>, String> {
    let ctx = app_context(&app)?;
    // cache-first（lead 裁决）：refresh=true 强制拉取；false/缺省时优先读缓存，
    // 无缓存再回退拉取（fetch_index 自带离线回退旧缓存）。
    if !refresh.unwrap_or(false) {
        if let Some(cached) = crate::workflow_registry::read_cached_index(&ctx) {
            return Ok(cached);
        }
    }
    let registry_url = workflow_registry_url(&ctx)?;
    crate::workflow_registry::fetch_index(&ctx, &registry_url)
}

#[tauri::command]
pub fn get_workflow_detail(
    app: AppHandle,
    slug: String,
) -> Result<crate::workflow_use::WorkflowDetail, String> {
    let ctx = app_context(&app)?;
    let workflow = crate::workflow::load(&ctx, &slug)?;
    let statuses = crate::workflow_use::compute_statuses(&ctx, &workflow)?;
    Ok(crate::workflow_use::WorkflowDetail { workflow, statuses })
}

#[tauri::command]
pub fn download_workflow(
    app: AppHandle,
    path: String,
) -> Result<crate::workflow::InstalledWorkflow, String> {
    let ctx = app_context(&app)?;
    let registry_url = workflow_registry_url(&ctx)?;
    // path 原样下传：traversal 防护由 registry-client 的 guard_registry_path 把关。
    let slug = crate::workflow_registry::download_to_installed(&ctx, &registry_url, &path)?;
    // M3：下载成功记录来源快照（薄转发 +1 行），供三态更新检查比对。
    crate::workflow_update::record_source(&ctx, &slug, &registry_url, &path)?;
    crate::workflow::list_installed(&ctx)?
        .into_iter()
        .find(|item| item.slug == slug)
        .ok_or_else(|| format!("Downloaded workflow '{slug}' is missing from installed list"))
}

#[tauri::command]
pub fn save_workflow(
    app: AppHandle,
    workflow: crate::workflow::Workflow,
    readme: Option<String>,
) -> Result<String, String> {
    let ctx = app_context(&app)?;
    crate::workflow::save(&ctx, &workflow, readme.as_deref())?;
    Ok(workflow.slug)
}

#[tauri::command]
pub fn delete_workflow(app: AppHandle, slug: String) -> Result<(), String> {
    let ctx = app_context(&app)?;
    crate::workflow::delete(&ctx, &slug)
}

#[tauri::command]
pub fn preview_use_workflow(
    app: AppHandle,
    slug: String,
    targets: Vec<AgentTarget>,
    method: String,
    output_form: crate::workflow_use::OutputForm,
) -> Result<SyncPlan, String> {
    let ctx = app_context(&app)?;
    crate::workflow_use::preview_use_workflow(&ctx, &slug, targets, method, output_form)
}

// ---------------------------------------------------------------------------
// Round 3 workflow-update（M3）：2 个薄转发。slug 合法性由核心校验。
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn check_workflow_updates(
    app: AppHandle,
) -> Result<Vec<crate::workflow_update::WorkflowUpdateStatus>, String> {
    let ctx = app_context(&app)?;
    crate::workflow_update::check_all(&ctx)
}

#[tauri::command]
pub fn update_workflow(
    app: AppHandle,
    slug: String,
    confirm_modified: bool,
) -> Result<crate::workflow_update::WorkflowUpdateStatus, String> {
    let ctx = app_context(&app)?;
    crate::workflow_update::apply_update(&ctx, &slug, confirm_modified)
}

// ---------------------------------------------------------------------------
// Round 3 workflow-share（M4）：3 个薄转发。save_export_to_path 仅桌面注册
//（R4/D7：web 不挂）；export/import 的 base64 编解码与校验链全在核心。
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn export_workflow_package(
    app: AppHandle,
    slug: String,
) -> Result<crate::workflow_share::ExportPackage, String> {
    let ctx = app_context(&app)?;
    crate::workflow_share::export_package_base64(&ctx, &slug)
}

#[tauri::command]
pub fn import_workflow_package(
    app: AppHandle,
    archive_base64: String,
) -> Result<crate::workflow_share::ImportResult, String> {
    let ctx = app_context(&app)?;
    crate::workflow_share::import_package_base64(&ctx, &archive_base64)
}

#[tauri::command]
pub fn save_export_to_path(path: String, base64: String) -> Result<(), String> {
    crate::workflow_share::save_export_to_path(&path, &base64)
}

// ---------------------------------------------------------------------------
// Round 3 skill-registry（M6）：4 个薄转发。slug/path 合法性由核心校验。
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_remote_skills(
    app: AppHandle,
    refresh: Option<bool>,
) -> Result<Vec<crate::skill_registry::RemoteSkillSummary>, String> {
    let ctx = app_context(&app)?;
    // cache-first（与 list_remote_workflows 同款裁决）：refresh=true 强制拉取；
    // false/缺省时优先读缓存，无缓存再回退拉取（fetch_index 自带离线回退旧缓存）。
    if !refresh.unwrap_or(false) {
        if let Some(cached) = crate::skill_registry::read_cached_index(&ctx) {
            return Ok(cached);
        }
    }
    let registry_url = skill_registry_url(&ctx)?;
    crate::skill_registry::fetch_index(&ctx, &registry_url)
}

#[tauri::command]
pub fn download_skill(app: AppHandle, path: String) -> Result<String, String> {
    let ctx = app_context(&app)?;
    let registry_url = skill_registry_url(&ctx)?;
    // path 原样下传：index 条目查找与 traversal 防护由 skill_registry 把关。
    crate::skill_registry::download_skill(&ctx, &registry_url, &path)
}

#[tauri::command]
pub fn check_registry_skill_updates(
    app: AppHandle,
) -> Result<Vec<crate::skill_registry::RegistrySkillUpdate>, String> {
    let ctx = app_context(&app)?;
    crate::skill_registry::check_updates(&ctx)
}

#[tauri::command]
pub fn update_registry_skill(app: AppHandle, slug: String) -> Result<(), String> {
    let ctx = app_context(&app)?;
    crate::skill_registry::apply_update(&ctx, &slug)
}

/// load_settings 已保证空值回填官方缺省；此处兜底仅为避免解包 panic。
fn skill_registry_url(ctx: &crate::context::AppContext) -> Result<String, String> {
    Ok(settings::load_settings(ctx)?
        .skill_registry_url
        .filter(|url| !url.trim().is_empty())
        .unwrap_or_else(|| settings::OFFICIAL_SKILL_REGISTRY_URL.to_string()))
}

/// load_settings 已保证空值回填官方缺省；此处兜底仅为避免解包 panic。
fn workflow_registry_url(ctx: &crate::context::AppContext) -> Result<String, String> {
    Ok(settings::load_settings(ctx)?
        .workflow_registry_url
        .filter(|url| !url.trim().is_empty())
        .unwrap_or_else(|| settings::OFFICIAL_WORKFLOW_REGISTRY_URL.to_string()))
}
