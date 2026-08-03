# 卡 C4 · workflow-share（M4 胖包导出/导入）

> 设计：DD §4、R7/R8、design-review.md §4（W3 结论）。依赖 C1；**导出路径的防交互 env/凭证注入依赖 C2 下沉先行**（DD §4.1 门-F-18）。

## 范围

- `Cargo.toml`：`zip = { version = "~4.0", default-features = false, features = ["deflate"] }` + `base64`；Cargo.lock 提交 zip 相关增量。**indexmap 不在 repo 内 pin**（2026-08-03 修订：repo 构建走 stable，pin 仅 1.77.2 scratch 实证需要；且当前图上 toml 1.1.2 要求 indexmap ≥2.13，pin 不可执行）
- 新建 `src-tauri/src/workflow_share.rs`：export_package（DD §4.1：临时根组装 → manifest.json → 逐 Ref skill checkout 抓取 → 失败不产半成品 → zip → 清理含 clone 根反推删除）、import_package（DD §4.2 校验链全 9 条）
- `commands.rs` + `web/routes.rs` + 登记：`export_workflow_package`、`import_workflow_package`（web 两端点挂 `DefaultBodyLimit::max(96MB)`，其余路由不动）、`save_export_to_path`（**仅桌面注册**）

## AC（可断言）

- [ ] zip 负例 10 条逐条拒绝：**base64 解码前超长字符串拒绝（门-F4 预检）**/穿越/绝对路径/非 UTF-8 条目名/超 50MB/解压合计超 200MB/缺 workflow.yaml/坏 yaml/坏 slug/已存在冲突
- [ ] 正例 roundtrip：导出 → 导入到干净 data_dir → workflow 可读、source.json 还原、manifest 字段正确；base64 编解码字节一致
- [ ] 导出后 `data_dir/updates/` 与 `data_dir/tmp/` 无残留（含构造单 skill 失败场景）
- [ ] cargo test 默认 + web 全绿；**MSRV 实证（W3 本意，2026-08-03 修订）**：scratch crate（zip 4.0.0 + indexmap pin 2.9.0 + base64 0.23 + 本模块实际使用的全部 zip/base64 API 形态）在 1.77.2 工具链 build+run 过，scratch 即删。背景：HEAD 的 rust-version=1.77.2 早已名存实亡（serde_yml 0.0.13=1.85/axum=1.80，R2 遗留，非本卡引入），全仓抢救移交开放问题（docs/rounds/round-3-workflow-v2/msrv-offenders.txt 存档）

## 守红线

- R7 安检全链（base64 解码**前**长度预检）；R8 修订为「新增依赖不得抬有效 MSRV（zip 子树 1.77.2 实证）」；导出 = origin 现拉（§4.1 声明语义）
- 摘桃：checkout/normalize 复用 pub(crate) 面，不改 skill_ops

## commit

`feat(workflow): 新增胖包导出与导入校验安装`
