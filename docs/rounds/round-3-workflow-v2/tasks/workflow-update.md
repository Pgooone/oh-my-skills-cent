# 卡 C3 · workflow-update（M3 三态更新检查）

> 设计：DD §3。依赖 C1。

## 范围

- 新建 `src-tauri/src/workflow_update.rs`（DD §3.1-3.3 全部：SourceMeta 读写、三态判定、check_all/check_one、apply_update、孤儿清理）
- `commands.rs` + `web/routes.rs`：既有 `download_workflow` 薄转发成功后 +1 行 `record_source`；新 command/endpoint：`check_workflow_updates`、`update_workflow`（薄转发，web 沿用 oneshot 测试模式）
- `lib.rs`/`web/mod.rs` 登记

## AC（可断言）

- [ ] 单测全过：三态判定矩阵 7 case（Local 无 source / **有 source 但注册表无条目 → Local（门-F-12 语义）** / UpToDate / UpdateAvailable version 不等 / UpdateAvailable 同 version 内容变 / Modified / Modified+remote_changed）、孤儿 source 惰性清理、Modified 未确认拒绝、备份目录产生且内容等于更新前、更新后本地 hash == 注册表缓存 hash、record_source 写读往返
- [ ] 既有 download 链路测试零修改通过（record_source 不破坏既有行为）
- [ ] cargo test 默认 + web 全绿

## 守红线

- 元数据在 `data_dir/workflow-sources/`（目录外，禁入 workflows/<slug>/ 内）；复用 `download_to_installed`/`fetch_index` 不改既有核心
- 测试造数据复刻真实流程（下载→检查→更新），禁预置期望终态

## commit

`feat(workflow): 新增工作流来源元数据与三态更新检查`
