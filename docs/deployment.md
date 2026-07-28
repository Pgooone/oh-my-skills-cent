# Web 版部署指南（oms-web）

本文档描述如何把 Oh My Skills 的 Web 版（`oms-web` 二进制）部署到 Linux 服务器，
并通过 SSH 隧道从本地浏览器访问。

> 产物形态：单个 `oms-web` 可执行文件，前端静态资源在 release 构建时通过
> rust-embed 内嵌进二进制，部署时无需携带 `dist/` 目录。
> 该二进制不链接 Tauri / webkit2gtk 等 GUI 系统库，可在无桌面环境的
> Linux 服务器上直接编译与运行。

文中所有命令、默认值与报错文案均与 `src-tauri/src/bin/oms-web.rs`、
`src-tauri/src/context.rs`、`src-tauri/src/web/` 源码逐一核对。

---

## 1. 构建

### 1.1 前置要求

| 依赖 | 版本 | 用途 |
| --- | --- | --- |
| Node.js | ≥ 18（vite 6 要求；CI 用 20，本机验证 24） | 构建前端 `dist/` |
| Rust 工具链 | ≥ 1.77.2（`rust-version` 字段，推荐 rustup） | 编译 `oms-web` |
| git | 任意近期版本 | 拉取源码；同时是**运行时依赖**（见 §3） |

### 1.2 构建步骤

一行命令（等价于下方两步的封装，`package.json` 中的 `web:build`）：

```bash
npm run web:build
```

或分步执行：

```bash
git clone https://github.com/Pgooone/oh-my-skills-cent.git
cd oh-my-skills-cent

# 1. 安装前端依赖并构建前端（产出仓库根目录的 dist/）
npm install
npm run build        # = tsc && vite build

# 2. 编译 Web 版二进制
cargo build --release --no-default-features --features web --bin oms-web \
  --manifest-path src-tauri/Cargo.toml
```

**顺序约束：必须先构建前端，再编译 Rust。** `oms-web` 通过
rust-embed 的 `#[folder = "../dist"]`（相对 crate 根 `src-tauri/`）在
**编译期**读取前端资源：release 构建会把 `dist/` 真正内嵌进二进制，
`dist/` 不存在则编译失败。

`--no-default-features --features web` 是关键：默认 features 是桌面壳
（`tauri-shell`，会链接 Tauri），web feature 只引入
axum / tower-http / rust-embed / tokio，不引入任何 GUI 依赖。

### 1.3 产物

```
src-tauri/target/release/oms-web       # Linux
src-tauri/target/release/oms-web.exe   # Windows（本地测试用）
```

把这一个文件拷贝到目标服务器即可运行。

---

## 2. 配置

`oms-web` 仅有两个环境变量，没有任何配置文件，也没有命令行参数。

| 环境变量 | 默认值 | 生效规则 |
| --- | --- | --- |
| `OMS_PORT` | `8477` | 值必须能解析为 `u16`（0–65535 的纯数字；0 表示绑定系统分配的临时端口），否则**静默回退 8477**。设置后以启动日志中的实际端口为准 |
| `OMS_DATA_DIR` | `~/.oh-my-skills-cent` | 空字符串视为未设置；未设置时使用「运行用户的 home 目录 + `/.oh-my-skills-cent`」 |

数据目录内存放 `settings.json`、盘点缓存（`inventory-cache.json`）、
同步计划与历史（`plans/`、`sync-history.json`）、备份（`backups/`）、
skills.sh 更新检出（`updates/`）等全部状态。**备份服务器时备份此目录即可。**

注意：Web 版默认数据目录与桌面版（Tauri `app_data_dir`，如
`~/.local/share/app.oh-my-skills.desktop`）天然隔离。若希望两端共享同一份
设置与中心库，显式把 `OMS_DATA_DIR` 指向桌面版数据目录。

---

## 3. 运行时依赖：系统 git

`oms-web` 本体是静态链接的单一二进制，但 **skills.sh 更新检查 / 更新技能**
功能会在运行时刻调用系统 `git`：

```
git clone --depth 1 <repo-url> <OMS_DATA_DIR>/updates/<slug>-<时间戳>/repo
```

（见 `src-tauri/src/skill_ops.rs` 的 `checkout_skills_sh_source`。）

