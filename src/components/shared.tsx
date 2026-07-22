import { AlertTriangle } from "lucide-react";
import type { CSSProperties } from "react";
import { agentIconAsset } from "../agentIconRegistry";
import { issueHint, issueLabel, normalizeLanguage, type Language } from "../i18n";
import type { AgentRecord, SkillIssue, SkillRecord } from "../types";

export function AgentIcon({ agent }: { agent: AgentRecord }) {
  const icon = agentIconAsset(agent.id);
  const fallback = agent.label.slice(0, 2).toUpperCase();
  const iconStyle = icon?.size ? ({ "--agent-icon-size": `${icon.size}px` } as CSSProperties) : undefined;

  return (
    <span className={`agent-icon ${agent.installed ? "installed" : ""}`}>
      {icon ? (
        <img alt="" aria-hidden="true" src={icon.src} style={iconStyle} />
      ) : (
        <em>{fallback}</em>
      )}
    </span>
  );
}

export function StatusPill({ status }: { status: string }) {
  const label = status === "installed" ? "已安装" : "未安装";
  return <span className={`status-pill ${status}`}>{label}</span>;
}

export function SkillState({ skill }: { skill: SkillRecord }) {
  if (skill.conflict) return <span className="status-pill residual">冲突</span>;
  if (skill.issues.length > 0) return <span className="status-pill residual">需检查</span>;
  if (skill.canonicalStatus === "imported") return <span className="status-pill installed">已导入</span>;
  return <span className="status-pill not-installed">外部</span>;
}

export function Coverage({ skill }: { skill: SkillRecord }) {
  const total = skill.installations.length + skill.missingAgents.length;
  const percent = total === 0 ? 0 : Math.round((skill.installations.length / total) * 100);
  return (
    <span className="coverage">
      <i style={{ width: `${percent}%` }} />
      <em>{skill.installations.length}/{total}</em>
    </span>
  );
}

export function InfoBlock({ label, value }: { label: string; value: string }) {
  return (
    <div className="info-block">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

export function AgentBadge({ label, status }: { label: string; status: string }) {
  return <span className={`agent-badge ${status}`}>{label}</span>;
}

export function IssueList({
  issues,
  language = "zh-CN"
}: {
  issues: SkillIssue[];
  language?: string;
}) {
  const lang = normalizeLanguage(language);
  const uniqueIssues = dedupeIssues(issues);

  return (
    <div className="issue-list">
      {uniqueIssues.map((issue, index) => (
        <div className={`issue ${issue.severity}`} key={`${issue.code}-${issue.path ?? ""}-${issue.agentId ?? ""}-${index}`}>
          <AlertTriangle size={14} />
          <span>
            <strong>{issueLabel(lang, issue.code, issue.message)}</strong>
            <small>{issueActionHint(issue, lang)}</small>
          </span>
        </div>
      ))}
    </div>
  );
}

export function issueActionHint(issue: SkillIssue, language: Language = "zh-CN") {
  return issueHint(language, issue.code, issue.path);
}

function dedupeIssues(issues: SkillIssue[]) {
  const seen = new Set<string>();
  return issues.filter((issue) => {
    const key = [issue.code, issue.severity, issue.path ?? "", issue.agentId ?? ""].join("\u0000");
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}
