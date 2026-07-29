# 任务卡：workflow-core（批次 2，与 registry-client 并行）

> 设计依据：`../detailed-design.md` §1。

- [ ] Cargo.toml：加 `serde_yml`（非 optional，两壳共享）
- [ ] 新建 `src-tauri/src/workflow.rs`：Workflow/WorkflowGroup/WorkflowStep/StepSkill(untagged)/SkillRef 模型 + `validate()` + yaml 读写
- [ ] 本地已安装存储：`data_dir/workflows/<slug>/` 的 list/load/save/delete（坏文件降级为错误条目；slug 防 `..`）
- [ ] `lib.rs`：`pub mod workflow;` 无条件导出
- [ ] 测试：真实 yaml fixture ×2（含占位）、untagged 边界（空数组/单 placeholder/单 ref）、validate 全部分支、存取往返
- [ ] 门禁：`cargo test`（默认）+ `cargo test --no-default-features --features web` 全绿

**红线**：零 tauri 依赖；不改既有文件（lib.rs 仅加 mod 行）；不 git commit。
