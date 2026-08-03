//! oms-web：Web 壳二进制入口。
//!
//! D4 修订护栏（R1）：bind 地址由 `OMS_BIND` 配置；非 loopback 地址的唯一
//! 合法形态是只读模式（`OMS_READONLY=1`），否则打印原因并 exit(1)。
//! localhost（loopback）行为与既有版本一致。配置三个环境变量：
//! - `OMS_BIND`：监听地址（`host:port` 全形式），默认 `127.0.0.1:8477`
//!   （取代已废弃的 OMS_PORT）
//! - `OMS_READONLY`：置 `1` 开启公共只读模式（/api 白名单默认拒绝 +
//!   PublicSettings + 访客上传限流）
//! - `OMS_DATA_DIR`：数据目录，默认 ~/.oh-my-skills-cent

use oh_my_skills_lib::context::AppContext;
use oh_my_skills_lib::web::{build_router, AppState};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

const DEFAULT_PORT: u16 = 8477;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("oms-web: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let readonly = std::env::var("OMS_READONLY").as_deref() == Ok("1");
    let addr = parse_bind(std::env::var("OMS_BIND").ok().as_deref())?;
    guard_bind(&addr, readonly)?;

    let ctx = AppContext::from_env()?;
    let data_dir = ctx.data_dir().to_path_buf();
    // AppState::new 内预热 load_settings：settings 缺失时启动期即初始化写盘
    // （发生在启动期而非首个请求期，门-readonly-F10）。
    let state = Arc::new(AppState::new(ctx, readonly)?);
    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|error| format!("Unable to bind {addr}: {error}"))?;

    let mode = if readonly { " [read-only]" } else { "" };
    println!(
        "oms-web listening on http://{addr}{mode} (data dir: {})",
        data_dir.display()
    );
    // ConnectInfo 供 contribute_upload / 只读 export 限流取客户端 IP（门-B5）。
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .await
        .map_err(|error| format!("Server error: {error}"))
}

/// OMS_BIND 解析：未设置/空 → `127.0.0.1:8477`；接受 `ipv4:port`、
/// `[ipv6]:port`、`localhost:port` 全形式。非法值拒绝启动（安全配置不静默
/// 回退，与 D4 护栏同源）。
fn parse_bind(value: Option<&str>) -> Result<SocketAddr, String> {
    let Some(raw) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(SocketAddr::from((Ipv4Addr::LOCALHOST, DEFAULT_PORT)));
    };
    if let Ok(addr) = raw.parse::<SocketAddr>() {
        return Ok(addr);
    }
    let (host, port) = raw
        .rsplit_once(':')
        .ok_or_else(|| format!("Invalid OMS_BIND '{raw}': expected host:port"))?;
    let port = port
        .parse::<u16>()
        .map_err(|_| format!("Invalid OMS_BIND '{raw}': port must be 0-65535"))?;
    let ip: IpAddr = if host == "localhost" {
        Ipv4Addr::LOCALHOST.into()
    } else {
        host.parse()
            .map_err(|_| format!("Invalid OMS_BIND '{raw}': unknown host"))?
    };
    Ok(SocketAddr::new(ip, port))
}

/// D4 修订护栏（R1）：非 loopback 且未开只读 → 拒绝启动并打印原因。
fn guard_bind(addr: &SocketAddr, readonly: bool) -> Result<(), String> {
    if addr.ip().is_loopback() || readonly {
        return Ok(());
    }
    Err(format!(
        "Refusing to start: OMS_BIND '{addr}' is not a loopback address while \
         OMS_READONLY is not '1'. A server without authentication may only listen \
         beyond localhost in read-only mode (set OMS_READONLY=1, or bind a \
         loopback address such as 127.0.0.1)."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- parse_bind 各形态 ----------------------------------------------------

    #[test]
    fn parse_bind_defaults_to_localhost() {
        assert_eq!(
            parse_bind(None).expect("default"),
            SocketAddr::from((Ipv4Addr::LOCALHOST, 8477))
        );
        assert_eq!(
            parse_bind(Some("")).expect("empty"),
            SocketAddr::from((Ipv4Addr::LOCALHOST, 8477))
        );
        assert_eq!(
            parse_bind(Some("  ")).expect("blank"),
            SocketAddr::from((Ipv4Addr::LOCALHOST, 8477))
        );
    }

    #[test]
    fn parse_bind_accepts_full_forms() {
        assert_eq!(
            parse_bind(Some("127.0.0.1:9000")).expect("v4"),
            SocketAddr::from((Ipv4Addr::LOCALHOST, 9000))
        );
        assert_eq!(
            parse_bind(Some("localhost:9000")).expect("hostname"),
            SocketAddr::from((Ipv4Addr::LOCALHOST, 9000))
        );
        assert_eq!(
            parse_bind(Some("0.0.0.0:8477")).expect("any"),
            SocketAddr::from((Ipv4Addr::UNSPECIFIED, 8477))
        );
        assert_eq!(
            parse_bind(Some("[::1]:8477")).expect("v6 loopback"),
            SocketAddr::from((IpAddr::V6(std::net::Ipv6Addr::LOCALHOST), 8477))
        );
    }

    #[test]
    fn parse_bind_rejects_invalid_values() {
        for raw in ["8477", "127.0.0.1", "127.0.0.1:abc", "127.0.0.1:99999", "example.com:8477", ":"] {
            let error = parse_bind(Some(raw)).expect_err("must reject");
            assert!(error.contains("Invalid OMS_BIND"), "'{raw}': {error}");
        }
    }

    // -- D4 修订护栏（R1）------------------------------------------------------

    #[test]
    fn guard_bind_rejects_non_loopback_without_readonly() {
        let addr = SocketAddr::from((Ipv4Addr::UNSPECIFIED, 8477));
        let error = guard_bind(&addr, false).expect_err("0.0.0.0 must be rejected");
        // 拒启动文案必须含原因（AC）。
        assert!(error.contains("Refusing to start"), "error: {error}");
        assert!(error.contains("not a loopback address"), "error: {error}");
        assert!(error.contains("OMS_READONLY"), "error: {error}");
    }

    #[test]
    fn guard_bind_allows_non_loopback_in_readonly() {
        let addr = SocketAddr::from((Ipv4Addr::UNSPECIFIED, 8477));
        guard_bind(&addr, true).expect("readonly opens the gate");
    }

    #[test]
    fn guard_bind_keeps_loopback_behavior_unchanged() {
        for ip in [
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(std::net::Ipv6Addr::LOCALHOST),
        ] {
            let addr = SocketAddr::new(ip, 8477);
            guard_bind(&addr, false).expect("loopback unchanged: {ip}");
            guard_bind(&addr, true).expect("loopback readonly: {ip}");
        }
    }
}
