# 任务卡：build-packaging（批次 4，与 deploy-docs 并行）

> 设计依据：`../detailed-design.md` §5。**前置**：批次 2、3 已验收。

- [ ] `package.json` scripts：`web:dev`（并行 cargo run web bin + vite）、`web:build`（`npm run build && cargo build --release --no-default-features --features web --bin oms-web`）
- [ ] 验证构建顺序：先 vite build 后 cargo build（embed 编译期需 dist 存在）；debug-embed 使 debug 构建从磁盘读
- [ ] Windows 本机：`npm run web:build` 产出 `oms-web.exe`，启动后浏览器全流程冒烟
- [ ] WSL2：`cargo build --release --no-default-features --features web --bin oms-web` 产出 Linux 二进制并启动冒烟（无 webkit 依赖验证：`ldd` 无 webkit2gtk）
- [ ] 确认桌面构建不受影响：`cargo build`（默认 features）与既有 CI 命令不变

**红线**：CI 文件本轮不动；不 git commit。
