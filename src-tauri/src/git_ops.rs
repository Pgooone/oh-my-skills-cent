//! M1 git-ops：git CLI 写操作统一层（R10）。全部 git 调用只准走这里——
//! 防交互 env、凭证 `-c http.extraheader` 注入（token 不进 URL）、身份
//! `-c user.name/email` 注入、stderr 捕获脱敏，约定全收敛在本模块。

use crate::github_auth::{basic_auth_value, redact_text};
use crate::skill_ops::normalize_github_url;
use std::path::Path;
use std::process::Command;

/// git commit 身份（detect_identity 探测或调用方指定）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitIdentity {
    pub name: String,
    pub email: String,
}

/// 基础 Command：GIT_TERMINAL_PROMPT=0 + GCM_INTERACTIVE=never（调研缺口 4①）。
/// 任何凭证弹窗都会让 headless 进程挂起，宁可失败不可交互。
pub fn base_command() -> Command {
    let mut cmd = Command::new("git");
    cmd.env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never");
    cmd
}

/// 凭证注入：`git -c http.extraheader="Authorization: Basic base64(x-access-token:{token})"`。
/// token 不进 URL（normalize 输出恒 https，无 userinfo）。
/// 注意：`-c` 是 git 全局选项，必须在追加子命令参数之前调用本函数。
pub fn with_auth(cmd: &mut Command, token: Option<&str>) {
    if let Some(token) = token.filter(|value| !value.trim().is_empty()) {
        cmd.arg("-c").arg(format!(
            "http.extraheader=Authorization: Basic {}",
            basic_auth_value(token)
        ));
    }
}

/// 身份注入：`-c user.name/email`；与 with_auth 同序约束（子命令参数之前）。
pub fn with_identity(cmd: &mut Command, id: &GitIdentity) {
    cmd.arg("-c")
        .arg(format!("user.name={}", id.name))
        .arg("-c")
        .arg(format!("user.email={}", id.email));
}

