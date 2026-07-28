# Round 1 · Web 壳 概要设计（high-level-design）

> 输入：`proposal.md`。唯一准星：**模块独立**——每个模块能单独理解、单独测试、改内部不破坏别人。

## 模块职责表

| 模块 | 职责 | 技术域 |
| --- | --- | --- |
| `core-context` | 定义 `AppContext { data_dir, home_dir }`，替换业务模块签名中的 `&AppHandle`；桌面/Web 两壳共用同一上下文抽象 | Rust 核心 |
| `web-server` | axum HTTP 服务：14 个既有 command 的 1:1 endpoint 薄转发、安全中间件（D4/D7/D8）、rust-embed 静态服务 | Rust 新增 |
| `frontend-adapter` | 前端统一 api client（invoke/fetch 双实现），`open_url`/`open_path`/确认框的 Web 降级替代 | TS 前端 |
| `dir-browser` | 替代 plugin-dialog 目录选择：后端目录列举 endpoint（jail 内）+ 前端目录选择组件 | Rust + TS |
| `build-packaging` | 单二进制构建链路：前端 build → rust-embed 内嵌 → cargo bin 目标；Windows/Linux 编译验证 | 构建 |
| `deploy-docs` | Linux 部署文档：systemd、SSH 隧道、git 运行时依赖、故障排查 | 文档 |

## 依赖关系与批次

```
批次 1（承重墙 spike）：
  core-context            —— 无依赖。绿了才证明「AppHandle 抽象是机械改动」前提成立

批次 2（契约定死后可并行）：
  web-server              —— 依赖 core-context
  frontend-adapter        —— 依赖 web-server 的 endpoint 契约（详细设计定死，实现可并行）

批次 3：
  dir-browser             —— 依赖 web-server 骨架 + frontend-adapter 的 api client

批次 4：
  build-packaging         —— 依赖 web-server（bin 入口）+ 前端产物完整
  deploy-docs             —— 依赖 build-packaging 的产物形态（可与同批起草）
```

## 关键边界声明

- **核心模块只改签名不改逻辑**：`scanner/registry/sync_plan/fs_ops/settings` 中 `&AppHandle` → `&AppContext` 是唯一允许的改动（NFR-4：文件不移动、不重命名）。
- **web-server 不含业务逻辑**：endpoint 只做参数反序列化 → 调核心函数 → serde 返回（NFR-2）。
- **frontend-adapter 是唯一知道「invoke 还是 fetch」的地方**：UI 组件不感知运行时。
- **安全中间件集中在 web-server 一层**：D4/D7/D8 不分散到各 endpoint 内（单层单责，便于负向测试）。
