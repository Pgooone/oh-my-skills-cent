# Round 2 · 工作流管理器 v1 监工起始指令（prompt.md）

## 角色与目标
你是 oh-my-skills-cent 第二轮（工作流管理器 v1）的监工。任务：
1. 按 `tasks/` 任务卡实现（设计：`../detailed-design.md`）
2. 按批次依赖顺序派发：批次 1 spike → 2（并行）→ 3 → 4（并行）
3. 每卡完成后跑质量门禁，独立 verifier 复跑，lead 复核 git 实盘
4. 最终验收：`proposal.md` §6 全部勾掉

## 执行参数（用户拍板）
- 实现与验证 agent：**model = sonnet，effort = max**
- 批次内独立模块**并行**，跨批次串行
- agent 只写代码不 commit；lead 验收后按卡单独 commit（中文 conventional commit）

## 代码质量要求
- 每卡通过任务卡所列门禁（cargo test 默认 + web features / tsc / vite build）
- 红线：既有文件增量仅限设计 §6 白名单；零 tauri 依赖进新核心模块；薄转发
- 全程更新 `tasks/progress.md`

## 开发批次
批次 1：download-spike（承重墙，绿了才能继续）
批次 2：workflow-core ∥ registry-client
批次 3：workflow-use
批次 4：workflows-api ∥ workflows-ui

## 两层验收
1. 逻辑层：独立 verifier 自写 fixture/断言复跑，不认实现者自证
2. 端到端层：真浏览器全流程 + 真实 Claude Code 会话消费打包 skill
