# 双形态：保留桌面版 + 新增 Web 壳

原项目是 macOS/Windows 的 Tauri 桌面应用；为支持 Linux 服务器部署，决定**保留桌面壳、新增 Web 壳**：Rust 核心模块（models / registry / scanner / sync_plan / fs_ops）两个壳共享，Web 壳是同 crate 新增的 HTTP 服务二进制（axum + rust-embed 单二进制交付），前端同一套 React 代码经 `isTauriRuntime()` 分支走 invoke 或 fetch。服务器语义为**单机平移**（单用户、管理服务器本机的 Agent 目录，无多租户）。v1 不做 Docker（用户砍范围；以后要做只是给二进制套壳）。

## Considered Options

- **Web-only 硬分叉**：删掉 Tauri 壳最简，但失去桌面体验与上游同步可能，被拒绝。
- **仅补 Linux 桌面 GUI**：不满足无图形界面服务器的部署诉求，被拒绝。

## Consequences

- 壳必须保持薄：tauri command 与 HTTP endpoint 都只是核心函数的薄转发（19 个 command，1:1 映射）。
- 业务模块签名中的 `&AppHandle` 抽象为 `AppContext { data_dir, home_dir }`；数据目录由配置（如 `OMS_DATA_DIR`）给定。
- 浏览器无原生文件对话框与「打开文件管理器」语义，Web 版用服务器端目录浏览 API 替代 `plugin-dialog`，`open_url` 改前端 `window.open`，`open_path` 改为显示路径。
- 所有接受路径参数的 HTTP endpoint 必须 jail 在已注册 skills 目录 + 中心库 + 数据目录内（现有删除防护过弱）。
- 运行时依赖系统 `git` CLI（更新检查走 `git clone`），部署文档需注明。
- Linux 的 Agent 检测需补 PATH 方案（skills 目录表为 home 相对路径，天然兼容）。
