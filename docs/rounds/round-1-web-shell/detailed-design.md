# Round 1 · Web 壳 详细设计（detailed-design）

> 输入：`proposal.md`、`high-level-design.md`、源码精读（commands.rs / settings.rs / 前端 invoke 与 dialog 调用点）。
> 按模块逐个定义：对外接口、数据结构、依赖、关键逻辑。

## 0. 承重约束（先读）

**web 二进制必须零 Tauri 链接**：Linux 服务器无 webkit2gtk 等 GUI 系统库，链接 Tauri 即无法编译/运行。因此 Cargo feature 重组（见 `web-server`）与 `core-context`（业务层去 AppHandle）互为前提——**业务模块必须先变成零 tauri 依赖，web bin 才能只链接业务模块**。这就是批次 1 spike 要证伪的承重前提：

1. `&AppHandle → &AppContext` 在 scanner/sync_plan/settings 中是纯机械替换（grep 已证实 tauri 引用仅 `use tauri::AppHandle` ×3 + settings 的 Manager）
2. feature 切分后 `cargo build --no-default-features --features web` 不引入 tauri 依赖树

## 1. core-context（Rust 核心）

**职责**：`AppContext` 定义与业务层签名替换。

**新文件 `src-tauri/src/context.rs`**（纯 Rust，零 tauri 依赖）：

```rust
pub struct AppContext { data_dir: PathBuf, home_dir: PathBuf }

impl AppContext {
    pub fn new(data_dir: PathBuf, home_dir: PathBuf) -> Self;
    pub fn from_env() -> Result<Self, String>;   // Web 壳用
    pub fn data_dir(&self) -> &Path;
    pub fn home_dir(&self) -> &Path;
}
```

- `from_env()`：`OMS_DATA_DIR` 环境变量优先；缺省 = `home_dir/.oh-my-skills-cent`（跨平台一致、好备份；与桌面版数据目录 `app.oh-my-skills.desktop` 天然隔离，想共享时显式指向同一目录）。`home_dir` 复用既有 `fs_ops::home_dir()`。
- **桌面侧适配**放 `lib.rs`（tauri 侧）：`fn app_context(app: &AppHandle) -> Result<AppContext, String>`（`app.path().app_data_dir()` + `home_dir()`）。

**签名替换（只允许签名改动，逻辑不动）**：

| 文件 | 改动 |
| --- | --- |
| `settings.rs` | 全部函数 `&AppHandle` → `&AppContext`；删 `use tauri::{AppHandle, Manager}` |
| `scanner.rs` | `scan` / `write_library_index` / `write_inventory_cache` / `read_inventory_cache` 的 `&AppHandle` → `&AppContext` |
| `sync_plan.rs` | `preview_*` ×6 / `apply_plan` / 内部 `save_plan` 等的 `&AppHandle` → `&AppContext` |
| `commands.rs` | 每个 command 首行 `let ctx = app_context(&app)?;`，调用传 `&ctx` |
| `registry.rs` / `fs_ops.rs` / `models.rs` | **不动**（本就零 tauri 依赖） |

**验证**：`cargo test`（桌面语义）全绿 + `cargo check --no-default-features`（无 tauri 特性时业务模块可独立编译）。

## 2. web-server（Rust 新增）

**职责**：axum HTTP 服务，12 个既有 command 映射 + 1 个新增 `list_dir` + 安全护栏 + 静态服务。

### 2.1 Cargo feature 重组（承重）

```toml
[features]
default = ["tauri-shell"]
tauri-shell = ["dep:tauri", "dep:tauri-plugin-dialog", "dep:tauri-plugin-opener"]
web = ["dep:axum", "dep:tower-http", "dep:rust-embed", "dep:tokio"]

[dependencies]
tauri = { version = "2", optional = true }
tauri-plugin-dialog = { version = "2", optional = true }
tauri-plugin-opener = { version = "2", optional = true }
axum = { version = "0.8", optional = true }
tower-http = { version = "0.6", features = ["fs"], optional = true }
rust-embed = { version = "8", features = ["debug-embed"], optional = true }
tokio = { version = "1", features = ["full"], optional = true }
# serde / serde_json / chrono / sha2 / walkdir 保持非 optional（业务模块需要）

[[bin]]
name = "oh-my-skills"        # 既有桌面二进制（原 src/main.rs）
path = "src/main.rs"
required-features = ["tauri-shell"]

[[bin]]
name = "oms-web"
path = "src/bin/oms-web.rs"
required-features = ["web"]
```