服务器上必须安装 git 并保证运行用户在 `PATH` 中能找到它：

```bash
# Debian / Ubuntu
sudo apt install git
# RHEL / Fedora
sudo dnf install git
```

git 缺失时，其余功能不受影响，仅 skills.sh 更新相关操作会报错：
`Unable to clone <url>: No such file or directory (os error 2)`。

---

## 4. 首次启动与验证

```bash
./oms-web
# 或自定义配置
OMS_PORT=9000 OMS_DATA_DIR=/var/lib/oh-my-skills-cent ./oms-web
```

启动成功时 stdout 打印（端口与数据目录为实际生效值）：

```
oms-web listening on http://127.0.0.1:8477 (data dir: /home/oms/.oh-my-skills-cent)
```

首次启动会自动创建数据目录并写入默认 `settings.json`（同时创建默认中心库
目录 `~/.oh-my-skills/skills`），因此数据目录权限问题在启动阶段即会暴露。

启动失败时错误打印到 stderr 并以退出码 1 退出，形如：

```
oms-web: Unable to bind 127.0.0.1:8477: Address already in use (os error 98)
```

健康检查（在服务器本机执行）：

```bash
curl http://127.0.0.1:8477/api/health
# {"ok":true}
```

---

## 5. systemd unit 示例

`/etc/systemd/system/oms-web.service`：

```ini
[Unit]
Description=Oh My Skills Web (oms-web)
After=network.target

[Service]
Type=simple
User=oms
WorkingDirectory=/opt/oh-my-skills
ExecStart=/opt/oh-my-skills/oms-web
Restart=on-failure
RestartSec=5
Environment=OMS_PORT=8477
# 数据目录默认是 User 的 ~/.oh-my-skills-cent；需要集中管理时显式指定：
# Environment=OMS_DATA_DIR=/var/lib/oh-my-skills-cent

[Install]
WantedBy=multi-user.target
```

部署与启停：

```bash
sudo install -m 0755 oms-web /opt/oh-my-skills/oms-web
sudo systemctl daemon-reload
sudo systemctl enable --now oms-web

# 状态与日志
systemctl status oms-web
journalctl -u oms-web -f
```

说明：

- `WorkingDirectory` 对 `oms-web` 无实际作用（release 构建不读工作目录下的
  任何文件），仅为运维惯例保留。
- 以 `User=oms` 运行时，默认数据目录为 `/home/oms/.oh-my-skills-cent`；
  若改用 `OMS_DATA_DIR=/var/lib/oh-my-skills-cent`，先
  `sudo install -d -o oms /var/lib/oh-my-skills-cent` 确保可写。
- 进程内的路径白名单（PathJail）以**运行用户的 home 目录**和各 Agent 的
  技能目录为允许根，请用最终使用技能的那个用户身份运行，不要用 root。

---

## 6. 远程访问：SSH 隧道

`oms-web` 只监听 `127.0.0.1`。从自己的工作站访问服务器上的 Web 界面，
建立 SSH 本地端口转发：

```bash
ssh -L 8477:127.0.0.1:8477 user@server
```

保持该 SSH 会话连接，然后在本地浏览器打开：

```
http://127.0.0.1:8477
```

若本地 8477 已被占用，可换本地端口：`ssh -L 9000:127.0.0.1:8477 user@server`，
浏览器相应访问 `http://127.0.0.1:9000`。

### 为什么只监听 localhost（ADR-0008 + D8）

这是刻意的安全设计，不是缺陷，依据
[ADR-0008](adr/0008-localhost-only-no-auth.md)：

1. **无认证层，绝不暴露到局域网/公网。** Web 版是单用户单机语义，没有实现
   任何认证。绑定地址在源码中硬编码为 `127.0.0.1`
   （`oms-web.rs` 的 `const BIND_ADDRESS`），**本轮不提供任何覆盖开关**——
   不存在一个环境变量能让它监听 `0.0.0.0`。「不开门」比「开门不锁」安全。
