# 卡 C7 · readonly-mode（M7 公共只读模式 + 访客上传）

> 设计：DD §6-R1/R2/R5/R9、§8.1-8.3。依赖 C1（token/裁剪）、C5（contribute_to_official）。

## 范围

- `bin/oms-web.rs`：`OMS_BIND`（host:port，缺省 127.0.0.1:8477，**取代 OMS_PORT**）+ `OMS_READONLY` + D4 修订护栏（非 localhost 且无 readonly → exit(1) 打印原因）+ serve 改 `into_make_service_with_connect_info::<SocketAddr>()` + 启动预热 load_settings + 文件头注释修订
- `web/mod.rs`：AppState + `readonly: bool`；只读白名单中间件（DD §8.2 订正版名单，默认拒绝）；`/api/health` + readonly 字段；import/contribute_upload 两路由 `DefaultBodyLimit::max(96MB)`
- `web/guard.rs`：**D8 配套修订**——readonly 放行任意 Host，保留 Sec-Fetch-Site: cross-site 403 与 POST Origin==Host 校验；非 readonly 维持现状
- `web/routes.rs`：get_settings 只读模式返回 PublicSettings（白名单 struct，字段名保留值置空，无 github_token 键）；list_remote_* 只读强制 refresh=false；`contribute_upload` endpoint（ConnectInfo 限流 5/h fail-closed + map 容量上限过期淘汰 + 20MB + 复用 §4.2 安检 + slug 校验 + staging 清理 + M5 contribute_to_official + gh CLI pr create（--version 先探测、GH_TOKEN env、stderr 过 redact、失败降级分支 URL））；export 只读模式并入限流 30/h
- `web/mod.rs`（static_handler）：拒绝含 `..` 段的请求路径（rust-embed debug-embed 遍历加固，DD §10 门-readonly-F12）——评审门 F1 孤儿项并入本卡
- `docs/deployment.md`：更新 OMS_BIND/OMS_READONLY/XFF 信任策略/bot 独立账号与主机单用户专用/admin 双实例运营约定；**修订 §8.6（OMS_PORT 已废弃）**——评审门 F2 孤儿项并入本卡

## AC（可断言）

- [ ] 单测全过：白名单正反组（放行端点 200 / scan_inventory 等写端点 403 / 未列名一律 403）、PublicSettings 响应 JSON 无 githubToken 键且字段齐全、list_remote_* 只读模式 refresh=true 被忽略、限流同 IP 第 6 次拒绝（5/h 桶）+ **export 桶第 31 次拒绝（30/h 桶）** + 窗口滑动恢复 + map 淘汰、D8（readonly 下公网 Host 放行 / cross-site 仍 403 / Origin 匹配校验保留；非 readonly 行为零变化）、D4（OMS_BIND=0.0.0.0 无 readonly 拒启动含原因文案；localhost 行为不变）、**ConnectInfo 提取失败分支 fail-closed 503（单测覆盖提取失败路径；真 TCP 验证归 C11 AC5）**
- [ ] contribute_upload 单测：未配 bot token 报「站点未开放贡献」、坏 zip 拒绝、slug 非 [a-z0-9-]+ 拒绝、限流触发
- [ ] static_handler 含 `..` 段路径拒绝；deployment.md 更新完成且 OMS_PORT 不再出现
- [ ] cargo test 默认 + web 全绿

## 守红线

- R1/R2/R5/R9 逐条；ConnectInfo 提取失败 fail-closed 503（禁静默放行）；白名单外默认拒绝
- gh 调用约定（R10）：stderr 过 redact_text、--version 先行

## commit

`feat(web): 新增公共只读模式与访客上传贡献`
