# 工作流包 = 目录 + workflow.yaml + README，步骤引用不内联

工作流包是一个目录：`workflow.yaml`（含 name/slug/version/description/author/tags/icon、阶段分组 groups、步骤 steps）+ `README.md`。步骤中以引用方式指向 Skill（复用 `skill.lock` 来源字段，允许占位步骤），**不内联 Skill 内容**。换来可靠更新与人机友好；代价是使用工作流时依赖网络拉取被引用 Skill。本决策于第一轮 grill 做出，2026-07-28 第二轮复核维持。
