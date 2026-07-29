# 任务卡：download-spike（批次 1 · 承重墙）

> 设计依据：`../detailed-design.md` §0。**探针收官即删**（或转为正式 fixture 测试）。

证伪/证实三条承重前提，每条都要给出命令 + 输出证据：

- [ ] **P1 skillPath 目录形式可解析**：真实执行 `git clone --depth 1 https://github.com/mattpocock/skills.git` 到临时目录，用 `skill_ops` 的路径解析逻辑（自定义 path 分支）验证 `skills/productivity/grill-me`、`skills/engineering/domain-modeling`、`skills/engineering/tdd` 三个目录均含 SKILL.md；同时验证不带 skillPath 时内建候选（`/<slug>`、`/skills/<slug>`、仓库根）是否也能命中（预期不能，记录之）
- [ ] **P2 serde_yml 解析真实 workflow.yaml**：用探针代码（临时 test 或 example）解析注册表两个真实 yaml（从刚 clone 的注册表读），确认 untagged 枚举正确区分 SkillRef 与 placeholder；确认 camelCase 字段映射
- [ ] **P3 注册表拉取链路**：git clone --depth 1 `https://github.com/Pgooone/oh-my-skills-workflows.git` 到临时目录，读根 index.json 与子目录 workflow.yaml 成功
- [ ] 输出：每条 PASS/FAIL + 证据；若 P1 发现 skillPath 需要**文件形式**（含 SKILL.md 后缀）或内建候选意外命中，明确报告（影响 schema 与 registry 内容，lead 需决策）

**红线**：不改动仓库任何文件（探针放系统临时目录）；不 git commit；克隆目标全部放临时目录，收官清理。
