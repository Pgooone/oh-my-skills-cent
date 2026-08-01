# Round 3 · 工作流 v2 概要设计

> 输入：`proposal.md`、`qa-decisions.md`（Q1–Q8）、ultracode 调研（25 条实证 + 7 缺口）。
> 设计红线：摘桃友好（新逻辑全部新文件，既有文件只列「最小修改清单」）、两壳薄转发、复用优先。

## 1. 模块职责表

| # | 模块 | 文件 | 职责（一句话） |
| --- | --- | --- | --- |
| M1 | **git-ops** | `git_ops.rs`（新） | git CLI 写操作统一层：带凭证/身份的 Command 构造、stderr 捕获脱敏、clone/push/ls_remote/commit |
| M2 | **github-auth** | `github_auth.rs`（新） | token 解析（env 优先 > settings）、脱敏器、fork/compare URL 生成、官方仓库判定 |
| M3 | **workflow-update** | `workflow_update.rs`（新） | 来源元数据（source.json）读写、三态更新检查、确认后备份覆盖更新 |
| M4 | **workflow-share** | `workflow_share.rs`（新） | 胖包导出（抓引用 skill + zip）与导入校验安装 |
| M5 | **workflow-push** | `workflow_push.rs`（新） | 注册表写侧契约（子目录 + index.json 代写 upsert）、一键推送、一键贡献（fork 分支 + compare URL） |
| M6 | **skill-registry** | `skill_registry.rs`（新） | skill 注册表客户端（缓存/浏览/下载写 lock 条目/更新）+ skill 贡献 |
| M7 | **只读模式** | `web/` + `bin/oms-web.rs`（既有，最小改） | OMS_BIND/OMS_READONLY、D4 修订、写端点熔断、访客上传（限流+bot PR） |
| M8 | **双壳接线** | `commands.rs`/`lib.rs`/`web/routes.rs`/`web/mod.rs`（既有，薄转发） | 新核心函数的 command/endpoint 注册与 jail |
| M9 | **前端** | `views/`、`components/`、`lib/`（既有改） | 按钮/徽标/对话框/设置项/只读适配/导入导出交互 |

依赖序：M1+M2 是全部写操作的地基 → M3/M4/M5/M6 独立并行 → M7 依赖 M5（贡献链路）→ M8 随各模块 → M9 最后。

## 2. 关键数据流

**一键推送（FR-1）**：详情面板 → push_workflow_to_registry(slug) → M5 读 settings.registry_url（官方地址→拒绝引导贡献）→ M1 clone（凭证可选）→ 写 `{slug}/` + index.json upsert → commit（身份注入）→ push → 返回 commit hash。

**一键贡献（FR-2，工作流/skill 同链路）**：contribute_{workflow,skill}(slug) → M2 由官方 URL + settings.githubUsername 派生 fork URL → M1 `git ls-remote` 探测 fork（无 → 返回 NeedFork{fork_page_url}，前端开浏览器）→ M1 clone fork → 新分支 `contrib/{slug}`（冲突加时间戳）→ 写入 + index upsert → push → M2 生成 compare URL（预填 title/body）→ 前端 window.open。

**更新检查（FR-4）**：check_workflow_updates() → refresh_cache（复用既有，一次）→ 逐已安装：无 source.json → `local`；hash_dir(本地)≠source.content_hash → `modified`；同则 index version ≠ 本地 version 或 hash_dir(缓存子目录)≠source.content_hash → `update-available`，否则 `up-to-date`。更新执行：modified 须 confirm=true → 备份 `data_dir/backups/workflow-updates/{ts}/{slug}` → 复用 download_to_installed 重建 → 重写 source.json。

**胖包导出（FR-5）**：export_workflow_package(slug) → 临时目录组装 workflow.yaml + README + source.json + `skills/<slug>/`（逐 Ref 步骤复用 skill_ops checkout 抓取；占位跳过记入 manifest.json；任一失败整体报错）→ zip → 字节返回（Tauri 存盘对话框 / Web base64 下载）。导入：校验（大小/路径穿越/必须含 workflow.yaml/yaml validate/slug 合法/已存在冲突）→ 仅装 workflow 定义与 source.json（包内 skills/ 供无应用消费方手动使用，不落入中心库——取舍见 DD §3.4）。

