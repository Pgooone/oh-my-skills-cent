//! 12 个既有 command 的 HTTP endpoint（薄转发，NFR-2）+ 新增 `list_dir`
//! （dir-browser，D3 目录选择替代）+ Round 2 workflows 组（7 个薄转发，
//! 见文件末尾 section）+ Round 3 workflow-update 组（2 个薄转发）+ `GET /api/health`。
//!
//! 契约（设计 §2.3）：
//! - `POST /api/commands/{command_name}`，请求 JSON = 参数 map（camelCase，同 tauri invoke）
//! - 200 → 返回值 JSON；422 → 业务错误 `{"error": ...}`；403 → jail/guard 拒绝
//!
//! 每个 endpoint 的请求 struct 逐一定义（serde camelCase），不用 Value 透传。
//!
//! 路径参数 jail 按 D7-R1 分层：
//! - Tier 1 注册/浏览类（list_dir / discover_project_workspaces.basePath /
//!   save_settings.libraryPath）走 `check_browse_path` 宽松规则；
//! - Tier 2 文件变更类（remove_skill_entries / update_skills_sh_skill /
//!   apply_sync_plan，以及只读但目标必然已注册的 check_skills_sh_update）
//!   走 `check_write_path` 严格 jail。

use super::AppState;
use crate::models::{
    AgentTarget, InstallationRef, ScanOptions, Settings, SyncReplacement,
};
use crate::workflow::Workflow;
use crate::workflow_use::OutputForm;
use crate::{registry, scanner, settings, skill_ops, skill_registry, sync_plan, workflow, workflow_push, workflow_registry, workflow_share, workflow_update, workflow_use};
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
//
// 出参裁剪（门-token-F1）：两个 settings endpoint 的响应一律经
// settings::redacted——githubToken 键不出现，hasGithubToken 告知是否已配置。
// ---------------------------------------------------------------------------

