# 卡 C8 · frontend-workflows（工作流 v2 前端接线）

> 设计：DD §8.5。依赖 C3/C4/C5（endpoints 就绪）。

## 范围

- `WorkflowDetailPanel.tsx`（33-48 操作行）：+[推送][导出][检查更新]
- `WorkflowsView.tsx`：toolbar +[检查全部更新][导入分享包]；InstalledRow 状态徽标（有更新/已修改/本地，来自 check_workflow_updates）；RemoteRow 已安装条目 +[贡献]
- 对话框三枚：更新确认（Modified 警告「将覆盖你的本地修改（会先备份）」）、贡献结果（**noToken→导出胖包+贡献指南引导** / needFork→window.open fork 页 / ready→window.open compare URL）、导入结果
- 导出/导入交互：web=Blob 下载/File 读取 base64；Tauri=plugin-dialog save + save_export_to_path
- `src/types.ts`：新返回类型（WorkflowUpdateStatus/ContributeOutcome 等）

## AC（可断言）

- [ ] tsc 绿、vite build 绿
- [ ] 接线核查（机器判据）：`grep -c 'callApi("push_workflow_to_registry"' ≥1`、`callApi("contribute_workflow"` ≥1、`callApi("export_workflow_package"` ≥1、`callApi("check_workflow_updates"` ≥1、`callApi("import_workflow_package"` ≥1；`confirmModified: true` 字面量仅出现在更新确认对话框回调中（grep 比对）；贡献三态按**返回体 status 字段**分支（"noToken"/"needFork"/"ready"，DD §5.2 订正版 wire）
- [ ] 只读适配不在本卡（C10）

## 守红线

- 不改动既有交互语义；演示空态（hasRealBackend=false）不破坏

## commit

`feat(ui): 工作流推送/导出/更新检查前端接线`
