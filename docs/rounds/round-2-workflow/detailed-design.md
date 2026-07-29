# Round 2 · 工作流管理器 v1 详细设计（detailed-design）

> 输入：`proposal.md`、`high-level-design.md`、既有源码（skill_ops / sync_plan / scanner / models）、
> 已上线注册表的真实 `index.json` 与两个 `workflow.yaml`。
> 按模块逐个定义：对外接口、数据结构、依赖、关键逻辑。

## 0. 承重前提（批次 1 spike 证伪对象）

1. `skill_ops::checkout_skills_sh_source(source_url, slug, skill_path)` 能以**目录形式**的 `skillPath`（如 `skills/productivity/grill-me`）解析到含 SKILL.md 的目录——其内建候选（自定义 path / `/<slug>` / `/skills/<slug>` / 仓库根）不覆盖 mattpocock 的 `skills/<category>/<slug>` 结构，必须走自定义 path 分支
2. `serde_yml` 能正确解析注册表两个真实 workflow.yaml（含 untagged 枚举区分 `SkillRef` 与 `placeholder`）
3. git clone --depth 1 注册表仓库 → 根 `index.json` + 子目录 `workflow.yaml` 可读取（沿用 skills.sh 下载同一模式）

## 1. workflow-core（Rust 核心，新文件 `src-tauri/src/workflow.rs`）

### 1.1 数据模型（serde yaml，camelCase 与 skill.lock 一致）

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workflow {
    pub name: String,
    pub slug: String,
    pub version: String,
    pub description: String,
    #[serde(default)] pub author: Option<String>,
    #[serde(default)] pub tags: Vec<String>,
    #[serde(default)] pub icon: Option<String>,
    #[serde(default)] pub groups: Vec<WorkflowGroup>,
    #[serde(default)] pub steps: Vec<WorkflowStep>,
}

pub struct WorkflowGroup { pub id: String, pub name: String }

#[serde(rename_all = "camelCase")]
pub struct WorkflowStep {
    pub name: String,
    pub group: String,                    // group id
    #[serde(default)] pub description: String,
    #[serde(default)] pub skills: Vec<StepSkill>,
}

#[serde(untagged)]
pub enum StepSkill {
    Ref(SkillRef),
    Placeholder { placeholder: String },
}

#[serde(rename_all = "camelCase")]
pub struct SkillRef {
    pub source_type: String,              // v1 仅 "github"
    pub source_url: String,
    pub slug: String,
    #[serde(default)] pub skill_path: Option<String>,  // 目录形式（含 SKILL.md 的目录）
}
```

- **yaml 解析库**：`serde_yml`（serde_yaml 已归档，用维护中 fork；web/tauri 两壳共享，放非 optional 依赖）
- 校验：`validate()`——slug 非空且 `[a-z0-9-]+`、steps[].group 必须存在于 groups、SkillRef.source_type 仅接受 `github`（v1）、source_url 过 `skill_ops::normalize_github_url`（NFR-5）；错误聚合返回

### 1.2 本地已安装存储

- 位置：`data_dir/workflows/<slug>/`（`workflow.yaml` + 可选 `README.md`）
- `list_installed(ctx) -> Vec<InstalledWorkflow>`（读目录、逐个解析、坏文件降级为 error 条目不拖垮列表）
- `load(ctx, slug) -> Workflow`；`save(ctx, &Workflow, readme?) -> ()`（写前 validate；slug 防 `..`）；`delete(ctx, slug)`

### 1.3 测试

- 解析注册表两个真实 yaml 的 fixture 拷贝（含占位/多 skill/缺省字段）
- untagged 枚举边界：`skills: []`、单 placeholder、单 ref
- validate：坏 slug / 引用不存在 group / 非 github 来源

## 2. registry-client（Rust 核心，新文件 `src-tauri/src/workflow_registry.rs`）

### 2.1 接口

```rust
pub struct RemoteWorkflowSummary {
    pub slug: String, pub name: String, pub version: String,
    pub description: String, pub author: Option<String>,
    pub tags: Vec<String>, pub icon: Option<String>, pub path: String,
    pub installed: bool,                      // 与本地 workflows/ 对照得出
}

pub fn fetch_index(ctx: &AppContext, registry_url: &str)
    -> Result<Vec<RemoteWorkflowSummary>, String>;
