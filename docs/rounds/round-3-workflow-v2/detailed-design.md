# Round 3 · 工作流 v2 详细设计（评审门订正版）

> 输入：`proposal.md`、`high-level-design.md`、`qa-decisions.md`、ultracode 调研实证、
> **设计评审门 59 条 findings（29 条确认 blocker/major，处置见 design-review.md）+ W3 spike 结论**。
> 纪律：模块独立三连问；红线核对见 §6；复用对照见 §7。
> 修订标记：【门】= 设计评审门确认的缺陷修订（编号对应 design-review.md）。

## 1. M1 git-ops（`git_ops.rs`，新）

git CLI 写操作统一层。**全部 git 调用只准走这里**（含既有两处 clone 的调用点替换，见 §6-R6 例外）。

```rust
pub struct GitIdentity { pub name: String, pub email: String }

/// 基础 Command：GIT_TERMINAL_PROMPT=0 + GCM_INTERACTIVE=never（调研缺口 4①）
pub fn base_command() -> std::process::Command;
/// 凭证注入：`git -c http.extraheader="Authorization: Basic base64(x-access-token:{token})"`。
/// token 不进 URL（normalize 输出恒 https，无 userinfo）。
pub fn with_auth(cmd: &mut Command, token: Option<&str>);
pub fn with_identity(cmd: &mut Command, id: &GitIdentity);   // `-c user.name/email`
/// 执行并捕获：status 非 0 → Err(redact(stderr))；成功 Ok(stdout)。
pub fn run(cmd: &mut Command, token: Option<&str>) -> Result<String, String>;

pub fn clone_repo(url: &str, dest: &Path, token: Option<&str>) -> Result<(), String>; // --depth 1
pub fn ls_remote(url: &str, token: Option<&str>) -> Result<String, String>;
pub fn create_branch(repo: &Path, branch: &str) -> Result<(), String>;
pub fn commit_all(repo: &Path, msg: &str, id: &GitIdentity) -> Result<String, String>;
pub fn push(repo: &Path, remote: &str, refspec: &str, token: Option<&str>) -> Result<(), String>;
pub fn detect_identity(repo: &Path, fallback_user: &str) -> GitIdentity;
```

- **URL 入参防线**【门-token-F6】：clone_repo/ls_remote/push 的 URL 一律先经 `normalize_github_url`（拒绝 userinfo/非 GitHub）；settings 保存时两个 *RegistryUrl 同样做 userinfo 拒绝校验（§2）。
- **依赖**：`base64` crate（W3 实证 0.23 可用）；单测：base64 编码、run 失败 redact、detect_identity fallback、URL userinfo 拒绝。

## 2. M2 github-auth（`github_auth.rs`，新）

```rust
pub fn resolve_token(ctx: &AppContext) -> Option<String>;   // OMS_GITHUB_TOKEN 优先 > settings.github_token
pub fn redact_text(text: &str, token: Option<&str>) -> String; // 纯字符串替换 token 本体与 base64 形态 → "***"
pub fn is_official_repo(url: &str, official: &str) -> bool;    // 双侧 normalize 相等（更新分流复用同款比较，见 §8.5）
pub fn parse_owner_repo(url: &str) -> Result<(String, String), String>;
pub fn fork_clone_url(username: &str, repo: &str) -> String;
pub fn fork_page_url(owner: &str, repo: &str) -> String;
pub fn compare_url(owner: &str, repo: &str, username: &str, branch: &str, title: &str, body: &str) -> String;
// base 分支恒 main——官方注册表契约约束（NFR-7 写入 README）【门-F-10】
```

**Settings 扩展**（models.rs/settings.rs 最小改）：`githubToken: Option<String>`、`githubUsername: Option<String>`、`skillRegistryUrl: Option<String>`，全 `#[serde(default)]`；load_settings 空值回填官方 skill 注册表 URL（GitHub-only，UI 与保存校验均拒绝非 GitHub【门-L4】）。

