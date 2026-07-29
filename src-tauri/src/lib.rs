#[cfg(feature = "tauri-shell")]
mod commands;
pub mod context;
pub mod fs_ops;
pub mod models;
pub mod registry;
pub mod scanner;
pub mod settings;
pub mod skill_ops;
pub mod sync_plan;
pub mod workflow;
pub mod workflow_registry;
pub mod workflow_use;
#[cfg(feature = "web")]
pub mod web;

#[cfg(feature = "tauri-shell")]
pub(crate) fn app_context(app: &tauri::AppHandle) -> Result<context::AppContext, String> {
    use tauri::Manager;

    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Unable to resolve app data directory: {error}"))?;
    Ok(context::AppContext::new(data_dir, fs_ops::home_dir()))
}

#[cfg(feature = "tauri-shell")]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::read_inventory_cache,
            commands::scan_inventory,
            commands::discover_project_workspaces,
            commands::read_skill_content,
            commands::read_skill_lock,
            commands::open_path,
            commands::remove_skill_entries,
            commands::open_url,
            commands::check_skills_sh_update,
            commands::update_skills_sh_skill,
            commands::preview_adopt,
            commands::preview_sync,
            commands::preview_sync_from_installation,
            commands::preview_quick_migration,
            commands::preview_batch_sync,
            commands::preview_batch_quick_migration,
            commands::apply_sync_plan
        ])
        .run(tauri::generate_context!())
        .expect("error while running Oh My Skills");
}
