//! Web 壳：axum router 构建、共享 state（AppContext + PathJail）、静态服务。
//!
//! 安全护栏集中在这一层：guard 中间件（D8）挂在所有 /api 路由上，
//! PathJail（D7）由 routes 内对路径参数调用。只读模式（OMS_READONLY=1，
//! R1/R2）追加白名单中间件：POST /api/commands/ 默认拒绝，仅放行
//! DD §8.2 订正版名单；D8 guard 同步放宽 Host（公网可达），其余校验不变。

pub mod guard;
pub mod jail;
pub mod routes;

use crate::context::AppContext;
use crate::models::Settings;
use axum::{
    extract::{DefaultBodyLimit, Request, State},
    http::{header, Method, StatusCode, Uri},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use rust_embed::RustEmbed;
use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

/// 胖包上/下行的请求体上限（门-B4/F4）：仅 export/import/contribute_upload
/// 端点挂，其余路由维持框架默认 2MB。base64 形态 50MB 包 ≈ 67MB 字符 + JSON 开销。
const SHARE_BODY_LIMIT: usize = 96 * 1024 * 1024;

/// per-IP 滑动窗口限流器（R9 / 门-M6）：contribute_upload 5/h、只读模式
/// export 并入 30/h 宽松桶。map 容量上限 + 过期淘汰，防公网访客撑爆内存。
pub struct RateLimiter {
    limit: usize,
    window: Duration,
    hits: HashMap<IpAddr, VecDeque<Instant>>,
}

/// 限流 map 容量上限（条）；满员时淘汰最久未活动的条目。
const RATE_LIMIT_CAPACITY: usize = 1024;

impl RateLimiter {
    pub fn new(limit: usize, window: Duration) -> Self {
        Self {
            limit,
            window,
            hits: HashMap::new(),
        }
    }

    /// 计一次访问：窗口内未满 → true（放行）；已满 → false（拒绝）。
    /// now 参数化以便单测注入假时间。
    pub fn check(&mut self, ip: IpAddr, now: Instant) -> bool {
        let window = self.window;
        // 过期淘汰：移出窗口的命中弹出，整条目清空即删除（map 不无界增长）。
        self.hits.retain(|_, deque| {
            while let Some(&oldest) = deque.front() {
                if now.saturating_duration_since(oldest) < window {
                    break;
                }
                deque.pop_front();
            }
            !deque.is_empty()
        });
        if !self.hits.contains_key(&ip) && self.hits.len() >= RATE_LIMIT_CAPACITY {
            // 容量上限：淘汰最久未活动的条目（其计数随之清零——宁可误松一次，
            // 不可内存无界）。
            if let Some(oldest) = self
                .hits
                .iter()
                .max_by_key(|(_, deque)| deque.back().map(|hit| now.saturating_duration_since(*hit)))
                .map(|(key, _)| *key)
            {
                self.hits.remove(&oldest);
            }
        }
        let deque = self.hits.entry(ip).or_default();
        if deque.len() >= self.limit {
            return false;
        }
        deque.push_back(now);
        true
    }
}

/// 共享 state：业务上下文 + 可刷新的路径白名单（save_settings 后重建）+
/// 只读开关与限流桶。
pub struct AppState {
    ctx: AppContext,
    jail: RwLock<jail::PathJail>,
    readonly: bool,
    upload_limiter: Mutex<RateLimiter>,
    export_limiter: Mutex<RateLimiter>,
}

impl AppState {
    /// 构造即预热 load_settings（settings 缺失时启动期初始化写盘，发生在
    /// 启动期而非请求期，门-readonly-F10）。
    pub fn new(ctx: AppContext, readonly: bool) -> Result<Self, String> {
        let settings = crate::settings::load_settings(&ctx)?;
        Ok(Self {
            jail: RwLock::new(jail::PathJail::new(&ctx, &settings)),
            ctx,
            readonly,
            upload_limiter: Mutex::new(RateLimiter::new(5, Duration::from_secs(3600))),
            export_limiter: Mutex::new(RateLimiter::new(30, Duration::from_secs(3600))),
        })
    }

    pub fn ctx(&self) -> &AppContext {
        &self.ctx
    }

    /// 只读模式（OMS_READONLY=1）：白名单熔断、PublicSettings、限流的总开关。
    pub fn readonly(&self) -> bool {
        self.readonly
    }

    /// contribute_upload 限流（5/h 滑动窗口，R9）：true=放行（本次已计数）。
    pub fn check_upload_rate(&self, ip: IpAddr) -> bool {
        self.upload_limiter
            .lock()
            .map(|mut limiter| limiter.check(ip, Instant::now()))
            .unwrap_or(false)
    }

    /// export_workflow_package 只读模式并入的宽松桶（30/h，门-M3）。
    pub fn check_export_rate(&self, ip: IpAddr) -> bool {
        self.export_limiter
            .lock()
            .map(|mut limiter| limiter.check(ip, Instant::now()))
            .unwrap_or(false)
    }

    /// D7 严格级校验入口（Tier 2 文件变更类）：routes 对写操作的路径参数调用。
    pub fn check_write_path(&self, raw: &str) -> Result<PathBuf, String> {
        self.jail
            .read()
            .map_err(|_| "Path jail lock poisoned".to_string())?
            .check_write(raw)
    }

    /// D7-R1 宽松级校验入口（Tier 1 注册/浏览类）：home 子树 + 允许根集 +
    /// Windows 盘符顶层一层。
    pub fn check_browse_path(&self, raw: &str) -> Result<PathBuf, String> {
        self.jail
            .read()
            .map_err(|_| "Path jail lock poisoned".to_string())?
            .check_browse(raw)
    }

    /// save_settings 成功后用新 settings 重建允许根集。
    pub fn refresh_jail(&self, settings: &Settings) -> Result<(), String> {
        let mut jail = self
            .jail
            .write()
            .map_err(|_| "Path jail lock poisoned".to_string())?;
        *jail = jail::PathJail::new(&self.ctx, settings);
        Ok(())
    }
}

pub fn build_router(state: Arc<AppState>) -> Router {
    let api = Router::new()
        .route("/api/health", get(routes::health))
        .route("/api/commands/get_settings", post(routes::get_settings))
        .route("/api/commands/save_settings", post(routes::save_settings))
        .route("/api/commands/scan_inventory", post(routes::scan_inventory))
        .route(
            "/api/commands/read_inventory_cache",
            post(routes::read_inventory_cache),
        )
        .route("/api/commands/read_skill_lock", post(routes::read_skill_lock))
        .route(
            "/api/commands/discover_project_workspaces",
            post(routes::discover_project_workspaces),
        )
        .route(
            "/api/commands/preview_batch_sync",
            post(routes::preview_batch_sync),
        )
        .route(
            "/api/commands/preview_batch_quick_migration",
            post(routes::preview_batch_quick_migration),
        )
        .route("/api/commands/apply_sync_plan", post(routes::apply_sync_plan))
        .route(
            "/api/commands/check_skills_sh_update",
            post(routes::check_skills_sh_update),
        )
        .route(
            "/api/commands/update_skills_sh_skill",
            post(routes::update_skills_sh_skill),
        )
        .route(
            "/api/commands/remove_skill_entries",
            post(routes::remove_skill_entries),
        )
        .route("/api/commands/list_dir", post(routes::list_dir))
        .route(
            "/api/commands/list_installed_workflows",
            post(routes::list_installed_workflows),
        )
        .route(
            "/api/commands/list_remote_workflows",
            post(routes::list_remote_workflows),
        )
        .route(
            "/api/commands/get_workflow_detail",
            post(routes::get_workflow_detail),
        )
        .route(
            "/api/commands/download_workflow",
            post(routes::download_workflow),
        )
        .route("/api/commands/save_workflow", post(routes::save_workflow))
        .route(
            "/api/commands/delete_workflow",
            post(routes::delete_workflow),
        )
        .route(
            "/api/commands/preview_use_workflow",
            post(routes::preview_use_workflow),
        )
        .route(
            "/api/commands/check_workflow_updates",
            post(routes::check_workflow_updates),
        )
        .route(
            "/api/commands/update_workflow",
            post(routes::update_workflow),
        )
        .route(
            "/api/commands/list_remote_skills",
            post(routes::list_remote_skills),
        )
        .route("/api/commands/download_skill", post(routes::download_skill))
        .route(
            "/api/commands/check_registry_skill_updates",
            post(routes::check_registry_skill_updates),
        )
        .route(
            "/api/commands/update_registry_skill",
            post(routes::update_registry_skill),
        )
        .route(
            "/api/commands/push_workflow_to_registry",
            post(routes::push_workflow_to_registry),
        )
        .route(
            "/api/commands/contribute_workflow",
            post(routes::contribute_workflow),
        )
        .route(
            "/api/commands/contribute_skill",
            post(routes::contribute_skill),
        )
        .route(
            "/api/commands/export_workflow_package",
            post(routes::export_workflow_package).layer(DefaultBodyLimit::max(SHARE_BODY_LIMIT)),
        )
        .route(
            "/api/commands/import_workflow_package",
            post(routes::import_workflow_package).layer(DefaultBodyLimit::max(SHARE_BODY_LIMIT)),
        )
        .route(
            "/api/commands/contribute_upload",
            post(routes::contribute_upload).layer(DefaultBodyLimit::max(SHARE_BODY_LIMIT)),
        )
        // D8 + R2：所有 /api 请求先过 guard（Host / Origin / Sec-Fetch-Site），
        // readonly 模式再叠白名单熔断。后注册的 route_layer 先执行（guard 最外层）。
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            readonly_whitelist_guard,
        ))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            guard::access_guard,
        ));

    Router::new()
        .merge(api)
        .fallback(static_handler)
        .with_state(state)
}

/// R2 只读熔断（白名单制默认拒绝，DD §8.2 全量对账订正版）：readonly 模式下
/// POST /api/commands/ 仅放行下列 command，其余一律 403（含 scan_inventory——
/// 它写盘点缓存；list_dir/discover_project_workspaces 会枚举 home/data_dir
/// 暴露面，一并移出）。/api/health 是前端探测 readonly 的唯一通道，放行。
const READONLY_COMMANDS: [&str; 9] = [
    "read_inventory_cache",
    "read_skill_lock",
    "get_settings",
    "list_installed_workflows",
    "list_remote_workflows",
    "get_workflow_detail",
    "list_remote_skills",
    "export_workflow_package",
    "contribute_upload",
];

async fn readonly_whitelist_guard(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    if !state.readonly() {
        return next.run(request).await;
    }
    let path = request.uri().path();
    if path == "/api/health" {
        return next.run(request).await;
    }
    let allowed = path
        .strip_prefix("/api/commands/")
        .is_some_and(|name| READONLY_COMMANDS.contains(&name));
    if allowed {
        next.run(request).await
    } else {
        routes::error_response(
            StatusCode::FORBIDDEN,
            "This command is not available in read-only mode",
        )
    }
}

// ---------------------------------------------------------------------------
// 静态服务（rust-embed）：`/` → index.html，嵌入资源命中即返回，其余 GET
// fallback index.html（单页无路由库）。debug 构建经 debug-embed 从磁盘读
// ../dist，release 才真正内嵌进二进制。
// ---------------------------------------------------------------------------

#[derive(RustEmbed)]
#[folder = "../dist"]
struct FrontendAssets;

