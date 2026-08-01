# 卡 C1 · git-foundation（M1 git-ops + M2 github-auth + settings 扩展）

> 设计：DD §1、§2、§6-R3/R10。后续全部写操作的地基。

## 范围（只动这些）

- 新建 `src-tauri/src/git_ops.rs`（DD §1 全部函数）
- 新建 `src-tauri/src/github_auth.rs`（DD §2 全部函数）
- `models.rs`：Settings +3 字段（githubToken/githubUsername/skillRegistryUrl，均 `#[serde(default)]`）
- `settings.rs`：默认值/空值回填（skill 注册表官方 URL）+ `merge_token` 三分支（None=保持/Some(非空)=替换/clearGithubToken=true=清除）+ unix `set_permissions(0o600)`（**代码注释如实注明：Windows 无对应语义，依赖用户 profile ACL**——评审门 F8/F-05）
- `commands.rs` + `web/routes.rs`：get_settings/**save_settings 返回值**一律裁剪（克隆置 None + hasGithubToken）；save 入参 userinfo/非 GitHub 拒绝（两个 *RegistryUrl）
- `lib.rs`/`web/mod.rs`：git_ops/github_auth 模块登记；`src/types.ts` + `SettingsSheet.tsx`：三字段（token=password 框+清除按钮+明文提示文案；两个 URL 输入校验拒绝 userinfo **与非 GitHub**——评审门 F3）
- fixture 补字段：`web/jail.rs:211`、`registry.rs:1433`

## AC（可断言）

- [ ] 新单测全过：base64 编码、redact 两形态（token 本体/base64）、detect_identity fallback、URL userinfo 拒绝、merge 三分支（含「改无关设置 token 不动」）、unix 0600 权限位断言、**get_settings 与 save_settings 两个响应 JSON 均无 githubToken 键且 hasGithubToken 正确**、is_official_repo 双侧 normalize、compare_url/fork_clone_url/fork_page_url/parse_owner_repo 纯函数组、resolve_token 的 env > settings 优先级
- [ ] cargo test 默认 + `--features web` 全绿（既有零回归）；tsc 绿；vite build 绿

## 守红线

- R3：token 正常落盘、API 边界一律裁剪、UI password 框；R10：git 调用约定全在 M1
- 摘桃：仅清单内文件；核心 8 文件零修改

## commit

`feat(core): 新增 git 写操作统一层与 GitHub 凭证管理`
