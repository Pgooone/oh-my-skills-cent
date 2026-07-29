# 任务卡：workflow-use（批次 3）

> 设计依据：`../detailed-design.md` §3。**前置**：workflow-core + registry-client 已验收。

- [ ] 新建 `src-tauri/src/workflow_use.rs`：StepSkillStatus 计算 + `preview_use_workflow()` + 双输出生成器（入口清单 / 打包 skill）
- [ ] `models.rs`：SyncPlan op 支持 `download-to-library`（增量，serde 兼容）
- [ ] `sync_plan.rs`：apply 增加 download-to-library 执行分支（调 skill_ops 下载执行器；**新增分支，不改既有分支**）
- [ ] 操作序列：downloads 在前 → 标准同步 ops → output ops；占位步骤不进 op 但进 preconditions warning
- [ ] 入口清单生成：`_workflow-<slug>/`（workflow.yaml 拷贝 + README 生成：分组→步骤→**有序** skill 列表（D5）+ 同级目录指引）
- [ ] 打包 skill 生成：`<workflow-slug>/`（SKILL.md 编排正文 + `skills/` 子目录结构化拷贝，ADR-0009）
- [ ] 测试：缺失计算三分支；混合 case op 序列断言；两个生成器 tempdir 断言（目录结构/frontmatter/有序列表/递归 diff）；fixture 本地 git 仓库当下载来源（允许测试钩子绕过 GitHub-only 校验）
- [ ] 门禁：默认 + web features cargo test 全绿

**红线**：既有文件增量仅限白名单（models/sync_plan/lib）；不 git commit。
