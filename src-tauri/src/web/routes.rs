//! 12 个既有 command 的 HTTP endpoint（薄转发，NFR-2）+ `GET /api/health`。
//!
//! 契约（设计 §2.3）：
//! - `POST /api/commands/{command_name}`，请求 JSON = 参数 map（camelCase，同 tauri invoke）
//! - 200 → 返回值 JSON；422 → 业务错误 `{"error": ...}`；403 → jail/guard 拒绝
//!
//! 每个 endpoint 的请求 struct 逐一定义（serde camelCase），不用 Value 透传。

use super::AppState;
use crate::models::{
    AgentTarget, InstallationRef, ScanOptions, Settings, SyncReplacement,
};
use crate::{registry, scanner, settings, skill_ops, sync_plan};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use std::sync::Arc;

#[derive(Debug, serde::Serialize)]
pub(crate) struct ErrorBody {
    pub error: String,
}

pub(crate) fn error_response(status: StatusCode, message: impl Into<String>) -> Response {
    (
        status,
        Json(ErrorBody {
            error: message.into(),
        }),
    )
        .into_response()
}

/// 核心业务函数返回的 String 错误 → 422。
fn business_error(message: String) -> Response {
    error_response(StatusCode::UNPROCESSABLE_ENTITY, message)
}

/// jail / 参数安全校验拒绝 → 403。
fn rejected(reason: impl Into<String>) -> Response {
    error_response(StatusCode::FORBIDDEN, reason)
}

fn respond(result: Result<impl serde::Serialize, String>) -> Response {
    match result {
        Ok(value) => Json(value).into_response(),
        Err(error) => business_error(error),
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    pub ok: bool,
}

pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { ok: true })
}

// ---------------------------------------------------------------------------
// get_settings / save_settings / scan_inventory / read_inventory_cache
// ---------------------------------------------------------------------------

pub async fn get_settings(State(state): State<Arc<AppState>>) -> Response {
    respond(settings::load_settings(state.ctx()))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveSettingsRequest {
    pub settings: Settings,
}

pub async fn save_settings(
    State(state): State<Arc<AppState>>,
    Json(request): Json<SaveSettingsRequest>,
) -> Response {
    // libraryPath 写时校验：不得指向当前允许根集之外。
    if let Err(error) = state.check_path(&request.settings.library_path) {
        return rejected(error);
    }
    if let Err(error) = settings::save_settings(state.ctx(), &request.settings) {
        return business_error(error);
    }
    match settings::load_settings(state.ctx()) {
        Ok(loaded) => {
            // 允许根集随 settings 变化刷新。
            if let Err(error) = state.refresh_jail(&loaded) {
                return business_error(error);
            }
            Json(loaded).into_response()
        }
        Err(error) => business_error(error),
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanInventoryRequest {
    pub options: Option<ScanOptions>,
}

pub async fn scan_inventory(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ScanInventoryRequest>,
) -> Response {
    let options = request.options.unwrap_or(ScanOptions {
        include_orphaned: false,
    });
    let snapshot = match scanner::scan(state.ctx(), options) {
        Ok(snapshot) => snapshot,
        Err(error) => return business_error(error),
    };
    if let Err(error) = scanner::write_library_index(state.ctx(), &snapshot) {
        return business_error(error);
    }
    if let Err(error) = scanner::write_inventory_cache(state.ctx(), &snapshot) {
        return business_error(error);
    }
    Json(snapshot).into_response()
}

pub async fn read_inventory_cache(State(state): State<Arc<AppState>>) -> Response {
    respond(scanner::read_inventory_cache(state.ctx()))
}

// ---------------------------------------------------------------------------
// read_skill_lock / discover_project_workspaces
// ---------------------------------------------------------------------------

pub async fn read_skill_lock() -> Response {
    respond(skill_ops::read_skill_lock())
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverProjectWorkspacesRequest {
    pub base_path: String,
}

pub async fn discover_project_workspaces(
    State(state): State<Arc<AppState>>,
    Json(request): Json<DiscoverProjectWorkspacesRequest>,
) -> Response {
    if let Err(error) = state.check_path(&request.base_path) {
        return rejected(error);
    }
    let settings = match settings::load_settings(state.ctx()) {
        Ok(settings) => settings,
        Err(error) => return business_error(error),
    };
    respond(registry::discover_project_workspaces(
        &request.base_path,
        &settings,
    ))
}

// ---------------------------------------------------------------------------
// preview_batch_sync / preview_batch_quick_migration / apply_sync_plan
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewBatchSyncRequest {
    pub sources: Vec<InstallationRef>,
    pub targets: Vec<AgentTarget>,
    pub replacements: Option<Vec<SyncReplacement>>,
}

pub async fn preview_batch_sync(
    State(state): State<Arc<AppState>>,
    Json(request): Json<PreviewBatchSyncRequest>,
) -> Response {
    respond(sync_plan::preview_batch_sync(
        state.ctx(),
        request.sources,
        request.targets,
        request.replacements.unwrap_or_default(),
    ))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewBatchQuickMigrationRequest {
    pub sources: Vec<InstallationRef>,
    pub targets: Vec<AgentTarget>,
    pub method: String,
}

pub async fn preview_batch_quick_migration(
    State(state): State<Arc<AppState>>,
    Json(request): Json<PreviewBatchQuickMigrationRequest>,
) -> Response {
    respond(sync_plan::preview_batch_quick_migration(
        state.ctx(),
        request.sources,
        request.targets,
        request.method,
    ))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplySyncPlanRequest {
    pub plan_id: String,
}

pub async fn apply_sync_plan(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ApplySyncPlanRequest>,
) -> Response {
    // planId 校验：仅 [A-Za-z0-9_-]+，防路径遍历（plan 以 id 拼文件路径读取）。
    let valid = !request.plan_id.is_empty()
        && request
            .plan_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if !valid {
        return rejected(format!("Invalid plan id: {}", request.plan_id));
    }
    respond(sync_plan::apply_plan(state.ctx(), request.plan_id))
}

// ---------------------------------------------------------------------------
// check_skills_sh_update / update_skills_sh_skill / remove_skill_entries
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsShUpdateRequest {
    pub slug: String,
    pub entry_path: String,
    pub source_url: String,
    pub skill_path: Option<String>,
}

pub async fn check_skills_sh_update(
    State(state): State<Arc<AppState>>,
    Json(request): Json<SkillsShUpdateRequest>,
) -> Response {
    if let Err(error) = state.check_path(&request.entry_path) {
        return rejected(error);
    }
    respond(skill_ops::check_skills_sh_update(
        state.ctx(),
        request.slug,
        request.entry_path,
        request.source_url,
        request.skill_path,
    ))
}

pub async fn update_skills_sh_skill(
    State(state): State<Arc<AppState>>,
    Json(request): Json<SkillsShUpdateRequest>,
) -> Response {
    if let Err(error) = state.check_path(&request.entry_path) {
        return rejected(error);
    }
    respond(skill_ops::update_skills_sh_skill(
        state.ctx(),
        request.slug,
        request.entry_path,
        request.source_url,
        request.skill_path,
    ))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveSkillEntriesRequest {
    pub paths: Vec<String>,
}

pub async fn remove_skill_entries(
    State(state): State<Arc<AppState>>,
    Json(request): Json<RemoveSkillEntriesRequest>,
) -> Response {
    // 最危险的 endpoint：每个路径都必须落在允许根集内，任一越界整体 403。
    for path in &request.paths {
        if let Err(error) = state.check_path(path) {
            return rejected(error);
        }
    }
    respond(skill_ops::remove_skill_entries(request.paths))
}