- `lib.rs`：`mod commands` 与 `pub fn run()` 加 `#[cfg(feature = "tauri-shell")]`；业务模块 `pub mod` 无条件编译。
- **桌面构建零变化**（默认 features 不变，CI 不受影响）；web 构建 `--no-default-features --features web`。

### 2.2 文件布局

| 文件 | 内容 |
| --- | --- |
| `src-tauri/src/bin/oms-web.rs` | main：读 env（`OMS_PORT` 默认 **8477**、`OMS_DATA_DIR`）、D4 护栏、构建 `AppContext::from_env()`、启动 axum |
| `src-tauri/src/web/mod.rs` | router 构建、共享 state（`AppContext` + `PathJail`） |
| `src-tauri/src/web/guard.rs` | D8 中间件：Host / Origin 校验 |
| `src-tauri/src/web/jail.rs` | D7 路径白名单 `PathJail` |
| `src-tauri/src/web/routes.rs` | 13 个 endpoint handler（薄转发） |

### 2.3 API 契约

```
POST /api/commands/{command_name}
请求：JSON 对象 = 参数 map（camelCase，与 tauri invoke 参数形态一致）
响应：200 → 返回值 JSON（serde camelCase，与桌面版一致）
      422 → { "error": "<核心函数返回的 String>" }   （业务错误）
      403 → { "error": "<jail/guard 拒绝原因>" }      （安全拒绝）
GET  /api/health → { "ok": true }                    （前端探测用）
```

**endpoint 清单**（前端实际使用，逐一定义请求 struct，不用 Value 透传）：

| endpoint | 参数 | 返回 | 路径参数需 jail |
| --- | --- | --- | --- |
| get_settings | — | Settings | — |
| save_settings | settings | Settings | libraryPath（写时校验） |
| scan_inventory | options? | InventorySnapshot | — |
| read_inventory_cache | — | InventorySnapshot \| null | — |
| read_skill_lock | — | Record<string, SkillLockEntry> | — |
| discover_project_workspaces | basePath | ProjectWorkspaceCandidate[] | ✓（读） |
| preview_batch_sync | sources, targets, replacements? | SyncPlan | —（plan 服务端生成） |
| preview_batch_quick_migration | sources, targets, method | SyncPlan | — |
| apply_sync_plan | planId | ApplyResult | planId 校验 `[A-Za-z0-9_-]+` 防遍历 |
| check_skills_sh_update | slug, entryPath, sourceUrl, skillPath? | SkillUpdateCheck | ✓ entryPath |
| update_skills_sh_skill | 同上 | SkillUpdateCheck | ✓ entryPath（写） |
| remove_skill_entries | paths | RemoveSkillEntriesResult | ✓ paths（写，最危险） |
| list_dir（新增） | path? | { path, parent, entries[] } | ✓（dir-browser 专用规则） |

`open_path` / `open_url` **不做 endpoint**（Web 语义下由前端就地降级，见 frontend-adapter）。

### 2.4 安全护栏

**D4（bind 护栏，main 内）**：bind 地址硬编码 `127.0.0.1`；若未来加 `OMS_HOST`，非 localhost 值直接 `exit(1)` 并打印原因。本轮不提供任何覆盖开关。

**D8（guard 中间件，所有 /api 请求）**：
- `Host` 头（去端口）必须 ∈ `{localhost, 127.0.0.1, [::1]}`，否则 403
- 写请求（POST）若带 `Origin` 头：Origin 的 host 部分必须 == 请求 Host，否则 403
- 带 `Sec-Fetch-Site: cross-site` 的请求一律 403

**D7（PathJail，routes 内对路径参数调用）**：
- 允许根集（启动时计算 + save_settings 后刷新）：`settings.library_path`、各 agent `global_roots`（registry::known_agents 展开）、`settings.project_folders` / `custom_roots` 下各 agent `project_roots`、`data_dir`、`~/.agents`（skill lock / skills.sh 更新目标）
- 校验：`expand_home` → 组件规范化（拒绝含 `..`）→ 必须位于某允许根之下，否则 403
- `list_dir` 专用规则：`home_dir` 子树 + 已注册根 + Windows 盘符顶层一层（便于跨盘选项目目录）

### 2.5 静态服务

```rust
#[derive(RustEmbed)]
#[folder = "../dist"]          // crate root = src-tauri，dist 在仓库根
struct FrontendAssets;
```

- `GET /` → index.html；`GET /assets/*` → 嵌入资源；其余 GET fallback index.html（前端无路由库，单页即可）
- `debug-embed` feature：debug 构建从磁盘读 dist（dev 不用重编译 Rust），release 才真正内嵌

## 3. frontend-adapter（TS 前端）

**职责**：唯一知道「invoke 还是 fetch」的地方；统一管理「有无真实后端」的判断。

