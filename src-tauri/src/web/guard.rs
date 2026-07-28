//! D8 防 CSRF / DNS rebinding 中间件（作用于所有 /api 请求）。
//!
//! 仅监听 localhost ≠ 只有本机进程能访问：浏览器内任意网页都能向 127.0.0.1
//! 发请求。对能删文件的无认证服务必须堵上：
//! - `Host` 头（去端口）必须 ∈ {localhost, 127.0.0.1, [::1]}
//! - 带 `Sec-Fetch-Site: cross-site` 的请求一律 403
//! - POST 若带 `Origin` 头：Origin 的 host 部分必须 == 请求 Host

use super::routes::error_response;
use axum::{
    extract::Request,
    http::{header, Method, StatusCode},
    middleware::Next,
    response::Response,
};

const ALLOWED_HOSTS: [&str; 3] = ["localhost", "127.0.0.1", "[::1]"];

pub async fn local_only_guard(request: Request, next: Next) -> Response {
    let headers = request.headers();

    let Some(host) = headers.get(header::HOST).and_then(|value| value.to_str().ok()) else {
        return forbidden("Missing Host header");
    };
    if !ALLOWED_HOSTS.contains(&host_without_port(host)) {
        return forbidden(format!("Host '{host}' is not allowed"));
    }

    let cross_site = headers
        .get("sec-fetch-site")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.eq_ignore_ascii_case("cross-site"))
        .unwrap_or(false);
    if cross_site {
        return forbidden("Cross-site requests are not allowed");
    }

    if request.method() == Method::POST {
        if let Some(origin) = headers
            .get(header::ORIGIN)
            .and_then(|value| value.to_str().ok())
        {
            match origin_host(origin) {
                Some(origin_host) if origin_host.eq_ignore_ascii_case(host) => {}
                _ => {
                    return forbidden(format!(
                        "Origin '{origin}' does not match Host '{host}'"
                    ));
                }
            }
        }
    }

    next.run(request).await
}

fn forbidden(reason: impl Into<String>) -> Response {
    error_response(StatusCode::FORBIDDEN, reason)
}

/// `localhost:8477` → `localhost`，`[::1]:8477` → `[::1]`。
fn host_without_port(host: &str) -> &str {
    if host.starts_with('[') {
        if let Some(end) = host.find(']') {
            return &host[..=end];
        }
    }
    host.split(':').next().unwrap_or(host)
}

/// Origin（`scheme://host[:port]`）的 host 部分；无法解析时 None（调用方拒绝）。
fn origin_host(origin: &str) -> Option<&str> {
    let (_, rest) = origin.split_once("://")?;
    let host = rest.split('/').next().unwrap_or("");
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_port_from_host_header() {
        assert_eq!(host_without_port("localhost:8477"), "localhost");
        assert_eq!(host_without_port("localhost"), "localhost");
        assert_eq!(host_without_port("127.0.0.1:8477"), "127.0.0.1");
        assert_eq!(host_without_port("[::1]:8477"), "[::1]");
        assert_eq!(host_without_port("[::1]"), "[::1]");
    }

    #[test]
    fn parses_origin_host_part() {
        assert_eq!(origin_host("http://localhost:1420"), Some("localhost:1420"));
        assert_eq!(origin_host("https://evil.example.com/"), Some("evil.example.com"));
        assert_eq!(origin_host("null"), None);
        assert_eq!(origin_host("http://"), None);
    }
}