**只读模式（FR-6）**：`oms-web` 启动读 `OMS_BIND`（缺省 127.0.0.1:8477）+ `OMS_READONLY`；**D4 修订**：host 非 localhost 且 READONLY≠1 → 拒绝启动。只读中间件白名单：仅放行读端点 + `export_workflow_package` + `contribute_upload`，其余 POST 一律 403；`get_settings` 只读模式返回裁剪版（隐去路径类字段）。访客上传：限流（per-IP 内存滑动窗口）→ 校验 → M5 贡献链路（bot token = env，目标=官方注册表本仓分支）→ **gh CLI（GH_TOKEN 环境变量传 token）建 PR** → 返回 PR URL。

**skill 注册表（FR-7）**：镜像 workflow_registry 模式（`data_dir/skill-registry/{staging,current,backup}`）。下载 = copy → `library_path/<slug>` + 写 `.skill-lock.json` 条目（sourceUrl=skill 注册表 URL、skillPath=path）→ **现有 check_skills_sh_update 原生可查**；更新执行复用「备份→删→重建」模板在模块内自实现（既有 update_skills_sh_skill 的 `.agents` 路径守卫不覆盖中心库，见 DD §3.6 裁决）。

## 3. 承重墙（spike→wire→verify）

| 墙 | 命门 | spike 内容 |
| --- | --- | --- |
| W1 | **token 注入与零泄漏** | 对测试私有仓库：`-c http.extraheader` push 通；故意错 token → 失败错误串全程无 token（stderr/错误消息/ps 与 env 参数面评估） |
| W2 | **只读熔断完备性** | 端点全量清单 × 只读白名单 对账（评审门已产出真实端点清单，实现后负向测试逐端点 403 + 真实 TCP 验证 ConnectInfo） |
| W3 | **zip 4.0.x MSRV 兼容** | ✅ **已 GO（评审门实证）**：解析 4.0.0，rust 1.77.2 工具链 build+run 全过；**前置条件 = Cargo.lock 入库 + `cargo update -p indexmap --precise 2.9.0`**（1.77 resolver 无 MSRV 感知）；构建工具链必须 ≥1.77.2（patch 级比较，1.77.0 被拒） |
| W4 | **lock 条目接管更新检查** | ⚠️ **评审门证伪后修订**：Rust 函数层成立，但前端触发链不存在（候选逻辑排除中心库 + 触发函数是死代码）——spike 断言改为端到端：「手写 lock + 中心库目录 → 前端真实发起 check 并呈现徽标」 |
| W5 | **index.json 代写契约** | 对测试仓库全流程：upsert 新条目/更新既有条目 → clone 回来逐字段校验 |

## 4. 既有文件最小修改清单（摘桃边界）

| 文件 | 修改 | 理由 |
| --- | --- | --- |
| `models.rs` | Settings +3 字段（githubToken/githubUsername/skillRegistryUrl，均 `#[serde(default)]`） | Q4/Q8；增量字段模式同 R2 |
| `settings.rs` | 默认值 + 空值回填 + token 合并三分支（merge_token）+ unix 0600 | Q4【评审门修订】 |
| `Cargo.toml` | +zip（`~4.0`，裁 features）+base64；**Cargo.lock 入库 + indexmap pin 2.9.0** | FR-5/M1【W3 spike 结论】 |
| `bin/oms-web.rs` | OMS_BIND（取代 OMS_PORT）/OMS_READONLY + D4 修订护栏 + connect_info serve + 启动预热 load_settings | FR-6【评审门修订】 |
| `web/mod.rs`、`web/routes.rs`、`web/guard.rs` | 只读白名单中间件、新 endpoint、PublicSettings、D8 配套修订（readonly 放行 Host）、body limit | FR-6/FR-8【评审门修订：guard.rs 入列】 |
| `commands.rs`、`lib.rs` | 新 command 薄转发 + 注册 | FR-8 |
| 前端 views/components/lib + **`skillUtils.ts`/`App.tsx`** | 见 M9；skillUtils/App.tsx 为注册表 skill 更新触发链的允许改动面（评审门 B6 证伪后新增） | FR-8 |
| **例外（评审门确认的最小侵入）**：① `workflow_registry.rs:136` 与 `skill_ops.rs:221` 两处 clone 调用点替换为 M1::clone_repo（凭证 None 时行为全同现状，换取私有注册表读凭证 + 防交互 env 全覆盖）；② `registry.rs:1433`、`web/jail.rs:211` 测试 fixture 机械补字段 | — | 私有注册表读（Q4 本意）/ 编译通过 |
| **不改**（业务逻辑）：workflow.rs / workflow_use.rs / sync_plan.rs / scanner.rs / fs_ops.rs / registry.rs（除 fixture） | — | 复用调用即可 |

> M6 的 `is_agents_skill_path` 维持不改（模块内自实现更新执行）——评审门确认该裁决不变。
