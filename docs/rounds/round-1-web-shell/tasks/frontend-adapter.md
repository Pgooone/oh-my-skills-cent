# 任务卡：frontend-adapter（批次 2，与 web-server 并行）

> 设计依据：`../detailed-design.md` §3。API 契约见 §2.3（`POST /api/commands/{name}`，422/403 `{error}`，`GET /api/health`）。

- [ ] 新建 `src/lib/api.ts`：`callApi<T>(command, args?)`（Tauri → invoke；Web → fetch）；`GET /api/health` 探测 + `hasRealBackend()`
- [ ] 新建 `src/lib/shell.ts`：`pickDirectory` / `openUrl` / `revealPath` / `askConfirm`（Tauri 走插件，Web 走降级；pickDirectory 的 Web 分支先留 DirPicker 挂载点，由 dir-browser 卡填充）
- [ ] `App.tsx`：invoke ×13 → callApi；open ×3 → pickDirectory；confirm → askConfirm；**18+ 处 demo 分支 `!isTauriRuntime()` → `!hasRealBackend()`**（仅「无后端用演示数据」语义的才改；桌面能力判断保留 isTauriRuntime 并收进 shell.ts）
- [ ] `SkillsView.tsx`：invoke ×2 → `openUrl` / `revealPath`
- [ ] `SettingsSheet.tsx`：open ×1 → pickDirectory（删现有 prompt 降级）
- [ ] `vite.config.ts`：dev 代理 `/api → http://127.0.0.1:8477`
- [ ] 门禁：`tsc` 无错、`npm run build` 绿
- [ ] 桌面回归：Tauri 运行时行为不变（invoke 路径逐条对照原参数）

**红线**：UI 组件零运行时感知（全部经 api/shell）；视觉零改动；不 git commit。