pub fn fetch_workflow(ctx: &AppContext, registry_url: &str, path: &str)
    -> Result<(Workflow, Option<String> /* README */), String>;
pub fn download_to_installed(ctx: &AppContext, registry_url: &str, path: &str)
    -> Result<String /* slug */, String>;
```

### 2.2 关键逻辑

- **拉取**：`git clone --depth 1 <url> <data_dir>/registry/remote-<ts>`（复用 skill_ops 的 clone 模式；URL 过 `normalize_github_url`），成功后替换 `data_dir/registry/current`（原子换名；失败保留旧缓存）
- `fetch_index`：读 `current/index.json` 解析（结构与我已发布的 index.json 一致），`installed` 由 `data_dir/workflows/<slug>` 是否存在计算
- `fetch_workflow`：读 `current/<path>/workflow.yaml`（+ README.md 可选）
- `download_to_installed`：复制 `current/<path>/` → `data_dir/workflows/<slug>/`（slug 以 workflow.yaml 内为准），D7 注意：这是 data_dir 内部操作，不涉 jail
- **设置项**：`Settings` 增加 `workflow_registry_url: Option<String>`（serde default；缺省 = `https://github.com/Pgooone/oh-my-skills-workflows.git`）。models.rs 的 Settings 为**增量字段**（向后兼容），settings.rs `load_settings` 空值回填缺省

### 2.3 测试

- index.json 解析 fixture；installed 对照逻辑
- 本地 fixture git 仓库（tempdir 手工构造 index + 子目录）走 fetch/download 全流程，**不依赖网络**

## 3. workflow-use（Rust 核心，新文件 `src-tauri/src/workflow_use.rs`）

### 3.1 每步 skill 状态计算

```rust
pub enum StepSkillStatus {
    Ready,              // 中心库已有（library_path/<slug>/SKILL.md 存在）
    Missing,            // 中心库没有（使用时将下载）
    Placeholder(String),
}
pub fn compute_statuses(ctx: &AppContext, wf: &Workflow)
    -> Vec<Vec<(StepSkillView, StepSkillStatus)>>;   // 按步骤对齐，供详情页与预览
```

### 3.2 使用工作流 = 生成 Sync Plan（ADR-0003）

```rust
pub fn preview_use_workflow(
    ctx: &AppContext, slug: &str,
    targets: Vec<AgentTarget>, method: String,          // copy | symlink（沿用既有同步策略）
    output_form: OutputForm,                             // EntryManifest | PackagedSkill
) -> Result<SyncPlan, String>;
```

操作序列（全部纳入 SyncPlan 预览，影响范围先可见）：

1. **download-to-library**（新 op_type，每个 Missing 的 SkillRef 一条）：执行时调 `skill_ops::checkout_skills_sh_source` 克隆解析 → 复制到 `library_path/<slug>/`（已有则跳过）；冲突（library 已有同名但内容不同）→ 走既有 blocked_conflicts 语义
2. **标准同步 ops**：复用 `sync_plan::preview_batch_sync` 的既有产物（library → targets 的 copy/symlink）
3. **output ops**（按 output_form 二选一，写入每个 target skills 根）：
   - `EntryManifest`：`<target>/_workflow-<slug>/` = `workflow.yaml`（原样拷贝，agent 可读）+ `README.md`（生成：分组 → 步骤 → 每步说明与**有序** skill 列表（D5）、各 skill 已独立安装于同级目录的指引）
   - `PackagedSkill`：`<target>/<workflow-slug>/` = `SKILL.md`（frontmatter name/description + 编排正文：分组 → 步骤 → 每步该做什么、按顺序读 `skills/` 下哪个）+ `skills/<skill-slug>/`（从中心库**结构化拷贝**，ADR-0009）
4. 占位步骤：不进任何 op；在 SyncPlan 的 `preconditions` 加一条 warning 条目（「步骤 X 为占位，已跳过」），详情页与预览页醒目展示

**models.rs 增量**：`SyncOperation.op_type` 允许新值 `download-to-library`（serde 上 op 是 struct 非 enum 则只加构造器；apply 分支调 skill_ops 下载执行器）。**sync_plan.rs 的 apply 增加该 op 的执行分支**——这是对既有文件唯一允许的增量改动（新增分支，不改既有分支逻辑）。