**token 持久化与出参（双壳统一，全部经核心函数）**：
- `githubToken` **正常序列化落盘**；unix 下保存后对 settings.json `set_permissions(0o600)`（`cfg(unix)`，含已存在文件 chmod 场景；Windows 无对应语义，依赖用户 profile ACL，如实注明）【门-F8/F-05】。
- **出参裁剪规则【门-token-F1 修订】**：**凡返回 Settings 的 API 一律裁剪**（get_settings 与 save_settings 的返回值，两壳同规则）——克隆后置 `github_token = None`、附加 `hasGithubToken: bool`。验收负向：保存 token 后断言 save_settings 响应体无 githubToken 键且 hasGithubToken=true。
- **保存 wire 契约【门-token-F3 钉死】**：入参 `githubToken: Option<String>` = `null/缺省`→保持、`Some(非空)`→替换；另有独立 `clearGithubToken: bool` = true→清除。合并逻辑在核心 `settings::merge_token` 单点实现，双壳薄转发；单测覆盖「改无关设置 token 不动」「显式清除后落盘无 token」。

## 3. M3 workflow-update（`workflow_update.rs`，新）

### 3.1 数据结构

`data_dir/workflow-sources/{slug}.json`（**目录外**，hash_dir 零侵入——本文修订 proposal FR-4 落点，已回改）：

```json
{ "registryUrl": "https://github.com/Pgooone/oh-my-skills-workflows.git",
  "path": "software-development", "contentHash": "<sha256hex>", "installedAt": "2026-07-29T..." }
```

```rust
pub enum WorkflowUpdateState {
    Local, UpToDate,
    UpdateAvailable { remote_version: String },
    Modified { remote_changed: bool, remote_version: Option<String> },
}
pub struct WorkflowUpdateStatus { pub slug: String, pub state: WorkflowUpdateState,
    pub local_version: Option<String>, pub remote_version: Option<String> }
```

### 3.2 接口

```rust
pub fn record_source(ctx, slug, registry_url, path) -> Result<()>;
pub fn read_source(ctx, slug) -> Option<SourceMeta>;
pub fn check_all(ctx) -> Result<Vec<WorkflowUpdateStatus>, String>;
// fetch_index(ctx, registry_url) 一次（内部已含缓存刷新）→ Vec<RemoteWorkflowSummary>
pub fn check_one(ctx, slug, summaries: &[RemoteWorkflowSummary]) -> WorkflowUpdateStatus;
pub fn apply_update(ctx, slug, confirm_modified: bool) -> Result<WorkflowUpdateStatus, String>;
```

### 3.3 关键逻辑

- **写入点**：commands/routes 的既有 `download_workflow` 薄转发成功后加一行 `record_source`（薄转发加一行，核心不动）；M4 导入后同样调用（包内有 source.json 则还原并复核哈希）。
- **判定**（check_one）：无 source → Local；`hash_dir(local) != source.content_hash` → Modified（再比远端填 remote_changed）；否则 summaries 按 slug 找条目：version 不等 → UpdateAvailable；version 等但 `hash_dir(data_dir/registry/current/{path}) != source.content_hash` → UpdateAvailable（调研 c9 兜底；缓存路径是公开约定）；找不到条目 → Local。**语义声明**【门-F-12】：换注册表后旧来源条目归 Local 不再检查，属已知取舍（不按 source.registryUrl 分组多拉）。
- **孤儿清理**：source 存在而 workflows/<slug> 不存在 → 删 source 文件（惰性，免改 workflow.rs::delete）。
- **apply_update**：Modified && !confirm_modified → Err；备份 `data_dir/backups/workflow-updates/{UTC ts}/{slug}` → 复用 `download_to_installed` → record_source 重写。
- **单测**：三态判定矩阵（6 case）、孤儿清理、Modified 未确认拒绝、备份产生、更新后 hash 一致。

## 4. M4 workflow-share（`workflow_share.rs`，新）

### 4.1 导出

```rust
pub fn export_package(ctx, slug) -> Result<(String /*filename*/, Vec<u8>), String>;
```

