//! 12 个既有 command 的 HTTP endpoint（薄转发，NFR-2）+ 新增 `list_dir`
//! （dir-browser，D3 目录选择替代）+ Round 2 workflows 组（7 个薄转发，
//! 见文件末尾 section）+ Round 3 workflow-update 组（2 个薄转发）+ `GET /api/health`。
//!
//! 只读模式（OMS_READONLY=1，M7）在本层的落点：get_settings 改出
//! PublicSettings 白名单 struct（门-M5）；list_remote_* 强制 refresh=false
//! （门-M2）；export_workflow_package 并入 30/h 限流（门-M3）；web 专用
//! contribute_upload（访客上传贡献，DD §8.3，见文件末尾 section）。
//! 白名单熔断本身在 web/mod.rs 中间件。
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
    AgentTarget, CustomRoot, InstallationRef, ScanOptions, Settings, SyncReplacement,
};
use crate::workflow::Workflow;
use crate::workflow_push::RegistryKind;
use crate::workflow_use::OutputForm;
use crate::{github_auth, registry, scanner, settings, skill_ops, skill_registry, sync_plan, workflow, workflow_push, workflow_registry, workflow_share, workflow_update, workflow_use};
use axum::{
    extract::{ConnectInfo, FromRequestParts, State},
    http::request::Parts,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

/// ConnectInfo 的非失败提取（门-B5 fail-closed 的需要）：axum 自带的
/// ConnectInfo 提取失败是 500 rejection，本 extractor 恒成功、缺失时给
/// None，由 handler 按业务语义判定——只读限流场景 None → 503（宁可拒绝，
/// 不静默放行）。
pub struct MaybeConnectInfo(pub Option<SocketAddr>);

impl<S> FromRequestParts<S> for MaybeConnectInfo
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(
            parts
                .extensions
                .get::<ConnectInfo<SocketAddr>>()
                .map(|info| info.0),
        ))
    }
}

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
    /// 只读模式探测的唯一通道（门-F10/F-14）：前端启动时经 /api/health 取
    /// readonly 存独立 state，决定隐藏全部写入口。
    pub readonly: bool,
}

pub async fn health(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    Json(HealthResponse {
        ok: true,
        readonly: state.readonly(),
    })
}

// ---------------------------------------------------------------------------
// get_settings / save_settings / scan_inventory / read_inventory_cache
//
// 出参裁剪（门-token-F1）：两个 settings endpoint 的响应一律经
// settings::redacted——githubToken 键不出现，hasGithubToken 告知是否已配置。
// ---------------------------------------------------------------------------

/// 只读模式出参（门-M5）：白名单 struct，与 Settings 的 serde 物理隔离——
/// 字段名保留以保前端类型零改动，敏感值置空，serde 层不存在 github_token
/// 键。language 与两个 registry URL 保留真值（前端 i18n 与来源徽标需要）；
/// hasGithubToken 恒 false（访客视角无 token）；readonly 恒 true。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicSettings {
    pub language: String,
    pub workflow_registry_url: Option<String>,
    pub skill_registry_url: Option<String>,
    pub has_github_token: bool,
    pub readonly: bool,
    pub library_path: String,
    pub project_folders: Vec<String>,
    pub custom_roots: Vec<CustomRoot>,
    pub show_raw_paths: bool,
}

