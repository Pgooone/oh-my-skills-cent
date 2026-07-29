# 任务卡：workflows-api（批次 4，与 workflows-ui 并行）

> 设计依据：`../detailed-design.md` §4。**前置**：批次 2/3 已验收。

- [ ] `commands.rs`（cfg tauri-shell）+ `web/routes.rs`：7 个新 command/endpoint 薄转发（list_installed / list_remote / get_workflow_detail / download_workflow / save_workflow / delete_workflow / preview_use_workflow），apply 复用既有
- [ ] slug 参数统一 `[a-z0-9-]+` 校验；web 端 D8 guard 自动覆盖，无文件路径参数故 jail 不涉及（确认 registry URL 仍走 normalize_github_url）
- [ ] 测试：web 端 endpoint 正例（tempdir fixture 建工作流 → list/detail/save/delete 往返 + preview 生成）；负例（坏 slug 422）
- [ ] 门禁：默认 + web features cargo test 全绿

**红线**：薄转发（NFR-2），业务逻辑零下沉；不 git commit。
