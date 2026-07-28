# Oh My Skills Cent

Oh My Skills 的分叉项目：在原有「跨平台 Agent Skills 管理」之上新增工作流管理器，并以双形态交付——桌面版（Tauri）与 Web 版（Linux 服务器单二进制，浏览器访问）。

## Language

**Skill（技能）**:
一个可被 Agent 加载的能力包，以目录形式存在，入口为 `SKILL.md`。

**Agent**:
AI 编码工具（Claude Code、Cursor、Codex 等），Skill 的安装目标与消费者。
_Avoid_: AI 工具、编辑器、IDE

**中心库 (Center Library)**:
用户统一收录优质 Skill 的本机目录，是所有同步动作的源头。
_Avoid_: 仓库、库目录

**采纳 (Adopt)**:
把一个 Skill 收入中心库的动作。

**Sync Plan（同步计划）**:
所有磁盘写入前生成的「预览 → 确认 → 执行」计划，含备份与冲突检测。是唯一的写入通道。

**来源 (Source)**:
Skill 的远程出处记录（`skill.lock` 中的来源字段），用于定位、下载与更新检查。

**工作流 (Workflow)**:
声明式、有序的步骤清单，不含可执行控制流（无条件、分支、参数传递）。

**步骤 (Step)**:
工作流中的一个环节，含有序的一个或多个 Skill 引用，附步骤说明。多 Skill 时数组顺序即使用顺序。

**阶段分组 (Group)**:
对步骤的高层归类（如：需求 / 记录 / 编程）。
_Avoid_: 阶段、阶段标签

**占位步骤 (Placeholder Step)**:
未绑定具体 Skill、标记「待补全」的步骤。

**工作流包 (Workflow Package)**:
一个目录：`workflow.yaml` + `README.md`，步骤中以引用方式指向各 Skill（不内联）。

**工作流注册表 (Workflow Registry)**:
工作流的远程来源。默认为官方公共 Git 仓库（独立注册表仓库），可切换为自建/团队仓库。
_Avoid_: 市场、商店、Marketplace

**入口清单 (Entry Manifest)**:
使用工作流的一种输出形态：在 Agent skills 目录生成工作流清单 + README，各 Skill 独立安装，Agent 按序发现加载。

**打包技能 (Packaged Skill)**:
使用工作流的另一种输出形态：整条流程打包成单一自包含 Skill 目录——`SKILL.md` 写编排说明，`skills/` 子目录放各被引用 Skill 的结构化拷贝。

**桌面版 (Desktop Shell)**:
Tauri 桌面应用形态（macOS / Windows）。

**Web 版 (Web Shell)**:
Linux 服务器单二进制形态，浏览器访问，仅监听 localhost，无认证层。

**使用工作流 (Use a Workflow)**:
把工作流引用的全部 Skill 经 Sync Plan 采纳并同步到目标 Agent 的动作，缺失的一并下载，影响范围先可见。