pub async fn get_settings(State(state): State<Arc<AppState>>) -> Response {
    if state.readonly() {
        let result = settings::load_settings(state.ctx()).map(|loaded| PublicSettings {
            language: loaded.language,
            workflow_registry_url: loaded.workflow_registry_url,
            skill_registry_url: loaded.skill_registry_url,
            has_github_token: false,
            readonly: true,
            library_path: String::new(),
            project_folders: Vec::new(),
            custom_roots: Vec::new(),
            show_raw_paths: false,
        });
        return respond(result);
    }
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
    // 只读模式强制 refresh=false（门-M2）：匿名访客不得触发 clone+写盘。
    if !request.refresh.unwrap_or(false) || state.readonly() {
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
    connect_info: MaybeConnectInfo,
    Json(request): Json<ExportWorkflowPackageRequest>,
) -> Response {
    // 只读模式并入限流（门-M3 / R9）：export 每次出站 clone，30/h 宽松桶防
    // 匿名访客放大。ConnectInfo 提取失败 fail-closed 503（不静默放行）。
    if state.readonly() {
        let Some(addr) = connect_info.0 else {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "Unable to identify the client address",
            );
        };
        if !state.check_export_rate(addr.ip()) {
            return error_response(
                StatusCode::TOO_MANY_REQUESTS,
                "Export rate limit exceeded (30 per hour); please retry later",
            );
        }
    }
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
    // 只读模式强制 refresh=false（门-M2）：匿名访客不得触发 clone+写盘。
    if !request.refresh.unwrap_or(false) || state.readonly() {
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

// ---------------------------------------------------------------------------
// Round 3 readonly-mode（M7）：contribute_upload——公共只读站的访客上传贡献
// 端点（web 专用，无 tauri 对侧，DD §8.3）。
//
// 链路（顺序即防线顺序）：ConnectInfo 取 IP（提取失败 fail-closed 503，
// 门-B5——宁可拒绝，不静默放行）→ 限流 5/h 滑动窗口 → 20MB 上限（base64
// 字符串先预检再解码）→ 复用 §4.2 安检链全量解包到 staging → 按 kind 校验
// （workflow: 合法 workflow.yaml + validate；skill: SKILL.md + frontmatter，
// slug 先过 [a-z0-9-]+ 再进分支名与 gh 参数）→ M5 contribute_to_official
// （bot token 未配 → Err「站点未开放贡献」）→ gh CLI pr create（--version
// 先探测；GH_TOKEN env；stderr 过 redact_text；失败降级返回分支 compare URL
// 并注明）。staging（data_dir/tmp/upload-{ts}/）成败统一清理。
// ---------------------------------------------------------------------------

/// 访客上传包上限 20MB（R9）。
const MAX_UPLOAD_BYTES: usize = 20 * 1024 * 1024;
/// 对应的 base64 字符串上限（解码前预检，与 §4.2 门-F4 同规则）。
const MAX_UPLOAD_BASE64_LEN: usize = (MAX_UPLOAD_BYTES + 2) / 3 * 4;

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContributeUploadRequest {
    pub kind: String,
    pub archive_base64: String,
}

/// 返回体（DD §8.4：{prUrl?, branchUrl?}）：gh 建 PR 成功 → prUrl；未装/失败
/// 降级 → branchUrl（分支 compare 页，可手动建 PR）+ note 注明。
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContributeUploadResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

pub async fn contribute_upload(
    State(state): State<Arc<AppState>>,
    connect_info: MaybeConnectInfo,
    Json(request): Json<ContributeUploadRequest>,
) -> Response {
    let Some(addr) = connect_info.0 else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "Unable to identify the client address",
        );
    };
    if !state.check_upload_rate(addr.ip()) {
        return error_response(
            StatusCode::TOO_MANY_REQUESTS,
            "Upload rate limit exceeded (5 per hour); please retry later",
        );
    }
    respond(run_contribute_upload(state.ctx(), &request))
}

fn run_contribute_upload(
    ctx: &crate::context::AppContext,
    request: &ContributeUploadRequest,
) -> Result<ContributeUploadResponse, String> {
    let kind = RegistryKind::parse(&request.kind).ok_or_else(|| {
        format!(
            "Invalid kind '{}': expected 'workflow' or 'skill'",
            request.kind
        )
    })?;

    // 20MB 上限（R9）：base64 字符串先预检（解码前拦截），解码后字节复核。
    if request.archive_base64.len() > MAX_UPLOAD_BASE64_LEN {
        return Err(format!(
            "Archive base64 is too long: {} chars (limit {MAX_UPLOAD_BASE64_LEN}, ≈ 20MB archive)",
            request.archive_base64.len()
        ));
    }
    let bytes = workflow_share::decode_archive_base64(&request.archive_base64)?;
    if bytes.len() > MAX_UPLOAD_BYTES {
        return Err(format!(
            "Archive exceeds the 20MB limit: {} bytes",
            bytes.len()
        ));
    }

    // staging：data_dir/tmp/upload-{ts}[-n]/，成败统一清理（含失败分支，
    // 门-readonly 复用 F13 清理约定）。
    let tmp_root = ctx.data_dir().join("tmp");
    crate::fs_ops::ensure_dir(&tmp_root)?;
    let staging = fresh_upload_dir(&tmp_root)?;
    let result = upload_from_staging(ctx, kind, &staging, &bytes);
    let _ = crate::fs_ops::remove_entry(&staging);
    result
}

fn upload_from_staging(
    ctx: &crate::context::AppContext,
    kind: RegistryKind,
    staging: &Path,
    bytes: &[u8],
) -> Result<ContributeUploadResponse, String> {
    workflow_share::unpack_archive(bytes, staging)?;
    let slug = validate_staged_upload(kind, staging)?;
    let outcome = workflow_push::contribute_to_official(ctx, kind, staging, &slug)?;

    // gh 建 PR（R10）：token 上一步已 resolve 成功（否则已 Err 返回）。
    let token = github_auth::resolve_token(ctx)
        .ok_or_else(|| "Contributions are not enabled on this site（站点未开放贡献）".to_string())?;
    let official = match kind {
        RegistryKind::Workflow => settings::OFFICIAL_WORKFLOW_REGISTRY_URL,
        RegistryKind::Skill => settings::OFFICIAL_SKILL_REGISTRY_URL,
    };
    let (owner, repo) = github_auth::parse_owner_repo(official)?;
    let noun = match kind {
        RegistryKind::Workflow => "workflow",
        RegistryKind::Skill => "skill",
    };
    let title = format!("Add {noun} {slug}");

    match create_pr_with_gh("gh", &owner, &repo, &outcome.branch, &title, &token) {
        Some(pr_url) => Ok(ContributeUploadResponse {
            pr_url: Some(pr_url),
            branch_url: None,
            note: None,
        }),
        None => Ok(ContributeUploadResponse {
            pr_url: None,
            branch_url: Some(outcome.branch_url),
            note: Some(
                "gh CLI 不可用或创建 PR 失败：分支已推送到官方注册表，请在该页面手动创建 PR（将由维护者人工审核）"
                    .to_string(),
            ),
        }),
    }
}

