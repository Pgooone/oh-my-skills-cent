# Round 3 · 工作流 v2 需求文档（proposal）

> 输入：`docs/QA决策文档.md`（§6 阶段三、§8 遗留开放问题）、`原始需求/工作流管理器 PRD`（§9 v2 范围）、
> 已收官的 R2 工作流 v1、ultracode 承重调研（2026-07-29，25 条结论 0 被驳 + 7 缺口）。
> 全部产品决策已拍板（见 `qa-decisions.md` Q1–Q8），本文档只做钉死与边界声明。

## 1. 项目概述

**目标**：让工作流（与 skill）从「自己用」走向「分享出去」——
① 个人实例：一键推送 / 一键贡献 / 私有仓库 token / 版本更新检查 / 导出导入分享包；
② 公共形态：同一程序部署为**只读展示站**（域名访问、浏览下载、访客上传贡献、管理员 PR 审核）；
③ 内容生态：新建 skill 注册表仓库，skill 获得与工作流同级的贡献与分发链路。

**性质**：个人开源项目（fork 演进）。**技术栈**：Rust 核心（两壳共享）+ React 18 + TS。

**受益形态**：桌面壳（Tauri）与 Web 壳同时获得全部功能；公共只读模式仅 Web 壳新增。

## 2. 架构简述

```
┌ 前端 ─────────────────────────────────────────────────────────────┐
│ WorkflowsView：详情面板 +[推送][导出][检查更新]；远程区 +[贡献]；已修改/有更新徽标 │
│ SkillsView：+ skill 注册表远程区（浏览/下载/贡献）                       │
│ SettingsSheet：+ token（密码框）/ githubUsername / skillRegistryUrl     │
│ 导入分享包入口；公共只读模式下隐藏写操作入口                                │
└──────────────┬─────────────────────────────────────────────────────┘
        tauri command（桌面） ║ HTTP endpoint（Web，沿用 D7/D8 + 只读中间件）
┌──────────────┴────────────── 共享 Rust 核心（全部新文件） ───────────┐
│ workflow_share    胖包导出（抓取引用 skill + zip）/ 导入校验安装         │
│ workflow_push     git 写操作统一层：token/身份注入、stderr 捕获脱敏、      │
│                   一键推送（代写 index.json）、贡献（fork 分支 + compare URL）│
│ workflow_update   来源元数据读写、三态更新检查、备份覆盖更新               │
│ skill_registry    skill 注册表客户端（镜像 workflow_registry 模式）+ 贡献   │
│ 公共只读模式      OMS_BIND/OMS_READONLY、写端点熔断、访客上传（限流+校验+PR）│
└──────────────────────────────────────────────────────────────────────┘
复用：hash_dir/copy/remove、normalize_github_url、refresh_cache 缓存机制、
     skill_ops checkout（胖包抓 skill）、.skill-lock.json（注册表 skill 更新检查）、
     「备份→删→重建」更新模板、staging→swap 原子替换约定
```

## 3. 功能需求

| # | 需求 | 依据 |
| --- | --- | --- |
| FR-1 | **一键推送**：把本地工作流推送到 `settings.workflowRegistryUrl` 指向的注册表仓库（官方地址禁止直推 → 引导走 FR-2 贡献）。推送 = clone → 写入 `{slug}/` 子目录 + 代写 index.json（upsert，version 取 workflow.yaml，path=slug）→ commit → push。git 调用约定：`GIT_TERMINAL_PROMPT=0` + `GCM_INTERACTIVE=never`、凭证 `-c http.extraheader` 注入（不进 URL）、身份注入（git config 优先，缺省用 token 用户名 + noreply email）、stderr 捕获且**脱敏** | PRD v2；调研缺口 2/4 |
| FR-2 | **一键贡献（个人实例）**：把选中工作流推送到用户 fork 的新分支（无 fork → 自动打开官方仓库 fork 页并提示完成后重试）→ 自动打开预填标题/正文的 compare URL，用户点 Create。零 token 降级：导出胖包 + 图文引导。fork 存在性用 `git ls-remote` 探测（零 API 依赖） | Q5 |
| FR-3 | **token 管理**：Settings 增 `githubToken`（UI 密码框掩码、API 回传仅「是否已配置」不回传明文、文件 0600）+ `githubUsername`（compare URL 用）；`OMS_GITHUB_TOKEN` 环境变量优先于设置。全链路脱敏（错误消息/日志/front-end） | Q4；调研 c10 |
| FR-4 | **版本更新检查**：下载/导入时写来源元数据（registry URL + path + 下载时内容哈希）到 `data_dir/workflow-sources/<slug>.json`（目录外，hash 零侵入——设计评审门后修订落点）。三态：未修改（哈希一致）→ 可一键更新（先备份）；已修改 → 徽标提示、更新需显式确认且先备份；本地自创（无来源元数据）→ 不参与。检查 = index version 前置比较 + hash_dir 内容确认；更新 = 「备份→删→重建」。列表区批量检查 + 详情单条更新 | Q7；调研缺口 1 |
| FR-5 | **导出胖包 + 导入**：导出 = zip（`workflow.yaml` + `README.md` + `source.json` + `skills/<slug>/` 全部引用 skill 完整拷贝，占位步骤跳过并记录）；任一 skill 拉取失败 → 整体报错列出，不产半成品。导入 = 选 zip → 校验（路径穿越/大小上限/yaml validate/slug 合法）→ 装为本地工作流（来源元数据随包落盘）。Tauri 壳走保存对话框，Web 壳经现有 JSON 通路（base64） | Q1/Q6；调研缺口 7 |
| FR-6 | **公共只读模式**：`oms-web` 增 `OMS_BIND`（缺省 127.0.0.1:8477，取代 OMS_PORT）。**D4 修订**：绑非 localhost 时强制 `OMS_READONLY=1`，否则拒绝启动。**D8 配套修订**：只读模式放行公网 Host，保留 cross-site/Origin 拦截。只读模式：全部本地写端点 403（settings 保存/下载/删除/同步/推送/更新执行/目录浏览），浏览/导出 200；前端隐藏写入口。访客上传端点：接收工作流或 skill 的 **zip 包**（评审门后统一为仅 zip）→ 服务端校验 → 用运营者 bot token 在注册表仓库建分支 + 开 PR（全自动）→ 返回 PR 链接；每 IP 限流 + 大小上限；建议配独立 bot 账号（部署文档） | Q2/Q3 |
| FR-7 | **skill 注册表全链路**：内置默认 skill 注册表 `Pgooone/oh-my-skills-skills`（Settings 可换）。Skills tab 增远程区：浏览/下载（下载写 `.skill-lock.json` 条目 → **现有更新检查原生接管**）。skill 贡献上传：与工作流贡献同链路（推 fork 分支 + compare URL；公共站访客上传同 FR-6） | Q8 |
| FR-8 | **双壳薄转发 + 前端接线**：全部核心函数经 commands.rs + web/routes.rs 薄转发；Web 端写操作沿用 D7 严格 jail 校验参数路径 | NFR-2 |

