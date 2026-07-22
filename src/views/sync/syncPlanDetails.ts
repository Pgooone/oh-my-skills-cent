import type { AgentRecord, SyncOperation, SyncPlan, SyncReplacement } from "../../types";

export type PlanDetail = {
  kind: "blocked" | "attention";
  title: string;
  body: string;
  label: string;
  skillId: string;
  agentLabel: string;
  path?: string;
  backupPath?: string;
  operation?: SyncOperation;
  canIncludeReplacement?: boolean;
};

export function groupDetailsBySkill(items: PlanDetail[]) {
  const groups = new Map<string, PlanDetail[]>();
  for (const item of items) {
    groups.set(item.skillId, [...(groups.get(item.skillId) ?? []), item]);
  }
  return Array.from(groups.entries());
}

export function buildPlanDetails(plan: SyncPlan, agents: AgentRecord[]): PlanDetail[] {
  const agentLabels = new Map(agents.map((agent) => [agent.id, agent.label]));
  const canIncludeReplacement = plan.kind.includes("sync");
  const details = plan.operations.flatMap((operation): PlanDetail[] => {
    const agentLabel = operation.agentId ? agentLabels.get(operation.agentId) ?? operation.agentId : "目标";
    const skillId = operation.skillId ?? "Skill";
    if (operation.opType === "content-conflict") {
      return [{
        kind: "blocked",
        title: `${agentLabel} 里已有同名 Skill，但内容和来源不同`,
        body: "为避免覆盖你的修改，本次不会执行。",
        label: "内容冲突",
        skillId,
        agentLabel,
        path: operation.targetPath,
        operation
      }];
    }
    if (operation.opType === "invalid-entry") {
      return [{
        kind: "blocked",
        title: `${agentLabel} 的目标入口无效或不可读取`,
        body: "本次不会执行，请先检查目标位置。",
        label: "无效入口",
        skillId,
        agentLabel,
        path: operation.targetPath,
        operation
      }];
    }
    if (operation.opType === "same-content-existing") {
      return [{
        kind: "attention",
        title: `${agentLabel} 里已有同名 Skill，内容相同`,
        body: canIncludeReplacement
          ? "已保留原入口，不会替换为中心库软链接。"
          : "已保留原入口，不会替换为软链接。",
        label: "已有相同内容",
        skillId,
        agentLabel,
        path: operation.targetPath,
        operation,
        canIncludeReplacement
      }];
    }
    if (operation.opType === "backup-existing") {
      return [{
        kind: "attention",
        title: `${agentLabel} 里的同名 Skill 将备份后替换`,
        body: "会先移到 Oh My Skills 的备份目录，再替换为中心库软链接。",
        label: "备份后替换",
        skillId,
        agentLabel,
        path: operation.targetPath,
        backupPath: operation.backupPath,
        operation
      }];
    }
    if (operation.opType === "remove-existing") {
      return [{
        kind: "attention",
        title: `${agentLabel} 里的目标位置是失效软链接`,
        body: "将移除旧入口并重新创建。",
        label: "修复失效链接",
        skillId,
        agentLabel,
        path: operation.targetPath,
        operation
      }];
    }
    if (operation.opType === "create-root") {
      return [{
        kind: "attention",
        title: `${agentLabel} 的目标 Skills 目录不存在`,
        body: "将先创建目录，再同步这个 Skill。",
        label: "创建目录",
        skillId,
        agentLabel,
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
      ...plan.blockedConflicts.map((message, index) => ({
        kind: "blocked" as const,
        title: message,
        body: "本次不会执行，请先处理这个问题。",
        label: "不可执行",
        skillId: `问题 ${index + 1}`,
        agentLabel: "目标"
      }))
    ];
  }
  return details;
}

export function replacementKey(agentId: string, skillId: string, targetPath: string) {
  return [agentId, skillId, targetPath].join("\u0000");
}

export function replacementFromKey(key: string): SyncReplacement {
  const [agentId, skillId, targetPath] = key.split("\u0000");
  return { agentId, skillId, targetPath };
}
