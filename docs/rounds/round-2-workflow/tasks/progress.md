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
- [x] workflows-api（verifier pass；7+7 薄转发、cache-first 断网探针、坏 slug 负例组、D8 覆盖实证；lead 复核 79/109）
- [x] workflows-ui（verifier pass；真浏览器注册表下载→详情→占位 banner 全流程、pageErrors=0；两处 lead 裁决落地：WorkflowDetail 形状、refresh cache-first）

## 最终验收（proposal §6，判据纪律见 docs/acceptance-standards.md）
- [x] AC1 真浏览器全流程：注册表下载 → 详情（3 分组/3 步骤全「将下载」）→ 预览含 3 条下载 ops → 执行 → 中心库 3 skill、目标目录 3 skill + `_workflow-software-development/`（README 步骤有序，D5）
- [x] AC2 打包形态：`software-development/` = SKILL.md（编排正文）+ skills/ 结构化拷贝（递归 diff ×3 逐字节一致）；**真实 agent 消费验证通过**（模拟 agent 读入口 → 按指引读到全部子 skill → 无死链 → 方法理解准确）；打包形态预览无同步方式区（⑦ 裁决实证）
- [x] AC3 占位步骤：列表行与详情「占位」徽标 + 预览 banner「含占位 skill（已跳过）」
- [x] AC4 本地创建：一步双 skill（有序）+ 占位 → 保存 → 已安装 → 详情正确；yaml 落盘保序
- [x] AC5 三门禁全绿：cargo test 默认 79 / web 109、tsc、vite build（lead 复跑）
- [x] AC6 pageErrors=0（全程 console 核查）
- [x] 卫生：测试写入全部还原（目标目录/中心库恢复空态、临时目录删除、服务按任务停止、8477 释放）
- [x] 彩蛋（活体证据）：两次执行后本会话即时发现新装 skill（domain-modeling/tdd 与打包 software-development）——agent 自动发现在真实 agent 身上发生
