# 开发环境依赖清单（dev-environment）

> 目的：换一台机器（尤其是云端 Linux 开发机）时，按本文 10 分钟重建环境继续项目。
> 本清单记录的是 oh-my-skills-cent 第一轮（Web 壳）开工时**实际安装并验证过**的依赖（2026-07-28）。

## 依赖总表

| 依赖 | 验证版本 | 用途 | 何时必需 |
| --- | --- | --- | --- |
| Node.js | v24（CI 用 20） | 前端构建（tsc / vite） | 总是 |
| Rust（rustup stable） | 1.97.1 | 后端编译与测试 | 总是 |
| C++ 构建工具 | Win: VS Build Tools 2022（VCTools，MSVC 14.44）；Linux: build-essential；macOS: Xcode CLT | 链接器（proc-macro 与最终二进制） | 总是 |
| git（系统命令） | 任意 | skills.sh 更新检查走 `git clone`，是**运行时**依赖 | 运行时 |
| WebView2 Runtime | Win 桌面壳 | Tauri 桌面构建/运行 | 仅桌面壳 |
| webkit2gtk-4.1 等系统库 | Linux | **编译** `tauri-shell` feature（默认 features） | 仅在 Linux 上编译桌面壳时 |

## Windows 快速安装（本机实际执行过的命令）

```powershell
# 1. VS Build Tools（C++ 工作负载，含 MSVC 链接器与 Windows SDK；数 GB）
winget install --id Microsoft.VisualStudio.2022.BuildTools `
  --override "--add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --wait --passive --norestart" `
  --accept-source-agreements --accept-package-agreements

# 2. rustup（stable + MSVC 目标）
Invoke-WebRequest -Uri https://win.rustup.rs/x86_64 -OutFile "$env:TEMP\rustup-init.exe"
& "$env:TEMP\rustup-init.exe" -y --default-toolchain stable --default-host x86_64-pc-windows-msvc
# 新开 shell 后 cargo 进 PATH；旧 shell 用全路径 $HOME/.cargo/bin/cargo.exe

# 3. 项目依赖
npm ci
cargo fetch --manifest-path src-tauri/Cargo.toml
```

## Linux 云开发机快速安装（Ubuntu / Debian）

```bash
# 1. 系统依赖（编译器 + 运行时 git）
sudo apt update && sudo apt install -y build-essential git curl pkg-config

# 2. Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source ~/.cargo/env

# 3. Node 24
curl -fsSL https://deb.nodesource.com/setup_24.x | sudo -E bash -
sudo apt install -y nodejs

# 4. 项目依赖
npm ci
cargo fetch --manifest-path src-tauri/Cargo.toml
```

**仅在云机上还要编译/测试 Tauri 桌面壳（默认 features）时**，追加 GUI 系统库（无头服务器可跳过）：

```bash
sudo apt install -y libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev \
  libjavascriptcoregtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev
```

## 无 GUI 云机的工作方式（重要）

- **Web 壳开发全程零 GUI 依赖**：`cargo check / cargo test --no-default-features --features web`
  ——业务核心 + Web 层不链接 Tauri，不装 webkit 也能编译测试（这正是 ADR-0006 feature 切分的意义）
- **桌面壳回归**（默认 features 的 `cargo test`）：需要上面的 webkit 包，或直接交给 GitHub CI（fork 的 macos/windows 矩阵）
- Web 版运行：`./oms-web` 监听 127.0.0.1:8477，SSH 隧道访问（见 `docs/deployment.md`）

## 相关文档

- 部署（生产）：`docs/deployment.md`（第一轮批次 4 产出）
- 本轮流程：`docs/rounds/round-1-web-shell/`
