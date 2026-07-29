# Round 2 · 工作流管理器 v1 需求文档（proposal）

> 输入：`docs/QA决策文档.md`、`docs/adr/0001–0009`、`原始需求/Oh My Skills · 工作流管理器 PRD`、
> 已上线的注册表 https://github.com/Pgooone/oh-my-skills-workflows、第一轮交付的双壳架构。
> 所有产品决策均已拍板（ADR-0001~0005 复核维持 + 0009），本文档只做钉死与边界声明。

## 1. 项目概述

**目标**：让用户不仅下载单个 skill，还能下载并使用由多个 skill **编排**好的工作流（如「软件开发工作流」= 需求 → 记录 → 编程）；并让 **AI agent 能直接读取工作流、自动下载对应 skill** 来执行整条流程。

**性质**：个人开源项目（fork 演进）。**技术栈**：Rust 核心（两壳共享）+ React 18 + TS（同一前端）。

**受益形态**：桌面版（Tauri command）与 Web 版（HTTP endpoint）同时获得——工作流逻辑全部落在共享 Rust 核心，这是第一轮双壳架构的直接红利。

## 2. 架构简述

```
┌ Workflows tab（第三主视图，src/views/WorkflowsView） ─────────────┐
│  已安装 │ 远程可下载 │ 详情（分组→步骤→skill/缺失标记）│ 创建/编排 │ 使用向导 │
└──────────────┬───────────────────────────────────────────────────┘
        tauri command（桌面） ║ HTTP endpoint（Web，沿用 D4/D7/D8 护栏）
┌──────────────┴─────────────── 共享 Rust 核心 ─────────────────────┐
│ workflow-core   模型 + workflow.yaml 解析 + 本地已安装存储          │
│ registry-client 注册表索引/包拉取（git clone --depth 1，同 skills.sh 模式）│
│ workflow-use    使用工作流：缺失计算 → 下载 → 采纳 → Sync Plan → 输出生成 │
└────────────────────────────────────────────────────────────────────┘
复用：skill_ops.checkout_skills_sh_source（下载）、sync_plan（预览/执行）、scanner（缺失判定）
```

## 3. 功能需求

| # | 需求 | 依据 |
| --- | --- | --- |
| FR-1 | **Workflows tab**：与 Skills / Sync 平级的第三主视图；分「已安装 / 远程可下载」两区；详情展示 分组 → 步骤 → 每步 skill 与缺失标记；占位步骤醒目标记 | PRD 5.1、8 |
| FR-2 | **创建/编排工作流（本地）**：填写 name/slug/version/description/author/tags/icon；按阶段分组组织步骤；每步可含有序多个 skill（从中心库/已有来源选择）或留占位；保存到本地 | PRD 5.2 |
| FR-3 | **使用工作流**：选目标 agent + 范围（global/project）→ Sync Plan 预览（引用的全部 skill 采纳并同步，**缺失的一并下载**，影响范围先可见）→ 确认执行 | PRD 5.3、ADR-0003 |
| FR-4 | **双输出形态（使用时二选一）**：① 入口清单：agent skills 目录生成工作流入口清单 + README，各 skill 独立安装；② 打包 skill：单一自包含 skill 目录（SKILL.md 编排说明 + skills/ 子目录结构化拷贝） | PRD 5.4、ADR-0004/0009 |
| FR-5 | **从官方注册表浏览下载**：内置 `Pgooone/oh-my-skills-workflows` 为默认注册表（根 index.json + 各工作流子目录）；设置中可切换自建/团队仓库 | PRD 5.5、ADR-0002、Q9 |
| FR-6 | **一步多 skill 有序**：steps[].skills 数组有序，数组顺序即加载/使用顺序，显式写入入口清单 README 与打包 SKILL.md | D5 |

## 4. 非功能需求

| # | 需求 | 验证 |
| --- | --- | --- |
| NFR-1 | 桌面壳零回归：cargo test 默认 features 全绿 | CI/本地 |
| NFR-2 | 工作流逻辑全部在共享核心；tauri command 与 web endpoint 只做薄转发 | review |
| NFR-3 | 核心模块单测：yaml 解析（含占位/多 skill/缺字段容错）、缺失计算、输出生成器 | cargo test |
| NFR-4 | 摘桃友好：工作流代码全部放新文件，不改既有核心模块逻辑（可复用调用） | ADR-0007 |
| NFR-5 | Web 端安全：工作流端点沿用 D7 jail 分层 + D8 guard；注册表 URL 沿用 GitHub-only 约束 | 负向测试 |

## 5. 明确不做（v1 非目标）

- 一键推送 / fork+PR 贡献引导、私有仓库 token、工作流版本/哈希更新检查、导出分享包（全部 v2）
- 工作流内可执行控制流（条件/分支/参数传递）（ADR-0001，永久非目标）
- 应用内直推官方仓库（贡献走 fork + PR）

## 6. 验收目标

1. 真浏览器全流程：Workflows tab 浏览注册表 → 下载「软件开发工作流」→ 详情可见分组/步骤/缺失标记 → 使用工作流（选目标 → Sync Plan 预览含缺失下载 → 执行）→ 目标 agent 目录出现入口清单 + 各 skill
2. 打包 skill 形态：生成单一目录，SKILL.md 编排说明 + skills/ 子目录完整拷贝；**用真实 Claude Code 会话验证 agent 能读懂并按指引使用**（端到端层）
3. 占位步骤：详情页醒目标记，使用时明确提示
4. 创建/编排：本地新建工作流（含一步多 skill + 占位）→ 保存 → 出现在「已安装」→ 可被使用
5. 三类门禁全绿：cargo test（默认 + web）/ tsc / vite build
