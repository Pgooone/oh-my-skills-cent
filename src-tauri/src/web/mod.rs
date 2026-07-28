//! Web 壳：axum router 构建、共享 state（AppContext + PathJail）、静态服务。
//!
//! 安全护栏集中在这一层：guard 中间件（D8）挂在所有 /api 路由上，
//! PathJail（D7）由 routes 内对路径参数调用。

pub mod guard;
pub mod jail;
pub mod routes;

use crate::context::AppContext;
use crate::models::Settings;
use axum::{
    http::{header, Method, StatusCode, Uri},
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use rust_embed::RustEmbed;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

/// 共享 state：业务上下文 + 可刷新的路径白名单（save_settings 后重建）。
pub struct AppState {
    ctx: AppContext,
    jail: RwLock<jail::PathJail>,
}

impl AppState {
    pub fn new(ctx: AppContext) -> Result<Self, String> {
        let settings = crate::settings::load_settings(&ctx)?;
        Ok(Self {
            jail: RwLock::new(jail::PathJail::new(&ctx, &settings)),
            ctx,
        })
    }

    pub fn ctx(&self) -> &AppContext {
        &self.ctx
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
        // D8：所有 /api 请求过 Host / Origin / Sec-Fetch-Site 校验。
        .route_layer(middleware::from_fn(guard::local_only_guard));

    Router::new()
        .merge(api)
        .fallback(static_handler)
        .with_state(state)
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
        let state = AppState::new(ctx).expect("state");
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
        assert_eq!(body_string(response).await, r#"{"ok":true}"#);
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
}
