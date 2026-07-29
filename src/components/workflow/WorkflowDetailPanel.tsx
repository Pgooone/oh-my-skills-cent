import { Pencil, Play, Trash2 } from "lucide-react";
import type { ReactNode } from "react";
import type { StepSkillStatus, Workflow, WorkflowDetailStep } from "../../types";

/**
 * 工作流详情：分组 → 步骤 → 每步 skill 状态徽标（可用/将下载/占位）。
 * 复用 SkillsView 的 skill-detail / detail-field 视觉，不新增样式。
 */
export function WorkflowDetailPanel({
  workflow,
  steps,
  busy,
  onUse,
  onEdit,
  onDelete
}: {
  workflow: Workflow;
  /** 与 workflow.steps 对齐的归一化视图（含每 skill 状态）。 */
  steps: WorkflowDetailStep[];
  busy: boolean;
  onUse: () => void;
  onEdit: () => void;
  onDelete: () => void;
}) {
  const metaParts = [
    `v${workflow.version}`,
    workflow.author ?? "",
    ...workflow.tags
  ].filter(Boolean);

  return (
    <div className="skill-detail">
      <DetailField label="操作">
        <div className="button-pair">
          <button className="primary-button" disabled={busy} onClick={onUse} type="button">
            <Play size={15} />
            使用工作流
          </button>
          <button className="secondary-button" disabled={busy} onClick={onEdit} type="button">
            <Pencil size={15} />
            编辑
          </button>
          <button className="secondary-button" disabled={busy} onClick={onDelete} type="button">
            <Trash2 size={15} />
            删除
          </button>
        </div>
      </DetailField>

      <DetailField label="信息">
        <p>{metaParts.join(" · ") || workflow.slug}</p>
      </DetailField>

      {workflow.description && (
        <DetailField label="描述">
          <p>{workflow.description}</p>
        </DetailField>
      )}

      {workflow.groups.length === 0 && steps.length === 0 && (
        <DetailField label="步骤">
          <p>尚未定义分组与步骤。</p>
        </DetailField>
      )}

      {workflow.groups.map((group) => {
        const groupSteps = steps
          .map((step, index) => ({ step, index }))
          .filter(({ step }) => step.group === group.id);
        return (
          <DetailField label={group.name} key={group.id}>
            {groupSteps.length === 0 ? (
              <p>此分组暂无步骤。</p>
            ) : (
              <div className="issue-list">
                {groupSteps.map(({ step, index }) => (
                  <StepBlock index={index} key={`${step.name}-${index}`} step={step} />
                ))}
              </div>
            )}
          </DetailField>
        );
      })}
    </div>
  );
}

function StepBlock({ index, step }: { index: number; step: WorkflowDetailStep }) {
  return (
    <div>
      <p>
        <strong>
          {index + 1}. {step.name}
        </strong>
        {step.description ? ` — ${step.description}` : ""}
      </p>
      <div className="detail-path-list">
        {step.skills.map((skill, skillIndex) => (
          <div className="detail-path-row" key={`${skill.slug ?? skill.placeholder ?? "skill"}-${skillIndex}`}>
            <StepSkillStatusPill status={skill.status} />
            <code title={skill.sourceUrl ?? skill.placeholder ?? skill.slug}>
              {skill.kind === "placeholder"
                ? skill.placeholder ?? "占位"
                : skill.slug ?? "未命名 Skill"}
            </code>
          </div>
        ))}
        {step.skills.length === 0 && <p>此步骤没有关联 Skill。</p>}
      </div>
    </div>
  );
}

function StepSkillStatusPill({ status }: { status: StepSkillStatus }) {
  if (status === "ready") {
    return <span className="status-pill installed" title="中心库已有此 Skill">可用</span>;
  }
  if (status === "missing") {
    return <span className="status-pill not-installed" title="中心库没有，使用时会先下载">将下载</span>;
  }
  return (
    <span className="status-pill residual" title={status.placeholder}>
      占位
    </span>
  );
}

function DetailField({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="detail-field">
      <span>{label}</span>
      <div>{children}</div>
    </div>
  );
}
