import { AlertTriangle, Check, Copy, FileText, FolderPlus, Globe2, Link2, Package, X } from "lucide-react";
import { useEffect, useState } from "react";
import { callApi } from "../../lib/api";
import { pickDirectory } from "../../lib/shell";
import { agentSignalSummary, compactPath, syncPlanSummary } from "../../lib/skillUtils";
import type { AgentRecord, AgentTarget, ApplyResult, InstalledWorkflow, OutputForm, SyncPlan } from "../../types";
import { PlanInfoDisclosure } from "../../views/sync/PlanDetailPanel";
import { SyncSection } from "../../views/sync/SyncSection";
import { applyResultSummary, planSummarySentence, projectDisplayName } from "../../views/sync/syncCopy";
import { buildPlanDetails } from "../../views/sync/syncPlanDetails";
import { AgentIcon } from "../shared";

/**
 * 使用工作流：目标 Agent + 生效范围 + 输出形态二选一 → preview_use_workflow 生成
 * SyncPlan → 复用 sync 预览组件 → 既有 apply_sync_plan 执行。
 */
export function UseWorkflowSheet({
  workflow,
  agents,
  onClose,
  onApplied
}: {
  workflow: InstalledWorkflow;
  agents: AgentRecord[];
  onClose: () => void;
  /** 执行完成后通知父级刷新（详情状态可能因下载补齐而变化）。 */
  onApplied: (result: ApplyResult) => void;
}) {
  const [selectedTargetIds, setSelectedTargetIds] = useState<Set<string>>(() => new Set());
  const [targetScope, setTargetScope] = useState<"global" | "project">("global");
  const [projectPath, setProjectPath] = useState<string | null>(null);
  const [outputForm, setOutputForm] = useState<OutputForm>("entryManifest");
  const [method, setMethod] = useState<"copy" | "symlink">("symlink");
  const [plan, setPlan] = useState<SyncPlan | null>(null);
  const [previewKey, setPreviewKey] = useState<string | null>(null);
  const [applyResult, setApplyResult] = useState<ApplyResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    const onEsc = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onEsc);
    return () => document.removeEventListener("keydown", onEsc);
  }, [onClose]);

  const selectedTargets = agents.filter((agent) => selectedTargetIds.has(agent.id));
  const targets: AgentTarget[] = selectedTargets.map((agent) => ({
    agentId: agent.id,
    scope: targetScope,
    projectPath: targetScope === "project" ? projectPath ?? undefined : undefined
  }));
  const draftKey = [
    workflow.slug,
    outputForm,
    method,
    targetScope,
    projectPath ?? "",
    selectedTargets.map((agent) => agent.id).sort().join("|")
  ].join("::");
  const stalePlan = Boolean(plan) && previewKey !== draftKey;
  const activePlan = stalePlan ? null : plan;
  const blocked = Boolean(activePlan?.blockedConflicts.length);
  const summary = activePlan ? syncPlanSummary(activePlan) : null;
  const planDetails = activePlan ? buildPlanDetails(activePlan, agents) : null;
  const missingProject = targetScope === "project" && !projectPath;
  const canPreview = selectedTargets.length > 0 && !missingProject && !busy;

  function toggleTarget(agentId: string) {
    setSelectedTargetIds((current) => {
      const next = new Set(current);
      if (next.has(agentId)) next.delete(agentId);
      else next.add(agentId);
      return next;
    });
  }

  async function chooseProject() {
    const selected = await pickDirectory("选择要同步的项目");
    if (typeof selected !== "string") return;
    setProjectPath(selected);
    setTargetScope("project");
  }

  async function preview() {
    if (!canPreview) return;
    setBusy(true);
    setError(null);
    setApplyResult(null);
    try {
      const next = await callApi<SyncPlan>("preview_use_workflow", {
        slug: workflow.slug,
        targets,
        method,
        outputForm
      });
      setPlan(next);
      setPreviewKey(draftKey);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(false);
    }
  }

  async function apply() {
    if (!activePlan || busy) return;
    setBusy(true);
    setError(null);
    try {
      const result = await callApi<ApplyResult>("apply_sync_plan", { planId: activePlan.planId });
      setApplyResult(result);
      onApplied(result);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="sheet-backdrop" onClick={onClose}>
      <aside
        aria-label={`使用工作流 ${workflow.name}`}
        aria-modal="true"
        className="settings-sheet"
        onClick={(event) => event.stopPropagation()}
        role="dialog"
      >
        <header className="settings-header">
          <div className="settings-header-top">
            <h1>使用 · {workflow.name}</h1>
            <button className="settings-close" onClick={onClose} title="关闭" type="button">
              <X size={16} />
            </button>
          </div>
        </header>

        <div className="settings-content">
          {error && (
            <div className="banner warning" role="alert">
              {error}
            </div>
          )}

          <SyncSection number="1" title="目标 Agent" titleHint="（可多选）">
            <div className="selected-target-row">
              {agents.length === 0 ? (
                <span className="target-helper">未检测到已安装的 Agent。</span>
              ) : (
                agents.map((agent) => {
                  const selected = selectedTargetIds.has(agent.id);
                  return (
                    <button
                      aria-pressed={selected}
                      className={`selected-target-card ${selected ? "active" : ""}`}
                      key={agent.id}
                      onClick={() => toggleTarget(agent.id)}
                      type="button"
                    >
                      <AgentIcon agent={agent} />
                      <span className="target-card-main">
                        <strong>{agent.label}</strong>
                        <small>{agentSignalSummary(agent) || "Agent"}</small>
                      </span>
                      <span className={`target-card-check ${selected ? "" : "idle"}`} aria-hidden="true">
                        {selected ? <Check size={14} /> : null}
                      </span>
                    </button>
                  );
                })
              )}
            </div>
          </SyncSection>

          <SyncSection number="2" title="生效范围">
            <div className="option-grid two">
              <button
                className={`choice-card ${targetScope === "global" ? "active" : ""}`}
                onClick={() => setTargetScope("global")}
                type="button"
              >
                <Globe2 size={21} />
                <span>
                  <strong>全局</strong>
                  <small>写入各 Agent 的全局 Skills 目录</small>
                </span>
              </button>
              <button
                className={`choice-card ${targetScope === "project" ? "active" : ""}`}
                onClick={() => void chooseProject()}
                type="button"
              >
                <FolderPlus size={21} />
                <span>
                  <strong>项目</strong>
                  <small>{projectPath ? compactPath(projectPath) : "选择本地项目并写入"}</small>
                </span>
              </button>
            </div>
            {targetScope === "project" && (
              <div className={`project-target-note ${projectPath ? "" : "empty"}`}>
                <span>
                  <strong>{projectPath ? projectDisplayName(projectPath) : "未选择项目"}</strong>
                  <small title={projectPath ?? ""}>
                    {projectPath ? compactPath(projectPath) : "点击“项目”选择一个本地项目"}
                  </small>
                </span>
                <button className="secondary-button compact" onClick={() => void chooseProject()} type="button">
                  {projectPath ? "更换" : "选择"}
                </button>
              </div>
            )}
          </SyncSection>

          <SyncSection number="3" title="输出形态">
            <div className="option-grid two">
              <button
                className={`choice-card ${outputForm === "entryManifest" ? "active" : ""}`}
                onClick={() => setOutputForm("entryManifest")}
                type="button"
              >
                <FileText size={20} />
                <span>
                  <strong>入口清单</strong>
                  <small>各 Skill 独立安装，另写 workflow.yaml + README 指引</small>
                </span>
              </button>
              <button
                className={`choice-card ${outputForm === "packagedSkill" ? "active" : ""}`}
                onClick={() => setOutputForm("packagedSkill")}
                type="button"
              >
                <Package size={20} />
                <span>
                  <strong>打包 Skill</strong>
                  <small>单个编排 SKILL.md + skills/ 自包含拷贝</small>
                </span>
              </button>
            </div>
          </SyncSection>

          {outputForm === "entryManifest" && (
            <SyncSection number="4" title="同步方式">
              <div className="option-grid two">
                <button
                  className={`choice-card ${method === "symlink" ? "active" : ""}`}
                  onClick={() => setMethod("symlink")}
                  type="button"
                >
                  <Link2 size={20} />
                  <span>
                    <strong>创建软链接</strong>
                    <small>目标 Agent 链接到中心库副本</small>
                  </span>
                </button>
                <button
                  className={`choice-card ${method === "copy" ? "active" : ""}`}
                  onClick={() => setMethod("copy")}
                  type="button"
                >
                  <Copy size={20} />
                  <span>
                    <strong>复制副本</strong>
                    <small>目标 Agent 拥有独立副本</small>
                  </span>
                </button>
              </div>
            </SyncSection>
          )}

          {activePlan && !applyResult && activePlan.preconditions.length > 0 && (
            <div className="banner warning" role="status">
              <AlertTriangle size={15} />
              <span>{activePlan.preconditions.join("；")}</span>
            </div>
          )}
        </div>

        <footer className="sheet-actions">
          {applyResult ? (
            <div className={`apply-result ${applyResult.errors.length ? "error" : "success"}`} role="status">
              <span>
                {applyResult.errors.length ? "执行完成，但有错误" : "执行完成"}
                {activePlan && summary
                  ? ` · ${applyResultSummary(activePlan, summary, 0, applyResult)}`
                  : ` · ${applyResult.appliedOperations.length} 已执行 · ${applyResult.skippedOperations.length} 已跳过`}
              </span>
              {applyResult.errors.map((item) => (
                <code key={item}>{item}</code>
              ))}
            </div>
          ) : activePlan ? (
            <div className="plan-status-wrap">
              <div className={`plan-status-pill ${blocked ? "blocked" : ""}`}>
                {blocked ? <AlertTriangle size={14} /> : <Check size={14} />}
                <span>{summary ? planSummarySentence(activePlan, summary, 0) : "同步预览已生成。"}</span>
              </div>
              {planDetails && (
                <PlanInfoDisclosure busy={busy} details={planDetails} onIncludeReplacement={() => undefined} />
              )}
            </div>
          ) : null}

          <div className="action-buttons-end">
            {applyResult ? (
              <button className="primary-button large" onClick={onClose} type="button">
                关闭
              </button>
            ) : plan ? (
              <div className="button-pair sync-plan-actions">
                <button
                  className="secondary-button large sync-plan-action"
                  disabled={!canPreview}
                  onClick={() => void preview()}
                  type="button"
                >
                  重新生成预览
                </button>
                <button
                  className="primary-button large sync-plan-action"
                  disabled={!activePlan || blocked || busy}
                  onClick={() => void apply()}
                  type="button"
                >
                  执行计划
                </button>
              </div>
            ) : (
              <button
                className="primary-button large"
                disabled={!canPreview}
                onClick={() => void preview()}
                type="button"
              >
                {selectedTargets.length === 0 ? "先选择目标 Agent" : missingProject ? "先选择项目" : "生成预览"}
              </button>
            )}
          </div>
        </footer>
      </aside>
    </div>
  );
}
