# 任务卡：dir-browser（批次 3）

> 设计依据：`../detailed-design.md` §4。**前置**：web-server + frontend-adapter 已验收。

- [ ] 后端 `POST /api/commands/list_dir`（routes.rs）：`{ path?: string }`（缺省 home_dir）→ `{ path, parent, entries: [{ name, path, isDir }] }`，只列一层、目录在前；jail 走 §2.4 专用规则（home 子树 + 注册根 + Windows 盘符顶层一层）
- [ ] 新建 `src/components/DirPicker.tsx`：modal——路径面包屑、上级、目录列表（仅目录、点击下钻）、「选择此目录」「取消」；样式对齐现有组件风格
- [ ] `shell.ts` 的 `pickDirectory` Web 分支接通 DirPicker（promise 化）
- [ ] 4 处调用点端到端验证：App.tsx ×3 + SettingsSheet.tsx ×1
- [ ] 测试：list_dir jail 负向（home 外路径 → 403）；缺省 path 返回 home
- [ ] 门禁：`cargo test --no-default-features --features web` 绿；`tsc` / `npm run build` 绿

**红线**：不改 core 模块；不 git commit。
