# 上游策略：选择性摘桃

oh-my-skills-cent 大体独立演进（Web 化与工作流管理器是上游没有的方向），上游（nextcaicai/oh-my-skills）出现重要修复/功能时选择性 cherry-pick，不做定期整体合并。

## Consequences

- 为让摘桃可干净应用，**核心模块文件路径保持稳定**：Web 服务以同 crate 新二进制加入，不拆 Cargo workspace、不移动既有模块文件。
- 每个实施阶段起步时评估一次上游 diff，决定摘哪些。
