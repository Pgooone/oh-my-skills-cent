import type { AgentRecord, SyncOperation, SyncPlan, SyncReplacement } from "../../types";

export type PlanDetail = {
  kind: "blocked" | "attention";
  label: string;
  skillId: string;
  agentLabel: string;
  /** Second-line outcome; does not repeat agent. */
  summary: string;
  path?: string;
  operation?: SyncOperation;
  canIncludeReplacement?: boolean;
};

export function buildPlanDetails(plan: SyncPlan, agents: AgentRecord[]): PlanDetail[] {
  const agentLabels = new Map(agents.map((agent) => [agent.id, agent.label]));
  const canIncludeReplacement = plan.kind.includes("sync");
  const details = plan.operations.flatMap((operation): PlanDetail[] => {
    const agentLabel = operation.agentId ? agentLabels.get(operation.agentId) ?? operation.agentId : "目标";
    const skillId = operation.skillId ?? "Skill";
    if (operation.opType === "content-conflict") {
      return [{
        kind: "blocked",
        label: "内容冲突",
        skillId,
        agentLabel,
        summary: "已有同名且内容不同，本次不会执行",
        path: operation.targetPath,
        operation
      }];
    }
    if (operation.opType === "invalid-entry") {
      return [{
        kind: "blocked",
        label: "无效入口",
        skillId,
        agentLabel,
        summary: "入口无效或不可读取，本次不会执行",
        path: operation.targetPath,
        operation
      }];
    }
    if (operation.opType === "same-content-existing") {
      return [{
        kind: "attention",
        label: "已有相同内容",
        skillId,
        agentLabel,
        summary: "已保留原入口",
        path: operation.targetPath,
        operation,
        canIncludeReplacement
      }];
    }
    if (operation.opType === "backup-existing") {
      return [{
        kind: "attention",
        label: "备份后替换",
        skillId,
        agentLabel,
        summary: "将备份后替换为中心库软链接",
        path: operation.targetPath,
        operation
      }];
    }
    if (operation.opType === "remove-existing") {
      return [{
        kind: "attention",
        label: "修复失效链接",
        skillId,
        agentLabel,
        summary: "将移除旧入口并重新创建",
        path: operation.targetPath,
        operation
      }];
    }
    if (operation.opType === "create-root") {
      return [{
        kind: "attention",
        label: "创建目录",
        skillId,
        agentLabel,
        summary: "将先创建 Skills 目录",
        path: operation.targetPath,
        operation
      }];
    }
    return [];
  });
  const hasBlockedDetail = details.some((item) => item.kind === "blocked");
  if (plan.blockedConflicts.length > 0 && !hasBlockedDetail) {
    return [
      ...details,
      ...plan.blockedConflicts.map((message, index) => {
        const parsed = parseBlockedConflict(message, index);
        return {
          kind: "blocked" as const,
          label: "不可执行",
          skillId: parsed.skillId,
          agentLabel: "目标",
          summary: parsed.summary
        };
      })
    ];
  }
  return details;
}

function parseBlockedConflict(message: string, index: number): { skillId: string; summary: string } {
  const notImported = message.match(/^([\w.@/+\-]+)\s+is not imported into the central library yet\.?$/i);
  if (notImported) {
    return {
      skillId: notImported[1],
      summary: "尚未导入中心库，本次不会执行"
    };
  }
  return {
    skillId: `问题 ${index + 1}`,
    summary: message.trim() || "本次不会执行，请先处理这个问题"
  };
}

export function detailPrimaryLine(item: PlanDetail): string {
  if (item.agentLabel === "目标") {
    return `${item.label} · ${item.skillId}`;
  }
  return `${item.label} · ${item.skillId} → ${item.agentLabel}`;
}

export function replacementKey(agentId: string, skillId: string, targetPath: string) {
  return [agentId, skillId, targetPath].join("\u0000");
}

export function replacementFromKey(key: string): SyncReplacement {
  const [agentId, skillId, targetPath] = key.split("\u0000");
  return { agentId, skillId, targetPath };
}
