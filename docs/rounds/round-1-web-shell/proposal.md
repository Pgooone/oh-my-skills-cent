# Round 1 · Web 壳 需求文档（proposal）

> 输入：`docs/QA决策文档.md`（11 题拍板 + 8 条推导决策）、`docs/adr/0001–0009`、`CONTEXT.md`、源码勘察事实。
> 本轮是「执行已定决策」，需求已全部拍板，本文档只做钉死与边界声明，不产生新需求。

## 1. 项目概述

**目标**：让 oh-my-skills-cent 现有的全部能力（发现 / 比较 / 采纳 / 同步 Agent Skills）通过**浏览器**可用，交付为**单二进制**，可部署到 Linux 服务器（仅 localhost），开发端 Windows 全程可开发测试。

**性质**：个人开源项目（fork 自 nextcaicai/oh-my-skills，选择性摘桃策略）。

**技术栈**：Rust（axum + rust-embed + tokio，与 Tauri 2 同生态）/ React 18 + TypeScript + Vite（现有前端复用）。

## 2. 架构简述

双形态（ADR-0006）：**桌面壳（Tauri，保留不动）+ Web 壳（本轮新增）**，共享同一套 Rust 核心模块。

```
┌─ Tauri 壳（不动）────────┐  ┌─ Web 壳（本轮新增）─────────┐
│ tauri command（19 个薄转发）│  │ axum HTTP endpoint（薄转发） │
└──────────┬───────────────┘  └──────────┬─────────────────┘
           │        共享 Rust 核心         │
     models / registry / scanner / sync_plan / fs_ops / settings
                    （AppHandle → AppContext 抽象）
```

- Web 服务：同 crate 新增二进制（不拆 workspace，摘桃友好，ADR-0007）
- 前端：同一套 React 代码，`isTauriRuntime()` 分支走 invoke（桌面）或 fetch（Web）
- 交付：rust-embed 把前端打进单个二进制，`./oh-my-skills-cent` 启动即服务

## 3. 功能需求

| # | 需求 | 依据 |
| --- | --- | --- |
| FR-1 | **AppContext 抽象**：`AppContext { data_dir, home_dir }` 替代业务模块签名中的 `&AppHandle`（仅 4 文件、仅路径解析用途）；数据目录支持环境变量配置 | D2 |
| FR-2 | **HTTP API**：前端实际使用的 14 个 command 1:1 映射为 JSON endpoint（薄转发：参数校验 → 核心函数 → serde 返回） | D2 |
| FR-3 | **前端 fetch 适配层**：`src/lib/` 新增统一 api client，`isTauriRuntime()` 为 true 走 invoke、false 走 fetch；UI 零分叉 | Q2、源码事实 |
| FR-4 | **桌面能力替代**：目录选择 → 服务器端目录浏览 API + 路径输入；`open_url` → `window.open`；`open_path` → 显示路径文本；确认框 → 浏览器/自制 modal | D3 |
| FR-5 | **安全护栏三件套**：① 绑定地址非 localhost 拒绝启动（D4）；② HTTP 层路径白名单 jail（D7）；③ Host 头校验 + 拒绝跨源写请求（D8） | D4 / D7 / D8 |
| FR-6 | **单二进制交付**：rust-embed 内嵌前端产物；监听 `127.0.0.1:<port>`，端口与数据目录可配置 | Q5 |
| FR-7 | **部署文档**：systemd 单元、SSH 隧道访问指南、git 运行时依赖说明、WSL2 验证路径 | Q5、风险表 |

## 4. 非功能需求

| # | 需求 | 验证方式 |
| --- | --- | --- |
| NFR-1 | **桌面壳零回归**：既有 `cargo test` 全绿，Tauri 构建（macos/windows）不受影响 | CI 既有矩阵 |
| NFR-2 | **壳薄**：endpoint / command 只做薄转发，业务逻辑只活在核心模块 | code review |
| NFR-3 | **可测试**：核心抽象与 HTTP 层均有自动化测试；HTTP 层含安全护栏的负向测试（非 localhost Host、跨源、越权路径被拒） | cargo test |
| NFR-4 | **摘桃友好**：不移动、不重命名既有核心模块文件；新增代码放新文件 | ADR-0007 |
| NFR-5 | **Windows 可开发**：`cargo run` 起服务、浏览器全流程可用（symlink 策略除外，用复制策略测试） | 手动验收 |

## 5. 明确不做（本轮非目标）

- 认证 / 多用户 / 公网部署支持（ADR-0008）
- Docker 镜像（Q4 砍除）
- 工作流管理器（第二轮）
- Linux 桌面 GUI 构建
- 前端 UI 改版（视觉与原桌面版保持一致）

## 6. 验收目标

1. Windows 开发机：`cargo run --bin <web>` → 浏览器完成「扫描 inventory → 预览 Sync Plan → 执行同步 → 查看历史」全流程（复制策略）
2. 前端构建产物内嵌后，单二进制启动即完整可用，无外部静态文件依赖
3. 三类门禁全绿：`cargo test`（含新增负向测试）、`tsc`、`vite build`
4. 安全护栏负向验证：非 localhost Host 头被拒、跨源 POST 被拒、越权路径被拒、绑 0.0.0.0 拒绝启动
5. Linux 部署验证（WSL2 或目标服务器）：单二进制运行 + SSH 隧道访问（可在收尾阶段执行）