async fn static_handler(method: Method, uri: Uri) -> Response {
    if method != Method::GET && method != Method::HEAD {
        return routes::error_response(StatusCode::NOT_FOUND, "Not found");
    }
    let path = uri.path().trim_start_matches('/');
    if path == "api" || path.starts_with("api/") {
        return routes::error_response(StatusCode::NOT_FOUND, "Unknown API endpoint");
    }
    // debug-embed 构建从磁盘读资源：拒绝含 `..` 段的路径（目录遍历加固，
    // 门-readonly-F12）。
    if path.split('/').any(|segment| segment == "..") {
        return routes::error_response(
            StatusCode::FORBIDDEN,
            "Parent path segments are not allowed",
        );
    }

    let path = if path.is_empty() { "index.html" } else { path };
    match embedded_response(path) {
        Some(response) => response,
        None => embedded_response("index.html")
            .unwrap_or_else(|| routes::error_response(StatusCode::NOT_FOUND, "Not found")),
    }
}

fn embedded_response(path: &str) -> Option<Response> {
    FrontendAssets::get(path).map(|file| {
        let mime = file.metadata.mimetype().to_string();
        ([(header::CONTENT_TYPE, mime)], file.data.into_owned()).into_response()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use tower::ServiceExt;

    fn test_state() -> (tempfile::TempDir, Arc<AppState>) {
        let temp = tempfile::tempdir().expect("temp");
        let ctx = AppContext::new(temp.path().join("data"), temp.path().join("home"));
        let state = AppState::new(ctx, false).expect("state");
        (temp, Arc::new(state))
    }

    /// 只读模式实例（OMS_READONLY=1 形态）。
    fn test_state_readonly() -> (tempfile::TempDir, Arc<AppState>) {
        let temp = tempfile::tempdir().expect("temp");
        let ctx = AppContext::new(temp.path().join("data"), temp.path().join("home"));
        let state = AppState::new(ctx, true).expect("state");
        (temp, Arc::new(state))
    }

    fn post_json(uri: &str, body: &str) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("host", "localhost:8477")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .expect("request")
    }

    async fn body_string(response: Response) -> String {
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        String::from_utf8(bytes.to_vec()).expect("utf8")
    }

    // -- health --------------------------------------------------------------

    #[tokio::test]
    async fn health_returns_ok() {
        let (_temp, state) = test_state();
        let app = build_router(state);

        let request = Request::builder()
            .uri("/api/health")
            .header("host", "localhost:8477")
            .body(Body::empty())
            .expect("request");
        let response = app.oneshot(request).await.expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_string(response).await, r#"{"ok":true,"readonly":false}"#);
    }

    // -- guard 负向 -----------------------------------------------------------

    #[tokio::test]
    async fn rejects_non_localhost_host() {
        let (_temp, state) = test_state();
        let app = build_router(state);

        for host in ["evil.example.com", "localhost.evil.com", "0.0.0.0:8477"] {
            let request = Request::builder()
                .uri("/api/health")
                .header("host", host)
                .body(Body::empty())
                .expect("request");
            let response = app.clone().oneshot(request).await.expect("response");
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "host: {host}");
        }
    }

    #[tokio::test]
    async fn rejects_cross_origin_post() {
        let (_temp, state) = test_state();
        let app = build_router(state);

        let request = Request::builder()
            .method("POST")
            .uri("/api/commands/get_settings")
            .header("host", "localhost:8477")
            .header("content-type", "application/json")
            .header("origin", "http://evil.example.com")
            .body(Body::from("{}"))
            .expect("request");
        let response = app.oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn rejects_sec_fetch_site_cross_site() {
        let (_temp, state) = test_state();
        let app = build_router(state);

        let request = Request::builder()
            .uri("/api/health")
            .header("host", "127.0.0.1:8477")
            .header("sec-fetch-site", "cross-site")
            .body(Body::empty())
            .expect("request");
        let response = app.oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn accepts_same_origin_post() {
        let (_temp, state) = test_state();
        let app = build_router(state);

        let request = Request::builder()
            .method("POST")
            .uri("/api/commands/get_settings")
            .header("host", "localhost:8477")
            .header("content-type", "application/json")
            .header("origin", "http://localhost:8477")
            .body(Body::from("{}"))
            .expect("request");
        let response = app.oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::OK);
    }

    // -- jail 负向（HTTP 层）---------------------------------------------------

    #[tokio::test]
    async fn rejects_dotdot_path_param() {
        let (_temp, state) = test_state();
        let app = build_router(state);

        let response = app
            .oneshot(post_json(
                "/api/commands/discover_project_workspaces",
                r#"{"basePath":"/tmp/../etc"}"#,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn rejects_path_outside_whitelist() {
        let (_temp, state) = test_state();
        let app = build_router(state);

        let response = app
            .oneshot(post_json(
                "/api/commands/remove_skill_entries",
                r#"{"paths":["/definitely/not/allowed/skills/demo"]}"#,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn rejects_traversal_plan_id() {
        let (_temp, state) = test_state();
        let app = build_router(state);

        let response = app
            .oneshot(post_json(
                "/api/commands/apply_sync_plan",
                r#"{"planId":"../settings"}"#,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    // -- endpoint 正例 ---------------------------------------------------------

    #[tokio::test]
    async fn get_and_save_settings_roundtrip() {
        let (temp, state) = test_state();
        let app = build_router(state.clone());

        let response = app
            .clone()
            .oneshot(post_json("/api/commands/get_settings", "{}"))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_string(response).await;
        let mut current: crate::models::Settings =
            serde_json::from_str(&body).expect("settings json");

        // libraryPath 改到 data_dir 下（当前允许根集之内），同时改语言。
        current.library_path =
            crate::fs_ops::path_to_string(&temp.path().join("data").join("library"));
        current.language = "en".to_string();
        let body = serde_json::json!({ "settings": current }).to_string();

        let response = app
            .clone()
            .oneshot(post_json("/api/commands/save_settings", &body))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let saved: crate::models::Settings =
            serde_json::from_str(&body_string(response).await).expect("saved json");
        assert_eq!(saved.language, "en");
        assert_eq!(saved.library_path, current.library_path);

        // 保存后 jail 已刷新：新 library 下的路径可通过写级校验。
        let new_entry = format!("{}/demo", saved.library_path);
        assert!(state.check_write_path(&new_entry).is_ok());
    }

    // D7-R1：libraryPath 属注册类，拒绝的语义从「允许根集之外」放宽为
    // 「home 子树之外（且非允许根、非盘符顶层）」。
    #[tokio::test]
    async fn save_settings_rejects_library_path_outside_home() {
        let (_temp, state) = test_state();
        let app = build_router(state);

        let body = serde_json::json!({
            "settings": {
                "libraryPath": "/etc/oms-library",
                "projectFolders": [],
                "customRoots": [],
                "showRawPaths": false,
                "language": "zh-CN"
            }
        })
        .to_string();
        let response = app
            .oneshot(post_json("/api/commands/save_settings", &body))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    // D7-R1 正例：home 之下未注册的新路径必须被接受（旧严格 jail 会 403）。
    #[tokio::test]
    async fn save_settings_accepts_unregistered_library_path_under_home() {
        let (temp, state) = test_state();
        let app = build_router(state);

        let new_library =
            crate::fs_ops::path_to_string(&temp.path().join("home").join("brand-new-library"));
        let body = serde_json::json!({
            "settings": {
                "libraryPath": new_library,
                "projectFolders": [],
                "customRoots": [],
                "showRawPaths": false,
                "language": "zh-CN"
            }
        })
        .to_string();
        let response = app
            .oneshot(post_json("/api/commands/save_settings", &body))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let saved: crate::models::Settings =
            serde_json::from_str(&body_string(response).await).expect("saved json");
        assert_eq!(saved.library_path, new_library);
    }

    // -- settings 出参裁剪（门-token-F1/R3）------------------------------------

    #[tokio::test]
    async fn settings_responses_never_expose_github_token() {
        let (temp, state) = test_state();
        let app = build_router(state);
        let library = crate::fs_ops::path_to_string(&temp.path().join("home").join("library"));

        // 保存带 token 的设置 → save_settings 响应：无 githubToken 键 + hasGithubToken=true。
        let body = serde_json::json!({
            "settings": {
                "libraryPath": library,
                "projectFolders": [],
                "customRoots": [],
                "showRawPaths": false,
                "language": "zh-CN",
                "githubToken": "ghp_web_secret"
            }
        })
        .to_string();
        let response = app
            .clone()
            .oneshot(post_json("/api/commands/save_settings", &body))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let saved: serde_json::Value =
            serde_json::from_str(&body_string(response).await).expect("saved json");
        assert!(
            saved.as_object().expect("object").get("githubToken").is_none(),
            "save 响应不得含 githubToken 键: {saved}"
        );
        assert_eq!(saved["hasGithubToken"].as_bool(), Some(true));

        // get_settings 响应同规则。
        let response = app
            .clone()
            .oneshot(post_json("/api/commands/get_settings", "{}"))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let fetched: serde_json::Value =
            serde_json::from_str(&body_string(response).await).expect("get json");
        assert!(
            fetched.as_object().expect("object").get("githubToken").is_none(),
            "get 响应不得含 githubToken 键: {fetched}"
        );
        assert_eq!(fetched["hasGithubToken"].as_bool(), Some(true));

        // 改无关设置（回传裁剪体，无 token）→ token 不动。
        let mut echo = fetched.clone();
        echo["language"] = serde_json::Value::from("en");
        let body = serde_json::json!({ "settings": echo }).to_string();
        let response = app
            .clone()
            .oneshot(post_json("/api/commands/save_settings", &body))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let saved: serde_json::Value =
            serde_json::from_str(&body_string(response).await).expect("saved json");
        assert_eq!(
            saved["hasGithubToken"].as_bool(),
            Some(true),
            "改无关设置 token 不动"
        );
        let disk =
            std::fs::read_to_string(temp.path().join("data").join("settings.json")).expect("disk");
        assert!(disk.contains("ghp_web_secret"), "token 正常落盘（R3）");

        // 显式清除 → hasGithubToken=false，落盘无 token。
        let mut clearing = saved.clone();
        clearing["clearGithubToken"] = serde_json::Value::from(true);
        let body = serde_json::json!({ "settings": clearing }).to_string();
        let response = app
            .oneshot(post_json("/api/commands/save_settings", &body))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let cleared: serde_json::Value =
            serde_json::from_str(&body_string(response).await).expect("cleared json");
        assert_eq!(cleared["hasGithubToken"].as_bool(), Some(false));
        let disk =
            std::fs::read_to_string(temp.path().join("data").join("settings.json")).expect("disk");
        assert!(!disk.contains("ghp_web_secret"), "显式清除后落盘无 token");
    }

    #[tokio::test]
    async fn save_settings_rejects_userinfo_and_non_github_registry_urls() {
        let (temp, state) = test_state();
        let app = build_router(state);
        let library = crate::fs_ops::path_to_string(&temp.path().join("home").join("library"));

        for (field, value) in [
            ("workflowRegistryUrl", "https://user:pw@github.com/owner/repo.git"),
            ("skillRegistryUrl", "https://user:pw@github.com/owner/repo.git"),
            ("workflowRegistryUrl", "https://gitlab.com/owner/repo.git"),
            ("skillRegistryUrl", "https://gitlab.com/owner/repo.git"),
        ] {
            let body = serde_json::json!({
                "settings": {
                    "libraryPath": library,
                    "projectFolders": [],
                    "customRoots": [],
                    "showRawPaths": false,
                    "language": "zh-CN",
                    field: value
                }
            })
            .to_string();
            let response = app
                .clone()
                .oneshot(post_json("/api/commands/save_settings", &body))
                .await
                .expect("response");
            assert_eq!(
                response.status(),
                StatusCode::UNPROCESSABLE_ENTITY,
                "{field}={value}"
            );
        }
    }

    // -- list_dir（dir-browser）------------------------------------------------

    #[tokio::test]
    async fn list_dir_defaults_to_home_and_sorts_dirs_first() {
        let (temp, state) = test_state();
        let home = temp.path().join("home");
        std::fs::create_dir_all(home.join("zeta-dir")).expect("mkdir");
        std::fs::create_dir_all(home.join("alpha-dir")).expect("mkdir");
        std::fs::write(home.join("notes.txt"), "hi").expect("write");
        let app = build_router(state);

        let response = app
            .oneshot(post_json("/api/commands/list_dir", "{}"))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_str(&body_string(response).await).expect("json");

        assert_eq!(
            body["path"].as_str().expect("path"),
            crate::fs_ops::path_to_string(&home)
        );
        let entries = body["entries"].as_array().expect("entries");
        let names: Vec<&str> = entries.iter().map(|e| e["name"].as_str().unwrap()).collect();
        // load_settings 可能已在 home 下创建默认中心库目录（.oh-my-skills），
        // 只断言本测试创建条目的相对顺序：目录在前、各自按名称排序。
        let created: Vec<&str> = names
            .into_iter()
            .filter(|name| ["alpha-dir", "zeta-dir", "notes.txt"].contains(name))
            .collect();
        assert_eq!(created, vec!["alpha-dir", "zeta-dir", "notes.txt"]);
        assert_eq!(entries[0]["isDir"], true);
        let notes = entries.iter().find(|e| e["name"] == "notes.txt").expect("notes");
        assert_eq!(notes["isDir"], false);
    }

    #[tokio::test]
    async fn list_dir_accepts_unregistered_path_under_home() {
        let (temp, state) = test_state();
        let home = temp.path().join("home");
        std::fs::create_dir_all(home.join("fresh")).expect("mkdir");
        let app = build_router(state);

        let body = serde_json::json!({ "path": crate::fs_ops::path_to_string(&home.join("fresh")) })
            .to_string();
        let response = app
            .oneshot(post_json("/api/commands/list_dir", &body))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let json: serde_json::Value =
            serde_json::from_str(&body_string(response).await).expect("json");
        // 上级 = home，在浏览规则内，应当给出。
        assert_eq!(
            json["parent"].as_str().expect("parent"),
            crate::fs_ops::path_to_string(&home)
        );
    }

    #[tokio::test]
    async fn list_dir_rejects_path_outside_browsable_area() {
        let (_temp, state) = test_state();
        let app = build_router(state);

        let response = app
            .oneshot(post_json(
                "/api/commands/list_dir",
                r#"{"path":"/definitely/outside/home"}"#,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn discover_accepts_unregistered_base_path_under_home() {
        let (temp, state) = test_state();
        let home = temp.path().join("home");
        std::fs::create_dir_all(home.join("scan-me")).expect("mkdir");
        let app = build_router(state);

        let body = serde_json::json!({
            "basePath": crate::fs_ops::path_to_string(&home.join("scan-me"))
        })
        .to_string();
        let response = app
            .oneshot(post_json("/api/commands/discover_project_workspaces", &body))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
    }

    // -- workflows（Round 2 workflows-api）-----------------------------------

    const ROUND_TRIP_WORKFLOW_JSON: &str = r#"{
        "name": "回测流程",
        "slug": "oms-web-round-trip",
        "version": "0.1.0",
        "description": "端点回测",
        "groups": [{"id": "g", "name": "组"}],
        "steps": [{
            "name": "步骤一",
            "group": "g",
            "skills": [
                {"sourceType": "github", "sourceUrl": "https://github.com/mattpocock/skills.git", "slug": "oms-web-test-missing"},
                {"placeholder": "待补充"}
            ]
        }]
    }"#;

    #[tokio::test]
    async fn workflows_save_list_delete_round_trip() {
        let (_temp, state) = test_state();
        let app = build_router(state);

        // 初始为空
        let response = app
            .clone()
            .oneshot(post_json("/api/commands/list_installed_workflows", "{}"))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_str(&body_string(response).await).expect("json");
        assert_eq!(body.as_array().expect("array").len(), 0);

        // save → 200 + 返回 slug
        let save_body = serde_json::json!({
            "workflow": serde_json::from_str::<serde_json::Value>(ROUND_TRIP_WORKFLOW_JSON)
                .expect("workflow json"),
            "readme": "# 回测"
        })
        .to_string();
        let response = app
            .clone()
            .oneshot(post_json("/api/commands/save_workflow", &save_body))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_str(&body_string(response).await).expect("json");
        // 返回裸 slug 字符串（与 tauri 侧 String 返回一致）
        assert_eq!(
            body.as_str().expect("slug string"),
            "oms-web-round-trip"
        );

        // list → 含步骤数与占位标记
        let response = app
            .clone()
            .oneshot(post_json("/api/commands/list_installed_workflows", "{}"))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_str(&body_string(response).await).expect("json");
        let items = body.as_array().expect("array");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["slug"].as_str().expect("slug"), "oms-web-round-trip");
        assert_eq!(items[0]["stepCount"].as_u64().expect("stepCount"), 1);
        assert_eq!(
            items[0]["hasPlaceholder"].as_bool().expect("hasPlaceholder"),
            true
        );

        // delete → 200
        let response = app
            .clone()
            .oneshot(post_json(
                "/api/commands/delete_workflow",
                r#"{"slug":"oms-web-round-trip"}"#,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);

        // 再次 list → 空
        let response = app
            .oneshot(post_json("/api/commands/list_installed_workflows", "{}"))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_str(&body_string(response).await).expect("json");
        assert_eq!(body.as_array().expect("array").len(), 0);
    }

    #[tokio::test]
    async fn get_workflow_detail_returns_workflow_and_aligned_statuses() {
        let (temp, state) = test_state();
        // 中心库预置 ready skill；工作流引用 ready + missing + 占位
        let ready = temp
            .path()
            .join("home")
            .join(".oh-my-skills")
            .join("skills")
            .join("oms-web-ready");
        std::fs::create_dir_all(&ready).expect("ready dir");
        std::fs::write(
            ready.join("SKILL.md"),
            "---\nname: oms-web-ready\ndescription: ready\n---\n",
        )
        .expect("ready SKILL.md");
        let dir = temp
            .path()
            .join("data")
            .join("workflows")
            .join("oms-web-detail-flow");
        std::fs::create_dir_all(&dir).expect("workflow dir");
        std::fs::write(
            dir.join("workflow.yaml"),
            "name: 详情流程\n\
             slug: oms-web-detail-flow\n\
             version: 0.1.0\n\
             description: 详情回测\n\
             groups:\n  - id: g\n    name: 组\n\
             steps:\n  - name: 步骤一\n    group: g\n    skills:\n\
             \x20     - sourceType: github\n\
             \x20       sourceUrl: https://github.com/mattpocock/skills.git\n\
             \x20       slug: oms-web-ready\n\
             \x20     - sourceType: github\n\
             \x20       sourceUrl: https://github.com/mattpocock/skills.git\n\
             \x20       slug: oms-web-test-missing\n\
             \x20     - placeholder: 待补充\n",
        )
        .expect("write workflow yaml");
        let app = build_router(state);

        let response = app
            .oneshot(post_json(
                "/api/commands/get_workflow_detail",
                r#"{"slug":"oms-web-detail-flow"}"#,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_str(&body_string(response).await).expect("json");

        assert_eq!(
            body["workflow"]["slug"].as_str().expect("slug"),
            "oms-web-detail-flow"
        );
        assert_eq!(
            body["workflow"]["steps"][0]["name"].as_str().expect("step name"),
            "步骤一"
        );
        // statuses 外层对齐 steps、内层对齐 skills；每项为 [view, status] 二元
        // 数组；status serde 形状："ready" / "missing" / {"placeholder": "..."}
        let statuses = body["statuses"].as_array().expect("statuses");
        assert_eq!(statuses.len(), 1);
        let first = statuses[0].as_array().expect("step statuses");
        assert_eq!(first.len(), 3);
        assert_eq!(first[0][0]["kind"].as_str().expect("kind"), "ref");
        assert_eq!(
            first[0][0]["slug"].as_str().expect("view slug"),
            "oms-web-ready"
        );
        assert_eq!(first[0][1], serde_json::json!("ready"));
        assert_eq!(first[1][1], serde_json::json!("missing"));
        assert_eq!(
            first[2][0]["kind"].as_str().expect("kind"),
            "placeholder"
        );
        assert_eq!(
            first[2][1],
            serde_json::json!({"placeholder": "待补充"})
        );
    }

    #[tokio::test]
    async fn list_remote_workflows_cache_first_without_network() {
        let (temp, state) = test_state();
        // 铺注册表缓存（不经 git clone）：current/index.json；alpha 已安装
        let current = temp.path().join("data").join("registry").join("current");
        std::fs::create_dir_all(&current).expect("cache dir");
        std::fs::write(
            current.join("index.json"),
            concat!(
                "{\"version\":1,\"workflows\":[",
                "{\"slug\":\"alpha-flow\",\"name\":\"Alpha\",\"version\":\"0.1.0\",",
                "\"description\":\"a\",\"path\":\"alpha-flow\"},",
                "{\"slug\":\"beta-flow\",\"name\":\"Beta\",\"version\":\"0.2.0\",",
                "\"description\":\"b\",\"path\":\"flows/beta-flow\"}",
                "]}"
            ),
        )
        .expect("cached index");
        std::fs::create_dir_all(
            temp.path().join("data").join("workflows").join("alpha-flow"),
        )
        .expect("installed dir");
        let app = build_router(state);

        // refresh 缺省 → cache-first 直返（零网络）
        let response = app
            .clone()
            .oneshot(post_json("/api/commands/list_remote_workflows", "{}"))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_str(&body_string(response).await).expect("json");
        let items = body.as_array().expect("array");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["slug"].as_str().expect("slug"), "alpha-flow");
        assert_eq!(items[0]["installed"].as_bool().expect("installed"), true);
        assert_eq!(items[1]["slug"].as_str().expect("slug"), "beta-flow");
        assert_eq!(items[1]["installed"].as_bool().expect("installed"), false);

        // refresh=false 显式同样走缓存
        let response = app
            .oneshot(post_json(
                "/api/commands/list_remote_workflows",
                r#"{"refresh":false}"#,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn preview_use_workflow_generates_plan() {
        let (temp, state) = test_state();
        // fixture：含一个中心库缺失的 ref + 一个占位（yaml 直接落盘，
        // 核心 load 时校验通过：GitHub 来源）
        let dir = temp
            .path()
            .join("data")
            .join("workflows")
            .join("oms-web-preview-flow");
        std::fs::create_dir_all(&dir).expect("workflow dir");
        std::fs::write(
            dir.join("workflow.yaml"),
            "name: 预览流程\n\
             slug: oms-web-preview-flow\n\
             version: 0.1.0\n\
             description: 预览回测\n\
             groups:\n  - id: g\n    name: 组\n\
             steps:\n  - name: 步骤一\n    group: g\n    skills:\n\
             \x20     - sourceType: github\n\
             \x20       sourceUrl: https://github.com/mattpocock/skills.git\n\
             \x20       slug: oms-web-test-missing\n\
             \x20     - placeholder: 待补充\n",
        )
        .expect("write workflow yaml");
        let app = build_router(state);

        // 未知 agent id：roots 解析为空 → blocked 提示，plan 仍生成（ops 仅下载段）
        let body = serde_json::json!({
            "slug": "oms-web-preview-flow",
            "targets": [{"agentId": "no-such-agent-xyz"}],
            "method": "copy",
            "outputForm": "entryManifest"
        })
        .to_string();
        let response = app
            .oneshot(post_json("/api/commands/preview_use_workflow", &body))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_str(&body_string(response).await).expect("json");

        assert_eq!(body["kind"].as_str().expect("kind"), "workflow-use");
        let operations = body["operations"].as_array().expect("operations");
        assert_eq!(operations.len(), 1);
        assert_eq!(
            operations[0]["opType"].as_str().expect("opType"),
            "download-to-library"
        );
        assert_eq!(
            operations[0]["skillId"].as_str().expect("skillId"),
            "oms-web-test-missing"
        );
        let preconditions = body["preconditions"].as_array().expect("preconditions");
        assert!(
            preconditions
                .iter()
                .any(|item| item.as_str().unwrap_or_default().contains("占位")),
            "preconditions: {preconditions:?}"
        );
        assert_eq!(
            body["riskLevel"].as_str().expect("riskLevel"),
            "blocked"
        );
    }

    // -- workflows 负例：坏 slug → 422（核心校验 [a-z0-9-]+ 浮出为业务错误）-----

    #[tokio::test]
    async fn workflow_endpoints_reject_bad_slugs() {
        let (_temp, state) = test_state();
        let app = build_router(state);

        for slug in ["..", "../settings", "UPPER", "a/b", ""] {
            let body = serde_json::json!({ "slug": slug }).to_string();
            let response = app
                .clone()
                .oneshot(post_json("/api/commands/delete_workflow", &body))
                .await
                .expect("response");
            assert_eq!(
                response.status(),
                StatusCode::UNPROCESSABLE_ENTITY,
                "delete slug '{slug}'"
            );

            let response = app
                .clone()
                .oneshot(post_json("/api/commands/get_workflow_detail", &body))
                .await
                .expect("response");
            assert_eq!(
                response.status(),
                StatusCode::UNPROCESSABLE_ENTITY,
                "detail slug '{slug}'"
            );

            let body = serde_json::json!({
                "slug": slug,
                "targets": [],
                "method": "copy",
                "outputForm": "entryManifest"
            })
            .to_string();
            let response = app
                .clone()
                .oneshot(post_json("/api/commands/preview_use_workflow", &body))
                .await
                .expect("response");
            assert_eq!(
                response.status(),
                StatusCode::UNPROCESSABLE_ENTITY,
                "preview slug '{slug}'"
            );
        }

        // save：workflow.slug 非法 → validate 拒绝，不落盘
        let mut workflow: serde_json::Value =
            serde_json::from_str(ROUND_TRIP_WORKFLOW_JSON).expect("workflow json");
        workflow["slug"] = serde_json::Value::from("Bad Slug");
        let body = serde_json::json!({ "workflow": workflow }).to_string();
        let response = app
            .oneshot(post_json("/api/commands/save_workflow", &body))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    // -- workflow-update（Round 3 M3）-----------------------------------------
    //
    // 真实流程造数据（复用 workflow_update::test_support）：本地 fixture 注册表
    // git 仓库 → 真 clone 铺 registry/current 缓存；settings 指向必然 clone
    // 失败的 GitHub 形态 URL（有无网络都失败）→ fetch_index /
    // download_to_installed 走生产级离线回退读预置缓存，结果确定性。

    use crate::workflow_update::test_support;

    /// AppState + settings 指向 UNCLONEABLE_URL + 预 clone 的 v1 缓存。
    /// 返回的 fixture TempDir 须由调用方持有存活（缓存 clone 自该仓库）。
    fn test_state_with_update_fixture() -> (tempfile::TempDir, Arc<AppState>, tempfile::TempDir) {
        let fixture = test_support::fixture_repo();
        let (temp, state) = test_state();
        test_support::point_registry_at_uncloneable_url(state.ctx());
        test_support::refresh_cache_from_fixture(
            state.ctx(),
            &test_support::repo_source(&fixture),
        );
        (temp, state, fixture)
    }

    #[tokio::test]
    async fn workflow_update_endpoints_full_round_trip() {
        let (temp, state, fixture) = test_state_with_update_fixture();
        let app = build_router(state.clone());

        // download（真实 download_to_installed 离线回退缓存）→ 200；
        // 薄转发 +1 行 record_source 接线验证：source 元数据落盘且哈希一致。
        let response = app
            .clone()
            .oneshot(post_json(
                "/api/commands/download_workflow",
                r#"{"path":"alpha-flow"}"#,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let installed = temp.path().join("data").join("workflows").join("alpha-flow");
        assert!(installed.join("workflow.yaml").is_file());
        let source_file = temp
            .path()
            .join("data")
            .join("workflow-sources")
            .join("alpha-flow.json");
        assert!(source_file.is_file(), "record_source 应随下载落盘");
        let meta: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&source_file).expect("source"))
                .expect("source json");
        assert_eq!(meta["path"].as_str(), Some("alpha-flow"));
        assert_eq!(
            meta["contentHash"].as_str().expect("contentHash"),
            crate::fs_ops::hash_dir(&installed).expect("hash")
        );

        // check → upToDate（下载完成立即检查即最新）
        let response = app
            .clone()
            .oneshot(post_json("/api/commands/check_workflow_updates", "{}"))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_str(&body_string(response).await).expect("json");
        let items = body.as_array().expect("array");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["slug"].as_str(), Some("alpha-flow"));
        assert_eq!(items[0]["state"]["kind"].as_str(), Some("upToDate"));
        assert_eq!(items[0]["localVersion"].as_str(), Some("0.1.0"));

        // 发布 v2 并刷新缓存 → check → updateAvailable
        let repo = fixture.path().join("repo");
        test_support::write_alpha(
            &repo,
            test_support::ALPHA_YAML_V2,
            test_support::ALPHA_README_V2,
            "0.2.0",
        );
        test_support::commit_fixture(&repo, "alpha v2");
        test_support::refresh_cache_from_fixture(
            state.ctx(),
            &test_support::repo_source(&fixture),
        );
        let response = app
            .clone()
            .oneshot(post_json("/api/commands/check_workflow_updates", "{}"))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_str(&body_string(response).await).expect("json");
        let items = body.as_array().expect("array");
        assert_eq!(items[0]["state"]["kind"].as_str(), Some("updateAvailable"));
        assert_eq!(items[0]["state"]["remoteVersion"].as_str(), Some("0.2.0"));

        // update → 200 upToDate 0.2.0 + 备份产生 + 安装内容 == 缓存内容
        let response = app
            .clone()
            .oneshot(post_json(
                "/api/commands/update_workflow",
                r#"{"slug":"alpha-flow","confirmModified":false}"#,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_str(&body_string(response).await).expect("json");
        assert_eq!(body["state"]["kind"].as_str(), Some("upToDate"));
        assert_eq!(body["localVersion"].as_str(), Some("0.2.0"));
        assert_eq!(
            crate::fs_ops::hash_dir(&installed).expect("post hash"),
            crate::fs_ops::hash_dir(
                &temp
                    .path()
                    .join("data")
                    .join("registry")
                    .join("current")
                    .join("alpha-flow")
            )
            .expect("cache hash")
        );
        assert!(temp
            .path()
            .join("data")
            .join("backups")
            .join("workflow-updates")
            .exists());
    }

    #[tokio::test]
    async fn update_workflow_rejects_unconfirmed_modified_and_bad_requests() {
        let (temp, state, _fixture) = test_state_with_update_fixture();
        let app = build_router(state);

        // 先经 download 端点安装（真实链路）。
        let response = app
            .clone()
            .oneshot(post_json(
                "/api/commands/download_workflow",
                r#"{"path":"alpha-flow"}"#,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);

        // 本地改动 → Modified 未确认 → 422
        std::fs::write(
            temp.path()
                .join("data")
                .join("workflows")
                .join("alpha-flow")
                .join("README.md"),
            "# 本地改动\n",
        )
        .expect("local edit");
        // modified 的 wire 形态钉死：kind + remoteChanged（camelCase）。
        let response = app
            .clone()
            .oneshot(post_json("/api/commands/check_workflow_updates", "{}"))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_str(&body_string(response).await).expect("json");
        assert_eq!(body[0]["state"]["kind"].as_str(), Some("modified"));
        assert_eq!(body[0]["state"]["remoteChanged"].as_bool(), Some(false));
        assert_eq!(body[0]["state"]["remoteVersion"].as_str(), Some("0.1.0"));

        // Modified 未确认 → 422
        let response = app
            .clone()
            .oneshot(post_json(
                "/api/commands/update_workflow",
                r#"{"slug":"alpha-flow","confirmModified":false}"#,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = body_string(response).await;
        assert!(body.contains("local modifications"), "body: {body}");

        // 坏 slug → 422（核心 [a-z0-9-]+ 校验浮出为业务错误）
        for slug in ["..", "../settings", "a/b", "", "UPPER"] {
            let body = serde_json::json!({ "slug": slug, "confirmModified": false }).to_string();
            let response = app
                .clone()
                .oneshot(post_json("/api/commands/update_workflow", &body))
                .await
                .expect("response");
            assert_eq!(
                response.status(),
                StatusCode::UNPROCESSABLE_ENTITY,
                "slug '{slug}'"
            );
        }

        // 无 source 的 slug → 422
        let response = app
            .oneshot(post_json(
                "/api/commands/update_workflow",
                r#"{"slug":"never-installed","confirmModified":false}"#,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn check_workflow_updates_without_cache_is_business_error() {
        let (_temp, state) = test_state();
        // settings 指向不可 clone URL 但不铺缓存：clone 失败且无旧缓存可回退。
        test_support::point_registry_at_uncloneable_url(state.ctx());
        let app = build_router(state);

        let response = app
            .oneshot(post_json("/api/commands/check_workflow_updates", "{}"))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    // -- skill-registry（Round 3 M6）-----------------------------------------
    //
    // 真实流程造数据（复用 skill_registry::test_support）：本地 fixture skill
    // 注册表 git 仓库 → 真 clone 铺 skill-registry/current 缓存；settings 指向
    // 必然 clone 失败的 GitHub 形态 URL → fetch_index / download_skill 走生产级
    // 离线回退读预置缓存，结果确定性。lock 落点为 ctx fake home（
    // ~/.agents/.skill-lock.json 以 ctx.home_dir() 展开），测试天然隔离。

    use crate::skill_registry::test_support as skill_fixtures;

    /// AppState + settings 指向 UNCLONEABLE_URL + 预 clone 的 v1 skill 缓存。
    /// 返回的 fixture TempDir 须由调用方持有存活（缓存 clone 自该仓库）。
    fn test_state_with_skill_fixture() -> (tempfile::TempDir, Arc<AppState>, tempfile::TempDir) {
        let fixture = skill_fixtures::fixture_repo();
        let (temp, state) = test_state();
        skill_fixtures::point_registry_at_uncloneable_url(state.ctx());
        skill_fixtures::seed_cache_from_fixture(
            state.ctx(),
            &skill_fixtures::repo_source(&fixture),
        );
        (temp, state, fixture)
    }

    fn skill_lock_json(temp: &tempfile::TempDir) -> serde_json::Value {
        let text = std::fs::read_to_string(
            temp.path()
                .join("home")
                .join(".agents")
                .join(".skill-lock.json"),
        )
        .expect("skill lock");
        serde_json::from_str(&text).expect("lock json")
    }

    #[tokio::test]
    async fn skill_registry_endpoints_full_round_trip() {
        let (temp, state, fixture) = test_state_with_skill_fixture();
        let app = build_router(state.clone());

        // list（cache-first，零网络）→ 两条目，均未安装
        let response = app
            .clone()
            .oneshot(post_json("/api/commands/list_remote_skills", "{}"))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_str(&body_string(response).await).expect("json");
        let items = body.as_array().expect("array");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["slug"].as_str(), Some("alpha-skill"));
        assert_eq!(items[0]["installed"].as_bool(), Some(false));
        assert_eq!(items[1]["slug"].as_str(), Some("beta-skill"));
        assert_eq!(items[1]["installed"].as_bool(), Some(false));

        // download（真实 download_skill 离线回退缓存）→ 200 裸 slug 字符串；
        // 中心库落盘 + lock 条目写入（归一化 https 形态）
        let response = app
            .clone()
            .oneshot(post_json(
                "/api/commands/download_skill",
                r#"{"path":"skills/alpha-skill"}"#,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_str(&body_string(response).await).expect("json");
        assert_eq!(body.as_str(), Some("alpha-skill"));
        let installed = temp
            .path()
            .join("home")
            .join(".oh-my-skills")
            .join("skills")
            .join("alpha-skill");
        assert!(installed.join("SKILL.md").is_file());
        let lock = skill_lock_json(&temp);
        assert_eq!(
            lock["skills"]["alpha-skill"]["sourceUrl"].as_str(),
            Some(skill_fixtures::UNCLONEABLE_URL)
        );
        assert_eq!(
            lock["skills"]["alpha-skill"]["sourceType"].as_str(),
            Some("github")
        );
        assert_eq!(
            lock["skills"]["alpha-skill"]["skillPath"].as_str(),
            Some("skills/alpha-skill")
        );
        assert!(lock["skills"]["alpha-skill"]["installedAt"].as_str().is_some());
        assert!(lock["skills"]["alpha-skill"]["updatedAt"].is_null());

        // list → installed 现算翻转
        let response = app
            .clone()
            .oneshot(post_json("/api/commands/list_remote_skills", "{}"))
            .await
            .expect("response");
        let body: serde_json::Value =
            serde_json::from_str(&body_string(response).await).expect("json");
        assert_eq!(body[0]["installed"].as_bool(), Some(true));
        assert_eq!(body[1]["installed"].as_bool(), Some(false));

        // check → 刚下载即最新（byte-verbatim 前提：hash 一致）
        let response = app
            .clone()
            .oneshot(post_json(
                "/api/commands/check_registry_skill_updates",
                "{}",
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_str(&body_string(response).await).expect("json");
        let updates = body.as_array().expect("array");
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0]["slug"].as_str(), Some("alpha-skill"));
        assert_eq!(updates[0]["updateAvailable"].as_bool(), Some(false));
        assert_eq!(updates[0]["remoteVersion"].as_str(), Some("0.1.0"));

        // 发布 v2 并刷新缓存 → check → available
        let repo = fixture.path().join("repo");
        skill_fixtures::publish_alpha_v2(&repo);
        skill_fixtures::commit_fixture(&repo, "alpha v2");
        skill_fixtures::seed_cache_from_fixture(
            state.ctx(),
            &skill_fixtures::repo_source(&fixture),
        );
        let response = app
            .clone()
            .oneshot(post_json(
                "/api/commands/check_registry_skill_updates",
                "{}",
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_str(&body_string(response).await).expect("json");
        assert_eq!(body[0]["updateAvailable"].as_bool(), Some(true));
        assert_eq!(body[0]["remoteVersion"].as_str(), Some("0.2.0"));

        // update → 200；安装 == 缓存（hash）；备份产生；lock.updatedAt 刷新
        let response = app
            .clone()
            .oneshot(post_json(
                "/api/commands/update_registry_skill",
                r#"{"slug":"alpha-skill"}"#,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            crate::fs_ops::hash_dir(&installed).expect("post hash"),
            crate::fs_ops::hash_dir(
                &temp
                    .path()
                    .join("data")
                    .join("skill-registry")
                    .join("current")
                    .join("skills")
                    .join("alpha-skill")
            )
            .expect("cache hash")
        );
        assert!(temp
            .path()
            .join("data")
            .join("backups")
            .join("skill-registry-updates")
            .exists());
        let lock = skill_lock_json(&temp);
        assert!(lock["skills"]["alpha-skill"]["updatedAt"].as_str().is_some());

        // 再 check → 回到最新
        let response = app
            .clone()
            .oneshot(post_json(
                "/api/commands/check_registry_skill_updates",
                "{}",
            ))
            .await
            .expect("response");
        let body: serde_json::Value =
            serde_json::from_str(&body_string(response).await).expect("json");
        assert_eq!(body[0]["updateAvailable"].as_bool(), Some(false));
    }

    #[tokio::test]
    async fn skill_registry_endpoints_reject_bad_requests() {
        let (_temp, state, _fixture) = test_state_with_skill_fixture();
        let app = build_router(state.clone());

        // 未在 index 中的 path（含穿越形态）→ 422（查无此条目）
        for path in ["no/such-skill", "../outside", "", "."] {
            let body = serde_json::json!({ "path": path }).to_string();
            let response = app
                .clone()
                .oneshot(post_json("/api/commands/download_skill", &body))
                .await
                .expect("response");
            assert_eq!(
                response.status(),
                StatusCode::UNPROCESSABLE_ENTITY,
                "path '{path}'"
            );
        }

        // 坏 slug → 422（核心 [a-z0-9-]+ 校验浮出为业务错误）
        for slug in ["..", "../settings", "a/b", "", "UPPER"] {
            let body = serde_json::json!({ "slug": slug }).to_string();
            let response = app
                .clone()
                .oneshot(post_json("/api/commands/update_registry_skill", &body))
                .await
                .expect("response");
            assert_eq!(
                response.status(),
                StatusCode::UNPROCESSABLE_ENTITY,
                "slug '{slug}'"
            );
        }

        // 无 lock 条目的 slug → 422
        let response = app
            .clone()
            .oneshot(post_json(
                "/api/commands/update_registry_skill",
                r#"{"slug":"never-installed"}"#,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

        // 下载后换注册表 → 同 slug 异源冲突 422（核心单测覆盖零副作用断言）
        let response = app
            .clone()
            .oneshot(post_json(
                "/api/commands/download_skill",
                r#"{"path":"skills/alpha-skill"}"#,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let mut settings = crate::settings::load_settings(state.ctx()).expect("settings");
        settings.skill_registry_url =
            Some("https://github.com/oms-fixture/other-nonexistent-000.git".to_string());
        crate::settings::save_settings(state.ctx(), &settings).expect("save settings");
        let response = app
            .clone()
            .oneshot(post_json(
                "/api/commands/download_skill",
                r#"{"path":"skills/alpha-skill"}"#,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn check_registry_skill_updates_empty_lock_or_missing_cache() {
        let (temp, state) = test_state();
        let app = build_router(state.clone());

        // 空 lock → 200 []（无跟踪条目不拉取，离线友好）
        let response = app
            .clone()
            .oneshot(post_json(
                "/api/commands/check_registry_skill_updates",
                "{}",
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_string(response).await, "[]");

        // 有跟踪条目但无缓存且 clone 必失败 → 422
        let lock_dir = temp.path().join("home").join(".agents");
        std::fs::create_dir_all(&lock_dir).expect("lock dir");
        std::fs::write(
            lock_dir.join(".skill-lock.json"),
            concat!(
                "{\"skills\":{\"alpha-skill\":{",
                "\"sourceUrl\":\"https://github.com/oms-fixture/nonexistent-skills-repo-000.git\",",
                "\"skillPath\":\"skills/alpha-skill\"}}}"
            ),
        )
        .expect("lock file");
        skill_fixtures::point_registry_at_uncloneable_url(state.ctx());
        let response = app
            .clone()
            .oneshot(post_json(
                "/api/commands/check_registry_skill_updates",
                "{}",
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    // -- workflow-share（Round 3 M4）-----------------------------------------
    //
    // export 端点正例用无 Ref skill 的占位工作流（生产 export_package 全程
    // 离线）；Ref 抓取链路由核心单测以真实 clone 覆盖。import 端点正例用核心
    // 真导出的包（本地 fixture skill 仓库 → 真 clone → 真导出 → HTTP 导入）。

    use crate::workflow_share::test_support as share_fixtures;

    const PLACEHOLDER_ONLY_YAML: &str = "name: 占位流程\n\
         slug: web-share-flow\n\
         version: 0.1.0\n\
         description: web 导出回测\n\
         groups:\n  - id: g\n    name: 组\n\
         steps:\n  - name: 步骤一\n    group: g\n    skills:\n\
         \x20     - placeholder: 待补充\n";

    #[tokio::test]
    async fn export_workflow_package_returns_filename_and_base64() {
        let (_temp, state) = test_state();
        share_fixtures::install_workflow_yaml(state.ctx(), "web-share-flow", PLACEHOLDER_ONLY_YAML);
        let app = build_router(state);

        let response = app
            .clone()
            .oneshot(post_json(
                "/api/commands/export_workflow_package",
                r#"{"slug":"web-share-flow"}"#,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_str(&body_string(response).await).expect("json");
        assert_eq!(
            body["filename"].as_str().expect("filename"),
            "web-share-flow-workflow.zip"
        );
        let base64 = body["base64"].as_str().expect("base64");
        use base64::Engine as _;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(base64)
            .expect("decode");
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip");
        assert!(archive.by_name("workflow.yaml").is_ok());
        assert!(archive.by_name("manifest.json").is_ok());
        // 无来源快照 → 无 source.json；占位流程 → 无 skills/ 条目。
        assert!(archive.by_name("source.json").is_err());

        // 负例：坏 slug / 未安装 → 422（核心校验浮出为业务错误）。
        for slug in ["..", "../settings", "UPPER", "a/b", ""] {
            let body = serde_json::json!({ "slug": slug }).to_string();
            let response = app
                .clone()
                .oneshot(post_json("/api/commands/export_workflow_package", &body))
                .await
                .expect("response");
            assert_eq!(
                response.status(),
                StatusCode::UNPROCESSABLE_ENTITY,
                "slug '{slug}'"
            );
        }
        let response = app
            .oneshot(post_json(
                "/api/commands/export_workflow_package",
                r#"{"slug":"not-installed"}"#,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn import_workflow_package_installs_real_export() {
        // 真导出（核心链路，本地 fixture skill 仓库真 clone）。
        let fixture = share_fixtures::fixture_skill_repo();
        let repo_source = share_fixtures::repo_source(&fixture);
        let export_temp = tempfile::tempdir().expect("temp dir");
        let export_ctx = share_fixtures::test_ctx(&export_temp);
        share_fixtures::install_share_workflow(&export_ctx);
        let (_filename, bytes) =
            share_fixtures::export_with_verbatim_fetch(&export_ctx, "share-flow", &repo_source)
                .expect("export");
        share_fixtures::assert_no_residue(&export_ctx);
        use base64::Engine as _;
        let archive_base64 = base64::engine::general_purpose::STANDARD.encode(&bytes);

        // HTTP 导入到干净实例。
        let (_temp, state) = test_state();
        let app = build_router(state);
        let body = serde_json::json!({ "archiveBase64": archive_base64 }).to_string();
        let response = app
            .clone()
            .oneshot(post_json("/api/commands/import_workflow_package", &body))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_str(&body_string(response).await).expect("json");
        assert_eq!(body["slug"].as_str(), Some("share-flow"));
        assert_eq!(body["hadSource"].as_bool(), Some(true));

        // 再导入 → 已存在冲突 422。
        let body = serde_json::json!({ "archiveBase64": archive_base64 }).to_string();
        let response = app
            .oneshot(post_json("/api/commands/import_workflow_package", &body))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn import_workflow_package_rejects_bad_packages() {
        let (_temp, state) = test_state();
        let app = build_router(state);

        // 非法 base64 → 422
        let response = app
            .clone()
            .oneshot(post_json(
                "/api/commands/import_workflow_package",
                r#"{"archiveBase64":"not-base64!!!"}"#,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

        // 合法 base64 但缺 workflow.yaml 的 zip → 422
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        writer
            .start_file(
                "README.md",
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )
            .expect("start");
        use std::io::Write as _;
        writer.write_all(b"# hi").expect("write");
        let zip_bytes = writer.finish().expect("finish").into_inner();
        use base64::Engine as _;
        let body = serde_json::json!({
            "archiveBase64": base64::engine::general_purpose::STANDARD.encode(zip_bytes)
        })
        .to_string();
        let response = app
            .oneshot(post_json("/api/commands/import_workflow_package", &body))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn share_endpoints_have_96mb_body_limit_others_keep_default() {
        let (_temp, state) = test_state();
        let app = build_router(state);

        // import 端点：超限（96MB + 余量）→ 413（body limit 先于 base64 预检）。
        let oversized = "A".repeat(SHARE_BODY_LIMIT + 1024);
        let body = format!(r#"{{"archiveBase64":"{oversized}"}}"#);
        let response = app
            .clone()
            .oneshot(post_json("/api/commands/import_workflow_package", &body))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);

        // import 端点：3MB（超框架默认 2MB、低于 96MB）→ 进业务层 422 而非 413。
        let under_limit = "A".repeat(3_000_000);
        let body = format!(r#"{{"archiveBase64":"{under_limit}"}}"#);
        let response = app
            .clone()
            .oneshot(post_json("/api/commands/import_workflow_package", &body))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

        // 未改动的路由维持默认 2MB：save_workflow 3MB → 413。
        let padded = "A".repeat(3_000_000);
        let body = format!(r#"{{"workflow":{{"name":"{padded}"}}}}"#);
        let response = app
            .oneshot(post_json("/api/commands/save_workflow", &body))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    // -- workflow-push（Round 3 M5）-----------------------------------------
    //
    // 真实流程造数据（复用 workflow_push::test_support）：本地 bare 注册表
    // git 仓库（main 分支，含一条 other-flow 既有条目）→ 生产端点真 clone →
    // 写入 → upsert → commit → push 全链路零外网；contribute 的 NeedFork 分支
    // 经必然 ls-remote 失败的 GitHub 形态 fork 地址驱动（有无网络结果一致）。

    use crate::workflow_push::test_support as push_fixtures;

    // contribute 三态对 OMS_GITHUB_TOKEN 敏感（resolve_token env 优先）；
    // 串行化需要操纵该环境变量的用例。
    static PUSH_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[tokio::test]
    async fn push_workflow_to_registry_endpoint_full_chain() {
        let fixture = push_fixtures::bare_registry_repo();
        let (temp, state) = test_state();
        push_fixtures::install_alpha_workflow(state.ctx());
        push_fixtures::point_workflow_registry_at(
            state.ctx(),
            &push_fixtures::bare_url(&fixture),
        );
        let app = build_router(state);

        // 正例：真推送全链路 → 200 {commitHash, registryUrl}。
        let response = app
            .clone()
            .oneshot(post_json(
                "/api/commands/push_workflow_to_registry",
                r#"{"slug":"alpha-flow"}"#,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_str(&body_string(response).await).expect("json");
        assert_eq!(
            body["registryUrl"].as_str().expect("registryUrl"),
            push_fixtures::bare_url(&fixture)
        );
        let commit_hash = body["commitHash"].as_str().expect("commitHash");
        assert_eq!(commit_hash.len(), 40);

        // 对端可独立核实：clone 回来 index 含 alpha-flow 8 字段条目。
        let verify = temp.path().join("verify");
        crate::git_ops::clone_repo_verbatim(
            &push_fixtures::bare_url(&fixture),
            &verify,
            None,
        )
        .expect("clone back");
        let index: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(verify.join("index.json")).expect("index"),
        )
        .expect("index json");
        let array = index["workflows"].as_array().expect("array");
        assert_eq!(array.len(), 2);
        assert_eq!(array[1]["slug"].as_str(), Some("alpha-flow"));
        assert_eq!(array[1]["version"].as_str(), Some("0.1.0"));
        assert_eq!(array[1]["path"].as_str(), Some("alpha-flow"));

        // 负例：官方地址（默认 settings）→ 422 引导贡献。
        let (_temp2, state2) = test_state();
        push_fixtures::install_alpha_workflow(state2.ctx());
        let app2 = build_router(state2);
        let response = app2
            .clone()
            .oneshot(post_json(
                "/api/commands/push_workflow_to_registry",
                r#"{"slug":"alpha-flow"}"#,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = body_string(response).await;
        assert!(body.contains("贡献"), "body: {body}");

        // 负例：坏 slug → 422（核心校验先于官方地址判定）。
        for slug in ["..", "../settings", "a/b", "", "UPPER"] {
            let body = serde_json::json!({ "slug": slug }).to_string();
            let response = app2
                .clone()
                .oneshot(post_json("/api/commands/push_workflow_to_registry", &body))
                .await
                .expect("response");
            assert_eq!(
                response.status(),
                StatusCode::UNPROCESSABLE_ENTITY,
                "slug '{slug}'"
            );
        }
    }

    #[tokio::test]
    async fn contribute_endpoints_need_fork_and_no_token_wire_forms() {
        let (_temp, state) = test_state();
        push_fixtures::provision_identity(state.ctx());
        push_fixtures::install_alpha_workflow(state.ctx());
        push_fixtures::install_skill(
            state.ctx(),
            "alpha-skill",
            push_fixtures::ALPHA_SKILL_MD,
        );
        // fork（alice 名下的同名仓）必然不存在 → ls-remote 失败 → NeedFork
        //（有无网络结果一致，速度取决于网络）。
        push_fixtures::point_workflow_registry_at(
            state.ctx(),
            "https://github.com/oms-fixture/nonexistent-workflows-000.git",
        );
        push_fixtures::point_skill_registry_at(
            state.ctx(),
            "https://github.com/oms-fixture/nonexistent-skills-000.git",
        );
        let app = build_router(state);

        let response = app
            .clone()
            .oneshot(post_json(
                "/api/commands/contribute_workflow",
                r#"{"slug":"alpha-flow"}"#,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_str(&body_string(response).await).expect("json");
        assert_eq!(body["status"].as_str(), Some("needFork"));
        assert_eq!(
            body["forkPageUrl"].as_str(),
            Some("https://github.com/oms-fixture/nonexistent-workflows-000/fork")
        );

        let response = app
            .clone()
            .oneshot(post_json(
                "/api/commands/contribute_skill",
                r#"{"slug":"alpha-skill"}"#,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_str(&body_string(response).await).expect("json");
        assert_eq!(body["status"].as_str(), Some("needFork"));
        assert_eq!(
            body["forkPageUrl"].as_str(),
            Some("https://github.com/oms-fixture/nonexistent-skills-000/fork")
        );

        // noToken：env 置空 + settings 无 token → Ok 载荷 {"status":"noToken"}。
        let (_temp2, state2) = test_state();
        let app2 = build_router(state2);
        let response = {
            let _guard = PUSH_ENV_LOCK.lock().expect("env lock");
            std::env::set_var("OMS_GITHUB_TOKEN", "");
            let response = app2
                .oneshot(post_json(
                    "/api/commands/contribute_workflow",
                    r#"{"slug":"alpha-flow"}"#,
                ))
                .await
                .expect("response");
            std::env::remove_var("OMS_GITHUB_TOKEN");
            response
        };
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_str(&body_string(response).await).expect("json");
        assert_eq!(
            body,
            serde_json::json!({ "status": "noToken" }),
            "wire 形态钉死：仅 status 一个键"
        );
    }

    #[tokio::test]
    async fn contribute_endpoints_reject_bad_requests() {
        let (_temp, state) = test_state();
        push_fixtures::provision_identity(state.ctx());
        let app = build_router(state);

        // 坏 slug → 422（token+username 已配，进入核心 slug 校验）。
        for slug in ["..", "../settings", "a/b", "", "UPPER"] {
            let body = serde_json::json!({ "slug": slug }).to_string();
            for endpoint in ["contribute_workflow", "contribute_skill"] {
                let response = app
                    .clone()
                    .oneshot(post_json(&format!("/api/commands/{endpoint}"), &body))
                    .await
                    .expect("response");
                assert_eq!(
                    response.status(),
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "{endpoint} slug '{slug}'"
                );
            }
        }

        // 未安装内容 → 422（真错误走 Err 通道，不进三态载荷）。
        for endpoint in ["contribute_workflow", "contribute_skill"] {
            let response = app
                .clone()
                .oneshot(post_json(
                    &format!("/api/commands/{endpoint}"),
                    r#"{"slug":"not-installed"}"#,
                ))
                .await
                .expect("response");
            assert_eq!(
                response.status(),
                StatusCode::UNPROCESSABLE_ENTITY,
                "{endpoint} not-installed"
            );
        }
    }

    // -- readonly-mode（Round 3 M7）------------------------------------------
    //
    // R2 白名单熔断正反组 / PublicSettings 形状 / refresh 强制忽略 / D8 配套
    // 修订 / 两桶限流 / ConnectInfo fail-closed / contribute_upload 负例组 /
    // static_handler `..` 拒绝 / gh 降级。ConnectInfo 在 oneshot 下经
    // request extension 注入（与 into_make_service_with_connect_info 同通道）；
    // 真 TCP 验证归 C11 AC5。

    use axum::extract::ConnectInfo;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::time::{Duration, Instant};

    const TEST_IP: &str = "203.0.113.7";

    /// 注入 ConnectInfo 的 POST（模拟真 TCP 连接携带的 peer addr）。
    fn post_json_with_ip(uri: &str, body: &str, ip: &str) -> Request<Body> {
        let addr = SocketAddr::new(ip.parse::<IpAddr>().expect("ip"), 40000);
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("host", "localhost:8477")
            .header("content-type", "application/json")
            .extension(ConnectInfo(addr))
            .body(Body::from(body.to_string()))
            .expect("request")
    }

    // -- RateLimiter 单测（窗口滑动 / 过期淘汰 / 容量上限）----------------------

    #[test]
    fn rate_limiter_rejects_beyond_limit_and_slides_window() {
        let mut limiter = RateLimiter::new(2, Duration::from_secs(3600));
        let ip = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1));
        let t0 = Instant::now();
        assert!(limiter.check(ip, t0), "第 1 次放行");
        assert!(limiter.check(ip, t0 + Duration::from_secs(1)), "第 2 次放行");
        assert!(!limiter.check(ip, t0 + Duration::from_secs(2)), "第 3 次拒绝");
        // 窗口滑动：1h 后最早命中移出窗口，重新放行。
        assert!(limiter.check(ip, t0 + Duration::from_secs(3601)));
        // 过期淘汰：整条目移出窗口即从 map 删除（不无界增长）。
        assert!(limiter.check(ip, t0 + Duration::from_secs(7202)));
        assert_eq!(limiter.hits.len(), 1, "过期命中已淘汰");
    }

    #[test]
    fn rate_limiter_caps_map_and_evicts_least_recent() {
        let mut limiter = RateLimiter::new(1, Duration::from_secs(3600));
        let t0 = Instant::now();
        // 打满容量：RATE_LIMIT_CAPACITY 个不同 IP 各计 1 次。
        for n in 0..RATE_LIMIT_CAPACITY {
            let ip = IpAddr::V4(Ipv4Addr::new(10, 0, (n / 256) as u8, (n % 256) as u8));
            assert!(limiter.check(ip, t0 + Duration::from_secs(n as u64)), "ip {n}");
        }
        assert_eq!(limiter.hits.len(), RATE_LIMIT_CAPACITY);

        // 第 CAPACITY+1 个新 IP：淘汰最久未活动条目（10.0.0.0，t+0），map 不越界。
        let new_ip = IpAddr::V4(Ipv4Addr::new(10, 255, 255, 255));
        assert!(limiter.check(new_ip, t0 + Duration::from_secs(2000)));
        assert_eq!(limiter.hits.len(), RATE_LIMIT_CAPACITY, "map 容量有界");
        assert!(
            !limiter
                .hits
                .contains_key(&IpAddr::V4(Ipv4Addr::new(10, 0, 0, 0))),
            "最久未活动条目被淘汰"
        );
        // 幸存条目计数未被误清：窗口内第二次仍拒绝。
        let survivor = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        assert!(!limiter.check(survivor, t0 + Duration::from_secs(2001)));
    }

    // -- R2 白名单正反组 --------------------------------------------------------

    #[tokio::test]
    async fn readonly_whitelist_allows_listed_reads() {
        let (temp, state) = test_state_readonly();
        // 铺 workflow/skill 注册表缓存与一条已安装 workflow（detail 用）。
        let current = temp.path().join("data").join("registry").join("current");
        std::fs::create_dir_all(&current).expect("cache dir");
        std::fs::write(
            current.join("index.json"),
            concat!(
                "{\"version\":1,\"workflows\":[",
                "{\"slug\":\"alpha-flow\",\"name\":\"Alpha\",\"version\":\"0.1.0\",",
                "\"description\":\"a\",\"path\":\"alpha-flow\"}]}",
            ),
        )
        .expect("cached index");
        let skill_current = temp
            .path()
            .join("data")
            .join("skill-registry")
            .join("current");
        std::fs::create_dir_all(&skill_current).expect("skill cache dir");
        std::fs::write(
            skill_current.join("index.json"),
            concat!(
                "{\"version\":1,\"skills\":[",
                "{\"slug\":\"alpha-skill\",\"name\":\"Alpha\",\"version\":\"0.1.0\",",
                "\"description\":\"a\",\"path\":\"skills/alpha-skill\"}]}",
            ),
        )
        .expect("cached skill index");
        share_fixtures::install_workflow_yaml(state.ctx(), "web-share-flow", PLACEHOLDER_ONLY_YAML);
        let app = build_router(state);

        // health 是只读探测唯一通道（门-F10）。
        let request = Request::builder()
            .uri("/api/health")
            .header("host", "localhost:8477")
            .body(Body::empty())
            .expect("request");
        let response = app.clone().oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_string(response).await, r#"{"ok":true,"readonly":true}"#);

        // 白名单纯读端点全放行（export/contribute_upload 的放行由各自
        // 限流/fail-closed 测试覆盖——它们还要求 ConnectInfo）。
        for command in [
            "get_settings",
            "read_inventory_cache",
            "read_skill_lock",
            "list_installed_workflows",
            "list_remote_workflows",
            "list_remote_skills",
        ] {
            let response = app
                .clone()
                .oneshot(post_json(&format!("/api/commands/{command}"), "{}"))
                .await
                .expect("response");
            assert_eq!(response.status(), StatusCode::OK, "{command}");
        }
        let response = app
            .clone()
            .oneshot(post_json(
                "/api/commands/get_workflow_detail",
                r#"{"slug":"web-share-flow"}"#,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK, "get_workflow_detail");
    }

    #[tokio::test]
    async fn readonly_whitelist_rejects_everything_else() {
        let (_temp, state) = test_state_readonly();
        let app = build_router(state);

        // 全部已注册但不在白名单的 command：一律 403（默认拒绝；含
        // scan_inventory——它写盘点缓存；含 list_dir/discover——枚举暴露面）。
        for command in [
            "save_settings",
            "scan_inventory",
            "discover_project_workspaces",
            "preview_batch_sync",
            "preview_batch_quick_migration",
            "apply_sync_plan",
            "check_skills_sh_update",
            "update_skills_sh_skill",
            "remove_skill_entries",
            "list_dir",
            "download_workflow",
            "save_workflow",
            "delete_workflow",
            "preview_use_workflow",
            "check_workflow_updates",
            "update_workflow",
            "download_skill",
            "check_registry_skill_updates",
            "update_registry_skill",
            "push_workflow_to_registry",
            "contribute_workflow",
            "contribute_skill",
            "import_workflow_package",
        ] {
            let response = app
                .clone()
                .oneshot(post_json(&format!("/api/commands/{command}"), "{}"))
                .await
                .expect("response");
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{command}");
        }
    }

    // -- PublicSettings 形状（门-M5/R3①）---------------------------------------

    #[tokio::test]
    async fn readonly_get_settings_returns_public_shape() {
        let (_temp, state) = test_state_readonly();
        // 核心直写带 token 与自定义 registry 的 settings（save_settings 端点
        // 只读模式已被熔断，绕过 HTTP）。
        let mut settings = crate::settings::load_settings(state.ctx()).expect("settings");
        settings.github_token = Some("ghp_public_secret".to_string());
        settings.workflow_registry_url =
            Some("https://github.com/acme/workflows.git".to_string());
        settings.skill_registry_url = Some("https://github.com/acme/skills.git".to_string());
        crate::settings::save_settings(state.ctx(), &settings).expect("save");

        let app = build_router(state);
        let response = app
            .oneshot(post_json("/api/commands/get_settings", "{}"))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_str(&body_string(response).await).expect("json");
        let object = body.as_object().expect("object");

        // serde 物理隔离：无 githubToken 键。
        assert!(
            object.get("githubToken").is_none(),
            "PublicSettings 不得含 githubToken 键: {body}"
        );
        // 9 字段齐全：字段名保留、敏感值置空、registry URL 保留真值。
        assert_eq!(object.len(), 9, "键集合恰好 9 个: {object:?}");
        assert_eq!(body["language"].as_str(), Some("zh-CN"));
        assert_eq!(
            body["workflowRegistryUrl"].as_str(),
            Some("https://github.com/acme/workflows.git")
        );
        assert_eq!(
            body["skillRegistryUrl"].as_str(),
            Some("https://github.com/acme/skills.git")
        );
        assert_eq!(body["hasGithubToken"].as_bool(), Some(false));
        assert_eq!(body["readonly"].as_bool(), Some(true));
        assert_eq!(body["libraryPath"].as_str(), Some(""));
        assert_eq!(
            body["projectFolders"].as_array().expect("array").len(),
            0
        );
        assert_eq!(body["customRoots"].as_array().expect("array").len(), 0);
        assert_eq!(body["showRawPaths"].as_bool(), Some(false));
    }

    // -- list_remote_* 只读强制 refresh=false（门-M2）---------------------------

    #[tokio::test]
    async fn readonly_list_remote_workflows_ignores_refresh() {
        let fixture = test_support::fixture_repo();
        let (_temp, state) = test_state_readonly();
        let source = test_support::repo_source(&fixture);
        // 缓存铺 v1；settings 指向 fixture（refresh 若生效可拉到 v2）。
        test_support::refresh_cache_from_fixture(state.ctx(), &source);
        let mut settings = crate::settings::load_settings(state.ctx()).expect("settings");
        settings.workflow_registry_url = Some(source.clone());
        crate::settings::save_settings(state.ctx(), &settings).expect("save");
        // 发布 v2（不刷新缓存）：远端已新、缓存仍旧。
        let repo = fixture.path().join("repo");
        test_support::write_alpha(
            &repo,
            test_support::ALPHA_YAML_V2,
            test_support::ALPHA_README_V2,
            "0.2.0",
        );
        test_support::commit_fixture(&repo, "alpha v2");

        let app = build_router(state);
        let response = app
            .clone()
            .oneshot(post_json(
                "/api/commands/list_remote_workflows",
                r#"{"refresh":true}"#,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_str(&body_string(response).await).expect("json");
        // refresh=true 被忽略 → 走缓存 → 仍 v1。若未忽略，本地 fixture 路径
        // 过不了 fetch_index 的 normalize_github_url（C2 逐字来源契约）会
        // 422；走通也只可能返回 v2——200 + v1 即「被忽略」的完整证明。
        assert_eq!(body[0]["slug"].as_str(), Some("alpha-flow"));
        assert_eq!(body[0]["version"].as_str(), Some("0.1.0"));
    }

    #[tokio::test]
    async fn readonly_list_remote_skills_ignores_refresh() {
        let fixture = skill_fixtures::fixture_repo();
        let (_temp, state) = test_state_readonly();
        let source = skill_fixtures::repo_source(&fixture);
        skill_fixtures::seed_cache_from_fixture(state.ctx(), &source);
        let mut settings = crate::settings::load_settings(state.ctx()).expect("settings");
        settings.skill_registry_url = Some(source);
        crate::settings::save_settings(state.ctx(), &settings).expect("save");
        let repo = fixture.path().join("repo");
        skill_fixtures::publish_alpha_v2(&repo);
        skill_fixtures::commit_fixture(&repo, "alpha v2");

        let app = build_router(state);
        let response = app
            .oneshot(post_json(
                "/api/commands/list_remote_skills",
                r#"{"refresh":true}"#,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_str(&body_string(response).await).expect("json");
        assert_eq!(body[0]["slug"].as_str(), Some("alpha-skill"));
        assert_eq!(body[0]["version"].as_str(), Some("0.1.0"), "refresh 被忽略走缓存");
    }

    // -- D8 配套修订（门-B2/R5）：readonly 放行公网 Host，CSRF 校验保留 ---------

    #[tokio::test]
    async fn readonly_guard_allows_public_host_but_keeps_csrf_checks() {
        let (_temp, state) = test_state_readonly();
        let app = build_router(state);

        // 公网 Host 放行（GET health；非 readonly 下此用例 403，由既有
        // rejects_non_localhost_host 覆盖，行为零变化）。
        let request = Request::builder()
            .uri("/api/health")
            .header("host", "oms.example.com")
            .body(Body::empty())
            .expect("request");
        let response = app.clone().oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::OK);

        // Sec-Fetch-Site: cross-site 仍 403。
        let request = Request::builder()
            .uri("/api/health")
            .header("host", "oms.example.com")
            .header("sec-fetch-site", "cross-site")
            .body(Body::empty())
            .expect("request");
        let response = app.clone().oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        // POST Origin host == Host 放行（公网同源表单自然满足）。
        let request = Request::builder()
            .method("POST")
            .uri("/api/commands/get_settings")
            .header("host", "oms.example.com")
            .header("content-type", "application/json")
            .header("origin", "https://oms.example.com")
            .body(Body::from("{}"))
            .expect("request");
        let response = app.clone().oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::OK);

        // Origin 不匹配仍 403。
        let request = Request::builder()
            .method("POST")
            .uri("/api/commands/get_settings")
            .header("host", "oms.example.com")
            .header("content-type", "application/json")
            .header("origin", "https://evil.example.com")
            .body(Body::from("{}"))
            .expect("request");
        let response = app.clone().oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    // -- static_handler `..` 段拒绝（门-readonly-F12）---------------------------

    #[tokio::test]
    async fn static_handler_rejects_dotdot_segments() {
        let (_temp, state) = test_state();
        let app = build_router(state);

        for path in ["/../settings.json", "/assets/../../etc/passwd", "/.."] {
            let request = Request::builder()
                .uri(path)
                .header("host", "localhost:8477")
                .body(Body::empty())
                .expect("request");
            let response = app.clone().oneshot(request).await.expect("response");
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "path: {path}");
        }
    }

    // -- export 只读并入 30/h 限流（门-M3/R9）-----------------------------------

    #[tokio::test]
    async fn readonly_export_is_rate_limited_per_ip() {
        let (_temp, state) = test_state_readonly();
        share_fixtures::install_workflow_yaml(state.ctx(), "web-share-flow", PLACEHOLDER_ONLY_YAML);
        let app = build_router(state);
        let body = r#"{"slug":"web-share-flow"}"#;

        // 前 30 次放行（占位流程导出全程离线，无网络）。
        for n in 1..=30 {
            let response = app
                .clone()
                .oneshot(post_json_with_ip(
                    "/api/commands/export_workflow_package",
                    body,
                    TEST_IP,
                ))
                .await
                .expect("response");
            assert_eq!(response.status(), StatusCode::OK, "request {n}");
        }
        // 第 31 次 → 429（30/h 桶触发）。
        let response = app
            .clone()
            .oneshot(post_json_with_ip(
                "/api/commands/export_workflow_package",
                body,
                TEST_IP,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        // 另一 IP 不受影响（per-IP 桶）。
        let response = app
            .clone()
            .oneshot(post_json_with_ip(
                "/api/commands/export_workflow_package",
                body,
                "203.0.113.8",
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn readonly_export_without_connect_info_is_fail_closed() {
        let (_temp, state) = test_state_readonly();
        share_fixtures::install_workflow_yaml(state.ctx(), "web-share-flow", PLACEHOLDER_ONLY_YAML);
        let app = build_router(state);

        // 无 ConnectInfo（oneshot 不注入）→ fail-closed 503，不静默放行。
        let response = app
            .clone()
            .oneshot(post_json(
                "/api/commands/export_workflow_package",
                r#"{"slug":"web-share-flow"}"#,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

        // 对照：非 readonly 不限流，无 ConnectInfo 照常 200。
        let (_temp2, state2) = test_state();
        share_fixtures::install_workflow_yaml(state2.ctx(), "web-share-flow", PLACEHOLDER_ONLY_YAML);
        let app2 = build_router(state2);
        let response = app2
            .oneshot(post_json(
                "/api/commands/export_workflow_package",
                r#"{"slug":"web-share-flow"}"#,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
    }

    // -- contribute_upload（DD §8.3）--------------------------------------------

    /// 造 zip → base64（Stored 不压缩，与 workflow_share 测试同款手法）。
    fn upload_zip_base64(entries: &[(&str, &[u8])]) -> String {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        use std::io::Write as _;
        for (name, content) in entries {
            writer.start_file(*name, options).expect("start file");
            writer.write_all(content).expect("write");
        }
        let bytes = writer.finish().expect("finish").into_inner();
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    const UPLOAD_GOOD_YAML: &str =
        "name: 上传流程\nslug: upload-flow\nversion: 0.1.0\ndescription: 上传回测\n";
    const UPLOAD_SKILL_MD: &str =
        "---\nname: upload-skill\ndescription: 上传回测\n---\n# body\n";

    fn upload_body(kind: &str, archive_base64: &str) -> String {
        serde_json::json!({ "kind": kind, "archiveBase64": archive_base64 }).to_string()
    }

    /// staging 清理断言：data_dir/tmp 不存在或无任何 upload-* 残留。
    fn assert_no_upload_residue(state: &AppState) {
        let tmp = state.ctx().data_dir().join("tmp");
        let Ok(mut entries) = std::fs::read_dir(&tmp) else {
            return;
        };
        assert!(
            entries.all(|entry| !entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .starts_with("upload-")),
            "tmp/ 不得有 upload-* 残留"
        );
    }

    #[tokio::test]
    async fn contribute_upload_without_connect_info_is_fail_closed() {
        // 非 readonly（隔离白名单）：无 ConnectInfo → fail-closed 503。
        let (_temp, state) = test_state();
        let app = build_router(state);
        let body = upload_body(
            "workflow",
            &upload_zip_base64(&[("workflow.yaml", UPLOAD_GOOD_YAML.as_bytes())]),
        );
        let response = app
            .oneshot(post_json("/api/commands/contribute_upload", &body))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

        // readonly 下同样 503（白名单放行后 handler 检查）。
        let (_temp2, state2) = test_state_readonly();
        let app2 = build_router(state2);
        let response = app2
            .oneshot(post_json("/api/commands/contribute_upload", &body))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn contribute_upload_rejects_bad_requests() {
        let (_temp, state) = test_state_readonly();
        let app = build_router(state.clone());

        // 每子用例换一个 IP（坏请求同样计 5/h 桶，防限流干扰）。
        let mut octet = 100u8;
        let mut post = |body: String| {
            octet += 1;
            post_json_with_ip(
                "/api/commands/contribute_upload",
                &body,
                &format!("203.0.113.{octet}"),
            )
        };

        let cases: Vec<(String, &str)> = vec![
            // 坏 kind。
            (upload_body("plugin", "AAAA"), "kind"),
            // 非法 base64。
            (upload_body("workflow", "not-base64!!!"), "base64"),
            // 合法 base64 但非 zip。
            (
                upload_body("workflow", &{
                    use base64::Engine as _;
                    base64::engine::general_purpose::STANDARD.encode(b"not a zip")
                }),
                "zip",
            ),
            // workflow：缺 workflow.yaml。
            (
                upload_body(
                    "workflow",
                    &upload_zip_base64(&[("README.md", b"# hi")]),
                ),
                "workflow.yaml",
            ),
            // workflow：坏 yaml。
            (
                upload_body(
                    "workflow",
                    &upload_zip_base64(&[("workflow.yaml", b"not: [valid")]),
                ),
                "yaml",
            ),
            // workflow：yaml slug 非法（validate [a-z0-9-]+）。
            (
                upload_body(
                    "workflow",
                    &upload_zip_base64(&[(
                        "workflow.yaml",
                        b"name: x\nslug: Bad_Slug\nversion: 0.1.0\ndescription: x\n",
                    )]),
                ),
                "slug",
            ),
            // skill：缺 SKILL.md。
            (
                upload_body("skill", &upload_zip_base64(&[("README.md", b"# hi")])),
                "SKILL.md",
            ),
            // skill：frontmatter 缺失。
            (
                upload_body("skill", &upload_zip_base64(&[("SKILL.md", b"# no frontmatter")])),
                "frontmatter",
            ),
            // skill：frontmatter name 缺失（slug 无来源）。
            (
                upload_body(
                    "skill",
                    &upload_zip_base64(&[("SKILL.md", b"---\ndescription: x\n---\n")]),
                ),
                "name",
            ),
            // skill：name 非 [a-z0-9-]+（进分支名与 gh 参数前把守）。
            (
                upload_body(
                    "skill",
                    &upload_zip_base64(&[("SKILL.md", b"---\nname: Bad_Slug\ndescription: x\n---\n")]),
                ),
                "slug",
            ),
        ];

        for (body, tag) in cases {
            let response = app.clone().oneshot(post(body)).await.expect("response");
            assert_eq!(
                response.status(),
                StatusCode::UNPROCESSABLE_ENTITY,
                "case: {tag}"
            );
        }
        // 全部失败分支 staging 清理干净。
        assert_no_upload_residue(&state);
    }

    #[tokio::test]
    async fn contribute_upload_without_bot_token_reports_not_enabled() {
        let (_temp, state) = test_state_readonly();
        let app = build_router(state.clone());

        // 合法包走通解压/校验/staging 全链，止于 bot token 检查
        // （resolve_token env 优先——置空确保与运行环境无关）。
        for (kind, entries) in [
            ("workflow", vec![("workflow.yaml", UPLOAD_GOOD_YAML.as_bytes())]),
            ("skill", vec![("SKILL.md", UPLOAD_SKILL_MD.as_bytes())]),
        ] {
            let body = upload_body(kind, &upload_zip_base64(&entries));
            let response = {
                let _guard = PUSH_ENV_LOCK.lock().expect("env lock");
                std::env::set_var("OMS_GITHUB_TOKEN", "");
                let response = app
                    .clone()
                    .oneshot(post_json_with_ip(
                        "/api/commands/contribute_upload",
                        &body,
                        TEST_IP,
                    ))
                    .await
                    .expect("response");
                std::env::remove_var("OMS_GITHUB_TOKEN");
                response
            };
            assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY, "{kind}");
            let text = body_string(response).await;
            assert!(text.contains("站点未开放贡献"), "{kind}: {text}");
        }
        assert_no_upload_residue(&state);
    }

    #[tokio::test]
    async fn contribute_upload_is_rate_limited_per_ip() {
        let (_temp, state) = test_state_readonly();
        let app = build_router(state);
        let body = upload_body(
            "workflow",
            &upload_zip_base64(&[("workflow.yaml", UPLOAD_GOOD_YAML.as_bytes())]),
        );

        let responses = {
            let _guard = PUSH_ENV_LOCK.lock().expect("env lock");
            std::env::set_var("OMS_GITHUB_TOKEN", "");
            let mut responses = Vec::new();
            // 同 IP 连发 6 次：前 5 次进业务（422 未开放贡献），第 6 次 429。
            for _ in 0..6 {
                let response = app
                    .clone()
                    .oneshot(post_json_with_ip(
                        "/api/commands/contribute_upload",
                        &body,
                        TEST_IP,
                    ))
                    .await
                    .expect("response");
                responses.push(response.status());
            }
            // 另一 IP 不受影响（per-IP 桶）。
            let response = app
                .clone()
                .oneshot(post_json_with_ip(
                    "/api/commands/contribute_upload",
                    &body,
                    "203.0.113.8",
                ))
                .await
                .expect("response");
            responses.push(response.status());
            std::env::remove_var("OMS_GITHUB_TOKEN");
            responses
        };
        assert_eq!(
            responses,
            vec![
                StatusCode::UNPROCESSABLE_ENTITY,
                StatusCode::UNPROCESSABLE_ENTITY,
                StatusCode::UNPROCESSABLE_ENTITY,
                StatusCode::UNPROCESSABLE_ENTITY,
                StatusCode::UNPROCESSABLE_ENTITY,
                StatusCode::TOO_MANY_REQUESTS,
                StatusCode::UNPROCESSABLE_ENTITY,
            ]
        );
    }

    // -- gh CLI 建 PR（R10：--version 先探测；失败降级）-------------------------

    #[test]
    fn create_pr_with_gh_missing_binary_falls_back() {
        assert!(routes::create_pr_with_gh(
            "definitely-not-a-real-gh-binary-xyz",
            "owner",
            "repo",
            "upload/demo-20260801T000000Z",
            "Add workflow demo",
            "ghp_token",
        )
        .is_none());
    }

    /// 假 gh 脚本（unix）：返回 (tempdir 持有存活, 可执行路径)。
    #[cfg(unix)]
    fn fake_gh(script: &str) -> (tempfile::TempDir, String) {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("gh");
        std::fs::write(&path, script).expect("write script");
        let mut permissions = std::fs::metadata(&path).expect("meta").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).expect("chmod");
        (temp, crate::fs_ops::path_to_string(&path))
    }

    #[cfg(unix)]
    #[test]
    fn create_pr_with_gh_success_and_failure() {
        // 成功：stdout 输出 PR URL → Some(url)。
        let (_dir, program) = fake_gh("#!/bin/sh\necho 'https://github.com/o/r/pull/123'\n");
        assert_eq!(
            routes::create_pr_with_gh(
                &program,
                "o",
                "r",
                "upload/demo-1",
                "Add workflow demo",
                "ghp_token",
            )
            .as_deref(),
            Some("https://github.com/o/r/pull/123")
        );

        // 失败：退出 1 + stderr 含 token → None 降级（脱敏逻辑本身由
        // github_auth::redact_text 单测覆盖）。
        let (_dir2, program2) = fake_gh("#!/bin/sh\necho 'ghp_leaked' >&2\nexit 1\n");
        assert!(routes::create_pr_with_gh(
            &program2,
            "o",
            "r",
            "upload/demo-1",
            "Add workflow demo",
            "ghp_leaked",
        )
        .is_none());
    }
}
