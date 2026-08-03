# 卡 C11 · e2e-acceptance（端到端验收收口卡，末位固定）

> 判据纪律：`docs/acceptance-standards.md` 6 条。前置：全部实现卡完成。**承重墙 verify 排序**：W4/W5 的端到端 verify 在本卡（其依赖卡已接线完成）。

## AC（proposal §6 七条，真浏览器 + 实测；▲=verifier 逻辑层机器面，●=lead 端到端/UI 面）

- [ ] **AC1 导出/导入闭环**：真浏览器导出胖包 → 换干净 OMS_DATA_DIR 启动 → 导入 → 详情正确 + 来源元数据落盘 → 使用工作流全链路；▲胖包 skills/ 与导出时刻新 clone 递归 diff 逐字节一致 ●全流程 UI 呈现
- [ ] **AC2 一键推送**：settings 指向 `Pgooone/oms-r3-scratch`（工作流注册表靶子仓）→ 推送 → ▲**`git ls-remote` 独立核实**远端新 commit（完整 hash 全等）+ clone 校验 index/子目录逐字段；▲故意错误 token → 错误串无 token 两形态 ●推送结果 UI 呈现
- [ ] **AC3 一键贡献**：无 fork → fork 页自动打开；有 fork → 推送分支成功 + ▲**fork 侧 `git ls-remote` 核实 contrib/{slug} 分支真实存在**（判据纪律第 6 条，与 AC2 同形态）+ compare URL 预填正确（owner/repo、username:branch、标题正文）；noToken 形态走导出+引导（靶子：fork 自 oms-r3-scratch 或官方工作流注册表，不真建 PR）
- [ ] **AC4 更新检查三态**（靶子仓 = oms-r3-scratch，settings 指向它期间造数据）：注册表侧改 version → 未修改工作流标「有更新」→ 确认更新 → ▲备份产生 + 内容与注册表一致；本地编辑后标「已修改」不误报；本地自创不参与 ●徽标与确认对话框 UI
- [ ] **AC5 只读模式**：▲`OMS_BIND=0.0.0.0` 无 readonly → 拒启动；readonly 下写端点全 403（verifier 逐端点负向脚本）、浏览/导出 200、真实 TCP 验证 ConnectInfo 限流触发；访客上传 → ▲测试仓出现分支/PR ●只读 UI（写按钮不渲染/横幅）+ **pageErrors=0**。**补验（C7 留痕）**：route_layer 顺序确定性判据——readonly 下构造「非白名单读命令 + `Sec-Fetch-Site: cross-site`」请求，须命中 guard 的 cross-site 403（而非白名单文案），证明 guard 真在最外层（oneshot 反组未带 cross-site 头，本判据只此一处可区分层顺序）
- [ ] **AC6 skill 注册表**（靶子仓 = `Pgooone/oh-my-skills-skills`，真实消费不污染——只浏览/下载；贡献仅验 URL 不真推分支）：远程区浏览 → 下载 → ▲lock 条目正确 + **下载完成立即 check 返回 current**（byte-verbatim）→ ●前端真实呈现更新链路（W4 端到端：触发链真实发起、徽标出现）；skill 贡献 → compare URL 正确
- [ ] **AC7 门禁**：▲cargo test 默认 + web / tsc / vite build / **vitest run** 全绿（lead 复跑）；●真浏览器全流程 pageErrors=0

## 双层验收分工

- 逻辑层（verifier）：全部 ▲ 项——自写 fixture 干净态复跑全部门禁 + 红线审计（token 三红线/只读熔断/摘桃清单）
- 端到端层（lead）：全部 ● 项——真浏览器亲跑亲看（造数据复刻真实流程，禁预置期望终态）

## 卫生

- [ ] 测试写入全部还原（中心库/目标目录/workflows/lock/oms-r3-scratch 垃圾 commit 与分支清理）
- [ ] 服务按端口/PID 杀（禁 pkill -f 宽匹配），8477/8478 释放
- [ ] spike 探针与临时目录全部删除