## 4. 非功能需求

| # | 需求 | 验证 |
| --- | --- | --- |
| NFR-1 | 桌面壳零回归：cargo test 默认 features 全绿 | CI/本地 |
| NFR-2 | 全部逻辑在共享核心新文件；两壳只薄转发；摘桃友好（不改既有核心逻辑，可复用调用） | review |
| NFR-3 | 核心单测：三态判定、index.json 代写 upsert、zip 导入负例（穿越/超限/坏 yaml/坏 slug）、token 脱敏（构造失败场景断言错误串无 token）、只读熔断、限流 | cargo test |
| NFR-4 | token 三红线：不明文回传、不进错误/日志、UI 仅掩码 | 负向测试 |
| NFR-5 | MSRV 1.77.2 约束：zip 选型钉 zip 4.0.x（裁 default-features）或手写 store-only zip | cargo build |
| NFR-6 | 公共只读模式安全：非 localhost + 无 readonly 拒绝启动；只读下写端点全 403 负向测试；访客上传限流触发 | 测试 |
| NFR-7 | 注册表写侧契约（index.json 代写、slug 唯一、path=slug）为**本项目自定义契约**，写入 docs 并同步注册表仓库 README | 文档 |

## 5. 明确不做（本轮非目标）

- 个人实例全自动 API 建 PR（公共站 bot 自动 PR 除外）（Q5）
- OS keyring 存储（Q4）；多注册表并存（调研 open question，维持单注册表）
- 非 GitHub 宿主（GitLab/Gitea/自托管）——维持全项目 GitHub-only 约束
- 访客上传的账号体系/验证码（限流 + 校验 + PR 审核三层即可）
- 瘦包格式（只产胖包一种，Q6）；skill 更新检查对**既有单条 check 机制**的改造（复用即可——但注册表 skill 的**批量检查** `check_registry_skill_updates` 为例外新增：避免逐 skill 整库 clone 的 N 倍开销，调研 c7；评审门后回改本条）
- Docker 镜像（QA Q4 继续有效）；多租户/权限体系（Q2）

## 6. 验收目标

1. **导出/导入闭环**：真浏览器导出胖包 → 换干净 OMS_DATA_DIR 启动 → 导入 → 详情正确 + 来源元数据落盘 → 使用工作流全链路；胖包内 skills/ 与「导出时刻从 sourceUrl 新 clone 的目录」递归 diff 逐字节一致（导出反映 origin 现拉内容，有意语义——评审门钉死）
2. **一键推送**：配置测试注册表仓库 → 推送 → `git ls-remote` 独立核实远端新 commit + clone 校验 index.json/子目录内容正确；token 脱敏负向（故意错误 token，错误串无 token）
3. **一键贡献**：无 fork → fork 页自动打开；推送分支成功 → compare URL 预填正确（owner/repo、username:branch、标题正文）
4. **更新检查三态**：注册表侧改 version → 未修改工作流标「有更新」→ 确认更新 → 备份目录产生 + 内容与注册表一致；本地编辑后 → 标「已修改」不误报有更新（除非远端也变）；本地自创 → 不参与检查
5. **公共只读模式**：`OMS_BIND=0.0.0.0` 无 readonly → 拒绝启动；readonly 下写端点全 403、浏览/导出 200、pageErrors=0；访客上传 → 测试注册表仓库出现分支/PR + 超限额触发拒绝
6. **skill 注册表**：远程区浏览 → 下载 → `.skill-lock.json` 条目正确 → **下载完成立即检查返回 current**（byte-verbatim 断言）→ 前端真实呈现更新检查链路（评审门证伪后修订：Rust check 函数复用 + 前端触发链新增）；skill 贡献 → compare URL 预填正确
7. **门禁**：cargo test（默认 + web）/ tsc / vite build 全绿；真浏览器全流程 pageErrors=0

## 7. 前置条件（用户侧准备）

- 建仓 `Pgooone/oh-my-skills-skills`（含初始 `index.json`：`{"version":1,"skills":[]}` + README），同工作流注册表模式
- 测试用注册表仓库 1 个（推送/贡献验收靶子，可用私有仓）
- （建议）公共站 bot 用独立 GitHub 账号/token，部署文档将注明
