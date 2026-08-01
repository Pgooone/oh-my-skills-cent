# 卡 C5 · workflow-push（M5 推送/贡献 + index 代写契约）

> 设计：DD §5、R10。依赖 C1（M1/M2 全部调用）；**开工前置：C0（W1 spike）必须 GO。**

## 范围

- 新建 `src-tauri/src/workflow_push.rs`：upsert_index_entry（缺文件/缺数组则建、按 slug upsert、pretty JSON）、push_workflow_to_registry（官方地址拒绝引导贡献）、contribute_workflow/contribute_skill（NoToken 结构化返回/NeedFork/Ready 三态）、`pub(crate) contribute_to_official`（供 C7）
- skill 条目字段映射按 DD §5.1（version=metadata.version 缺省 "0.1.0" 等）
- `commands.rs` + `web/routes.rs` + 登记：3 个 command/endpoint（薄转发）

## AC（可断言）

- [ ] 单测全过：upsert 新建/更新/数组保序/8 字段完整（含 skill 条目缺省映射）；官方地址直推拒绝且错误文案引导贡献；NoToken 分支（Ok 载荷 `{status:"noToken"}`——wire 形态按 DD §5.2 订正版）；NeedFork 分支（ls_remote 失败）；**Ready 分支（fork clone→分支命名→push→compare_url 各字段逐字段断言）**；**push rejected → Err「远端已更新，请重试」语义**
- [ ] **真 git 全链路（本地 bare repo fixture，file://，零外网）**：clone→写入→upsert→commit→push→对端 `git log` 见新 commit、clone 回来逐字段校验 index 与包目录（W5 契约实证）
- [ ] 全部 git 调用经 M1（grep 本模块无 Command::new）；构造 push 失败场景断言错误串无 token 两形态
- [ ] **NFR-7 契约文本产出**：写侧契约（index 8 字段/默认分支 main/skill 条目字段映射）写入本地注册表克隆 `Oh-My-Skills/oh-my-skills-workflows/README.md`（lead 负责推送；skill 注册表 README 待建仓后同样补）——评审门 F2
- [ ] cargo test 默认 + web 全绿

## 守红线

- R10 调用约定；compare_url base 恒 main（契约）；分支命名 `contrib/{slug}`（冲突加 UTC 时间戳）
- 摘桃：仅新文件 + 薄转发

## commit

`feat(workflow): 新增注册表一键推送与 fork 贡献链路`
