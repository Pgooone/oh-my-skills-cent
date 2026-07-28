# 任务卡：web-server（批次 2）

> 设计依据：`../detailed-design.md` §2。**前置**：core-context 已验收。

- [ ] `Cargo.toml`：新增 `web` feature（axum 0.8 / tower-http 0.6 / rust-embed 8 + debug-embed / tokio 1，全 optional）；`[[bin]] oms-web`（`src/bin/oms-web.rs`，required-features = ["web"]）
- [ ] `src-tauri/src/web/mod.rs`：router 构建 + 共享 state（AppContext + PathJail）
- [ ] `src-tauri/src/web/guard.rs`：D8 中间件（Host ∈ {localhost,127.0.0.1,[::1]}；POST 的 Origin host 须同源；Sec-Fetch-Site: cross-site 拒）→ 403
- [ ] `src-tauri/src/web/jail.rs`：PathJail（允许根集：library_path / agent global_roots / project roots / data_dir / ~/.agents；拒 `..`；save_settings 后刷新）
- [ ] `src-tauri/src/web/routes.rs`：12 个既有 command endpoint（契约见 §2.3 清单，请求 struct 逐一定义，禁 Value 透传）+ `GET /api/health`
- [ ] `src-tauri/src/bin/oms-web.rs`：main（OMS_PORT 默认 8477 / OMS_DATA_DIR / D4 硬编码 127.0.0.1 / 启动日志）
- [ ] rust-embed 静态服务：`#[folder = "../dist"]`，`/` → index.html，GET fallback
- [ ] 测试：guard 负向（假 Host / 跨源 Origin / cross-site → 403）；jail 负向（`..`、白名单外 → 403）；health 200；endpoint 正例（tempdir fixture 走 get_settings/save_settings）
- [ ] 门禁：`cargo test --no-default-features --features web` 绿；`cargo test`（默认）仍绿
- [ ] 门禁：`cargo tree --no-default-features --features web -e normal` 不含 tauri

**红线**：endpoint 只做薄转发（NFR-2）；不改核心模块逻辑；不 git commit。
