# 任务卡：端到端验收（批次 5 · 每轮末位固定收口卡）

> 判据纪律：`docs/acceptance-standards.md` 全文适用（本卡即其 §7 的落地）。
> **lead 亲跑**，UI 走真浏览器（chrome-devtools MCP），留证据。

## AC 清单（proposal §6，逐条确定性判据）

- [ ] **AC1 真浏览器全流程**：Workflows tab 浏览注册表 → 下载「软件开发工作流」→ 详情可见分组/步骤/缺失标记 → 使用（Sync Plan 预览含 download ops）→ 执行 → 目标 agent 目录出现 `_workflow-software-development/`（入口清单 + 有序 README）
  - 判据：执行前目标目录**无**该清单（DOM/文件系统双证）→ 执行后**有**且 README 步骤顺序 = workflow.yaml skills 数组顺序（D5）
  - **造数据复刻真实流程**：从注册表真实下载，禁止预置已安装状态
- [ ] **AC2 打包 skill 形态**：对同一工作流选打包形态 → 目标目录出现 `<slug>/`（SKILL.md + skills/ 结构化拷贝，递归 diff 与中心库一致）→ **真实 Claude Code 会话验证可消费**（新会话读入口 SKILL.md 后按指引找到子目录 skill）
- [ ] **AC3 占位步骤**：code-review-flow 详情页占位醒目；使用时 preconditions 含跳过提示
  - 判据：占位步骤**零 op**（预览 ops 中无其条目）且 warning 文案出现
- [ ] **AC4 本地创建/编排**：编辑器新建工作流（一步多 skill + 占位）→ 保存 → 出现在已安装 → 可使用
- [ ] **AC5 三门禁全绿**：cargo test 默认 + web / tsc / vite build
- [ ] **AC6 pageErrors=0**：全流程浏览器控制台零错误

## 卫生（§5）

- [ ] 验收产生的目录/文件全部还原（目标 agent 目录恢复原状、临时 fixture 删除）
- [ ] oms-web 进程按 PID/端口杀（严禁 pkill -f 宽匹配）；8477 端口确认释放
- [ ] 推送后 `git ls-remote origin` 与本地 rev-parse 完整 hash 全等（§6）

## 回写

- [ ] progress.md 验收结论：逐条 AC 结果 + 踩坑「写给后人」+ 证据（截图/日志要点）
