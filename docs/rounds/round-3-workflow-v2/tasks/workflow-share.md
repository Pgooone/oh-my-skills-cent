# 卡 C4 · workflow-share（M4 胖包导出/导入）

> 设计：DD §4、R7/R8、design-review.md §4（W3 结论）。依赖 C1；**导出路径的防交互 env/凭证注入依赖 C2 下沉先行**（DD §4.1 门-F-18）。

## 范围

- `Cargo.toml`：`zip = { version = "~4.0", default-features = false, features = ["deflate"] }` + `base64`；**Cargo.lock 入库 + `cargo update -p indexmap --precise 2.9.0`**（W3 spike 前置，漏了 1.77 编不过）
- 新建 `src-tauri/src/workflow_share.rs`：export_package（DD §4.1：临时根组装 → manifest.json → 逐 Ref skill checkout 抓取 → 失败不产半成品 → zip → 清理含 clone 根反推删除）、import_package（DD §4.2 校验链全 9 条）
- `commands.rs` + `web/routes.rs` + 登记：`export_workflow_package`、`import_workflow_package`（web 两端点挂 `DefaultBodyLimit::max(96MB)`，其余路由不动）、`save_export_to_path`（**仅桌面注册**）

## AC（可断言）

- [ ] zip 负例 10 条逐条拒绝：**base64 解码前超长字符串拒绝（门-F4 预检）**/穿越/绝对路径/非 UTF-8 条目名/超 50MB/解压合计超 200MB/缺 workflow.yaml/坏 yaml/坏 slug/已存在冲突
- [ ] 正例 roundtrip：导出 → 导入到干净 data_dir → workflow 可读、source.json 还原、manifest 字段正确；base64 编解码字节一致
- [ ] 导出后 `data_dir/updates/` 与 `data_dir/tmp/` 无残留（含构造单 skill 失败场景）
- [ ] cargo test 默认 + web 全绿；`cargo +1.77.2 check`（或本机最低可用工具链）编译过

## 守红线

- R7 安检全链（base64 解码**前**长度预检）；R8 钉版 zip ~4.0 + indexmap pin；导出 = origin 现拉（§4.1 声明语义）
- 摘桃：checkout/normalize 复用 pub(crate) 面，不改 skill_ops

## commit

`feat(workflow): 新增胖包导出与导入校验安装`
