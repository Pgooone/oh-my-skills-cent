# Round 2 · 工作流管理器 v1 总进度

> 每卡完成 = 实现 agent 交付 + 独立 verifier 复跑通过 + lead 复核 git 实盘 + commit。
> 本轮队员参数：**model = sonnet，effort = max**。

## 批次 1 · 承重墙 spike
- [ ] download-spike（证伪下载解析/yaml 解析/注册表链路三条前提）

## 批次 2 · 并行
- [ ] workflow-core
- [ ] registry-client

## 批次 3
- [ ] workflow-use

## 批次 4 · 并行
- [ ] workflows-api
- [ ] workflows-ui

## 最终验收（proposal §6）
- [ ] 真浏览器全流程：浏览注册表 → 下载软件开发工作流 → 详情（分组/步骤/缺失标记）→ 使用（Sync Plan 预览含下载 → 执行）→ 入口清单落目标目录
- [ ] 打包 skill 形态生成 + 真实 Claude Code 会话验证可消费
- [ ] 占位步骤醒目标记 + 使用时提示
- [ ] 本地创建/编排（一步多 skill + 占位）→ 保存 → 可使用
- [ ] 三类门禁全绿：cargo test（默认 + web）/ tsc / vite build
