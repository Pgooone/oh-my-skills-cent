# 卡 C9 · frontend-skills（skill 注册表前端 + 更新触发链）

> 设计：DD §8.5（评审门 B6 修订）。依赖 C6；SkillsView「贡献」入口依赖 C5 的 contribute_skill endpoint。本卡含 W4 承重墙的单元层验证。

## 范围

- **前端测试基建（本轮新增，评审门 AC-01）**：`package.json` + devDependency **vitest**（项目首个前端测试运行器）+ `test:ui` 脚本；测试文件 `src/lib/skillUtils.test.ts`
- `SkillsView`：skill 注册表远程区（镜像 Workflows 远程区模式：列表/下载/贡献徽标）+ registry 来源徽标 + 更新提示徽标
- `skillUtils.ts`：`skillsShUpdateSource` 增兜底——lock 命中且 `skill.canonicalStatus=="imported"` 时以 `skill.canonicalPath` 作 entryPath；仅当存在非中心库引用的 `.agents/skills` 实目录时沿用旧候选
- `App.tsx`：refreshInventory 完成后调用 `refreshSkillsShUpdateChecks(allSkills, locks)`（现为死代码）
- 更新执行分流：lock.sourceUrl 双侧归一化 == skillRegistryUrl → `check_registry_skill_updates`/`update_registry_skill`；其余走既有 command

## AC（可断言）

- [ ] tsc 绿、vite build 绿、**`vitest run` 绿**
- [ ] **W4 单元层断言（vitest，机器可复跑）**：构造 lock 条目 + `canonicalStatus=="imported"` 的 skill → `skillsShUpdateSource` 返回非 null 且 entryPath == canonicalPath（**先写测试见证红再改**，改前必失败）；分流用例：registry 来源 → 新 command、skills.sh 来源 → 既有 command
- [ ] 调用点核查：`grep -c 'callApi("download_skill"' ≥1`、`callApi("contribute_skill"` ≥1

## 守红线

- 前端触发链两处小改仅限 DD §8.5 允许面；不碰 scanner/registry 语义

## commit

`feat(ui): skill 注册表远程区与更新触发链`
