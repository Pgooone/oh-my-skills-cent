# 卡 C2 · git-ops-adoption（两处既有 clone 下沉 M1）

> 设计：HLD §4 例外①、DD §6-R6。依赖 C1。

## 范围

- `workflow_registry.rs:136`（refresh_cache 的 clone）与 `skill_ops.rs:221`（checkout_skill_from_clone_source 的 clone）两个调用点替换为 `git_ops::clone_repo(url, dest, token)`，token 取 `github_auth::resolve_token(ctx)`（**None 时行为与现状完全一致**）
- 除这两处调用点与必要 use 外，两文件零改动

## AC

- [ ] cargo test 默认 + web 全绿（既有注册表/skill 相关测试**零修改**全部通过 = 行为兼容实证；判据为「零修改 + 全过」，非数量）
- [ ] `grep Command::new` 全 src 生产代码仅剩 git_ops.rs（测试 helper 除外）
- [ ] 错误消息形态与既有一致（下游断言错误文案的测试不破）

## 守红线

- 摘桃例外仅限这两行调用点；不改任何函数签名与行为语义

## commit

`refactor(core): 既有两处 clone 调用点收敛至 git_ops`
