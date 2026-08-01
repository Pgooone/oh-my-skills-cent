# Round 3 · 设计评审门记录（2026-07-29）

> 形式：2 路评审 + 3 路承重前提反驳者 + W3 实证 spike（全 sonnet，11 agents）。
> 结果：**59 条 findings，确认 blocker/major 29 条**；W3 spike **GO**。
> 本文档 = 每条确认/吸收 findings 的处置映射；DD/HLD/proposal/QA 已按此回写为订正版。

## 1. 确认的 blocker（6 组，全部已修入订正版）

| # | 问题（一句话） | 处置 |
| --- | --- | --- |
| B1 | 只读白名单两个 command 名不存在（list_skills/get_skill_content），boot 必需的 read_inventory_cache 未放行；scan_inventory 含写盘不能放行 | DD §8.2 全量对账订正版白名单；read_skill_content 无需处理（web 无路由且前端不调用） |
| B2 | D8 guard 不联动 D4 修订：公网 Host 下 /api 全 403（含 health），整站退化 demo 态 | DD §6-R5：readonly 放行 Host、保留 cross-site 403 与 Origin==Host 校验；正反组测试 |
| B3 | M6 skill-registry 无详细设计章节 | DD §5.5 补全章（模型/缓存/下载写 lock/冲突语义/批量 check/更新执行/单测） |
| B4 | axum 默认 2MB body limit：import/contribute_upload 必然 413 | DD §4.3/§8.3：两路由单独挂 DefaultBodyLimit::max(96MB)，其余维持默认；解码前长度预检 |
| B5 | ConnectInfo 在现有 serve 形态下不可用（限流必然失效或 500） | DD §8.1：serve 改 into_make_service_with_connect_info；提取失败 fail-closed 503；真 TCP 集成测试 |
| B6 | **Q8「lock 红利」承重前提证伪**：前端触发链结构性排除中心库安装（L1）且 refreshSkillsShUpdateChecks 是死代码（L2） | DD §8.5 新增前端触发链两处改动（候选兜底 + refreshInventory 后触发）；W4 spike 断言改端到端；QA Q8 表述修订 |

## 2. 确认的 major（摘录要点，全部已修入订正版）

| # | 问题 | 处置 |
| --- | --- | --- |
| M1 | registry.rs:1433 Settings fixture 漏列清单 | HLD §4 例外②补录 |
| M2 | list_remote_* 的 refresh=true 让匿名访客触发 clone+写盘 | 只读模式强制 refresh=false |
| M3 | export 白名单放行但无限制（N 次出站 clone 放大端点） | 只读模式并入限流（30/h 宽松桶） |
| M4 | list_dir/discover 向公网枚举 home/data_dir | 移出白名单；前端只读模式不调用 |
| M5 | PublicSettings 形状未定（黑名单式会把 token 发给访客/前端 boot 崩） | 白名单 struct 逐字段钉死（字段名保留值置空，serde 物理隔离无 github_token 键） |
| M6 | 限流 map 无界增长 + 反代 IP 坍缩/XFF 伪造未定 | map 容量上限+过期淘汰；XFF 信任策略入部署文档 |
| M7 | save_settings 返回值未裁剪（token 明文随响应回前端反复过网） | 裁剪规则改「凡返回 Settings 的 API 一律裁剪」+ 验收负向用例 |
| M8 | save_settings 清除语义矛盾（双壳必分叉） | wire 契约钉死：null=保持/非空=替换/clearGithubToken=清除，核心 merge 单点 |
| M9 | GH_TOKEN env 进程级可见性未列例外清单 | R10 例外面补记（与 extraheader 同级接受）；gh stderr 过 redact |
| M10 | FR-2 零 token 降级无落实点 | §5.3 NoToken 结构化返回 + §8.5 前端导出+引导流程 |
| M11 | skill index 8 字段与 SKILL.md frontmatter 断裂（version/author/tags/icon 无出处） | §5.1 字段映射钉死（version=metadata.version 缺省 0.1.0 等），入 NFR-7 契约 |
| M12 | token 落盘 0600 无实现点 | settings.rs save 后 unix set_permissions(0o600) + 单测断言 |
| M13 | zip 导入负例与限流无单测列明 | DD §4.2/§9 补测试清单 |
| M14 | lock 同 slug 异源覆盖 → 永久误报 + 静默换源 | §5.5.4 下载前冲突检查拒绝（换源走先删后下） |
| M15 | token 只覆盖写路径 → 私有注册表读全灭（Q4 本意含读） | HLD §4 例外①：两处既有 clone 调用点换 M1::clone_repo（凭证 None 时行为全同） |

