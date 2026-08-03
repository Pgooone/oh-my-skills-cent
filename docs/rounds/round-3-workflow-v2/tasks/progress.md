# Round 3 · 工作流 v2 总进度

> 每卡完成 = 实现 agent 交付 + 独立 verifier 复跑通过 + lead 复核 git 实盘 + commit。
> 本轮队员参数：**model = sonnet，effort = max**。**子流程：spec-first（全流程）**。
> 质量门禁（每卡必过）：cargo test 默认 + `--features web` / tsc / vite build（按卡涉及面）；**C9 起前端卡加 `vitest run`**。
> 判据纪律：`docs/acceptance-standards.md`；红线：DD §6 R1-R10。

## 前置（用户侧）

- [x] 建仓 `Pgooone/oh-my-skills-skills`（2026-08-01 lead 代建：index.json `{"version":1,"skills":[]}` + README；ls-remote 核实 31ac376 与本地一致）
- [x] 建测试私有仓库 `Pgooone/oms-r3-scratch`（2026-08-01 lead 代建，含初始 commit 44501d3，ls-remote 核实）
- [ ] fine-grained PAT（仅 scratch 仓 Contents RW）——**C0 spike 与 C5/AC2 的前置**；亦可临时复用 gh 登录 token（gho_，repo scope）

## 批次 1 · 地基 + 承重 spike

- [x] C1 git-foundation（M1 git-ops + M2 github-auth + settings 扩展）（60190b4；cargo 默认 104/web 136/tsc/build 全绿；一处偏差返工闭环：fallback email → noreply 形态）
- [x] C0 W1 token spike（2026-08-03 lead 亲跑 **GO**：判别性探针组（credential.helper 置空后）纯注入 clone/push 通 + ls-remote hash 全等、失败输出无 token 两形态、无凭证快速失败 exit=128；负对照因本环境无 tty 未复现挂起（风险面在桌面交互终端，防护保留）；环境发现：gh auth login 会给 git 配 credential helper，首跑探针被其掩盖——教训：涉及凭证的探针必须先置空 helper 保证判别性）

## 批次 2 · 下沉

- [x] C2 git-ops-adoption（两处既有 clone → M1 clone_repo_verbatim）（开工阻断→lead 裁决方案 2' 扩围；cargo 默认 105/web 137 全绿、零测试修改；Command::new 生产仅剩 git_ops.rs）

## 批次 3 · 功能核心（串行，共享 commands/routes/mod 锁序）

- [x] C3 workflow-update（三态更新检查）（f2d74b5；cargo 默认 122/web 156 全绿（Linux 原生，含 unix 0600 实跑）；**环境切换：本轮后续在主副本 /home/pgoone/oms-wsl/oh-my-skills-cent 施工**）
- [x] C4 workflow-share（胖包导出/导入）（门禁 137/175 全绿；MSRV 停点 → lead 裁决缩回 W3 本意（scratch 实证过），R8 修订「新增依赖不抬有效 MSRV」；两处队员自主优化：快照差集清扫、导出侧 slug 守卫）
- [x] C5 workflow-push（推送/贡献；前置 C0 GO）（门禁 169/213 全绿；裸仓全链路真 git 实证；NFR-7 契约已写入注册表克隆 README，lead 负责推送）

## 批次 4 · skill 注册表

- [x] C6 skill-registry（客户端 + lock + 批量检查 + 更新执行）（门禁 149/190 全绿；lock 路径裁决：ctx.home_dir()（生产与 expand_home 恒等、测试零竞态），lead 批准留痕）

## 批次 5 · 只读模式

- [ ] C7 readonly-mode（D4/D8 修订 + 白名单 + PublicSettings + 访客上传）

## 批次 6 · 前端

- [ ] C8 frontend-workflows
- [ ] C9 frontend-skills（含 W4 单元层红→绿）
- [ ] C10 frontend-readonly

## 批次 7 · 收口

- [ ] C11 e2e-acceptance（双层验收；W4/W5 端到端 verify 在本卡）

## 最终验收（proposal §6，判据纪律见 docs/acceptance-standards.md）

- [ ] AC1 导出/导入闭环（真浏览器 + 逐字节 diff）
- [ ] AC2 一键推送（ls-remote hash 独立核实 + token 脱敏负向）
- [ ] AC3 一键贡献（三态：noToken/needFork/ready）
- [ ] AC4 更新检查三态
- [ ] AC5 只读模式（拒启动/写 403/浏览 200/pageErrors=0/限流触发/访客上传出 PR）
- [ ] AC6 skill 注册表全链路（byte-verbatim + W4 端到端）
- [ ] AC7 三门禁全绿（lead 复跑）
- [ ] 卫生：测试写入还原、服务按端口/PID 杀、探针即删