/// staging 内容校验并定 slug（DD §8.3）：workflow = 根含合法 workflow.yaml +
/// validate（slug 取 yaml 字段，[a-z0-9-]+ 由 validate 把守）；skill = 根含
/// SKILL.md + 合法 frontmatter（slug 取 frontmatter.name，先过 [a-z0-9-]+
/// 再进分支名与 gh 参数）。
fn validate_staged_upload(kind: RegistryKind, staging: &Path) -> Result<String, String> {
    match kind {
        RegistryKind::Workflow => {
            let file = staging.join("workflow.yaml");
            let text = std::fs::read_to_string(&file).map_err(|error| {
                format!(
                    "Archive must contain workflow.yaml at its root ({}: {error})",
                    crate::fs_ops::path_to_string(&file)
                )
            })?;
            let workflow = Workflow::from_yaml(&text)?;
            if let Err(errors) = workflow.validate() {
                return Err(format!(
                    "Uploaded workflow failed validation: {}",
                    errors.join("; ")
                ));
            }
            Ok(workflow.slug)
        }
        RegistryKind::Skill => {
            let file = staging.join("SKILL.md");
            let text = std::fs::read_to_string(&file).map_err(|error| {
                format!(
                    "Archive must contain SKILL.md at its root ({}: {error})",
                    crate::fs_ops::path_to_string(&file)
                )
            })?;
            let (frontmatter, _body) = scanner::parse_skill_markdown(&text);
            let frontmatter = frontmatter
                .ok_or_else(|| "SKILL.md is missing valid frontmatter".to_string())?;
            let slug = frontmatter
                .name
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    "SKILL.md frontmatter is missing 'name' (it becomes the skill slug)"
                        .to_string()
                })?;
            // 与 workflow_update::is_valid_slug 同规则（[a-z0-9-]+，模块内同
            // 规则拷贝，DD §7 先例）——slug 要进分支名与 gh 参数。
            if !slug
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            {
                return Err(format!(
                    "Invalid skill slug '{slug}': must be non-empty and match [a-z0-9-]+"
                ));
            }
            Ok(slug.to_string())
        }
    }
}

/// 分配空 staging 目录：upload-{UTCts}，同秒冲突追加 -1/-2…。
fn fresh_upload_dir(tmp_root: &Path) -> Result<PathBuf, String> {
    let stamp = Utc::now().format("%Y%m%dT%H%M%SZ");
    for attempt in 0..100u32 {
        let candidate = if attempt == 0 {
            tmp_root.join(format!("upload-{stamp}"))
        } else {
            tmp_root.join(format!("upload-{stamp}-{attempt}"))
        };
        if !candidate.exists() {
            crate::fs_ops::ensure_dir(&candidate)?;
            return Ok(candidate);
        }
    }
    Err("Unable to allocate an upload staging directory".to_string())
}

/// gh CLI 建 PR（R10）：`gh --version` 先探测（未装 → 降级）；GH_TOKEN env
/// 注入 + 禁交互；stderr 过 redact_text 后进服务日志。返回 None = 降级，
/// 调用方回退分支 compare URL。program 参数化以便单测注入假 gh。
pub(crate) fn create_pr_with_gh(
    program: &str,
    owner: &str,
    repo: &str,
    branch: &str,
    title: &str,
    token: &str,
) -> Option<String> {
    if Command::new(program).arg("--version").output().is_err() {
        eprintln!("oms-web: gh CLI not found; falling back to the branch compare URL");
        return None;
    }
    let body = format!(
        "Uploaded via the public read-only site.\n\n\
         ## 贡献自测清单\n\n\
         - [ ] 包目录与 index 条目 slug 一致\n\
         - [ ] index 条目 8 字段完整（slug/name/version/description/author/tags/icon/path）\n\
         - [ ] 内容经维护者人工审核\n"
    );
    let output = Command::new(program)
        .args([
            "pr",
            "create",
            "--repo",
            &format!("{owner}/{repo}"),
            "--base",
            "main",
            "--head",
            branch,
            "--title",
            title,
            "--body",
            &body,
        ])
        .env("GH_TOKEN", token)
        .env("GH_PROMPT_DISABLED", "1")
        .output();
    match output {
        Ok(output) if output.status.success() => {
            let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
            // 防御：gh 成功时 stdout 应为一行 PR URL；不符即降级（不臆造 URL）。
            if url.starts_with("https://github.com/") {
                Some(url)
            } else {
                eprintln!("oms-web: gh pr create returned unexpected output; falling back");
                None
            }
        }
        Ok(output) => {
            let stderr =
                github_auth::redact_text(&String::from_utf8_lossy(&output.stderr), Some(token));
            eprintln!("oms-web: gh pr create failed: {stderr}");
            None
        }
        Err(error) => {
            eprintln!("oms-web: unable to run gh pr create: {error}");
            None
        }
    }
}
