//! oms-web：Web 壳二进制入口。
//!
//! D4 护栏：bind 地址硬编码 127.0.0.1（无认证服务绝不暴露到局域网/公网），
//! 本轮不提供任何覆盖开关。配置仅两个环境变量：
//! - `OMS_PORT`：监听端口，默认 8477
//! - `OMS_DATA_DIR`：数据目录，默认 ~/.oh-my-skills-cent

use oh_my_skills_lib::context::AppContext;
use oh_my_skills_lib::web::{build_router, AppState};
use std::sync::Arc;

const BIND_ADDRESS: [u8; 4] = [127, 0, 0, 1];
const DEFAULT_PORT: u16 = 8477;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("oms-web: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let port = std::env::var("OMS_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(DEFAULT_PORT);

    let ctx = AppContext::from_env()?;
    let data_dir = ctx.data_dir().to_path_buf();
    let state = Arc::new(AppState::new(ctx)?);
    let app = build_router(state);

    let addr = std::net::SocketAddr::from((BIND_ADDRESS, port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|error| format!("Unable to bind {addr}: {error}"))?;

    println!("oms-web listening on http://{addr} (data dir: {})", data_dir.display());
    axum::serve(listener, app)
        .await
        .map_err(|error| format!("Server error: {error}"))
}