### 3.3 测试

- 缺失计算：library 有/无、占位
- preview 操作序列：missing×2 + installed×1 + placeholder×1 的混合 case，断言 op 类型与顺序（downloads 在前）
- 两个输出生成器：tempdir 跑生成，断言目录结构、SKILL.md frontmatter、README 含**有序** skill 列表、结构化拷贝完整性（递归 diff）
- 端到端 fixture：本地 git 仓库当来源（`file://` 或本地路径经 normalize 的 GitHub-only 约束在测试中可注入——允许测试钩子绕过 URL 校验）

## 4. workflows-api（两壳薄转发）

### 4.1 command / endpoint 清单（名字一致，web 前缀 `/api/commands/`）

| command | 参数 | 返回 | 说明 |
| --- | --- | --- | --- |
| list_installed_workflows | — | InstalledWorkflow[] | 含步骤数/占位标记 |
| list_remote_workflows | refresh?: bool | RemoteWorkflowSummary[] | 触发 registry 拉取 |
| get_workflow_detail | slug | WorkflowDetail | steps + 每 skill 状态（§3.1） |
| download_workflow | path | InstalledWorkflow | registry → 已安装 |
| save_workflow | workflow, readme? | slug | 创建/更新本地 |
| delete_workflow | slug | — | 删除本地 |
| preview_use_workflow | slug, targets, method, outputForm | SyncPlan | 复用 SyncView 预览组件 |
| （apply 复用既有 `apply_sync_plan`） | planId | ApplyResult | 零新增 |

- 桌面：commands.rs 追加（cfg tauri-shell）；Web：routes.rs 追加（D8 guard 自动覆盖；路径参数仅 slug（`[a-z0-9-]+` 校验），无文件路径参数，jail 不涉及）
- 全部薄转发（NFR-2）

### 4.2 前端 API（src/lib/api.ts 零结构改动）

仅新增调用点；类型进 `src/types.ts`（Workflow 等镜像 Rust 模型，camelCase）。

## 5. workflows-ui（TS 前端）

### 5.1 结构

- App.tsx：view 状态加 `"workflows"`，顶栏第三个按钮「工作流」（与 发现 Skills / 同步 Skills 平级）
- `src/views/WorkflowsView.tsx`：两区列表（**已安装 / 远程可下载**），搜索过滤，延续 SkillsView 的列表优先视觉
- `src/components/workflow/WorkflowDetailPanel.tsx`：分组 → 步骤 → 每步 skill + 状态徽标（可用/将下载/占位）
- `src/components/workflow/WorkflowEditor.tsx`：创建/编辑（meta 表单 + groups 增删 + steps 增删排序（上移/下移按钮，零新依赖）+ 每步 skills 选择器（从中心库 inventory 选）+ 占位开关）
- `src/components/workflow/UseWorkflowSheet.tsx`：选目标 agent + 范围 + 输出形态二选一 → 生成预览（**复用** `src/views/sync/` 的 PlanDetailPanel）→ 调既有 apply 执行

### 5.2 设置

SettingsSheet「数据」tab 增加「工作流注册表 URL」字段（缺省官方仓库；改后下次 list_remote 生效）。

## 6. 既有文件的增量改动白名单（除此之外不许动）

| 文件 | 改动 |
| --- | --- |
| `models.rs` | Settings 加 `workflow_registry_url`；SyncPlan 相关类型加 download-to-library op 支持 |
| `sync_plan.rs` | apply 增加 download-to-library 执行分支（新增，不改既有分支） |
| `settings.rs` | load_settings 空值回填官方注册表缺省 |
| `commands.rs` / `routes.rs` / `lib.rs` / `web/mod.rs` | 追加薄转发 |
| `App.tsx` / `api.ts` / `types.ts` / `SettingsSheet.tsx` | 视图状态、调用点、类型、注册表设置字段 |

## 7. 批次（与概要设计一致）

```
批次 1：download-spike（证伪 §0 三条）
批次 2：workflow-core ∥ registry-client
批次 3：workflow-use
批次 4：workflows-api ∥ workflows-ui
批次 5：lead 终验（proposal §6）
```
