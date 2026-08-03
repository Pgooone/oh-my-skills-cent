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

`oms-web` 仅有三个环境变量，没有任何配置文件，也没有命令行参数。

| 环境变量 | 默认值 | 生效规则 |
| --- | --- | --- |
| `OMS_BIND` | `127.0.0.1:8477` | 监听地址（`host:port` 全形式：`ipv4:port` / `[ipv6]:port` / `localhost:port`；端口 0 表示绑定系统分配的临时端口）。空字符串视为未设置；**非法值直接拒绝启动**（安全配置不静默回退），以启动日志中的实际地址为准 |
| `OMS_READONLY` | 未设置 | 置 `1` 开启公共只读模式（见 §6.2）。仅当 `OMS_BIND` 为非 loopback 地址时是必需项，见下方 D4 护栏 |
| `OMS_DATA_DIR` | `~/.oh-my-skills-cent` | 空字符串视为未设置；未设置时使用「运行用户的 home 目录 + `/.oh-my-skills-cent`」 |

**D4 护栏**：`OMS_BIND` 绑定的地址不是 loopback（127.0.0.1 / ::1）且
`OMS_READONLY` 未置 `1` 时，启动直接失败并打印原因、以退出码 1 退出——
无认证服务只允许以只读模式暴露到 localhost 之外。localhost 部署行为与
既有版本一致，无需任何额外配置。

数据目录内存放 `settings.json`、盘点缓存（`inventory-cache.json`）、
同步计划与历史（`plans/`、`sync-history.json`）、备份（`backups/`）、
注册表缓存（`registry/`、`skill-registry/`）、skills.sh 更新检出
（`updates/`）、访客上传暂存（`tmp/`，自动清理）等全部状态。
**备份服务器时备份此目录即可。**

注意：Web 版默认数据目录与桌面版（Tauri `app_data_dir`，如
`~/.local/share/app.oh-my-skills.desktop`）天然隔离。若希望两端共享同一份
设置与中心库，显式把 `OMS_DATA_DIR` 指向桌面版数据目录。

---

## 3. 运行时依赖：系统 git（必需）与 gh CLI（可选）

`oms-web` 本体是静态链接的单一二进制，但 **skills.sh 更新检查 / 更新技能 /
注册表拉取与推送** 功能会在运行时刻调用系统 `git`：

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

