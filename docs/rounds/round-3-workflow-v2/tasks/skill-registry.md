# 卡 C6 · skill-registry（M6 注册表客户端）

> 设计：DD §5.5（评审门补章）。依赖 C1。

## 范围

- 新建 `src-tauri/src/skill_registry.rs`：RemoteSkillSummary、OFFICIAL_SKILL_REGISTRY_URL、缓存镜像（`data_dir/skill-registry/` staging→swap→离线回退）、fetch_index/read_cached_index、download_skill（**冲突拒绝** + **byte-verbatim 直拷** + lock 写入归一化 https 形态）、check_updates（批量一次 clone）、apply_update（备份→删→重建 + lock.updatedAt）、路径/slug 安检（模块内同规则拷贝）
- `commands.rs` + `web/routes.rs` + 登记：`list_remote_skills`、`download_skill`、`check_registry_skill_updates`、`update_registry_skill`

## AC（可断言）

- [ ] 单测全过：index 解析（含 installed 现算）、下载后 lock 五字段全对且 sourceUrl 为归一化 https 形态、**下载完成立即 hash_dir 比对缓存目录相等（byte-verbatim）**、同 slug 异源下载拒绝、批量判定（current/available/本地被改三态）、apply_update 备份产生 + 更新后 hash 一致 + lock.updatedAt 刷新
- [ ] 缓存测试：staging/swap 无残留、clone 失败离线回退（本地 fixture repo）
- [ ] cargo test 默认 + web 全绿

## 守红线

- 缓存镜像为有意取舍（不泛化既有，DD §5.5.2）；clone 经 M1（私有注册表读凭证）
- 不碰 `is_agents_skill_path` 与 skill_ops 既有守卫

## commit

`feat(skill): 新增 skill 注册表客户端与下载更新链路`
