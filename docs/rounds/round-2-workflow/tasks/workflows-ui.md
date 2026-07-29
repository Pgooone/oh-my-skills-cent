# 任务卡：workflows-ui（批次 4，与 workflows-api 并行）

> 设计依据：`../detailed-design.md` §5。API 契约见 §4.1（名字/参数/返回已钉死）。

- [ ] `src/types.ts`：Workflow 等类型镜像（camelCase）
- [ ] `App.tsx`：view 加 `"workflows"`，顶栏第三按钮「工作流」
- [ ] `src/views/WorkflowsView.tsx`：已安装 / 远程可下载两区列表 + 搜索，视觉延续 SkillsView
- [ ] `src/components/workflow/WorkflowDetailPanel.tsx`：分组→步骤→skill 状态徽标（可用/将下载/占位醒目）
- [ ] `src/components/workflow/WorkflowEditor.tsx`：创建/编辑（meta + groups + steps 排序（上移/下移按钮）+ skills 选择器（中心库 inventory）+ 占位开关），零新依赖
- [ ] `src/components/workflow/UseWorkflowSheet.tsx`：目标 + 范围 + 输出形态二选一 → 预览（复用 `src/views/sync/` PlanDetailPanel）→ 既有 apply 执行
- [ ] `SettingsSheet.tsx`：「数据」tab 加「工作流注册表 URL」字段
- [ ] 门禁：`tsc` + `npm run build` 绿；grep 确认 UI 只经 api.ts/shell.ts 访问后端

**红线**：视觉语言与现有一致（看 SkillsView/SyncView 既有组件）；不 git commit。