pub async fn get_settings(State(state): State<Arc<AppState>>) -> Response {
    respond(settings::load_settings(state.ctx()).map(|loaded| settings::redacted(&loaded)))
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
    // libraryPath 属注册类（Tier 1）：宽松校验，允许指向 home 之下未注册的新位置。
    if let Err(error) = state.check_browse_path(&request.settings.library_path) {
        return rejected(error);
    }
    // 入参校验（两个 *RegistryUrl 拒绝 userinfo/非 GitHub）与 token 三分支合并
    // 单点在核心 save_settings_with_merge。
    match settings::save_settings_with_merge(state.ctx(), &request.settings) {
        Ok(saved) => {
            // 允许根集随 settings 变化刷新。
            if let Err(error) = state.refresh_jail(&saved) {
                return business_error(error);
            }
            Json(settings::redacted(&saved)).into_response()
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
    // basePath 属注册/浏览类（Tier 1）：宽松校验。
    if let Err(error) = state.check_browse_path(&request.base_path) {
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
    // 只读但目标必然来自已注册 skills 目录，维持严格 jail（Tier 2）。
    if let Err(error) = state.check_write_path(&request.entry_path) {
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
    if let Err(error) = state.check_write_path(&request.entry_path) {
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
        if let Err(error) = state.check_write_path(path) {
            return rejected(error);
        }
    }
    respond(skill_ops::remove_skill_entries(request.paths))
}

// ---------------------------------------------------------------------------
// list_dir（dir-browser 新增，D3 目录选择替代）
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListDirRequest {
    pub path: Option<String>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListDirEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListDirResponse {
    pub path: String,
    pub parent: Option<String>,
    pub entries: Vec<ListDirEntry>,
}

pub async fn list_dir(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ListDirRequest>,
) -> Response {
    // 缺省 path = home_dir（永远在浏览规则内，无需校验）。
    let target = match request.path.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        Some(raw) => match state.check_browse_path(raw) {
            Ok(path) => path,
            Err(error) => return rejected(error),
        },
        None => state.ctx().home_dir().to_path_buf(),
    };

    let read_dir = match std::fs::read_dir(&target) {
        Ok(read_dir) => read_dir,
        Err(error) => {
            return business_error(format!(
                "Unable to read directory {}: {error}",
                crate::fs_ops::path_to_string(&target)
            ))
        }
    };

    let mut entries: Vec<ListDirEntry> = read_dir
        .filter_map(|entry| entry.ok())
        .map(|entry| {
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            ListDirEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                path: crate::fs_ops::path_to_string(&entry.path()),
                is_dir,
            }
        })
        .collect();
    // 只列一层，目录在前；各自按名称（大小写不敏感）排序。
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    // 上级仅在它本身也落在浏览规则内时给出，否则为 null（前端隐藏「上级」）。
    let parent = target.parent().and_then(|parent| {
        let raw = crate::fs_ops::path_to_string(parent);
        state.check_browse_path(&raw).ok().map(|_| raw)
    });

    Json(ListDirResponse {
        path: crate::fs_ops::path_to_string(&target),
        parent,
        entries,
    })
    .into_response()
}

// ---------------------------------------------------------------------------
// Round 2 workflows-api：7 个 workflow endpoint 薄转发（NFR-2），apply 复用既有
// apply_sync_plan。D8 guard 由路由层统一覆盖；本组无文件路径参数（slug 由核心
// 校验 [a-z0-9-]+，失败即业务错误 422；download_workflow.path 原样下传，
// traversal 由 registry-client 的 guard_registry_path 把关），jail 不涉及。
// ---------------------------------------------------------------------------

pub async fn list_installed_workflows(State(state): State<Arc<AppState>>) -> Response {
    respond(workflow::list_installed(state.ctx()))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListRemoteWorkflowsRequest {
    pub refresh: Option<bool>,
}

pub async fn list_remote_workflows(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ListRemoteWorkflowsRequest>,
) -> Response {
    // cache-first（lead 裁决）：refresh=true 强制拉取；false/缺省时优先读缓存，
    // 无缓存再回退拉取（fetch_index 自带离线回退旧缓存）。
    if !request.refresh.unwrap_or(false) {
        if let Some(cached) = workflow_registry::read_cached_index(state.ctx()) {
            return Json(cached).into_response();
        }
    }
    let registry_url = match workflow_registry_url(state.ctx()) {
        Ok(url) => url,
        Err(error) => return business_error(error),
    };
    respond(workflow_registry::fetch_index(state.ctx(), &registry_url))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetWorkflowDetailRequest {
    pub slug: String,
}

pub async fn get_workflow_detail(
    State(state): State<Arc<AppState>>,
    Json(request): Json<GetWorkflowDetailRequest>,
) -> Response {
    let workflow = match workflow::load(state.ctx(), &request.slug) {
        Ok(workflow) => workflow,
        Err(error) => return business_error(error),
    };
    match workflow_use::compute_statuses(state.ctx(), &workflow) {
        Ok(statuses) => Json(workflow_use::WorkflowDetail { workflow, statuses }).into_response(),
        Err(error) => business_error(error),
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadWorkflowRequest {
    pub path: String,
}

pub async fn download_workflow(
    State(state): State<Arc<AppState>>,
    Json(request): Json<DownloadWorkflowRequest>,
) -> Response {
    let registry_url = match workflow_registry_url(state.ctx()) {
        Ok(url) => url,
        Err(error) => return business_error(error),
    };
    let slug = match workflow_registry::download_to_installed(
        state.ctx(),
        &registry_url,
        &request.path,
    ) {
        Ok(slug) => slug,
        Err(error) => return business_error(error),
    };
    // M3：下载成功记录来源快照（薄转发 +1 行），供三态更新检查比对。
    if let Err(error) = workflow_update::record_source(state.ctx(), &slug, &registry_url, &request.path) {
        return business_error(error);
    }
    let installed = match workflow::list_installed(state.ctx()) {
        Ok(installed) => installed,
        Err(error) => return business_error(error),
    };
    match installed.into_iter().find(|item| item.slug == slug) {
        Some(item) => Json(item).into_response(),
        None => business_error(format!(
            "Downloaded workflow '{slug}' is missing from installed list"
        )),
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveWorkflowRequest {
    pub workflow: Workflow,
    pub readme: Option<String>,
}

pub async fn save_workflow(
    State(state): State<Arc<AppState>>,
    Json(request): Json<SaveWorkflowRequest>,
) -> Response {
    let slug = request.workflow.slug.clone();
    // 返回裸 slug 字符串（JSON string），与 tauri 侧的 String 返回两壳一致。
    respond(workflow::save(state.ctx(), &request.workflow, request.readme.as_deref()).map(|()| slug))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteWorkflowRequest {
    pub slug: String,
}

pub async fn delete_workflow(
    State(state): State<Arc<AppState>>,
    Json(request): Json<DeleteWorkflowRequest>,
) -> Response {
    respond(workflow::delete(state.ctx(), &request.slug))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewUseWorkflowRequest {
    pub slug: String,
    pub targets: Vec<AgentTarget>,
    pub method: String,
    pub output_form: OutputForm,
}

pub async fn preview_use_workflow(
    State(state): State<Arc<AppState>>,
    Json(request): Json<PreviewUseWorkflowRequest>,
) -> Response {
    respond(workflow_use::preview_use_workflow(
        state.ctx(),
        &request.slug,
        request.targets,
        request.method,
        request.output_form,
    ))
}

// ---------------------------------------------------------------------------
// Round 3 workflow-update（M3）：2 个薄转发。无文件路径参数（slug 由核心
// 校验 [a-z0-9-]+，失败即业务错误 422），jail 不涉及。
// ---------------------------------------------------------------------------

pub async fn check_workflow_updates(State(state): State<Arc<AppState>>) -> Response {
    respond(workflow_update::check_all(state.ctx()))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateWorkflowRequest {
    pub slug: String,
    pub confirm_modified: bool,
}

pub async fn update_workflow(
    State(state): State<Arc<AppState>>,
    Json(request): Json<UpdateWorkflowRequest>,
) -> Response {
    respond(workflow_update::apply_update(
        state.ctx(),
        &request.slug,
        request.confirm_modified,
    ))
}

// ---------------------------------------------------------------------------
// Round 3 workflow-share（M4）：2 个薄转发。无文件路径参数（slug 核心校验），
// jail 不涉及；请求体上限在 web/mod.rs 路由层单独挂 DefaultBodyLimit（门-B4/F4），
// base64 预检/解码与导入校验链全在核心 workflow_share。
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportWorkflowPackageRequest {
    pub slug: String,
}

pub async fn export_workflow_package(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ExportWorkflowPackageRequest>,
) -> Response {
    respond(workflow_share::export_package_base64(
        state.ctx(),
        &request.slug,
    ))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportWorkflowPackageRequest {
    pub archive_base64: String,
}

pub async fn import_workflow_package(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ImportWorkflowPackageRequest>,
) -> Response {
    respond(workflow_share::import_package_base64(
        state.ctx(),
        &request.archive_base64,
    ))
}

/// load_settings 已保证空值回填官方缺省；此处兜底仅为避免解包 panic。
fn skill_registry_url(ctx: &crate::context::AppContext) -> Result<String, String> {
    Ok(settings::load_settings(ctx)?
        .skill_registry_url
        .filter(|url| !url.trim().is_empty())
        .unwrap_or_else(|| settings::OFFICIAL_SKILL_REGISTRY_URL.to_string()))
}

// ---------------------------------------------------------------------------
// Round 3 skill-registry（M6）：4 个薄转发。无文件路径参数（slug 由核心校验
// [a-z0-9-]+，失败即业务错误 422；download_skill.path 仅作 index 查条目、拷贝
// 目标经核心 guard_registry_path 把关），jail 不涉及。
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListRemoteSkillsRequest {
    pub refresh: Option<bool>,
}

pub async fn list_remote_skills(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ListRemoteSkillsRequest>,
) -> Response {
    // cache-first（与 list_remote_workflows 同款裁决）：refresh=true 强制拉取；
    // false/缺省时优先读缓存，无缓存再回退拉取（fetch_index 自带离线回退旧缓存）。
    if !request.refresh.unwrap_or(false) {
        if let Some(cached) = skill_registry::read_cached_index(state.ctx()) {
            return Json(cached).into_response();
        }
    }
    let registry_url = match skill_registry_url(state.ctx()) {
        Ok(url) => url,
        Err(error) => return business_error(error),
    };
    respond(skill_registry::fetch_index(state.ctx(), &registry_url))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadSkillRequest {
    pub path: String,
}

pub async fn download_skill(
    State(state): State<Arc<AppState>>,
    Json(request): Json<DownloadSkillRequest>,
) -> Response {
    let registry_url = match skill_registry_url(state.ctx()) {
        Ok(url) => url,
        Err(error) => return business_error(error),
    };
    // 返回裸 slug 字符串（JSON string），与 tauri 侧的 String 返回两壳一致。
    respond(skill_registry::download_skill(
        state.ctx(),
        &registry_url,
        &request.path,
    ))
}

pub async fn check_registry_skill_updates(State(state): State<Arc<AppState>>) -> Response {
    respond(skill_registry::check_updates(state.ctx()))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRegistrySkillRequest {
    pub slug: String,
}

pub async fn update_registry_skill(
    State(state): State<Arc<AppState>>,
    Json(request): Json<UpdateRegistrySkillRequest>,
) -> Response {
    respond(skill_registry::apply_update(state.ctx(), &request.slug))
}

/// load_settings 已保证空值回填官方缺省；此处兜底仅为避免解包 panic。
fn workflow_registry_url(ctx: &crate::context::AppContext) -> Result<String, String> {
    Ok(settings::load_settings(ctx)?
        .workflow_registry_url
        .filter(|url| !url.trim().is_empty())
        .unwrap_or_else(|| settings::OFFICIAL_WORKFLOW_REGISTRY_URL.to_string()))
}

// ---------------------------------------------------------------------------
// Round 3 workflow-push（M5）：3 个薄转发。无文件路径参数（slug 由核心校验
// [a-z0-9-]+，失败即业务错误 422），jail 不涉及；贡献三态走 Ok 载荷
// （{"status": ...}），Err 只给真错误。
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushWorkflowToRegistryRequest {
    pub slug: String,
}

pub async fn push_workflow_to_registry(
    State(state): State<Arc<AppState>>,
    Json(request): Json<PushWorkflowToRegistryRequest>,
) -> Response {
    respond(workflow_push::push_workflow_to_registry(
        state.ctx(),
        &request.slug,
    ))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContributeWorkflowRequest {
    pub slug: String,
}

pub async fn contribute_workflow(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ContributeWorkflowRequest>,
) -> Response {
    respond(workflow_push::contribute_workflow(
        state.ctx(),
        &request.slug,
    ))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContributeSkillRequest {
    pub slug: String,
}

pub async fn contribute_skill(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ContributeSkillRequest>,
) -> Response {
    respond(workflow_push::contribute_skill(state.ctx(), &request.slug))
}