1. 读 `workflows/<slug>/` + `workflow-sources/<slug>.json`（可选）→ 写入临时根 `data_dir/tmp/export-{ts}/`。
2. 写 `manifest.json`：`{workflowSlug, skills:[...], placeholders:[step 名...], exportedAt}`。
3. 逐 step 的 Ref skill：`normalize_github_url(sourceUrl)` → `checkout_skill_from_clone_source(ctx, slug, clone_url, skillPath)`（pub(crate)，返回解析目录 PathBuf）→ copy 到 `skills/<slug>/`。**任一失败 → 清理临时目录，Err 列出全部失败项，不产半成品**（Q6）。
4. zip（W3 实证：`zip = "~4.0"` 解析 4.0.0，`default-features=false, features=["deflate"]`；**必须配 Cargo.lock 且 `cargo update -p indexmap --precise 2.9.0`**；构建工具链 ≥1.77.2——rust-version 为 patch 级比较，1.77.0 不够；API 用 `ZipWriter`/`SimpleFileOptions`/`ZipArchive`，4.0 形态见 design-review.md W3 节）→ 字节；filename = `{slug}-workflow.zip`。
5. **清理约定【门-F13】**：clone 根 = 返回 PathBuf 向上回溯至 `data_dir/updates/` 的直接子目录，整体 remove_entry（含失败分支）；单测断言导出后 updates/ 无残留。
6. **复用例外声明【门-F-18】**：checkout 内部 clone（skill_ops.rs:221）将被替换为 M1 调用（§6-R6 例外②），自动获得防交互 env 与凭证注入——GCM 弹窗风险随例外②消除。
7. **导出语义声明【门-F-17】**：导出反映 **origin 当前内容**（现拉），非本地中心库快照——有意语义，与 Q6 自包含消费一致；AC1 的「源目录」= 导出时刻的新 clone。

### 4.2 导入

```rust
pub fn import_package(ctx, bytes: &[u8]) -> Result<ImportResult, String>; // {slug, hadSource: bool}
```

校验链（任一不过即 Err，不落半成品）：base64 解码**前**先查字符串长度上限【门-F4】→ 大小 ≤ 50MB → zip 可读 → 逐条目路径安检（拒绝绝对路径、`..`、非 UTF-8）→ 解压合计 ≤ 200MB（防炸弹）→ 必须含 `workflow.yaml` → 解析 + `Workflow::validate` → slug 合法且 `workflows/<slug>` 不存在（存在 → Err「已存在」）→ 落 `workflow.yaml`/`README.md` → 包内 `source.json` 存在则写 `workflow-sources/` 并复核 contentHash（不一致 → 按 Modified 自然呈现，不算错误）。
**包内 `skills/` 不装入中心库**（取舍：有应用者使用时按 sourceUrl 现拉；无应用者手动放 agent skills 目录；记 ADR 候选）。
**单测【门-F-06】**：穿越/绝对路径/非 UTF-8/超 50MB/解压超 200MB/缺 workflow.yaml/坏 yaml/坏 slug/已存在冲突，逐条负例。

### 4.3 两壳投递

统一 command `export_workflow_package(slug) -> {filename, base64}`；Web 前端 Blob 下载（**web 端该 endpoint 配 DefaultBodyLimit::max(96MB)**【门-F4】）；Tauri 前端 plugin-dialog `save()` 选路径后调桌面专用 `save_export_to_path(path, base64)`（不注册 web）。导入：File → base64 → `import_workflow_package`（web 端同样配 96MB body limit）。

## 5. M5 workflow-push（`workflow_push.rs`，新）

### 5.1 注册表写侧契约（NFR-7 入注册表 README）

- 包目录 `{path}/`（path 恒 = slug）；index.json 根 `{"version":1,"workflows":[...]}`（skill 注册表为 `"skills"`）；条目 8 字段 slug/name/version/description/author/tags/icon/path；默认分支约束为 **main**【门-F-10】。
- **工作流条目来源**：workflow.yaml 同名字段。
- **skill 条目字段映射【门-F-04/F7 钉死】**：slug=目录名、name=frontmatter.name、description=frontmatter.description、**version=frontmatter.metadata.version 缺省 `"0.1.0"`**、author/tags/icon=metadata 对应键缺省空（tags 缺省 `[]`、author/icon 缺省省略）。写入 NFR-7 契约与注册表 README。
- `upsert_index_entry(index_path, array_key, entry) -> Result<()>`：缺文件/缺数组则建；按 slug upsert；pretty JSON。

