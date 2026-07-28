# Round 1 · Web 壳 总进度

> 每卡完成 = 实现 agent 交付 + 独立 verifier 复跑通过 + lead 复核 git 实盘 + commit。

## 批次 1 · 承重墙 spike
- [x] core-context（ce0607a 之后单独 commit；verifier 通过，lead 复核 39/39 绿）

## 批次 2 · 并行
- [x] web-server（verifier 通过；lead 复核 55/55 绿 + TCP 冒烟；范围偏差 commands→skill_ops 抽取已核实）
- [x] frontend-adapter（verifier 通过；lead 复核 build 绿；实际 15 处 invoke 全部替换）

## 批次 3
- [x] dir-browser（含 D7-R1 分层 jail 修订；verifier 12 条 TCP 断言 + 真浏览器 DirPicker 闭环；lead 复核 64/64 + 39/39 绿）

## 批次 4 · 并行
- [x] build-packaging（verifier 5/5；release 内嵌负向验证铁证；web:dev 双 script 形态备案）
- [x] deploy-docs（verifier 判死 1 处：缺 npm run web:build 主命令 → lead 已修复，另修 Node 版本与 u16 范围两处瑕疵 + clone URL 改为 fork）

## 最终验收（proposal §6）
- [ ] Windows：`cargo run` 起服务，浏览器完成「扫描 → 预览 → 执行同步 → 历史」全流程
- [ ] 单二进制独立可用（release 构建，无外部静态文件）
- [ ] 三类门禁全绿：cargo test / tsc / vite build
- [ ] 安全负向：假 Host / 跨源 POST / 越权路径 均被拒
- [ ] Linux（WSL2 或服务器）启动 + SSH 隧道访问验证
