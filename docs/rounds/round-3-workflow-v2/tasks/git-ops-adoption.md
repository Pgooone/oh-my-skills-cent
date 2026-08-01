# 卡 C2 · git-ops-adoption（两处既有 clone 下沉 M1）

> 设计：HLD §4 例外①、DD §6-R6。依赖 C1。
> **卡面修订（2026-08-01，lead 实现级裁决）**：队员开工核查发现 clone_repo 的无条件归一化与两处调用点的「逐字来源测试钩子」契约冲突（本地 fixture 路径过 normalize 必败，亲验 repo_source=本地路径属实）。裁决方案 2'：git_ops.rs 新增 `clone_repo_verbatim`（逐字 URL 原语，C2 范围扩大允许改 git_ops.rs），调用点替换并保留 `Unable to clone {source}` 错误前缀；URL 防线仍由上游边界（settings 保存校验 / Workflow::validate / 公开 API normalize）把守。方案 1（调用点自行组装命令）否决：组装泄漏出 git_ops 且无命名原语防重蹈。

## 范围

- `git_ops.rs`：新增 `clone_repo_verbatim(url, dest, token)`（逐字 URL 不过 normalize；base_command + with_auth + run 脱敏；doc 注明逐字契约与上游把关理由）+ 本地 fixture 正例单测（init+commit 后 clone 成功）
- `workflow_registry.rs:136`（refresh_cache 的 clone）与 `skill_ops.rs:221`（checkout_skill_from_clone_source 的 clone）两个调用点替换为 `git_ops::clone_repo_verbatim(url, dest, github_auth::resolve_token(ctx))`（**None 时行为与现状完全一致**），map_err 包装保留 `Unable to clone {source}: ` 前缀；refresh_cache 的 clone 失败+旧缓存→Ok(()) fallback 语义原样留在调用点
- 除上述外，两文件零改动；其余文件一律不碰

## AC

- [ ] cargo test 默认 + web 全绿（既有注册表/skill 相关测试**零修改**全部通过 = 行为兼容实证；判据为「零修改 + 全过」，非数量）
- [ ] clone_repo_verbatim 本地 fixture 正例单测通过（不归一化的本地路径 clone 成功）
- [ ] `grep Command::new` 全 src 生产代码仅剩 git_ops.rs（测试 helper 除外）
- [ ] 错误消息前缀 `Unable to clone {source}` 保持（failed_pull_keeps 等断言不破裂）

## 守红线

- 摘桃例外仅限两处调用点 + git_ops.rs 新增函数；不改任何既有函数签名与行为语义

## commit

`refactor(core): 既有两处 clone 调用点收敛至 git_ops`
