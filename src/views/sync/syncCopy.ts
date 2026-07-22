import { syncPlanSummary } from "../../lib/skillUtils";
import type { AgentRecord, ApplyResult, SyncPlan } from "../../types";
import type { QuickMigrationMethod, SyncMode } from "../../uiTypes";

export function draftPlanSentence(
  mode: SyncMode,
  method: QuickMigrationMethod,
  skillCount: number,
  targetCount: number,
  scope: "global" | "project",
  projectPath: string | null
) {
  if (skillCount === 0) return "请先选择至少 1 个 Skill。";
  if (targetCount === 0) return "请选择至少 1 个目标 Agent。";
  if (scope === "project" && !projectPath) return "请选择要同步的本地项目。";
  const scopeText = scope === "project" ? `到项目 ${projectDisplayName(projectPath ?? "")}` : "到全局";
  if (mode === "managed") {
    return `导入中心库 ${skillCount} 个 Skill，并为 ${targetCount} 个 Agent ${scopeText} 创建软链接。`;
  }
  return method === "copy"
    ? `复制 ${skillCount} 个 Skill ${scopeText}的 ${targetCount} 个 Agent 技能目录`
    : `为 ${skillCount} 个 Skill ${scopeText}的 ${targetCount} 个 Agent 创建软链接`;
}

export function targetPathPreview(agent: AgentRecord, scope: "global" | "project", projectPath: string | null) {
  if (scope === "project") {
    const root = agent.projectRoots[0];
    if (!projectPath || !root) return undefined;
    return joinPath(projectPath, root);
  }
  return agent.globalRoots[0];
}

export function joinPath(base: string, relative: string) {
  return `${base.replace(/\/+$/, "")}/${relative.replace(/^\/+/, "")}`;
}

export function projectDisplayName(path: string) {
  const clean = path.replace(/\/+$/, "");
  return clean.split("/").pop() || clean;
}

export function applyResultSummary(
  plan: SyncPlan,
  summary: ReturnType<typeof syncPlanSummary>,
  skillCount: number,
  applyResult: ApplyResult
) {
  if (applyResult.errors.length > 0) {
    return `失败 ${applyResult.errors.length} 项，已停止后续操作`;
  }
  const preview = planSummarySentence(plan, summary, skillCount);
  return preview.startsWith(`${skillCount} 个 Skills：将`)
    ? preview.replace(`${skillCount} 个 Skills：将`, `${skillCount} 个 Skills：已`)
    : preview.startsWith("将")
    ? preview.replace("将", "已")
    : preview;
}

export function getOperationPreview(
  skillCount: number,
  targetCount: number,
  scope: "global" | "project",
  isGenerated: boolean,
  quickMethod: QuickMigrationMethod,
  syncMode: SyncMode
): string {
  const dir = scope === "project" ? "项目目录" : "全局目录";

  if (syncMode === "managed") {
    return isGenerated
      ? `导入中心库并用软链接分发 ${skillCount} 个 Skill 到 ${targetCount} 个 Agent 的${dir}`
      : `导入中心库后，用软链接分发 ${skillCount} 个 Skill 到 ${targetCount} 个 Agent 的${dir}`;
  }

  if (quickMethod === "symlink") {
    return isGenerated
      ? `为 ${skillCount} 个 Skill 在 ${targetCount} 个 Agent 的${dir} 创建软链接`
      : `将为 ${skillCount} 个 Skill 在 ${targetCount} 个 Agent 的${dir} 创建软链接`;
  }

  return isGenerated
    ? `复制 ${skillCount} 个 Skill 到 ${targetCount} 个 Agent 的${dir}`
    : `将复制 ${skillCount} 个 Skill 到 ${targetCount} 个 Agent 的${dir}`;
}

export function planSummarySentence(plan: SyncPlan, summary: ReturnType<typeof syncPlanSummary> | null, skillCount: number) {
  if (!summary) return "同步预览已生成。";
  const prefix = skillCount > 1 ? `${skillCount} 个 Skills：` : "";
  if (plan.blockedConflicts.length > 0) {
    if (summary.contentConflict > 0 && summary.invalidEntry === 0) {
      return `${prefix}发现 ${summary.contentConflict} 个内容冲突，需处理后再执行`;
    }
    if (summary.invalidEntry > 0 && summary.contentConflict === 0) {
      return `${prefix}发现 ${summary.invalidEntry} 个无效入口，需处理后再执行`;
    }
    return `${prefix}发现 ${plan.blockedConflicts.length} 个问题，需处理后再执行`;
  }
  const { actionParts, stateParts } = summaryParts(summary);
  if (actionParts.length > 0 && stateParts.length > 0) {
    return `${prefix}将${actionParts.join("，")}，${stateParts.join("，")}`;
  }
  if (actionParts.length > 0) {
    return `${prefix}将${actionParts.join("，")}`;
  }
  return `${prefix}${stateParts.join("，") || "无需变更"}`;
}

function summaryParts(summary: ReturnType<typeof syncPlanSummary>) {
  const actionParts = [];
  const stateParts = [];
  if (summary.createRoot > 0) actionParts.push(`创建 ${summary.createRoot} 个 Skills 目录`);
  if (summary.importLibrary > 0) actionParts.push(`导入中心库 ${summary.importLibrary} 个 Skill`);
  if (summary.repair > 0) actionParts.push(`修复 ${summary.repair} 个失效链接`);
  if (summary.symlink > 0) actionParts.push(`新增 ${summary.symlink} 个软链接`);
  if (summary.copy > 0) actionParts.push(`复制 ${summary.copy} 个 Skill 副本`);
  if (summary.replace > 0) actionParts.push(`备份后替换 ${summary.replace} 个同名 Skill`);
  if (summary.sameContent > 0) stateParts.push(`${summary.sameContent} 个已有相同内容`);
  if (summary.noop > 0) stateParts.push(`${summary.noop} 个无需变更`);
  return { actionParts, stateParts };
}
