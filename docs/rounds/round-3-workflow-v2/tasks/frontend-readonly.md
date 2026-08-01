# 卡 C10 · frontend-readonly（只读模式前端适配）

> 设计：DD §8.5（health 单通道）。依赖 C7。

## 范围

- `src/lib/api.ts`：probeRealBackend 解析 /api/health 响应体取 `readonly`（桌面壳恒 false）
- `App.tsx`：readonly 存独立 state 并下发各视图
- 各视图只读适配：隐藏写入口（新建/编辑/删除/使用/推送/贡献/更新执行/设置保存/导入/目录浏览入口）；不调用 scan_inventory/list_dir/discover；`WorkflowsView` toolbar 改显 [上传贡献]（文件选择 → contribute_upload）；只读横幅

## AC（可断言）

- [ ] tsc 绿、vite build 绿
- [ ] 逻辑层（代码审读 + grep）：readonly 条件渲染分支覆盖写入口清单（新建/编辑/删除/使用/推送/贡献/更新执行/设置保存/导入/目录浏览——逐一对账）；`grep 'scan_inventory\|list_dir\|discover_project_workspaces'` 调用点均有 `!readonly` 守卫
- [ ] 端到端层（归 C11 AC5 真浏览器逐视图核）：readonly=true 时写按钮全部不渲染（非仅禁用）、toolbar 出现 [上传贡献]

## 守红线

- health 为唯一探测通道（不从 settings 取 readonly）；演示空态不破坏

## commit

`feat(ui): 公共只读模式前端适配`
