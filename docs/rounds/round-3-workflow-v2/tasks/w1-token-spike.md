# 卡 C0 · W1 token spike（承重墙，lead 亲跑，非队员卡）

> 目的：`-c http.extraheader` 推送机制 go/no-go + 泄漏面评估。**C5（workflow-push）开工前必须 GO。**
> 前置（用户准备）：测试私有仓库 `Pgooone/oms-r3-scratch` + fine-grained PAT（仅该仓 Contents RW）。

## 探针（纯 shell，不碰项目代码；探针目录用完即删）

1. **凭证注入有效**：`git -c http.extraheader="Authorization: Basic $(printf 'x-access-token:%s' "$TOKEN" | base64 -w0)" clone https://github.com/Pgooone/oms-r3-scratch.git <tmpdir>` → 成功
2. **推送有效 + 远端核实**：写入 → commit → push origin HEAD → 成功；`git ls-remote origin` 与本地 `git rev-parse HEAD` 完整 hash 全等
3. **错误脱敏**：换错误 token 重跑 clone，捕获全部 stderr/stdout → 明文 token 与其 base64 形态均不出现
4. **防交互**：`GIT_TERMINAL_PROMPT=0 GCM_INTERACTIVE=never timeout 20 git clone <私有仓库>`（无凭证）→ **退出码非 124**（124=挂起被杀=NO-GO）；立即失败，不弹 GCM
5. **进程面记录**：push 存活期间命令行含 base64（ps 可见）——已知接受面，结论记入 DD R10

## AC（确定性判据）

- [ ] 1/2 成功且 ls-remote hash 全等
- [ ] 3 的捕获输出 grep 不到 token 两种形态（真失败与真通过取值不同）
- [ ] 4 退出码非 124 且快速返回

**GO/NO-GO 阈值**：4 条 AC 全勾 = **GO**；任一不过 = **NO-GO** → 备选路径 = 评估 GIT_ASKPASS/credential helper 注入方案重 spike（lead 牵头，结论记 ADR，C5 顺延）。
- [ ] 结论与参数面记录回写 `progress.md`；临时目录已删
