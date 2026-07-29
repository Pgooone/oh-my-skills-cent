# Round 2 · 工作流管理器 v1 概要设计（high-level-design）

> 输入：`proposal.md`。唯一准星：模块独立。

## 模块职责表

| 模块 | 职责 | 技术域 |
| --- | --- | --- |
| `workflow-core` | 工作流领域模型（Workflow/Group/Step/SkillRef/Placeholder）+ workflow.yaml 解析与序列化 + 本地已安装工作流的存取（数据目录 `workflows/`） | Rust 核心（新文件） |
| `registry-client` | 注册表远程拉取：git clone --depth 1 → 读根 index.json → 按 path 读各 workflow.yaml/README；本地缓存（数据目录 `registry/`） | Rust 核心（新文件） |
| `workflow-use` | 「使用工作流」编排：缺失计算（对照 scanner 盘点）→ 缺失下载（复用 skill_ops）→ 采纳进中心库 → 复用 sync_plan 生成/执行 → 双输出形态生成器（入口清单 / 打包 skill） | Rust 核心（新文件） |
| `workflows-api` | 两壳薄转发：tauri commands + web endpoints（沿用 D4/D7-R1/D8） | Rust 壳层 |
| `workflows-ui` | Workflows tab 前端：列表两区、详情、创建/编排编辑器、使用向导 | TS 前端（新视图） |

## 依赖关系与批次

```
批次 1（承重墙 spike）：
  download-spike —— 证伪「source 引用 → 本地 skill 目录」下载解析链路
  （真实 clone mattpocock/skills + skillPath 目录形式 + yaml 解析探针；探针收官即删/转为正式测试）

批次 2（并行，纯 Rust 新模块互不依赖）：
  workflow-core  ∥  registry-client

批次 3：
  workflow-use —— 依赖 core（模型）+ 既有 skill_ops / sync_plan / scanner

批次 4（契约定死后并行）：
  workflows-api  —— 依赖 2/3 的全部后端产物
  workflows-ui   —— 依赖批次 4 的 API 契约（详细设计定死，实现可并行）

批次 5：终验（lead 亲跑，见 proposal §6）
```

## 关键边界声明

- **workflow-core/registry-client/workflow-use 全部是零 tauri 新文件**（`src-tauri/src/workflow*.rs`），不动既有核心模块逻辑（NFR-4）；对既有模块只调用不修改
- **下载只复用** `skill_ops::checkout_skills_sh_source`（git clone --depth 1 + 路径解析 + SKILL.md 校验），不重造轮子；来源 URL 沿用 GitHub-only 约束（NFR-5）
- **Sync Plan 是唯一写入通道**（ADR-0003）：workflow-use 产生的最终磁盘写入（同步到 agent）一律走 sync_plan 预览/执行；输出生成器写入 agent skills 目录视为 Sync Plan 操作的一部分纳入预览
- **UI 不感知运行时**：继续只经 `src/lib/api.ts` / `shell.ts` 访问后端
