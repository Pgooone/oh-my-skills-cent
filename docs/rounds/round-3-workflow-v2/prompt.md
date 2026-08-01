# Round 3 · 工作流 v2 监工起始指令（prompt.md）

## 角色与目标
你是 oh-my-skills-cent 第三轮（工作流 v2）的监工。任务：
1. 按 `tasks/` 任务卡实现（设计：`../detailed-design.md` 评审门订正版；评审记录：`../design-review.md`）
2. 按批次依赖顺序派发：批次 1（C1 + C0 spike）→ 2 → 3（C3→C4→C5）→ 4 → 5 → 6（C8→C9→C10）→ 7 收口
3. 每卡完成后跑质量门禁，独立 verifier 复跑，lead 复核 git 实盘
4. 最终验收：`../proposal.md` §6 全部勾掉

## 执行参数（用户拍板）
- 实现与验证 agent：**model = sonnet，effort = max**；命名队员可 SendMessage 返工续聊
- **全串行**（commands.rs/routes.rs/mod.rs/lib.rs 为共享文件，禁并行写）
- agent 只写代码不 commit；lead 验收后按卡单独 commit（中文 conventional commit，bugfix 必含根因）

## 代码质量要求
- 每卡通过任务卡所列门禁（cargo test 默认 + web features / tsc / vite build）
- 红线：DD §6 R1-R10；既有文件改动仅限 HLD §4 最小清单（含评审门确认的两条例外）；薄转发；token 三红线
- 全程更新 `tasks/progress.md`
- **C5（workflow-push）开工前置：C0 W1 spike 必须 GO**（需用户测试私有仓库 + PAT）
- **C4 注意**：Cargo.lock 入库 + `cargo update -p indexmap --precise 2.9.0`（W3 spike 前置，漏了 1.77 编不过）

## 开发批次
批次 1：C1 git-foundation ∥ C0 W1 spike（lead 亲跑）
批次 2：C2 git-ops-adoption
批次 3：C3 workflow-update → C4 workflow-share → C5 workflow-push
批次 4：C6 skill-registry
批次 5：C7 readonly-mode
批次 6：C8 frontend-workflows → C9 frontend-skills → C10 frontend-readonly
批次 7：C11 e2e-acceptance（末位收口，W4/W5 端到端 verify 在本卡）

## 两层验收
1. 逻辑层：独立 verifier 自写 fixture/断言复跑，不认实现者自证
2. 端到端层：真浏览器全流程 + 真私有仓库推送/贡献实测 + 只读模式实测（远端 hash 独立核实）

判据纪律补充见 `docs/acceptance-standards.md`（6 条增量：确定性判据 /
复刻真实流程造数据 / 禁 fetch 探针 / 承重 verify 排序 / 禁 pkill -f /
推送远端 hash 核实）。

**待用户 greenlight 再开第一卡（C1）。**
