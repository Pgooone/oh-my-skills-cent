# Round 2 · 工作流管理器 v1 总进度

> 每卡完成 = 实现 agent 交付 + 独立 verifier 复跑通过 + lead 复核 git 实盘 + commit。
> 本轮队员参数：**model = sonnet，effort = max**。

## 批次 1 · 承重墙 spike
- [x] download-spike（三条前提全 PASS：skillPath 目录形式唯一正确、serde_yml 解析正确、注册表链路通畅；内建候选不命中 mattpocock 结构 → skillPath 实为必填）

## 批次 2 · 并行
- [x] workflow-core（verifier 通过；25 行 URL 镜像重复经 verifier 抓出、lead 收敛复用）
- [x] registry-client（verifier 通过；Settings 向后兼容实证、原子换缓存、9 个零外网测试）

## 批次 3
- [x] workflow-use（verifier pass（自建探针 8 case 全过）；lead 裁决 ⑦ 打包形态跳过独立同步 ops，agent team 返工闭环后 78/103 双绿）

## 批次 4 · 并行
- [ ] workflows-api
- [ ] workflows-ui

## 最终验收（proposal §6，判据纪律见 docs/acceptance-standards.md）
- [ ] 真浏览器全流程：浏览注册表 → 下载软件开发工作流 → 详情（分组/步骤/缺失标记）→ 使用（Sync Plan 预览含下载 → 执行）→ 入口清单落目标目录
- [ ] 打包 skill 形态生成 + 真实 Claude Code 会话验证可消费
- [ ] 占位步骤醒目标记 + 使用时提示
- [ ] 本地创建/编排（一步多 skill + 占位）→ 保存 → 可使用
- [ ] 三类门禁全绿：cargo test（默认 + web）/ tsc / vite build
