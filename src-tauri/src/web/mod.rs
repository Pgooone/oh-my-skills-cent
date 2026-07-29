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
}