### 5.2 接口

```rust
pub fn push_workflow_to_registry(ctx, slug) -> Result<PushResult, String>; // {commitHash, registryUrl}
/// 贡献三态统一走 Ok 载荷（wire 形态钉死，复审 AC-02）：前端按 status 字段分支，Err 通道只给真错误
pub enum ContributeOutcome { NoToken, NeedFork { fork_page_url: String }, Ready { compare_url: String, branch: String } }
pub fn contribute_workflow(ctx, slug) -> Result<ContributeOutcome, String>;
pub fn contribute_skill(ctx, slug) -> Result<ContributeOutcome, String>;
// M7 复用：clone 官方本仓 → upload/{slug}-{ts} 分支 → push → gh 建 PR
pub(crate) fn contribute_to_official(ctx, kind, staged_dir, slug) -> Result<UploadOutcome, String>;
```

### 5.3 关键逻辑

- **推送**：`is_official_repo(url, OFFICIAL_WORKFLOW_REGISTRY_URL)` → Err 引导贡献；clone（M1，token 可选）→ copy → upsert index → commit（detect_identity）→ `push origin HEAD` → rev-parse 取 hash → 清理。push rejected → Err「远端已更新，请重试」（不自动合并）。
- **贡献**：`resolve_token` 为 None → **零 token 降级【门-F-02】**：返回 `ContributeOutcome::NoToken`（前端按 status 字段分支：导出胖包 + 打开贡献指南，§8.5）；githubUsername 缺 → Err 引导去设置。`ls_remote(fork_clone_url)` 探测（失败/空 → NeedFork）→ clone fork → 分支 `contrib/{slug}`（已存在则 `-{UTCts}`）→ 写入+upsert → commit → push → compare_url（base 恒 main；title=`Add workflow {slug}`，body=checklist 模板）→ Ready。
- **contribute_skill**：内容 = `library_path/<slug>/` 整目录；目标 repo 派生自 settings.skillRegistryUrl；array_key="skills"；字段映射按 §5.1。
- **单测**：upsert 新建/更新/保序；官方地址拒绝；NoToken 分支；NeedFork 分支；本地 bare repo fixture 全链路（clone/commit/push 真 git 零外网）。

## 5.5 M6 skill-registry（`skill_registry.rs`，新）【门-F3/F-03 补章】

### 5.5.1 模型与常量

```rust
pub const OFFICIAL_SKILL_REGISTRY_URL: &str = "https://github.com/Pgooone/oh-my-skills-skills.git";
#[derive(/* serde camelCase */)]
pub struct RemoteSkillSummary { pub slug: String, pub name: String, pub version: String,
    pub description: String, pub author: Option<String>, pub tags: Vec<String>,
    pub icon: Option<String>, pub path: String, pub installed: bool }
```
index.json：`{"version":1,"skills":[RemoteSkillSummary 除 installed 外全字段]}`；installed 由 `library_path/<slug>` 存在性现算（镜像 workflow 模式）。

### 5.5.2 缓存（镜像 workflow_registry 模式，有意取舍）

`data_dir/skill-registry/{current, remote-{ts}, backup-{ts}}`：`git clone --depth 1`（**经 M1 clone_repo，token 可选**——私有注册表读因此获得凭证【门-token-F7】）→ staging → rename 换 current（backup 回滚）→ clone 失败且有旧缓存 → 离线回退。与 workflow_registry 结构相似属**有意镜像**（泛化既有 = 改核心文件，摘桃成本更高；与 R2「收敛复用」先例的边界：那次是同一新模块内部重复，这次是跨模块且既有为私有实现）。路径安检自实现 guard（仅 Normal 段，同 guard_registry_path 规则 10 行）；slug 安检自实现 `[a-z0-9-]+` 同规则拷贝。

