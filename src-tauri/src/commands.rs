use crate::models::{
    AgentTarget, ApplyResult, InstallationRef, InventorySnapshot, ProjectWorkspaceCandidate,
    ScanOptions, Settings, SkillContent, SkillLockEntry, SkillLockFile, SkillRef, SkillUpdateCheck,
    SyncPlan, SyncReplacement,
};
use crate::{app_context, fs_ops, registry, scanner, settings, sync_plan};
use chrono::Utc;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
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
    let path = fs_ops::expand_home("~/.agents/.skill-lock.json");
    let Ok(text) = fs::read_to_string(&path) else {
        return Ok(BTreeMap::new());
    };
    let lock = serde_json::from_str::<SkillLockFile>(&text).map_err(|error| {
        format!(
            "Unable to parse skill lock {}: {error}",
            fs_ops::path_to_string(&path)
        )
    })?;
    Ok(lock.skills)
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

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveSkillEntriesResult {
    pub removed: Vec<String>,
    pub failed: Vec<RemoveSkillEntryFailure>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveSkillEntryFailure {
    pub path: String,
    pub error: String,
}

/// Delete skill installation entries (directories or symlinks) from disk.
/// Only paths whose parent directory is named `skills` are accepted, so the
/// command cannot wipe arbitrary folders.
#[tauri::command]
pub fn remove_skill_entries(paths: Vec<String>) -> Result<RemoveSkillEntriesResult, String> {
    if paths.is_empty() {
        return Err("No paths provided".to_string());
    }

    let mut removed = Vec::new();
    let mut failed = Vec::new();
    let mut seen = std::collections::BTreeSet::new();

    for raw in paths {
        let path = fs_ops::expand_home(&raw);
        let display = fs_ops::path_to_string(&path);
        if !seen.insert(display.clone()) {
            continue;
        }

        if let Err(error) = validate_removable_skill_entry(&path) {
            failed.push(RemoveSkillEntryFailure {
                path: display,
                error,
            });
            continue;
        }

        match fs_ops::remove_entry(&path) {
            Ok(()) => removed.push(display),
            Err(error) => failed.push(RemoveSkillEntryFailure {
                path: display,
                error,
            }),
        }
    }

    if removed.is_empty() && !failed.is_empty() {
        return Err(failed
            .into_iter()
            .map(|item| format!("{}: {}", item.path, item.error))
            .collect::<Vec<_>>()
            .join("; "));
    }

    Ok(RemoveSkillEntriesResult { removed, failed })
}

fn validate_removable_skill_entry(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty() {
        return Err("Path is empty".to_string());
    }
    if path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err("Path must not contain '..'".to_string());
    }

    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| "Skill entry must live under a skills directory".to_string())?;
    let parent_name = parent
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Skill entry must live under a skills directory".to_string())?;
    if !parent_name.eq_ignore_ascii_case("skills") {
        return Err(format!(
            "Refusing to delete path outside a skills directory: {}",
            fs_ops::path_to_string(path)
        ));
    }

    let entry_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty() && *name != "." && *name != "..")
        .ok_or_else(|| "Invalid skill entry name".to_string())?;
    if entry_name.eq_ignore_ascii_case("skills") {
        return Err("Refusing to delete a skills root directory".to_string());
    }

    // Accept existing dirs/files and broken symlinks (symlink_metadata succeeds
    // when the link entry itself is present even if the target is missing).
    fs::symlink_metadata(path).map_err(|error| {
        format!(
            "Unable to inspect {}: {error}",
            fs_ops::path_to_string(path)
        )
    })?;

    Ok(())
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
    let local_path = fs_ops::expand_home(&entry_path);
    let remote_path = checkout_skills_sh_source(&ctx, &slug, &source_url, skill_path.as_deref())?;
    let local_hash = fs_ops::hash_dir(&local_path)?;
    let remote_hash = fs_ops::hash_dir(&remote_path)?;
    let available = local_hash != remote_hash;

    Ok(SkillUpdateCheck {
        status: if available { "available" } else { "current" }.to_string(),
        message: None,
        local_hash: Some(local_hash),
        remote_hash: Some(remote_hash),
    })
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
    let local_path = fs_ops::expand_home(&entry_path);
    if !is_agents_skill_path(&local_path, &slug) {
        return Err(format!(
            "Refusing to update non-skills.sh path {}",
            fs_ops::path_to_string(&local_path)
        ));
    }

    let remote_path = checkout_skills_sh_source(&ctx, &slug, &source_url, skill_path.as_deref())?;
    let local_hash = fs_ops::hash_dir(&local_path).ok();
    let remote_hash = fs_ops::hash_dir(&remote_path)?;

    let backup_root = crate::settings::app_data_dir(&ctx)?
        .join("backups")
        .join("skills-sh-updates")
        .join(Utc::now().format("%Y%m%d%H%M%S").to_string());
    fs_ops::ensure_dir(&backup_root)?;
    if local_path.exists() {
        fs_ops::copy_dir_recursive(&local_path, &backup_root.join(&slug))?;
        fs_ops::remove_entry(&local_path)?;
    }
    fs_ops::copy_dir_recursive(&remote_path, &local_path)?;

    Ok(SkillUpdateCheck {
        status: "current".to_string(),
        message: Some(format!("Updated {slug} from {source_url}")),
        local_hash,
        remote_hash: Some(remote_hash),
    })
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

