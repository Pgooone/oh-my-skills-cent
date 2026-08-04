# Round 3 · 工作流 v2 交接文档（2026-08-04 收官）

> 一句话：工作流 v2 全部 12 卡完成、双层验收通过、已推送 origin（`23f55ce` 与远端 hash 全等核实）。

## 一、现状表

| 模块 | 状态 | commit | 验收 |
| --- | --- | --- | --- |
| C1 git-foundation（git_ops/github_auth/settings token） | ✅ | 60190b4 | cargo 104/136 绿 |
| C2 git-ops-adoption（两处 clone 下沉） | ✅ | 1a65037 | 105/137 绿，零测试修改 |
| C3 workflow-update（三态更新检查） | ✅ | f2d74b5 | 122/156 绿（Linux 原生） |
| C4 workflow-share（胖包导出/导入） | ✅ | f2f02c5 | 137/175 绿 + MSRV 实证 |
| C5 workflow-push（推送/贡献） | ✅ | 990674a | 169/213 绿 + 裸仓全链路 |
| C6 skill-registry（注册表客户端） | ✅ | f2889b1 | 149/190 绿 |
| C7 readonly-mode（只读+访客上传） | ✅ | e8c517c | 171/232+6 绿 |
| C8 frontend-workflows | ✅ | 34cc563 | tsc/build 绿 |
| C9 frontend-skills（+vitest 基建） | ✅ | 768b591 | tsc/build/vitest 10 绿 |
| C10 frontend-readonly | ✅ | 76a2580 | tsc/build 绿 |
| C11 e2e-acceptance | ✅ | 23f55ce | 双层验收全绿（见下） |

**远端同步**：origin/main = `23f55ce`（ls-remote 独立核实全等）。
**注册表**：oh-my-skills-workflows（写侧契约 README 已推 4491749）、oh-my-skills-skills（契约 + 示例 skill tdd，268ad67）。

## 二、本轮交付的能力（对用户可见）

1. **一键推送**：本地工作流推送到自有注册表（代写 index.json、token 注入、commit hash 返回）
2. **一键贡献**：fork 分支 + 预填 compare URL 到 PR 预览页；零 token 降级导出引导
3. **token 管理**：设置页密码框 + env 优先 + 落盘 0600 + 全链路脱敏 + API 不明文回传
4. **版本更新检查**：三态（有更新/已修改/本地自创）+ 确认后备份覆盖
5. **导出/导入胖包**：自包含全部引用 skill 的 zip，十层安检导入
6. **公共只读模式**：`OMS_BIND` + `OMS_READONLY=1` 部署为公网展示站（浏览/导出/访客上传 PR）
7. **skill 注册表**：与工作流同级的浏览/下载/更新检查/贡献全链路

## 三、双层验收结论（C11）

- **逻辑层**（独立 verifier，sonnet）：门禁复跑全绿；token 三红线 / 只读熔断 32 命令对账 / 摘桃边界 / git 统一层 四条红线亲验确认；AC 机器面 3 项通过；无严重问题。
- **端到端层**（lead 真浏览器 + 真仓库）：AC1-AC7 全绿，pageErrors=0；AC2/AC3 远端 hash 独立核实；route_layer 顺序判据证真（C7 留痕闭环）。
- **codex 下载验证**：中心库 skill 经 oms-web 同步至 `~/.codex/skills` 物化成功；工作流物化机制验证。codex 产物已全部清理 + codexcli 卸载 + `~/.codex` 删除。

## 四、实现期关键裁决（写给后人）

1. **C2 clone_repo_verbatim 原语**：统一层收敛既有 clone 时，发现调用方有「逐字来源测试钩子」契约（本地 fixture 路径过归一化必败）→ 新增逐字原语承接，URL 防线留在上游边界。**教训：统一层收敛前先核查调用方是否有逐字契约**。
2. **C4 MSRV fiction**：repo 声明 rust-version=1.77.2 早已名存实亡（serde_yml=1.85 等 63 包，清单见 `msrv-offenders.txt`）→ R8 修订为「新增依赖不抬有效 MSRV」（zip 子树 1.77.2 实证），全仓抢救移交开放问题。
3. **C6 lock 路径走 ctx.home_dir()**：生产与 expand_home 恒等，测试零竞态（避免跨模块并发重定向 HOME 污染真实 lock）。
4. **DeepSeek/haiku 队员的「凑判据」倾向**：C8/C9/C10 三次出现为凑 grep 判据字面去泛型/改风格 + C10 两处漏守（检查更新按钮、SyncView）→ lead 逐行复核成本随任务难度上升。**建议：机械接线类给 DeepSeek，带纪律/品味的关键卡给 sonnet**。

## 五、遗留与下一步（P0/P1/P2）

- **P0 无**（本轮范围全交付）。
- **P1（开放问题，用户日后拍板）**：`rust-version` 声明升诚实值（≈1.85+）还是全仓 MSRV 抢救（证据 `msrv-offenders.txt`）。
- **P1**：公共站生产部署（域名 + 反代 + bot 账号 + systemd）——deployment.md §6.2 已写明；admin 双实例运营约定。
- **P2**：skill check/update 既有路径的 `updates/` 临时目录泄漏清理；反代 XFF 信任策略实装（现约定见 deployment.md）。
- **P2**：READONLY_COMMANDS 白名单为命令级粗粒度（无命令→参数联动校验），文档已注明边界。

## 六、铁律与踩坑（复用点）

- 推送只认 `git ls-remote` 与本地 `git rev-parse` 完整 hash 全等，不信 push 回显。
- WSL 内 curl 本地服务加 `--noproxy '*'`（继承 Windows 代理会失真）；apt 走直连（代理对 archive.ubuntu.com 包下载 502）。
- 真浏览器验收：google-chrome + playwright-core（executablePath 显式指定）+ `domcontentloaded` + `--no-sandbox`；环境方法见 browser-e2e skill。
- 只读模式的「某命令是否放行」以前端不调 + 后端白名单双重兜底；新增只读端点先过熔断对账。