### 5.5.3 接口

```rust
pub fn fetch_index(ctx, registry_url) -> Result<Vec<RemoteSkillSummary>, String>;
pub fn read_cached_index(ctx) -> Option<Vec<RemoteSkillSummary>>;
pub fn download_skill(ctx, registry_url, path) -> Result<String, String>;       // 返回 slug
pub fn check_updates(ctx) -> Result<Vec<RegistrySkillUpdate>, String>;          // 批量，一次 clone
pub fn apply_update(ctx, slug) -> Result<(), String>;                            // 备份→删→重建
```

### 5.5.4 下载（含 lock 写入与冲突语义）

1. 查 index 条目 → slug 安检 → **冲突检查【门-L3】**：读 lock，同 slug 既有条目且 `normalize(sourceUrl) != normalize(registry_url)` → Err「与既有安装来源冲突，请先移除」（不做静默换源；换源走「先删后下」两步，前端引导）。
2. copy `current/{path}` → `library_path/<slug>/`——**逐字节直拷，不增删改任何文件**（hash 语义前提，实现约束钉死【门-L6】）。
3. 写 lock 条目（`~/.agents/.skill-lock.json`，读-改-写 SkillLockFile）：`skills[slug] = { source: Some(registry_url 归一化 https 形态)【门-L5】, sourceType: Some("github"), sourceUrl: 同左, skillPath: Some(path), installedAt: now, updatedAt: None }`。
4. **单测**：下载后 lock 字段全对；同 slug 异源拒绝；byte-verbatim（下载后立即 hash_dir 比对缓存目录相等）。

### 5.5.5 批量更新检查与执行

- `check_updates`：读 lock → 筛 `normalize(sourceUrl) == normalize(settings.skillRegistryUrl)` 的条目 → fetch_index 一次 → 逐条目：`hash_dir(library/<slug>)` vs `hash_dir(current/{path})` 不等 → UpdateAvailable{remote_version}。**设计例外声明【门-F-11】**：proposal §5「不做注册表专属改造」指复用既有单条 check 机制；批量 check 为避免「逐 skill 整库 clone N 次」的新增（N 倍开销，调研 c7），proposal 已回改。
- `apply_update`：备份 `data_dir/backups/skill-registry-updates/{UTC ts}/{slug}` → remove + copy → lock.updatedAt 更新。（不改既有 `is_agents_skill_path`——模块内 30 行，摘桃成本低于放宽既有守卫。）
- 前端分流（§8.5）：lock.sourceUrl 归一化 == skillRegistryUrl → 走这两个 command；其余走既有 check_skills_sh_update/update_skills_sh_skill。
- **单测**：批量判定（current/available/本地被改）、备份产生、更新后 hash 一致、lock.updatedAt 刷新。

## 6. 红线核对（逐条）