公共只读站（§6.2）若要自动把访客上传创建为 PR，还需安装
[GitHub CLI（`gh`）](https://cli.github.com/)。`gh` 是**可选**依赖：
缺失时上传链路仍然可用——分支照常推送到官方注册表，响应降级为分支
compare 页 URL，访客在该页面手动创建 PR 即可（服务端启动时会先用
`gh --version` 探测，不装不会有任何报错）。

---

## 4. 首次启动与验证

```bash
./oms-web
# 或自定义配置
OMS_BIND=127.0.0.1:9000 OMS_DATA_DIR=/var/lib/oh-my-skills-cent ./oms-web
```

启动成功时 stdout 打印（地址与数据目录为实际生效值；只读模式带
`[read-only]` 标记）：

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
# {"ok":true,"readonly":false}
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
Environment=OMS_BIND=127.0.0.1:8477
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

## 6. 远程访问

`oms-web` 有两种部署形态，由 `OMS_BIND` + `OMS_READONLY` 组合决定：

| 形态 | 配置 | 适用 |
| --- | --- | --- |
| 私有部署（默认） | `OMS_BIND=127.0.0.1:8477`（或任意 loopback） | 单用户完整功能，经 SSH 隧道访问 |
| 公共只读站 | `OMS_BIND=0.0.0.0:8477` + `OMS_READONLY=1` | 面向公众的只读浏览 + 访客上传贡献 |

### 6.1 私有部署：SSH 隧道

默认形态下 `oms-web` 只监听 `127.0.0.1`。从自己的工作站访问服务器上的
Web 界面，建立 SSH 本地端口转发：

```bash
ssh -L 8477:127.0.0.1:8477 user@server
```

保持该 SSH 会话连接，然后在本地浏览器打开：

```
http://127.0.0.1:8477
```

若本地 8477 已被占用，可换本地端口：`ssh -L 9000:127.0.0.1:8477 user@server`，
浏览器相应访问 `http://127.0.0.1:9000`。

#### 为什么默认只监听 localhost（ADR-0008 + D4 修订 + D8）

这是刻意的安全设计，不是缺陷，依据
[ADR-0008](adr/0008-localhost-only-no-auth.md)：

1. **无认证层，暴露形态唯一（D4 修订）。** Web 版是单用户单机语义，没有
   实现任何认证。非 loopback 地址的唯一合法形态是**只读模式**——
   `OMS_BIND` 绑定非 loopback 地址且 `OMS_READONLY` 未置 `1` 时，
   启动直接失败（D4 护栏，见 §2）。不存在「监听 0.0.0.0 且功能全开」的
   配置组合。「不开门」比「开门不锁」安全。
2. **仅监听 localhost ≠ 只有本机进程能访问（D8）。** 浏览器里任意网页都能
   向 `127.0.0.1` 发请求（CSRF / DNS rebinding 风险）。因此所有 `/api`
   请求还要过 guard 中间件：
   - `Host` 头（去端口）必须是 `localhost` / `127.0.0.1` / `[::1]` 之一，
     否则 403（只读模式下此校验放宽，见 §6.2）；
   - 带 `Sec-Fetch-Site: cross-site` 的请求一律 403；
   - POST 请求若带 `Origin` 头，其 host 必须与 `Host` 一致，否则 403。

由此得出两条运维结论：

- **公网访问请走 SSH 隧道**（或自行加一层带认证的反向代理）。
- 反向代理方案必须保留 `Host: localhost`（如 nginx 的
  `proxy_set_header Host localhost;`），否则所有请求会被 D8 guard 拒绝。
  任何多用户 / 公网访问需求都应先重开 ADR-0008 再谈实现。

### 6.2 公共只读站（OMS_READONLY=1）

面向公众的只读浏览站（例如展示官方工作流/技能注册表内容并接受访客上传
贡献）：

```bash
OMS_BIND=0.0.0.0:8477 OMS_READONLY=1 OMS_DATA_DIR=/var/lib/oh-my-skills-cent ./oms-web
```

只读模式的防护语义（红线 R2，白名单制默认拒绝）：

- **命令白名单**：`POST /api/commands/` 仅放行纯读命令
  （`read_inventory_cache` / `read_skill_lock` / `get_settings` /
  `list_installed_workflows` / `list_remote_workflows` /
  `get_workflow_detail` / `list_remote_skills`）与两个限流端点
  （`export_workflow_package` / `contribute_upload`）；其余一律 403，
  包括任何写盘命令（`scan_inventory` / `save_settings` / `save_workflow`
  等）与目录枚举命令（`list_dir` / `discover_project_workspaces`）。
- **设置出参**：`get_settings` 返回 PublicSettings 白名单结构——路径类
  字段置空、序列化层面不存在 token 明文键（`hasGithubToken` 恒 false），
  前端可据此隐藏全部写入口。
  `/api/health` 响应含 `readonly: true`，是前端探测只读的唯一通道。
- **注册表缓存只出不进**：`list_remote_*` 强制忽略 `refresh=true`
  （访客无法触发 clone+写盘），内容保鲜由管理员双实例负责（见下）。
- **限流**（per-IP 滑动窗口，限流 map 容量有界 + 过期淘汰）：
  - `contribute_upload`：5 次/小时，超限 429；
  - `export_workflow_package`：30 次/小时宽松桶，超限 429。
- **D8 配套变化**：`Host` 校验放宽为任意 Host（公网域名可达）；
  `Sec-Fetch-Site: cross-site` 仍一律 403；POST `Origin` host 仍须与
  `Host` 一致（公网同源表单自然满足）。

**X-Forwarded-For 信任策略**：限流按**TCP 对端地址**计数，当前版本一律
**不采信** `X-Forwarded-For` 头（直连部署下该头可被访客任意伪造）。默认
且推荐的部署形态是**直连**；若置于反向代理之后，所有访客在限流视角下
共享代理出口 IP（同一桶），请改在代理层做 per-IP 限流，或重开设计评审
引入显式的 XFF 采信开关。

**访客上传贡献（contribute_upload）**：访客上传 zip 包（≤ 20MB），服务端
依次做 zip 安检（路径穿越/炸弹，与导入同一安检链）→ 内容校验（workflow
需合法 workflow.yaml，skill 需 SKILL.md + frontmatter，slug 限
`[a-z0-9-]+`）→ 以 bot 身份推送 `upload/{slug}-{时间戳}` 分支到官方注册表
→ 经 `gh pr create` 建 PR（**PR 由维护者人工审核**，合并权始终在人工）。

部署要求：

- **bot 独立账号**：上传用的 GitHub token 经环境变量
  `OMS_GITHUB_TOKEN=ghp_...` 注入（或 settings 中配置），请使用**专用 bot
  账号**的细粒度 token（仅授予官方注册表仓的 contents/pull-requests 写
  权限），不要用个人账号。
- **bot 主机单用户专用**：token 以环境变量/进程参数形态在进程存活期间对
  同机同用户可见（`ps`、`/proc/environ`）——与个人单机明文 settings 同级
  可接受，但公共站主机应**单用户专用**，不要与其他服务/用户共享。
- **gh CLI 可选**（见 §3）：缺失时上传链路降级为返回分支 compare 页 URL，
  访客手动建 PR。

**管理员双实例运营约定**：只读实例的注册表缓存保鲜不由访客触发（refresh
被强制忽略）。在同一台主机、用**同一 `OMS_DATA_DIR`** 另跑一个 localhost
非只读实例（默认配置即可，仅监听 127.0.0.1）：

```bash
# 只读实例（公网）
OMS_BIND=0.0.0.0:8477 OMS_READONLY=1 OMS_DATA_DIR=/var/lib/oh-my-skills-cent ./oms-web
# 管理实例（仅本机，低峰期操作）
OMS_BIND=127.0.0.1:8478 OMS_DATA_DIR=/var/lib/oh-my-skills-cent ./oms-web
```

管理员经 SSH 隧道访问管理实例，在低峰期执行刷新（`list_remote_*` 带
refresh）与内容管理；两个实例读写同一数据目录，只读实例下次读缓存即
生效。**注意避免两实例同时写操作**（注册表缓存刷新属目录级替换，低峰
串行执行即可）。

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
ss -ltnp | grep 8477                    # 查看谁占用了 8477
OMS_BIND=127.0.0.1:9000 ./oms-web       # 或改用其他端口
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

### 8.6 启动即失败：bind 地址相关

**现象 A**：stderr 报
`oms-web: Invalid OMS_BIND '<值>': ...`，退出码 1。

**原因**：`OMS_BIND` 不是合法的 `host:port` 全形式（缺端口、端口非数字或
超出 0–65535、主机名不可识别）。非法值**不会**静默回退默认值——这是
刻意的安全配置行为。修正为形如 `127.0.0.1:8477` 的完整地址后重启。

**现象 B**：stderr 报
`oms-web: Refusing to start: OMS_BIND '0.0.0.0:8477' is not a loopback address while OMS_READONLY is not '1'. ...`，
退出码 1。

**原因**：D4 护栏（见 §2）——绑定了非 loopback 地址但未开只读模式。

**处理**：确认部署形态二选一——私有部署改回 loopback 地址（如
`OMS_BIND=127.0.0.1:8477`，经 SSH 隧道访问）；公共只读站显式加
`OMS_READONLY=1`（防护语义见 §6.2）。