2. **仅监听 localhost ≠ 只有本机进程能访问（D8）。** 浏览器里任意网页都能
   向 `127.0.0.1` 发请求（CSRF / DNS rebinding 风险）。因此所有 `/api`
   请求还要过 guard 中间件：
   - `Host` 头（去端口）必须是 `localhost` / `127.0.0.1` / `[::1]` 之一，
     否则 403；
   - 带 `Sec-Fetch-Site: cross-site` 的请求一律 403；
   - POST 请求若带 `Origin` 头，其 host 必须与 `Host` 一致，否则 403。

由此得出两条运维结论：

- **公网访问请走 SSH 隧道**（或自行加一层带认证的反向代理）。
- 反向代理方案必须保留 `Host: localhost`（如 nginx 的
  `proxy_set_header Host localhost;`），否则所有请求会被 D8 guard 拒绝。
  任何多用户 / 公网访问需求都应先重开 ADR-0008 再谈实现。

---

## 7. Linux 构建验证

二进制是跨平台编译产物，以下两条路径任选其一验证 Linux 构建：

**路径 A：目标 Linux 服务器上直接构建（推荐）**。按 §1 在服务器上执行
完整构建步骤，然后按 §4 做健康检查。这同时验证了服务器工具链与运行环境。

**路径 B：本机 WSL2 中构建（Windows 开发机可选）**。在 WSL2 发行版中安装
Node.js、Rust、git 后执行 §1 相同命令，得到 `oms-web`（无扩展名）的 Linux
产物，再拷贝到目标服务器。

> 注意：本仓库本轮迭代在纯 Windows 环境开发，未在 WSL2 中实测；路径 B
> 与路径 A 命令完全一致，差异仅在执行环境。

---

## 8. 故障排查

### 8.1 端口占用

**现象**：启动失败，stderr 报
`oms-web: Unable to bind 127.0.0.1:8477: Address already in use (os error 98)`。

**处理**：找出占用进程或换端口。

```bash
ss -ltnp | grep 8477          # 查看谁占用了 8477
OMS_PORT=9000 ./oms-web       # 或改用其他端口
```

### 8.2 git 缺失

**现象**：skills.sh 更新检查 / 更新技能时报
`Unable to clone https://github.com/...: No such file or directory (os error 2)`。

**处理**：安装 git（见 §3）。该错误只影响 skills.sh 更新链路，
扫描 / 同步等本地功能不受影响。

### 8.3 数据目录权限

**现象**：启动即失败，stderr 报
`oms-web: Unable to write settings at <数据目录>/settings.json: Permission denied (os error 13)`。

**原因**：首次启动要创建数据目录并写入默认 `settings.json`，运行用户对
`OMS_DATA_DIR`（默认 `~/.oh-my-skills-cent`）没有写权限。

**处理**：

```bash
sudo install -d -o <运行用户> /var/lib/oh-my-skills-cent   # 自定义目录时
# 或修正默认目录归属
sudo chown -R <运行用户>:<运行用户> ~<运行用户>/.oh-my-skills-cent
```

systemd 场景特别注意：`User=` 与数据目录归属必须一致。

### 8.4 从局域网 / 公网直接访问不通

**现象**：浏览器访问 `http://<服务器IP>:8477` 连接被拒绝。

**这是设计行为**：`oms-web` 只绑定 `127.0.0.1`，操作系统层面就不接受外部
连接。请按 §6 使用 SSH 隧道，不要试图找「监听 0.0.0.0 的开关」——它不存在。

### 8.5 请求返回 403（Host / Origin 被拒绝）

**现象**：服务在运行、`curl` 能连通，但响应为 403，body 如
`{"error":"Host 'example.com' is not allowed"}`。

**原因**：D8 guard 校验失败（见 §6）。常见触发：

- 通过域名或服务器 IP 访问（Host 不是 localhost）；
- 反向代理转发时改写了 Host；
- 第三方网页跨站发起请求。

**处理**：经 SSH 隧道访问 `http://127.0.0.1:8477`；反向代理场景加
`proxy_set_header Host localhost;`（并自行在代理层补齐认证）。

### 8.6 OMS_PORT 设了但没生效

**现象**：设置了 `OMS_PORT` 却仍监听 8477。

**原因**：值无法解析为 `u16`（含空格、非数字、超出 1–65535）时会静默回退
默认值。以启动日志 `oms-web listening on http://...` 中的实际端口为准。