| # | 红线 | 本设计落实 |
| --- | --- | --- |
| R1 | D4 修订：非 localhost 唯一合法形态 = readonly | oms-web 启动：host ∉ {127.0.0.1, localhost, ::1} 且 `OMS_READONLY != "1"` → 打印原因 exit(1)；localhost 行为不变 |
| R2 | 只读熔断完备性（W2） | **白名单制默认拒绝**：POST /api/commands/{name} 仅放行 §8.2 白名单（已按真实 command 全量对账【门-B1】）；get_settings 只读换 PublicSettings 白名单 struct【门-M5】；list_remote_* 只读模式强制 refresh=false【门-M2】 |
| R3 | token 三红线（Q4） | ① 落盘 0600（unix，Windows 依赖 profile ACL 已注明）；**凡返回 Settings 的 API 一律裁剪**（含 save_settings 返回值）【门-token-F1】；② git/gh 错误经 redact_text；③ UI 一律 password 框；wire 契约三分支（§2）【门-token-F3】 |
| R4 | D7 jail | 新端点无用户路径参数（slug/kind 核心校验）；临时目录全在 data_dir；桌面专用 save_export_to_path 不注册 web；只读模式 list_dir/discover 移出白名单【门-M4】 |
| R5 | D8 guard | **D8 配套修订【门-B2】**：readonly 模式 Host 校验放行（任意 Host），`Sec-Fetch-Site: cross-site` 仍 403、POST `Origin` host 仍须 == Host（公网同源表单自然满足）；非 readonly 维持 localhost 三值白名单。正反组测试覆盖 |
| R6 | 摘桃边界 | 既有核心 8 文件业务逻辑零修改，**例外两条【门修订】**：①两处 clone 调用点（workflow_registry.rs:136、skill_ops.rs:221）替换为 M1::clone_repo（凭证 None 时行为与现状完全一致，换取私有注册表读凭证 + 防交互 env 全覆盖）；②测试 fixture 机械补字段（web/jail.rs:211、registry.rs:1433）。其余改动仅限最小清单 |
| R7 | zip 安全 | 穿越/绝对路径/非 UTF-8 拒绝；50MB 压缩上限 + 200MB 解压上限 + base64 解码前长度预检；导入不落半成品；contribute_upload 复用同一安检链【门-readonly-F11】 |
| R8 | MSRV 1.77.2 | zip `~4.0`（W3 实证 4.0.0）+ base64 0.23；**Cargo.lock 入库 + indexmap pin 2.9.0**；构建工具链 ≥1.77.2（patch 级比较） |
| R9 | 访客滥用面 | contribute_upload：per-IP 滑动窗口 5/h + 20MB + 校验 + PR 人工审核；**export_workflow_package 只读模式并入限流（30/h 宽松桶）**【门-M3】；限流 map 容量上限 + 过期淘汰【门-M6】；bot 独立账号建议；反代 XFF 信任策略见部署文档 |
| R10 | git/gh 调用约定 | git 全部经 M1（GIT_TERMINAL_PROMPT=0/GCM_INTERACTIVE=never/凭证 -c 注入/身份 -c 注入/stderr 捕获脱敏）；**gh 调用同样捕获 stderr 过 redact_text、`gh --version` 先行探测【门-F15】**；例外面记录【门-F16/token-F4】：`-c extraheader` 与 `GH_TOKEN` env 在进程存活期间对同机同用户可见（ps、/proc/environ、Windows PEB）——个人单机与 Q4 明文文件同级可接受；公共站部署文档注明 bot 主机单用户专用 |

## 7. 复用对照（新代码 → 既有函数）

| 新模块 | 复用（file:line 经调研核实 + 评审复核） |
| --- | --- |
| M3/M6 | `fs_ops::hash_dir`（54-88）、`copy_dir_recursive`/`remove_entry`/`ensure_dir` |
| M3 | `workflow_registry::download_to_installed`（pub，70-77）、`fetch_index`/`read_cached_index`（pub，43-56）、`RemoteWorkflowSummary`（pub，21） |
| M4 | `skill_ops::checkout_skill_from_clone_source`（pub(crate)，209-214）+ `normalize_github_url`（pub(crate)，247-266）、`Workflow::validate` |
| M5/M6 | `skill_ops::normalize_github_url`、`scanner::parse_skill_markdown`（pub，scanner.rs:659）；slug/path 安检模块内同规则拷贝（is_valid_slug/guard_registry_path 均私有） |
| M6 | `.skill-lock.json` 模型 `SkillLockFile/SkillLockEntry`（models.rs:185-198）；既有 check 函数 Rust 层可复用，**前端触发链需新增**（见 §8.5，QA Q8 表述已修订【门-B6】） |
| M2 | settings 增量字段三件套模式（models.rs:13-14 + settings.rs:58-66 先例） |

## 8. M7 只读模式 + 双壳清单 + M9 前端锚点

### 8.1 oms-web 启动