## 3. 吸收的 minor（摘要）

OMS_BIND 取代 OMS_PORT；readonly 探测收敛 health 单通道；proposal 三处回改（source 落点/仅 zip/批量 check 例外）；HLD Cargo +base64；clone 根清理从返回 PathBuf 反推；compare_url base 恒 main 入契约；check_all 换注册表语义声明；AC1 源目录=origin 现拉钉死；skillRegistryUrl GitHub-only 声明；更新分流双侧 normalize + lock 写归一化形态；下载 byte-verbatim 声明 + AC 断言；get_settings 启动预热；contribute_upload 复用 §4.2 安检 + staging 清理 + slug 校验；static_handler 拒 `..` 段（debug-embed 实证留实现期）；-c extraheader 进程参数面复核结论入 R10；registry URL userinfo 防线（M1 normalize + 保存校验）；「既有两处 clone」勘误为三处（第三处是测试 helper 不算生产）。

## 4. W3 spike 结论（GO，zip MSRV 实证）

- `zip = "~4.0"` 解析 **4.0.0**（4.0.x 区间上限），自身 rust-version 1.75 ✓；base64 0.23 ✓。
- **坑 1（必须动作）**：zip 4.0.0 直接依赖 indexmap="2"，当前最新 2.14.0 的 manifest 是 edition2024，1.77 cargo resolve 即报错且无 MSRV 感知回退 → **必须 Cargo.lock 入库 + `cargo update -p indexmap --precise 2.9.0`**（联动 hashbrown 锁 0.15.5）。
- **坑 2**：rust-version 比较含 patch——工具链必须 ≥**1.77.2**（1.77.0 被「requires 1.77.2」拒绝）。
- 传递依赖体检全过（zopfli 0.8.3/zlib-rs 0.6.6/flate2 1.1.9 等声明均 ≤1.75）。
- API 形态（实证可编译）：`ZipWriter::new(Cursor)` + `SimpleFileOptions::default().compression_method(Deflated)`；`add_directory("d/")`（尾斜杠必须）/ `start_file` / `finish()?.into_inner()`；`ZipArchive::new` → `by_index`/`by_name`，ZipFile 实现 Read；base64 0.23 走 `base64::engine::general_purpose::STANDARD`。
- 探针已删（临时 crate 不存在），项目代码零触碰。

## 5. 门结论

确认 blocker 0 残留（6 组全修入订正版）；major 全处置；W3 high-confidence GO；W1/W2/W4/W5 spike 排入实现批次 1。
**待用户批准订正版设计 → 拆任务卡。**

---

## 6. ⑥ 拆后二次复审记录（2026-08-01，2 路复审 + 2 路裁决，全 sonnet）

任务卡 12 张 × DD 一致性 + AC 可断言性对账：18 条 findings，**确认 major 4 条**，全部已修入任务卡/DD：

| # | 问题 | 处置 |
| --- | --- | --- |
| ⑥-F1 | static_handler 拒 `..` 段（门-readonly-F12 的实现期动作）无卡认领 | 并入 C7 范围 + AC |
| ⑥-F2 | NFR-7 契约入注册表 README、deployment.md 更新（OMS_BIND/XFF/bot/双实例 + OMS_PORT 废弃）无卡认领 | 契约文本并入 C5（本地克隆，lead 推送）；deployment.md 并入 C7 |
| ⑥-AC01 | C9 W4 红绿断言无执行载体（前端零测试基建） | C9 引入 vitest（项目首个前端测试基建）+ skillUtils.test.ts；门禁 +`vitest run` |
| ⑥-AC02 | 贡献三态 wire 矛盾（§5.3 Err 通道 vs §8.4 Ok status 字段），跨卡分叉风险 | 钉死 Ok 载荷 `{status:"noToken"/"needFork"/"ready"}`（DD §5.2/§5.3/§8.5 与 C5/C8 同步订正） |

吸收的 minor（13 条，已逐条修卡）：C0 go/no-go 阈值 + NO-GO 备选路径 + timeout 退出码判别；C1 Windows 0600 注明 + 前端非 GitHub 拒绝 + get_settings 裁剪断言 + M2 函数组/resolve_token 优先级断言；C2 数量型弱判据改「零修改全过」；C3 矩阵补第 7 case；C4 负例补 base64 预检共 10 条；C4/C5/C9 卡头依赖补齐；C5 Ready 分支/push rejected 断言；C7 export 30/h 桶触发 + fail-closed 归属；C8 判据机器化（grep 模式）；C10 判据双层表述；C11 AC3 fork 侧 ls-remote、双层分工逐条 ▲/● 标注、AC4/AC6 靶子仓钉死。
