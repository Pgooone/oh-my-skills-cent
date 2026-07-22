import { AlertTriangle, Check, ChevronLeft, ChevronRight, Copy, FolderPlus, Globe2, Link2, Plus, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { AgentIcon } from "../components/shared";
import { agentSignalSummary, compactPath, firstValidInstallation, syncPlanSummary } from "../lib/skillUtils";
import type { AgentRecord, AgentTarget, ApplyResult, Settings as AppSettings, SkillRecord, SyncOperation, SyncPlan, SyncReplacement } from "../types";
import type { QuickMigrationMethod, SyncMode } from "../uiTypes";
import { PlanInfoDisclosure } from "./sync/PlanDetailPanel";
import { SyncSection } from "./sync/SyncSection";
import {
  applyResultSummary,
  draftPlanSentence,
  getOperationPreview,
  planSummarySentence,
  projectDisplayName,
  targetPathPreview
} from "./sync/syncCopy";
import { buildPlanDetails, replacementFromKey, replacementKey } from "./sync/syncPlanDetails";

export function SyncView({
  agents,
  queuedSkills,
  settings,
  plan,
  applyResult,
  busy,
  onRemoveSkill,
  onPreviewGlobal,
  onPreviewProject,
  onPreviewQuick,
  onApply,
  onGoSkills,
  onChooseProject,
  syncMode,
  onSyncModeChange,
  selectedTargetIds,
  onSelectedTargetIdsChange
}: {
  agents: AgentRecord[];
  queuedSkills: SkillRecord[];
  settings: AppSettings;
  plan: SyncPlan | null;
  applyResult: ApplyResult | null;
  busy: boolean;
  onRemoveSkill: (id: string) => void;
  onPreviewGlobal: (targets: AgentTarget[], replacements: SyncReplacement[]) => void;
  onPreviewProject: (targets: AgentTarget[], replacements: SyncReplacement[]) => void;
  onPreviewQuick: (method: QuickMigrationMethod, targets: AgentTarget[]) => void;
  onApply: () => void;
  onGoSkills: () => void;
  onChooseProject: () => Promise<string | null>;
  syncMode: SyncMode;
  onSyncModeChange: (mode: SyncMode) => void;
  /** Lifted to App so tab switches do not clear agent picks until app restart. */
  selectedTargetIds: Set<string>;
  onSelectedTargetIdsChange: (ids: Set<string>) => void;
}) {
  const [quickMethod, setQuickMethod] = useState<QuickMigrationMethod>("copy");
  const [targetScope, setTargetScope] = useState<"global" | "project">("global");
  const [selectedProjectPath, setSelectedProjectPath] = useState<string | null>(null);
  const [selectedReplacementKeys, setSelectedReplacementKeys] = useState<Set<string>>(() => new Set());
  const [selectedSkillScrollState, setSelectedSkillScrollState] = useState({ left: false, right: false });
  const [previewDraftKey, setPreviewDraftKey] = useState<string | null>(null);
  const selectedSkillBarRef = useRef<HTMLDivElement>(null);
  const selectedSkill = queuedSkills[0] ?? null;
  const selectedSkillCount = queuedSkills.length;

  useEffect(() => {
    const validIds = new Set(agents.map((agent) => agent.id));
    const next = new Set([...selectedTargetIds].filter((id) => validIds.has(id)));
    if (next.size !== selectedTargetIds.size) {
      onSelectedTargetIdsChange(next);
    }
  }, [agents, selectedTargetIds, onSelectedTargetIdsChange]);

  const updateSelectedSkillScrollState = () => {
    const element = selectedSkillBarRef.current;
    if (!element) {
      setSelectedSkillScrollState({ left: false, right: false });
      return;
    }

    const maxScroll = element.scrollWidth - element.clientWidth;
    setSelectedSkillScrollState({
      left: element.scrollLeft > 2,
      right: element.scrollLeft < maxScroll - 2
    });
  };

  useEffect(() => {
    window.requestAnimationFrame(updateSelectedSkillScrollState);
  }, [queuedSkills.length]);

  useEffect(() => {
    window.addEventListener("resize", updateSelectedSkillScrollState);
    return () => window.removeEventListener("resize", updateSelectedSkillScrollState);
  }, []);

  const selectedTargets = agents.filter((agent) => selectedTargetIds.has(agent.id));
  const targets = selectedTargets.map((agent) => ({
    agentId: agent.id,
    scope: targetScope,
    projectPath: targetScope === "project" ? selectedProjectPath ?? undefined : undefined
  }));
  const draftKey = [
    syncMode,
    quickMethod,
    targetScope,
    selectedProjectPath ?? "",
    queuedSkills.map((skill) => skill.id).sort().join("|"),
    selectedTargets.map((agent) => agent.id).sort().join("|"),
    [...selectedReplacementKeys].sort().join("|")
  ].join("::");
  const generatedPlan = Boolean(plan);
  const stalePlan = generatedPlan && previewDraftKey !== draftKey;
  const activePlan = stalePlan ? null : plan;
  const blocked = Boolean(activePlan?.blockedConflicts.length);
  const summary = activePlan ? syncPlanSummary(activePlan) : null;
  const missingProject = targetScope === "project" && !selectedProjectPath;
  const actionDisabled = selectedSkillCount === 0 || selectedTargets.length === 0 || missingProject || busy;
  const previewLabel = selectedSkillCount === 0
    ? "先选择 Skill 再生成预览"
    : missingProject
    ? "先选择项目"
    : "生成同步预览";
  const centralPath = selectedSkillCount === 1 && selectedSkill ? `${settings.libraryPath}/${selectedSkill.slug}` : settings.libraryPath;
  const confirmationText = activePlan
    ? planSummarySentence(activePlan, summary, selectedSkillCount)
    : draftPlanSentence(syncMode, quickMethod, selectedSkillCount, selectedTargets.length, targetScope, selectedProjectPath);
  const planDetails = activePlan ? buildPlanDetails(activePlan, agents) : null;
  const canShowBottomPreview =
    selectedSkillCount > 0 && selectedTargets.length > 0 && !missingProject;
  const bottomPreviewText = canShowBottomPreview
    ? getOperationPreview(
        selectedSkillCount,
        selectedTargets.length,
        targetScope,
        Boolean(activePlan),
        quickMethod,
        syncMode
      )
    : null;

  function toggleTarget(agentId: string) {
    const next = new Set(selectedTargetIds);
    if (next.has(agentId)) next.delete(agentId);
    else next.add(agentId);
    onSelectedTargetIdsChange(next);
  }

  async function chooseProjectScope() {
    const projectPath = await onChooseProject();
    if (!projectPath) return;
    setSelectedProjectPath(projectPath);
    setTargetScope("project");
  }

  function previewPlan(replacementKeys = selectedReplacementKeys) {
    if (missingProject) return;
    const nextDraftKey = [
      syncMode,
      quickMethod,
      targetScope,
      selectedProjectPath ?? "",
      queuedSkills.map((skill) => skill.id).sort().join("|"),
      selectedTargets.map((agent) => agent.id).sort().join("|"),
      [...replacementKeys].sort().join("|")
    ].join("::");
    const replacements = [...replacementKeys].map(replacementFromKey);
    setPreviewDraftKey(nextDraftKey);
    if (syncMode === "quick") {
      onPreviewQuick(quickMethod, targets);
    } else if (targetScope === "project") {
      onPreviewProject(targets, replacements);
    } else {
      onPreviewGlobal(targets, replacements);
    }
  }

  function includeReplacement(operation: SyncOperation) {
    if (!operation.agentId || !operation.skillId || !operation.targetPath) return;
    const next = new Set(selectedReplacementKeys);
    next.add(replacementKey(operation.agentId, operation.skillId, operation.targetPath));
    setSelectedReplacementKeys(next);
    previewPlan(next);
  }

  function scrollSelectedSkillBar(direction: "left" | "right") {
    const element = selectedSkillBarRef.current;
    if (!element) return;
    element.scrollBy({
      left: direction === "left" ? -520 : 520,
      behavior: "smooth"
    });
    window.setTimeout(updateSelectedSkillScrollState, 260);
  }

  return (
    <div className="sync-page">
      <section className="sync-main-pane">
        <div className="sync-mode-header">
          <div className="sync-toolbar">
            <div className="scope-tabs sync-mode-tabs" role="tablist" aria-label="同步模式">
              <button
                className={syncMode === "quick" ? "active" : ""}
                onClick={() => onSyncModeChange("quick")}
                role="tab"
                type="button"
                aria-selected={syncMode === "quick"}
              >
                快速同步
              </button>
              <button
                className={syncMode === "managed" ? "active" : ""}
                onClick={() => onSyncModeChange("managed")}
                role="tab"
                type="button"
                aria-selected={syncMode === "managed"}
              >
                中心库同步
              </button>
            </div>
          </div>
          <div className="sync-mode-desc">
            {syncMode === "quick" ? (
              <>
                <span className="mode-tag">最快完成</span>
                直接复制或创建软链接到目标 Agent，不使用中心库。有冲突的内容会在预览中被拦住，不会直接覆盖。
              </>
            ) : (
              <>
                <span className="mode-tag">长期管理</span>
                先复制到中心库，再用软链接分发到目标 Agent。有冲突的内容会在预览中被拦住，不会直接覆盖。
              </>
            )}
          </div>
        </div>

        <div className="sync-work-grid">
          <section className="sync-form-pane">
            <SyncSection
              number="1"
              title="已选 Skill"
              action={(
                <button className="sync-section-icon-action" onClick={onGoSkills} title="选择 Skill" type="button">
                  <Plus size={16} />
                </button>
              )}
            >
              <div className={`selected-skill-shell ${queuedSkills.length === 0 ? "empty" : ""}`}>
                {selectedSkillScrollState.left && (
                  <button
                    className="selected-skill-scroll left"
                    onClick={() => scrollSelectedSkillBar("left")}
                    title="向左滑动"
                    type="button"
                  >
                    <ChevronLeft size={16} />
                  </button>
                )}
                <div
                  className="selected-skill-list"
                  onScroll={updateSelectedSkillScrollState}
                  ref={selectedSkillBarRef}
                >
                  {queuedSkills.map((skill) => {
                    const selectedSource = firstValidInstallation(skill);
                    const sourcePath = selectedSource?.entryPath ?? skill.canonicalPath ?? "";
                    const selectionKey = skill.selectionKey ?? skill.id;
                    return (
                      <div className="selected-skill-card" key={selectionKey}>
                        <span>
                          <strong>{skill.displayName}</strong>
                          <small title={sourcePath || skill.slug}>
                            {sourcePath ? compactPath(sourcePath) : skill.slug}
                          </small>
                        </span>
                        <button className="selected-skill-remove" onClick={() => onRemoveSkill(selectionKey)} title="取消选择" type="button">
                          <X size={14} />
                        </button>
                      </div>
                    );
                  })}
                  {queuedSkills.length === 0 && <span className="selected-skill-empty">请至少选择 1 个 Skill。</span>}
                </div>
                {selectedSkillScrollState.right && (
                  <button
                    className="selected-skill-scroll right"
                    onClick={() => scrollSelectedSkillBar("right")}
                    title="向右滑动"
                    type="button"
                  >
                    <ChevronRight size={16} />
                  </button>
                )}
              </div>
            </SyncSection>

            {syncMode === "quick" ? (
              <SyncSection number="2" title="同步方式">
                <div className="option-grid two">
                  <button className={`choice-card ${quickMethod === "copy" ? "active" : ""}`} onClick={() => setQuickMethod("copy")} type="button">
                    <Copy size={20} />
                    <span>
                      <strong>复制副本</strong>
                      <small>复制后目标 Agent 拥有独立副本</small>
                    </span>
                  </button>
                  <button className={`choice-card ${quickMethod === "symlink" ? "active" : ""}`} onClick={() => setQuickMethod("symlink")} type="button">
                    <Link2 size={20} />
                    <span>
                      <strong>创建软链接</strong>
                      <small>在目标 Agent 中创建软链接，指向原位置</small>
                    </span>
                  </button>
                </div>
              </SyncSection>
            ) : (
              <SyncSection number="2" title="中心库副本">
                <div className="managed-library-card active">
                  <Link2 size={20} />
                  <span>
                    <strong>先复制到中心库，再用软链接分发到目标 Agent</strong>
                    <code title={centralPath}>{centralPath ? compactPath(centralPath) : "等待选择 Skill"}</code>
                  </span>
                </div>
              </SyncSection>
            )}

            <SyncSection number="3" title="目标 Agent" titleHint="（可多选）">
              <div className="selected-target-row">
                {agents.length === 0 ? (
                  <span className="target-helper">未检测到已安装的 Agent。</span>
                ) : (
                  agents.map((agent) => {
                    const selected = selectedTargetIds.has(agent.id);
                    const pathPreview = targetPathPreview(agent, targetScope, selectedProjectPath);
                    const signal = agentSignalSummary(agent) || "Agent";
                    return (
                      <button
                        aria-pressed={selected}
                        className={`selected-target-card ${selected ? "active" : ""}`}
                        key={agent.id}
                        onClick={() => toggleTarget(agent.id)}
                        title={pathPreview ? compactPath(pathPreview) : selected ? "取消选择" : "选择目标"}
                        type="button"
                      >
                        <AgentIcon agent={agent} />
                        <span className="target-card-main">
                          <strong>{agent.label}</strong>
                          <small>{signal}</small>
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

            <SyncSection number="4" title="生效范围">
              <div className="option-grid two">
                <button className={`choice-card ${targetScope === "global" ? "active" : ""}`} onClick={() => setTargetScope("global")} type="button">
                  <Globe2 size={21} />
                  <span>
                    <strong>全局</strong>
                    <small>同步到各 Agent 的全局 Skills 目录</small>
                  </span>
                </button>
                <button className={`choice-card ${targetScope === "project" ? "active" : ""}`} onClick={() => void chooseProjectScope()} type="button">
                  <FolderPlus size={21} />
                  <span>
                    <strong>项目</strong>
                    <small>{selectedProjectPath ? compactPath(selectedProjectPath) : "选择本地项目并同步进去"}</small>
                  </span>
                </button>
              </div>
              {targetScope === "project" && (
                <div className={`project-target-note ${selectedProjectPath ? "" : "empty"}`}>
                  <span>
                    <strong>{selectedProjectPath ? projectDisplayName(selectedProjectPath) : "未选择项目"}</strong>
                    <small title={selectedProjectPath ?? ""}>{selectedProjectPath ? compactPath(selectedProjectPath) : "点击“项目”选择一个本地项目"}</small>
                  </span>
                  <button className="secondary-button compact" onClick={() => void chooseProjectScope()} type="button">
                    {selectedProjectPath ? "更换" : "选择"}
                  </button>
                </div>
              )}
            </SyncSection>
          </section>
        </div>

        <div className="sync-action-bar">
          {applyResult ? (
            <div className={`apply-result ${applyResult.errors.length ? "error" : "success"}`} role="status">
              <span>
                {applyResult.errors.length ? "执行完成，但有错误" : "执行完成"} ·{" "}
                {activePlan && summary
                  ? applyResultSummary(activePlan, summary, selectedSkillCount, applyResult)
                  : `${applyResult.appliedOperations.length} 已执行 · ${applyResult.skippedOperations.length} 已跳过`}
              </span>
              {applyResult.errors.map((item) => (
                <code key={item}>{item}</code>
              ))}
            </div>
          ) : activePlan ? (
            <div className="plan-status-wrap">
              <div className={`plan-status-pill ${blocked ? "blocked" : ""}`}>
                {blocked ? <AlertTriangle size={14} /> : <Check size={14} />}
                <span>{confirmationText}</span>
              </div>
              {planDetails && (
                <PlanInfoDisclosure details={planDetails} onIncludeReplacement={includeReplacement} busy={busy} />
              )}
            </div>
          ) : bottomPreviewText ? (
            <div className="action-preview">
              <span className="preview-label">操作预览</span>
              <span className="preview-sep">{" · "}</span>
              {bottomPreviewText}
            </div>
          ) : null}
          <div className="action-buttons-end">
            {generatedPlan ? (
              <div className="button-pair sync-plan-actions">
                <button className="secondary-button large sync-plan-action" disabled={actionDisabled} onClick={() => previewPlan()}>
                  重新生成预览
                </button>
                <button className="primary-button large sync-plan-action" disabled={!activePlan || blocked || busy || Boolean(applyResult)} onClick={onApply}>
                  {applyResult ? "执行完成" : "执行同步计划"}
                </button>
              </div>
            ) : (
              <button className="primary-button large sync-preview-action" disabled={actionDisabled} onClick={() => previewPlan()}>
                {previewLabel}
              </button>
            )}
          </div>
        </div>
      </section>
    </div>
  );
}