`OMS_BIND`（host:port 全形式，缺省 `127.0.0.1:8477`，**取代 OMS_PORT——OMS_PORT 废弃**，文件头注释同步修订【门-F9】）；readonly = `OMS_READONLY=="1"`；R1 护栏；AppState 增 `readonly: bool`；serve 改 `into_make_service_with_connect_info::<SocketAddr>()`【门-B5】；**启动时预热 load_settings**（初始化写发生在启动期而非请求期【门-readonly-F10】）；`/api/health` 响应加 `readonly` 字段（**唯一探测通道**，§8.5 对齐【门-F10/F-14】）。

### 8.2 只读白名单（全量对账订正版【门-B1】）

POST /api/commands/ 下仅放行（其余一律 403，含 scan_inventory——它含写盘）：

```
read_inventory_cache, read_skill_lock, get_settings(→PublicSettings),
list_installed_workflows, list_remote_workflows(强制 refresh=false),
get_workflow_detail, list_remote_skills(强制 refresh=false),
export_workflow_package(限流 30/h), contribute_upload(限流 5/h)
```
**移出**：list_dir、discover_project_workspaces（公网枚举 home/data_dir 暴露面【门-M4】；前端只读模式不调用）；read_skill_content（web 本就无路由且前端不调用，无需处理）。
**PublicSettings 白名单 struct【门-M5】**（与 Settings serde 物理隔离，无 github_token 键，测试断言响应 JSON 无 githubToken）：字段名保留、值置空以保前端类型零改动——`{language, workflowRegistryUrl, skillRegistryUrl, hasGithubToken:false, readonly:true, libraryPath:"", projectFolders:[], customRoots:[], showRawPaths:false}`。

### 8.3 访客上传（contribute_upload）

- **body limit【门-B4】**：该路由与 import_workflow_package 单独挂 `DefaultBodyLimit::max(96MB)`，其余路由维持框架默认 2MB；校验链先查 base64 字符串长度再解码。
- 流程：`ConnectInfo` 取 IP（**提取失败 fail-closed 503，不静默放行**【门-B5】）→ 限流（5/h 滑动窗口，map 容量上限+过期淘汰【门-M6】）→ 20MB 上限 → 解 zip（**复用 §4.2 全部安检**）→ 校验（workflow: 合法 workflow.yaml + validate；skill: 含 SKILL.md + frontmatter，**slug 先过 [a-z0-9-]+ 再进分支名与 gh 参数**）→ staging `data_dir/tmp/upload-{ts}/`（**含失败分支的清理责任**）→ M5 contribute_to_official（bot token = env；未配 → Err「站点未开放贡献」）→ gh CLI `pr create`（`gh --version` 先探测；env GH_TOKEN；stderr 过 redact_text；失败 → 降级返回分支 compare 页 URL + 注明）。
- 集成测试注意：ConnectInfo 走真实 TCP（oneshot 不覆盖）【门-B5】。

### 8.4 新 command/endpoint 清单（M8，两壳同构薄转发）

| command | 参数 → 返回 | 备注 |
| --- | --- | --- |
| export_workflow_package | slug → {filename, base64} | 只读放行（限流）；web 96MB body |
| import_workflow_package | archiveBase64 → {slug} | web 96MB body |
| save_export_to_path | path, base64 → () | **仅桌面注册** |
| push_workflow_to_registry | slug → {commitHash} | |
| contribute_workflow / contribute_skill | slug → {status: noToken/needFork/ready, forkPageUrl?, compareUrl?} | |
| check_workflow_updates | () → [WorkflowUpdateStatus] | |
| update_workflow | slug, confirmModified → status | |
| list_remote_skills | refresh? → [RemoteSkillSummary] | 只读放行（强制 refresh=false） |
| download_skill | path → {slug} | |
| check_registry_skill_updates | () → [{slug, updateAvailable, remoteVersion}] | proposal §5 已回改为例外【门-F-11】 |
| update_registry_skill | slug → () | |
| contribute_upload | kind, archiveBase64 → {prUrl?, branchUrl?} | **web 专用**；只读放行（限流） |

### 8.5 前端锚点（M9）

