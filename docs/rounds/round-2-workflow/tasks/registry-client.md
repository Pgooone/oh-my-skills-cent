# 任务卡：registry-client（批次 2，与 workflow-core 并行）

> 设计依据：`../detailed-design.md` §2。

- [ ] 新建 `src-tauri/src/workflow_registry.rs`：RemoteWorkflowSummary + fetch_index / fetch_workflow / download_to_installed
- [ ] 拉取：git clone --depth 1 → `data_dir/registry/remote-<ts>` → 原子替换 `current`（失败保留旧缓存）；URL 过 `skill_ops::normalize_github_url`
- [ ] `models.rs` Settings 增量字段 `workflow_registry_url: Option<String>`（serde default）；`settings.rs` load 空值回填官方缺省 `https://github.com/Pgooone/oh-my-skills-workflows.git`
- [ ] `lib.rs`：`pub mod workflow_registry;`
- [ ] 测试：本地 fixture git 仓库（tempdir 构造 index + 子目录 + git init）走 fetch/download 全流程（不依赖网络）；installed 对照；坏 index 容错
- [ ] 门禁：默认 + web features 的 cargo test 全绿

**红线**：零 tauri 依赖；既有文件增量仅限白名单（models/settings/lib）；不 git commit。
