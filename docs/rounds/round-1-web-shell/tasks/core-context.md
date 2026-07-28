# 任务卡：core-context（批次 1 · 承重墙 spike）

> 设计依据：`../detailed-design.md` §0、§1。**本卡即承重墙 spike**：绿了才证明「AppHandle 抽象是纯机械改动 + feature 可切分」前提成立。

- [ ] 新建 `src-tauri/src/context.rs`：`AppContext { data_dir, home_dir }` + `new` / `from_env`（`OMS_DATA_DIR`，缺省 `home/.oh-my-skills-cent`）/ 访问器，零 tauri 依赖
- [ ] `Cargo.toml`：tauri / tauri-plugin-dialog / tauri-plugin-opener 改 `optional = true`；新增 features：`default = ["tauri-shell"]`、`tauri-shell = [...]`；既有桌面二进制登记 `[[bin]] required-features = ["tauri-shell"]`
- [ ] `lib.rs`：`mod commands` 与 `pub fn run()` 加 `#[cfg(feature = "tauri-shell")]`；新增 `app_context(&AppHandle)` 适配（cfg tauri-shell）；业务模块 `pub mod` 无条件导出
- [ ] `settings.rs`：全部函数 `&AppHandle` → `&AppContext`，删 `use tauri::...`
- [ ] `scanner.rs` / `sync_plan.rs`：签名 `&AppHandle` → `&AppContext`（逻辑零改动）
- [ ] `commands.rs`：每个 command 首行 `let ctx = app_context(&app)?;`
- [ ] `context.rs` 单测：from_env 环境变量优先 / 缺省路径
- [ ] 门禁：`cargo test`（默认 features）全绿
- [ ] 门禁：`cargo check --no-default-features` 绿（业务模块无 tauri 可独立编译）
- [ ] 门禁：`cargo tree --no-default-features` 不含 tauri

**红线**：不移动/重命名既有文件（NFR-4）；不改业务逻辑（只动签名）；不 git commit（lead 验收后统一提交）。