- **注册表 skill 更新触发链（新增，允许改动面【门-B6】）**：① `skillUtils.skillsShUpdateSource` 增兜底——lock 命中且 `skill.canonicalStatus=="imported"` 时以 `skill.canonicalPath`（中心库路径）作 entryPath，仅当存在非中心库引用的 `.agents/skills` 实目录时沿用旧候选；② `App.tsx` 在 refreshInventory 完成后调用 `refreshSkillsShUpdateChecks(allSkills, locks)`（现为死代码，全 src 无调用点）。
- **更新执行分流**：lock.sourceUrl 经 M2 同款双侧 normalize == skillRegistryUrl → `check_registry_skill_updates`/`update_registry_skill`；其余走既有 command。M6 写 lock 时 sourceUrl 恒写归一化 https 形态【门-L5】。
- `WorkflowDetailPanel.tsx:33-48` 操作行 +[推送][导出][检查更新]；`WorkflowsView` toolbar +[检查全部更新][导入分享包]；InstalledRow 状态徽标（有更新/已修改/本地）；RemoteRow 已安装条目 +[贡献]。
- 对话框：更新确认（Modified 警告）；贡献结果（**按返回体 status 字段三分支：noToken→导出胖包+贡献指南引导**【门-F-02】；needFork→开 fork 页；ready→开 compare URL）；导入结果。
- `SkillsView` 远程区（skill 注册表，镜像 Workflows 远程区）+ 下载/贡献 + registry 来源徽标 + 更新提示（经新触发链）。
- `SettingsSheet` data tab +三字段（token=password 框 + 清除按钮（clearGithubToken）+ 明文存储提示）；两个 *RegistryUrl 输入校验拒绝 userinfo/非 GitHub；jail.rs:211 与 **registry.rs:1433** fixture 同步补字段【门-F13】。
- 只读适配：**启动时经 /api/health 取 readonly（桌面壳恒 false）存独立 state**【门-F10】；readonly 时隐藏全部写入口（新建/编辑/删除/使用/推送/贡献/更新执行/设置保存/导入/目录浏览），不调 scan_inventory/list_dir/discover；WorkflowsView toolbar 改显 [上传贡献]（→ contribute_upload）+ 只读横幅。

## 9. 测试策略

- 单测（cargo）：§1（编码/redact/userinfo 拒绝）、§2（merge 三分支、0600 unix 权限位、裁剪断言）、§3.3（三态矩阵/孤儿/备份）、§4.2（zip 负例组 9 条）、§5.3（upsert/拒绝/分支）、§5.5（lock 字段/冲突/byte-verbatim/批量判定）、§8（只读白名单正反组、PublicSettings 无 githubToken 键、限流第 6 次拒绝与窗口滑动、D8 guard 公网 Host 放行 + cross-site 仍 403）。
- 真 git 链路：本地 bare repo fixture（file://）零外网。
- spike（W1/W2/W4/W5）先行，探针收官即删；W3 已 GO（结论与 API 形态见 design-review.md）。
- 端到端：proposal §6 七条 AC，真浏览器 + 远端 hash 独立核实；AC6 强化「下载完成立即 check 返回 current」【门-L6】。

## 10. 开工前自审（评审门后更新）

**自审一（文档评审）**：评审门 59 条 findings 已逐条处置（design-review.md）；§7 复用面经 lead 亲核 + 评审复核双重确认。

**自审二（流程漏洞）**：有意取舍清单——① 胖包 skills/ 不装中心库；② push rejected 不自动合并；③ 批量 check 例外（避免 N 倍 clone）；④ 换注册表后旧来源归 Local；⑤ 导出反映 origin 现拉非本地快照；⑥ M6 缓存有意镜像不泛化既有；⑦ 访客上传仅 zip 格式（proposal FR-6 已回改）。

**遗留开放问题（不阻塞）**：skill check/update 既有路径 updates/ 泄漏；公共站内容保鲜的 admin 双实例运营约定（部署文档：localhost 非只读实例同 OMS_DATA_DIR 管理刷新，低峰操作）；反代 XFF 信任策略（部署文档：默认直连，显式配置才采信 XFF）；rust-embed debug-embed 路径遍历实证（实现期 static_handler 拒 `..` 段一条【门-readonly-F12】）。
