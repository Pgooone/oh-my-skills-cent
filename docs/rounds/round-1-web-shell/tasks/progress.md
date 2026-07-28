# Round 1 · Web 壳 总进度

> 每卡完成 = 实现 agent 交付 + 独立 verifier 复跑通过 + lead 复核 git 实盘 + commit。

## 批次 1 · 承重墙 spike
- [x] core-context（ce0607a 之后单独 commit；verifier 通过，lead 复核 39/39 绿）

## 批次 2 · 并行
- [ ] web-server
- [ ] frontend-adapter

## 批次 3
- [ ] dir-browser

## 批次 4 · 并行
- [ ] build-packaging
- [ ] deploy-docs

## 最终验收（proposal §6）
- [ ] Windows：`cargo run` 起服务，浏览器完成「扫描 → 预览 → 执行同步 → 历史」全流程
- [ ] 单二进制独立可用（release 构建，无外部静态文件）
- [ ] 三类门禁全绿：cargo test / tsc / vite build
- [ ] 安全负向：假 Host / 跨源 POST / 越权路径 均被拒
- [ ] Linux（WSL2 或服务器）启动 + SSH 隧道访问验证
