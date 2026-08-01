//! M2 github-auth：token 解析（env 优先）、错误消息脱敏、GitHub URL 纯函数组。
//! token 三红线（R3）的核心实现点：git/gh 的错误消息出模块前必须过 redact_text。

use crate::context::AppContext;
use crate::skill_ops::normalize_github_url;
use base64::Engine;

/// token 注入 git/gh 时的 Basic 凭证形态：base64("x-access-token:{token}")。
/// 错误消息脱敏需同时识别该形态（redact_text），编码只此一处。
pub(crate) fn basic_auth_value(token: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{token}"))
}

/// 解析有效 token：OMS_GITHUB_TOKEN 环境变量优先（Q4），其次 settings 落盘值。
/// 两侧都取不到（或 settings 不可读）→ None。
pub fn resolve_token(ctx: &AppContext) -> Option<String> {
    if let Ok(value) = std::env::var("OMS_GITHUB_TOKEN") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    crate::settings::load_settings(ctx)
        .ok()?
        .github_token
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// 脱敏：纯字符串替换 token 本体与其 base64 凭证形态为 "***"（R3②）。
/// token 为 None/空白时原样返回。
pub fn redact_text(text: &str, token: Option<&str>) -> String {
    let Some(token) = token.filter(|value| !value.is_empty()) else {
        return text.to_string();
    };
    text.replace(token, "***")
        .replace(&basic_auth_value(token), "***")
}

/// 双侧归一化后相等即视为同一 GitHub 仓库（更新分流复用同款比较，见 §8.5）。
/// 任一侧非法（非 GitHub / 不可解析）→ false，不 panic。
pub fn is_official_repo(url: &str, official: &str) -> bool {
    match (normalize_github_url(url), normalize_github_url(official)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

/// 从 GitHub URL 提取 (owner, repo)；拒绝非 GitHub / userinfo / 非两段路径。
pub fn parse_owner_repo(url: &str) -> Result<(String, String), String> {
    let normalized = normalize_github_url(url)?;
    let path = normalized
        .strip_prefix("https://github.com/")
        .and_then(|rest| rest.strip_suffix(".git"))
        .ok_or_else(|| format!("Unable to parse GitHub repository from {url}"))?;
    let parts: Vec<&str> = path.split('/').collect();
    match parts.as_slice() {
        [owner, repo] if !owner.is_empty() && !repo.is_empty() => {
            Ok((owner.to_string(), repo.to_string()))
        }
        _ => Err(format!("Unable to parse GitHub repository from {url}")),
    }
}

/// 用户 fork 的 clone URL（https 形态，无 userinfo）。
pub fn fork_clone_url(username: &str, repo: &str) -> String {
    format!("https://github.com/{username}/{repo}.git")
}

/// fork 创建页（NeedFork 分支由前端打开）。
pub fn fork_page_url(owner: &str, repo: &str) -> String {
    format!("https://github.com/{owner}/{repo}/fork")
}

/// PR 预览 compare URL；base 分支恒 main——官方注册表契约约束（门-F-10）。
pub fn compare_url(
    owner: &str,
    repo: &str,
    username: &str,
    branch: &str,
    title: &str,
    body: &str,
) -> String {
    format!(
        "https://github.com/{owner}/{repo}/compare/main...{username}:{branch}?expand=1&title={}&body={}",
        percent_encode(title),
        percent_encode(body)
    )
}

/// query 值 percent-encode（自实现，不引 url crate）：unreserved 之外逐字节
/// %XX，UTF-8 多字节自然展开。
fn percent_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(char::from(byte))
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // resolve_token 读进程级环境变量；串行化避免用例间互相观测。
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn basic_auth_value_matches_github_basic_credential() {
        // base64("x-access-token:tok")，已知向量。
        assert_eq!(basic_auth_value("tok"), "eC1hY2Nlc3MtdG9rZW46dG9r");
    }

    #[test]
    fn redact_text_replaces_token_and_base64_form() {
        let token = "ghp_secret";
        let encoded = basic_auth_value(token);
        let text = format!("fatal: auth failed for {token} (header {encoded})");
        let redacted = redact_text(&text, Some(token));
        assert!(!redacted.contains(token), "token 本体必须脱敏");
        assert!(!redacted.contains(&encoded), "base64 形态必须脱敏");
        assert_eq!(redacted.matches("***").count(), 2);
    }

    #[test]
    fn redact_text_without_token_is_noop() {
        assert_eq!(redact_text("plain error", None), "plain error");
        assert_eq!(redact_text("plain error", Some("")), "plain error");
    }

    #[test]
    fn resolve_token_prefers_env_over_settings() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let temp = tempfile::tempdir().expect("temp");
        let ctx = AppContext::new(temp.path().join("data"), temp.path().join("home"));
        let mut settings = crate::settings::default_settings(&ctx).expect("defaults");
        settings.github_token = Some("file-token".to_string());
        crate::settings::save_settings(&ctx, &settings).expect("save");

        std::env::set_var("OMS_GITHUB_TOKEN", "env-token");
        assert_eq!(resolve_token(&ctx).as_deref(), Some("env-token"));
        std::env::remove_var("OMS_GITHUB_TOKEN");
        assert_eq!(resolve_token(&ctx).as_deref(), Some("file-token"));
    }

    #[test]
    fn resolve_token_blank_env_falls_back_and_missing_returns_none() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let temp = tempfile::tempdir().expect("temp");
        let ctx = AppContext::new(temp.path().join("data"), temp.path().join("home"));

        std::env::set_var("OMS_GITHUB_TOKEN", "   ");
        // 空白 env 视为未配置，回退 settings；此处亦无落盘 → None。
        assert_eq!(resolve_token(&ctx), None);
        std::env::remove_var("OMS_GITHUB_TOKEN");
        assert_eq!(resolve_token(&ctx), None);
    }

    #[test]
    fn is_official_repo_normalizes_both_sides() {
        let official = "https://github.com/Pgooone/oh-my-skills-skills.git";
        assert!(is_official_repo(
            "git@github.com:Pgooone/oh-my-skills-skills",
            official
        ));
        assert!(is_official_repo("Pgooone/oh-my-skills-skills", official));
        assert!(is_official_repo(
            "https://github.com/Pgooone/oh-my-skills-skills/",
            official
        ));
        assert!(!is_official_repo(
            "https://github.com/Pgooone/other-repo.git",
            official
        ));
        assert!(!is_official_repo(
            "https://gitlab.com/Pgooone/oh-my-skills-skills.git",
            official
        ));
        assert!(!is_official_repo("garbage", official));
    }

    #[test]
    fn parse_owner_repo_extracts_two_segments() {
        assert_eq!(
            parse_owner_repo("https://github.com/owner/repo.git").expect("parse"),
            ("owner".to_string(), "repo".to_string())
        );
        assert_eq!(
            parse_owner_repo("owner/repo").expect("slug"),
            ("owner".to_string(), "repo".to_string())
        );
        assert!(parse_owner_repo("https://user:pw@github.com/owner/repo").is_err());
        assert!(parse_owner_repo("https://github.com/owner/repo/extra").is_err());
        assert!(parse_owner_repo("https://gitlab.com/owner/repo").is_err());
    }

    #[test]
    fn url_builders_match_github_conventions() {
        assert_eq!(
            fork_clone_url("alice", "repo"),
            "https://github.com/alice/repo.git"
        );
        assert_eq!(
            fork_page_url("owner", "repo"),
            "https://github.com/owner/repo/fork"
        );
        assert_eq!(
            compare_url(
                "owner",
                "repo",
                "alice",
                "contrib/demo",
                "Add workflow demo",
                "line1\n中文 & more"
            ),
            "https://github.com/owner/repo/compare/main...alice:contrib/demo?expand=1&title=Add%20workflow%20demo&body=line1%0A%E4%B8%AD%E6%96%87%20%26%20more"
        );
    }
}