### 3.1 新文件

**`src/lib/api.ts`**：

```ts
export async function callApi<T>(command: string, args?: Record<string, unknown>): Promise<T>;
// isTauriRuntime() → invoke<T>(command, args)
// 否则 → fetch POST /api/commands/{command}；非 200 时 throw new Error(body.error)
```

**`src/lib/shell.ts`**（桌面能力替代的统一入口）：

```ts
export function pickDirectory(title: string): Promise<string | null>;
//   Tauri → plugin-dialog open({directory:true})；Web → DirPicker modal（promise 化）
export function openUrl(url: string): void;
//   Tauri → invoke("open_url")；Web → window.open(url, "_blank")
export function revealPath(path: string): void;
//   Tauri → invoke("open_path")；Web → 路径展示 modal（复制友好）
export function askConfirm(message: string, title: string): Promise<boolean>;
//   从 App.tsx:914 迁入（Tauri → dialog confirm；Web → window.confirm）
```

### 3.2 「有无真实后端」判断（关键改造）

现状：App.tsx 有 18+ 处 `if (!isTauriRuntime())` 走 **demoData**——Web 模式下 `isTauriRuntime()` 也是 false，会把真实 Web 后端误判为 demo 模式。

设计：`api.ts` 启动时 `GET /api/health` 探测，导出 `hasRealBackend()`（Tauri → true；Web 且 health 通 → true）。把 App.tsx / SkillsView.tsx 中**表示「没有后端、用演示数据」的 `!isTauriRuntime()` 分支**全部改为 `!hasRealBackend()`；仅表示「需要桌面能力」的分支（dialog/opener）保留 `isTauriRuntime()` 判断（已收进 shell.ts）。

### 3.3 机械替换清单

| 文件 | 替换 |
| --- | --- |
| `App.tsx` | invoke ×13 → `callApi`；open ×3 → `pickDirectory`；confirm → `shell.askConfirm`；demo 分支判断改造 |
| `SkillsView.tsx` | invoke ×2（open_path/open_url）→ `revealPath` / `openUrl` |
| `SettingsSheet.tsx` | open ×1 → `pickDirectory`（删除现有 prompt 降级，统一走 DirPicker） |

**零 UI 分叉**：组件不感知运行时，全部经 api.ts / shell.ts。

### 3.4 vite dev 代理

`vite.config.ts` 加 `server.proxy: { "/api": "http://127.0.0.1:8477" }`（dev 时前端 1420 热更新、后端 8477，仅开发期配置，不进产物）。

## 4. dir-browser（Rust + TS）

**职责**：替代 plugin-dialog 目录选择（4 处调用点）。

- **后端** `POST /api/commands/list_dir`：参数 `{ path?: string }`（缺省 = home_dir）；返回 `{ path, parent: string|null, entries: [{ name, path, isDir }] }`，只列一层、目录在前；jail 用 §2.4 专用规则
- **前端 `src/components/DirPicker.tsx`**：modal——当前路径面包屑、上级按钮、目录列表（仅显示目录，点击下钻）、「选择此目录」「取消」；promise 化封装在 `shell.ts` 的 `pickDirectory`

## 5. build-packaging（构建）

- feature 接线见 §2.1
- `package.json` scripts：
  - `web:dev`：并行起 `cargo run --no-default-features --features web --bin oms-web` + `vite`（配合 dev 代理）
  - `web:build`：`npm run build && cargo build --release --no-default-features --features web --bin oms-web`
- 构建顺序：**先 vite build 后 cargo build**（rust-embed 编译期需要 `dist/` 存在）；CI 本轮不动（桌面矩阵不受影响），Linux 产物本轮靠 WSL2 手动构建验证
- 产物：`src-tauri/target/release/oms-web`（Linux）/ `oms-web.exe`（Windows 本地测试用）

## 6. deploy-docs（文档）

产出 `docs/deployment.md`（跨轮资产，放仓库 docs/ 根部）：构建步骤、`OMS_PORT` / `OMS_DATA_DIR`、systemd unit 示例、SSH 隧道（`ssh -L 8477:127.0.0.1:8477 user@server`）、**git 为运行时依赖**、WSL2 验证步骤、故障排查（端口占用 / git 缺失 / 数据目录权限）。

## 7. 模块依赖与批次（与概要设计一致）

```
批次 1：core-context          （spike：签名替换 + feature 切分 + 双 feature 编译绿）
批次 2：web-server ∥ frontend-adapter（契约 §2.3 已定死，可并行）
批次 3：dir-browser
批次 4：build-packaging ∥ deploy-docs
```
