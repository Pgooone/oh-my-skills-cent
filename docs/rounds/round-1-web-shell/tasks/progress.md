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
- [x] Windows：起服务，真浏览器完成「扫描（118 skills）→ 预览 → 执行同步 → 历史落账」全流程（lead 实测；symlink 前置检查正确拦截 Windows 无特权场景——设计行为）
- [x] 单二进制独立可用（release 内嵌负向验证：dist 改名后仍逐字节服务页面）
- [x] 三类门禁全绿：cargo test（64 web + 39 默认）/ tsc / vite build
- [x] 安全负向：假 Host / 跨源 POST / cross-site 全 403，同源 200（lead curl 实测）；浏览器全程零 pageError
- [x] Linux（WSL2 Ubuntu）实机验证通过：二进制 `ldd` 仅 libc/libm/libgcc（**零 webkit/gtk**）；启动后 health 200、假 Host/跨源全 403；假 `claude` CLI 触发 agent 检测、扫描发现测试 skill；Windows 侧 localhost 转发访问通（≈SSH 隧道路径）；测试残留已清理