/// 执行并捕获：status 非 0 → Err(redact(stderr))；成功 Ok(stdout)。
/// stderr 先过 redact_text，token 本体与 base64 形态都不会进错误消息（R3②）。
pub fn run(cmd: &mut Command, token: Option<&str>) -> Result<String, String> {
    let output = cmd
        .output()
        .map_err(|error| format!("Unable to run git: {error}"))?;
    if !output.status.success() {
        let stderr = redact_text(&String::from_utf8_lossy(&output.stderr), token);
        return Err(format!(
            "git exited with {}: {}",
            output.status,
            stderr.trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// `git clone --depth 1`。URL 一律先过 GitHub-only 归一化（门-token-F6）。
pub fn clone_repo(url: &str, dest: &Path, token: Option<&str>) -> Result<(), String> {
    let url = normalize_github_url(url)?;
    let mut cmd = base_command();
    with_auth(&mut cmd, token);
    cmd.arg("clone").arg("--depth").arg("1").arg(url).arg(dest);
    run(&mut cmd, token).map(|_| ())
}

/// `git clone --depth 1`，URL 逐字使用（不过 GitHub-only 归一化）。
/// 逐字来源执行器——单元测试用本地 fixture git 仓库当来源；生产路径的
/// GitHub-only / userinfo 把关在上游边界（settings 保存校验 /
/// `Workflow::validate` / 公开 API normalize）。
pub fn clone_repo_verbatim(url: &str, dest: &Path, token: Option<&str>) -> Result<(), String> {
    let mut cmd = base_command();
    with_auth(&mut cmd, token);
    cmd.arg("clone").arg("--depth").arg("1").arg(url).arg(dest);
    run(&mut cmd, token).map(|_| ())
}

/// `git ls-remote <url>`：探测远端可达性与 refs（贡献流程探测 fork 用）。
pub fn ls_remote(url: &str, token: Option<&str>) -> Result<String, String> {
    let url = normalize_github_url(url)?;
    let mut cmd = base_command();
    with_auth(&mut cmd, token);
    cmd.arg("ls-remote").arg(url);
    run(&mut cmd, token)
}

/// `git checkout -b <branch>`。
pub fn create_branch(repo: &Path, branch: &str) -> Result<(), String> {
    let mut cmd = base_command();
    cmd.arg("-C")
        .arg(repo)
        .arg("checkout")
        .arg("-b")
        .arg(branch);
    run(&mut cmd, None).map(|_| ())
}

/// `git add -A` + `git commit -m`，返回 commit hash（rev-parse HEAD）。
pub fn commit_all(repo: &Path, msg: &str, id: &GitIdentity) -> Result<String, String> {
    let mut add = base_command();
    add.arg("-C").arg(repo).arg("add").arg("-A");
    run(&mut add, None)?;

    let mut commit = base_command();
    with_identity(&mut commit, id);
    commit
        .arg("-C")
        .arg(repo)
        .arg("commit")
        .arg("-m")
        .arg(msg);
    run(&mut commit, None)?;

    let mut rev_parse = base_command();
    rev_parse.arg("-C").arg(repo).arg("rev-parse").arg("HEAD");
    Ok(run(&mut rev_parse, None)?.trim().to_string())
}

/// `git push <remote> <refspec>`。remote 为 URL 形态时先过 GitHub-only 归一化；
/// 为远端名（如 origin）时原样使用（clone 来源已经过归一化把关）。
pub fn push(repo: &Path, remote: &str, refspec: &str, token: Option<&str>) -> Result<(), String> {
    let remote = if looks_like_url(remote) {
        normalize_github_url(remote)?
    } else {
        remote.to_string()
    };
    let mut cmd = base_command();
    with_auth(&mut cmd, token);
    cmd.arg("-C").arg(repo).arg("push").arg(remote).arg(refspec);
    run(&mut cmd, token).map(|_| ())
}

/// 探测 repo 的 commit 身份：git config user.name/email 读不到（非 repo、未
/// 配置、git 缺失任一情形）则回退 fallback_user /
/// {fallback_user}@users.noreply.github.com——noreply 形态让推到 GitHub 的
/// 贡献 commit 正确关联账户（DD §1）。
pub fn detect_identity(repo: &Path, fallback_user: &str) -> GitIdentity {
    let name = config_value(repo, "user.name").unwrap_or_else(|| fallback_user.to_string());
    let email = config_value(repo, "user.email")
        .unwrap_or_else(|| format!("{fallback_user}@users.noreply.github.com"));
    GitIdentity { name, email }
}

fn config_value(repo: &Path, key: &str) -> Option<String> {
    let mut cmd = base_command();
    cmd.arg("-C").arg(repo).arg("config").arg("--get").arg(key);
    run(&mut cmd, None)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// remote 参数是否 URL 形态（区分远端名与 URL）。
fn looks_like_url(value: &str) -> bool {
    value.contains("://") || value.starts_with("git@")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(cmd: &Command) -> Vec<String> {
        cmd.get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn base_command_disables_interactive_prompts() {
        let cmd = base_command();
        let envs: Vec<(String, String)> = cmd
            .get_envs()
            .filter_map(|(key, value)| {
                value.map(|value| {
                    (
                        key.to_string_lossy().into_owned(),
                        value.to_string_lossy().into_owned(),
                    )
                })
            })
            .collect();
        assert!(envs.contains(&("GIT_TERMINAL_PROMPT".to_string(), "0".to_string())));
        assert!(envs.contains(&("GCM_INTERACTIVE".to_string(), "never".to_string())));
    }

    #[test]
    fn with_auth_injects_extraheader_basic_credential() {
        let mut cmd = base_command();
        with_auth(&mut cmd, Some("tok"));
        assert_eq!(
            args(&cmd),
            vec![
                "-c".to_string(),
                "http.extraheader=Authorization: Basic eC1hY2Nlc3MtdG9rZW46dG9r".to_string()
            ]
        );
    }

    #[test]
    fn with_auth_skips_blank_or_missing_token() {
        let mut cmd = base_command();
        with_auth(&mut cmd, None);
        with_auth(&mut cmd, Some("   "));
        assert!(args(&cmd).is_empty());
    }

    #[test]
    fn with_identity_injects_user_config() {
        let id = GitIdentity {
            name: "Bot".to_string(),
            email: "bot@localhost".to_string(),
        };
        let mut cmd = base_command();
        with_identity(&mut cmd, &id);
        assert_eq!(
            args(&cmd),
            vec![
                "-c".to_string(),
                "user.name=Bot".to_string(),
                "-c".to_string(),
                "user.email=bot@localhost".to_string()
            ]
        );
    }

    #[test]
    fn run_reports_failure_with_status() {
        let temp = tempfile::tempdir().expect("temp");
        let mut cmd = base_command();
        cmd.arg("-C").arg(temp.path().join("no-such-dir")).arg("status");
        let error = run(&mut cmd, None).expect_err("missing repo must fail");
        assert!(error.contains("git exited with"), "error: {error}");
    }

    /// URL 防线（门-token-F6）：userinfo / 非 GitHub 在 spawn git 之前即拒绝。
    #[test]
    fn url_guard_rejects_userinfo_and_non_github_before_spawning() {
        let temp = tempfile::tempdir().expect("temp");
        let dest = temp.path().join("dest");
        for bad in [
            "https://user:pw@github.com/owner/repo",
            "https://user:pw@github.com/owner/repo.git",
            "https://gitlab.com/owner/repo.git",
            "https://example.com/owner/repo",
        ] {
            assert!(clone_repo(bad, &dest, None).is_err(), "clone {bad}");
            assert!(ls_remote(bad, None).is_err(), "ls-remote {bad}");
            assert!(push(temp.path(), bad, "HEAD", None).is_err(), "push {bad}");
        }
        assert!(!dest.exists(), "拒绝前不得产生任何 git 调用产物");
    }

    /// 逐字变体正例：本地 fixture 仓库（非 GitHub URL）不过归一化也能 clone——
    /// 这正是 workflow_registry / skill_ops 测试钩子依赖的契约（门-F-18）。
    #[test]
    fn clone_repo_verbatim_clones_local_fixture_repo() {
        let temp = tempfile::tempdir().expect("temp");
        let repo = temp.path().join("repo");
        std::fs::create_dir_all(&repo).expect("repo dir");

        let mut init = base_command();
        init.arg("-C").arg(&repo).arg("init");
        run(&mut init, None).expect("git init");
        std::fs::write(repo.join("README.md"), "fixture").expect("fixture file");
        let mut add = base_command();
        add.arg("-C").arg(&repo).arg("add").arg("-A");
        run(&mut add, None).expect("git add");
        let id = GitIdentity {
            name: "Test Bot".to_string(),
            email: "bot@example.com".to_string(),
        };
        let mut commit = base_command();
        with_identity(&mut commit, &id);
        commit
            .arg("-C")
            .arg(&repo)
            .arg("-c")
            .arg("commit.gpgsign=false")
            .arg("commit")
            .arg("-m")
            .arg("fixture");
        run(&mut commit, None).expect("git commit");

        let source = repo.to_string_lossy().into_owned();
        let dest = temp.path().join("dest");
        clone_repo_verbatim(&source, &dest, None).expect("verbatim clone");
        assert!(dest.join("README.md").is_file());
    }

    #[test]
    fn detect_identity_falls_back_when_git_config_unavailable() {
        let temp = tempfile::tempdir().expect("temp");
        // 目录不存在 → git -C 直接失败 → 回退（与机器全局 git 配置无关，确定性）。
        let id = detect_identity(&temp.path().join("no-such-dir"), "oms-bot");
        assert_eq!(
            id,
            GitIdentity {
                name: "oms-bot".to_string(),
                email: "oms-bot@users.noreply.github.com".to_string()
            }
        );
    }

    /// 正例：真 git（与 workflow_registry 测试同款前提：本机 git 可用）。
    #[test]
    fn detect_identity_reads_repo_local_config() {
        let temp = tempfile::tempdir().expect("temp");
        let repo = temp.path().join("repo");
        std::fs::create_dir_all(&repo).expect("repo dir");

        let mut init = base_command();
        init.arg("-C").arg(&repo).arg("init");
        run(&mut init, None).expect("git init");
        for (key, value) in [("user.name", "Test Bot"), ("user.email", "bot@example.com")] {
            let mut config = base_command();
            config.arg("-C").arg(&repo).arg("config").arg(key).arg(value);
            run(&mut config, None).expect("git config");
        }

        let id = detect_identity(&repo, "fallback");
        assert_eq!(
            id,
            GitIdentity {
                name: "Test Bot".to_string(),
                email: "bot@example.com".to_string()
            }
        );
    }
}