fn checkout_skills_sh_source(
    ctx: &crate::context::AppContext,
    slug: &str,
    source_url: &str,
    skill_path: Option<&str>,
) -> Result<PathBuf, String> {
    let clone_url = normalize_github_url(source_url)?;
    let checkout_root = crate::settings::app_data_dir(ctx)?
        .join("updates")
        .join(format!("{}-{}", slug, Utc::now().timestamp_millis()));
    let repo_path = checkout_root.join("repo");
    fs_ops::ensure_dir(&checkout_root)?;

    let status = Command::new("git")
        .args(["clone", "--depth", "1", &clone_url])
        .arg(&repo_path)
        .status()
        .map_err(|error| format!("Unable to clone {clone_url}: {error}"))?;
    if !status.success() {
        return Err(format!(
            "Unable to clone {clone_url}: git exited with {status}"
        ));
    }

    let source = resolve_skill_path(&repo_path, slug, skill_path).ok_or_else(|| {
        format!(
            "Unable to find skill '{slug}' in cloned repository {}",
            fs_ops::path_to_string(&repo_path)
        )
    })?;
    if !source.join("SKILL.md").exists() {
        return Err(format!(
            "Remote skill source is missing SKILL.md: {}",
            fs_ops::path_to_string(&source)
        ));
    }
    Ok(source)
}

fn normalize_github_url(source_url: &str) -> Result<String, String> {
    let trimmed = source_url
        .trim()
        .trim_end_matches('/')
        .trim_end_matches(".git");

    let path = if let Some(rest) = trimmed.strip_prefix("git@github.com:") {
        rest.to_string()
    } else if let Some(rest) = trimmed.strip_prefix("github.com/") {
        rest.to_string()
    } else if let Some(rest) = trimmed.strip_prefix("https://github.com/") {
        rest.to_string()
    } else if looks_like_github_slug(trimmed) {
        trimmed.to_string()
    } else {
        return Err("skills.sh update currently supports GitHub sources only".to_string());
    };

    Ok(format!("https://github.com/{path}.git"))
}

fn looks_like_github_slug(value: &str) -> bool {
    let parts: Vec<&str> = value.split('/').collect();
    parts.len() == 2 && parts.iter().all(|p| !p.is_empty())
}

fn resolve_skill_path(repo_path: &Path, slug: &str, skill_path: Option<&str>) -> Option<PathBuf> {
    let custom = skill_path
        .filter(|path| !path.trim().is_empty())
        .map(|path| repo_path.join(path.trim_start_matches('/')));

    std::iter::once(custom)
        .flatten()
        .chain([
            repo_path.join(slug),
            repo_path.join("skills").join(slug),
            repo_path.to_path_buf(),
        ])
        .find(|candidate| candidate.join("SKILL.md").exists())
}

fn is_agents_skill_path(path: &Path, slug: &str) -> bool {
    let expected_suffix = PathBuf::from(".agents").join("skills").join(slug);
    path.ends_with(expected_suffix)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn refuses_paths_outside_skills_directory() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("not-skills").join("foo");
        fs::create_dir_all(&path).expect("create");
        let err = validate_removable_skill_entry(&path).expect_err("should refuse");
        assert!(err.contains("outside a skills directory"), "{err}");
    }

    #[test]
    fn removes_skill_entry_under_skills_root() {
        let temp = tempfile::tempdir().expect("temp");
        let skills = temp.path().join("skills");
        let entry = skills.join("demo-skill");
        fs::create_dir_all(&entry).expect("create skill");
        fs::write(entry.join("SKILL.md"), "---\nname: demo\n---\n").expect("write");

        let result = remove_skill_entries(vec![fs_ops::path_to_string(&entry)]).expect("remove");
        assert_eq!(result.removed.len(), 1);
        assert!(!entry.exists());
        assert!(skills.exists());
    }

    #[test]
    fn removes_broken_symlink_skill_entry() {
        let temp = tempfile::tempdir().expect("temp");
        let skills = temp.path().join("skills");
        fs::create_dir_all(&skills).expect("skills root");
        let entry = skills.join("broken-skill");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(temp.path().join("missing-target"), &entry)
                .expect("broken symlink");
        }
        #[cfg(not(unix))]
        {
            // On non-unix CI, create a normal dir so the command path still runs.
            fs::create_dir_all(&entry).expect("entry");
        }

        let result = remove_skill_entries(vec![fs_ops::path_to_string(&entry)]).expect("remove");
        assert_eq!(result.removed.len(), 1);
        assert!(fs::symlink_metadata(&entry).is_err());
    }
}
