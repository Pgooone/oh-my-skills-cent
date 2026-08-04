import { AlertTriangle, ChevronDown, ChevronRight, Download, RefreshCw, Search, Upload } from "lucide-react";
import { Fragment, useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { ProjectEmptyVisual } from "../components/EmptyStateVisuals";
import { UseWorkflowSheet } from "../components/workflow/UseWorkflowSheet";
import { WorkflowDetailPanel } from "../components/workflow/WorkflowDetailPanel";
import { WorkflowEditor } from "../components/workflow/WorkflowEditor";
import { callApi, hasRealBackend } from "../lib/api";
import { askConfirm, openUrl, saveExportPackage } from "../lib/shell";
import type {
  AgentRecord,
  ApplyResult,
  ContributeOutcome,
  ContributeUploadResponse,
  ExportPackage,
  ImportResult,
  InstalledWorkflow,
  PushResult,
  RemoteWorkflowSummary,
  Settings as AppSettings,
  SkillLockEntry,
  SkillRecord,
  Workflow,
  WorkflowDetail,
  WorkflowDetailStep,
  WorkflowUpdateStatus
} from "../types";

const installedBoardStyle = { "--skill-table-columns": "minmax(260px, 1fr) 180px 90px" } as CSSProperties;
const remoteBoardStyle = { "--skill-table-columns": "minmax(260px, 1fr) 180px 100px" } as CSSProperties;

const DEFAULT_REGISTRY_LABEL = "Pgooone/oh-my-skills-workflows";

/**
 * 工作流视图：已安装 / 远程注册表两区列表 + 搜索。
 * 数据自管（经 callApi），视觉复用 SkillsView 的列表语言；演示模式只给空态。
 */
export function WorkflowsView({
  readonly,
  agents,
  librarySkills,
  skillLocks,
  settings,
  onRequestScan
}: {
  readonly: boolean;
  agents: AgentRecord[];
  librarySkills: SkillRecord[];
  skillLocks: Record<string, SkillLockEntry>;
  settings: AppSettings;
  onRequestScan: () => void;
}) {
  const realBackend = hasRealBackend();
  const [installed, setInstalled] = useState<InstalledWorkflow[]>([]);
  const [remote, setRemote] = useState<RemoteWorkflowSummary[] | null>(null);
  const [remoteError, setRemoteError] = useState<string | null>(null);
  const [remoteRefreshing, setRemoteRefreshing] = useState(false);
  const [query, setQuery] = useState("");
  const [selectedSlug, setSelectedSlug] = useState<string | null>(null);
  const [detail, setDetail] = useState<{ workflow: Workflow; steps: WorkflowDetailStep[] } | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [editorState, setEditorState] = useState<{ initial: Workflow | null } | null>(null);
  const [useTarget, setUseTarget] = useState<InstalledWorkflow | null>(null);
  const [busy, setBusy] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [updateStates, setUpdateStates] = useState<Record<string, WorkflowUpdateStatus>>({});
  const [checkingUpdates, setCheckingUpdates] = useState(false);
  const importInputRef = useRef<HTMLInputElement | null>(null);
  const detailRunRef = useRef(0);
  const bootRef = useRef(false);

  useEffect(() => {
    if (bootRef.current) return;
    bootRef.current = true;
    if (!realBackend) return;
    // 仅首挂载加载一次；后续刷新走 toolbar 与远程区的刷新按钮。
    void refreshInstalled();
    void refreshRemote(false);
    void refreshUpdateStates(true);
  }, []);

  useEffect(() => {
    if (!toast) return undefined;
    const timer = window.setTimeout(() => setToast(null), 3000);
    return () => window.clearTimeout(timer);
  }, [toast]);

  const filteredInstalled = useMemo(
    () => filterWorkflows(installed, query),
    [installed, query]
  );
  const filteredRemote = useMemo(
    () => filterWorkflows(remote ?? [], query),
    [remote, query]
  );
  const downloadableCount = useMemo(
    () => (remote ?? []).filter((item) => !item.installed).length,
    [remote]
  );
  const registryLabel = registrySourceLabel(settings.workflowRegistryUrl);

  async function refreshInstalled() {
    try {
      const list = await callApi<InstalledWorkflow[]>("list_installed_workflows");
      setInstalled(list);
    } catch (reason) {
      setError(reasonMessage(reason));
    }
  }

  async function refreshRemote(refresh: boolean) {
    setRemoteRefreshing(true);
    setRemoteError(null);
    try {
      const list = await callApi<RemoteWorkflowSummary[]>("list_remote_workflows", { refresh });
      setRemote(list);
    } catch (reason) {
      setRemote(null);
      setRemoteError(reasonMessage(reason));
    } finally {
      setRemoteRefreshing(false);
    }
  }

  /** 批量拉取更新状态并刷新徽标；silent=true（首挂载等后台场景）失败时静默。 */
  async function refreshUpdateStates(silent: boolean): Promise<WorkflowUpdateStatus[] | null> {
    try {
      const statuses = await callApi<WorkflowUpdateStatus[]>("check_workflow_updates");
      setUpdateStates(indexBySlug(statuses));
      return statuses;
    } catch (reason) {
      if (!silent) setError(reasonMessage(reason));
      return null;
    }
  }

  async function checkAllUpdates() {
    setCheckingUpdates(true);
    setError(null);
    try {
      const statuses = await refreshUpdateStates(false);
      if (!statuses) return;
      const available = statuses.filter((entry) => entry.state.kind === "updateAvailable").length;
      setToast(available > 0 ? `${available} 个工作流有新版本` : "全部工作流已是最新");
    } finally {
      setCheckingUpdates(false);
    }
  }

  async function push(item: InstalledWorkflow) {
    setBusy(`推送 ${item.name}`);
    setError(null);
    try {
      const result = await callApi<PushResult>("push_workflow_to_registry", { slug: item.slug });
      setToast(`已推送 ${item.name}（${result.commitHash.slice(0, 7)}）`);
    } catch (reason) {
      setError(reasonMessage(reason));
    } finally {
      setBusy("");
    }
  }

  async function exportPackage(item: InstalledWorkflow) {
    setBusy(`导出 ${item.name}`);
    setError(null);
    try {
      const pkg = await callApi<ExportPackage>("export_workflow_package", { slug: item.slug });
      const saved = await saveExportPackage(pkg.filename, pkg.base64);
      if (saved) setToast(`已导出 ${item.name}`);
    } catch (reason) {
      setError(reasonMessage(reason));
    } finally {
      setBusy("");
    }
  }

  /** 检查单个工作流更新；Modified 状态先警告「将覆盖本地修改（会先备份）」，确认才发 confirmModified=true。 */
  async function checkUpdate(item: InstalledWorkflow) {
    setBusy(`检查更新 ${item.name}`);
    setError(null);
    try {
      const statuses = await refreshUpdateStates(true);
      const status = statuses?.find((entry) => entry.slug === item.slug);
      if (!status || status.state.kind === "local") {
        setToast(`${item.name} 没有远程来源，无需更新`);
        return;
      }
      if (status.state.kind === "upToDate") {
        setToast(`${item.name} 已是最新版本`);
        return;
      }
      if (status.state.kind === "modified") {
        const confirmed = await askConfirm(
          `「${item.name}」包含本地修改。更新将覆盖你的本地修改（更新前会自动备份），是否继续？`,
          "更新工作流"
        );
        if (!confirmed) return;
        await callApi<WorkflowUpdateStatus>("update_workflow", {
          slug: item.slug,
          confirmModified: true
        });
      } else {
        const confirmed = await askConfirm(
          `检测到新版本 v${status.state.remoteVersion}，是否立即更新「${item.name}」？`,
          "更新工作流"
        );
        if (!confirmed) return;
        await callApi<WorkflowUpdateStatus>("update_workflow", {
          slug: item.slug,
          confirmModified: false
        });
      }
      await refreshInstalled();
      await refreshUpdateStates(true);
      setToast(`已更新 ${item.name}`);
      if (selectedSlug === item.slug) void reloadDetail(item.slug, false);
    } catch (reason) {
      setError(reasonMessage(reason));
    } finally {
      setBusy("");
    }
  }

  /** 一键贡献：按返回体 status 字段三分支（noToken→导出胖包+贡献指南引导 / needFork→开 fork 页 / ready→开 compare URL）。 */
  async function contribute(item: RemoteWorkflowSummary) {
    setBusy(`贡献 ${item.name}`);
    setError(null);
    try {
      const outcome = await callApi<ContributeOutcome>("contribute_workflow", { slug: item.slug });
      if (outcome.status === "noToken") {
        const confirmed = await askConfirm(
          `未配置 GitHub Token，无法一键贡献。\n\n可先导出分享包，并按官方仓库（${registryLabel}）的贡献指南手动提交。\n是否立即导出分享包？`,
          "贡献工作流"
        );
        const home = registryHomeUrl(settings.workflowRegistryUrl);
        if (home) openUrl(home);
        if (confirmed) {
          const pkg = await callApi<ExportPackage>("export_workflow_package", { slug: item.slug });
          const saved = await saveExportPackage(pkg.filename, pkg.base64);
          if (saved) setToast(`已导出 ${item.name}，请按贡献指南提交`);
        }
      } else if (outcome.status === "needFork") {
        openUrl(outcome.forkPageUrl);
        setToast("请先在 fork 页面创建你的 fork，再重新贡献");
      } else {
        openUrl(outcome.compareUrl);
        setToast(`已推送贡献分支 ${outcome.branch}，请创建 PR`);
      }
    } catch (reason) {
      setError(reasonMessage(reason));
    } finally {
      setBusy("");
    }
  }

  function pickImportFile() {
    importInputRef.current?.click();
  }

  async function handleImportFile(file: File) {
    setBusy(`导入 ${file.name}`);
    setError(null);
    try {
      const archiveBase64 = await readFileAsBase64(file);
      const result = await callApi<ImportResult>("import_workflow_package", { archiveBase64 });
      await askConfirm(
        `已导入工作流「${result.slug}」${result.hadSource ? "（含来源记录）" : ""}。`,
        "导入成功"
      );
      await refreshInstalled();
      await refreshUpdateStates(true);
    } catch (reason) {
      setError(reasonMessage(reason));
    } finally {
      setBusy("");
    }
  }

  /** 只读模式的上传贡献（DD §8.3）：zip 分享包 → contribute_upload，提示 prUrl / branchUrl。 */
  async function handleContributeUpload(file: File) {
    setBusy(`上传贡献 ${file.name}`);
    setError(null);
    try {
      const archiveBase64 = await readFileAsBase64(file);
      const result = await callApi<ContributeUploadResponse>("contribute_upload", {
        kind: "workflow",
        archiveBase64
      });
      if (result.prUrl) {
        openUrl(result.prUrl);
        setToast("贡献已提交：PR 已创建，请等待维护者审核");
      } else if (result.branchUrl) {
        openUrl(result.branchUrl);
        setToast(result.note ?? "贡献分支已推送，请在该页面手动创建 PR");
      } else {
        setToast("贡献已提交");
      }
    } catch (reason) {
      setError(reasonMessage(reason));
    } finally {
      setBusy("");
    }
  }

  async function openDetail(slug: string) {
    if (selectedSlug === slug) {
      detailRunRef.current += 1;
      setSelectedSlug(null);
      setDetail(null);
      return;
    }
    setSelectedSlug(slug);
    setDetail(null);
    await reloadDetail(slug, true);
  }

  /** 始终重新拉取详情（无 toggle 语义）；silent 时不切换加载占位。 */
  async function reloadDetail(slug: string, showLoading: boolean) {
    const runId = detailRunRef.current + 1;
    detailRunRef.current = runId;
    if (showLoading) setDetailLoading(true);
    try {
      const data = await callApi<WorkflowDetail>("get_workflow_detail", { slug });
      if (detailRunRef.current === runId) {
        setDetail({ workflow: data.workflow, steps: normalizeDetailSteps(data) });
      }
    } catch (reason) {
      if (detailRunRef.current === runId && showLoading) {
        setSelectedSlug(null);
        setError(reasonMessage(reason));
      }
    } finally {
      if (detailRunRef.current === runId && showLoading) setDetailLoading(false);
    }
  }

  async function download(item: RemoteWorkflowSummary) {
    setBusy(`下载 ${item.name}`);
    setError(null);
    try {
      const saved = await callApi<InstalledWorkflow>("download_workflow", { path: item.path });
      await refreshInstalled();
      setRemote((current) =>
        current?.map((entry) => (entry.path === item.path ? { ...entry, installed: true } : entry)) ?? current
      );
      setToast(`已安装 ${saved.name}`);
    } catch (reason) {
      setError(reasonMessage(reason));
    } finally {
      setBusy("");
    }
  }

  async function remove(item: InstalledWorkflow) {
    const confirmed = await askConfirm(
      `确定删除工作流「${item.name}」吗？\n\n此操作不可撤销。`,
      "确认删除"
    );
    if (!confirmed) return;
    setBusy(`删除 ${item.name}`);
    setError(null);
    try {
      await callApi("delete_workflow", { slug: item.slug });
      if (selectedSlug === item.slug) {
        detailRunRef.current += 1;
        setSelectedSlug(null);
        setDetail(null);
      }
      await refreshInstalled();
      setRemote((current) =>
        current?.map((entry) => (entry.slug === item.slug ? { ...entry, installed: false } : entry)) ?? current
      );
      setToast(`已删除 ${item.name}`);
    } catch (reason) {
      setError(reasonMessage(reason));
    } finally {
      setBusy("");
    }
  }

  async function openEditor(item: InstalledWorkflow) {
    setBusy(`读取 ${item.name}`);
    setError(null);
    try {
      const data = await callApi<WorkflowDetail>("get_workflow_detail", { slug: item.slug });
      setEditorState({ initial: data.workflow });
    } catch (reason) {
      setError(reasonMessage(reason));
    } finally {
      setBusy("");
    }
  }

  async function saveEditor(workflow: Workflow, readme?: string) {
    setBusy("保存工作流");
    try {
      const slug = await callApi<string>("save_workflow", {
        workflow,
        readme: readme ?? null
      });
      setEditorState(null);
      await refreshInstalled();
      setToast(`已保存 ${workflow.name}`);
      setDetail(null);
      setSelectedSlug(slug);
      void reloadDetail(slug, true);
    } catch (reason) {
      throw reason instanceof Error ? reason : new Error(reasonMessage(reason));
    } finally {
      setBusy("");
    }
  }

  function handleApplied(result: ApplyResult) {
    if (result.errors.length === 0) {
      setToast("工作流执行完成");
    }
    if (result.inventoryRefreshRecommended && result.errors.length === 0) {
      onRequestScan();
    }
    // 下载补齐后详情页的就绪状态会变化，后台静默重拉当前详情。
    if (selectedSlug) void reloadDetail(selectedSlug, false);
  }

  const isFiltered = Boolean(query.trim());

  if (!realBackend) {
    return (
      <div className="skills-page">
        <section className="skills-workbench">
          <section className="agent-empty-state" aria-label="工作流空状态">
            <ProjectEmptyVisual />
            <div className="agent-empty-copy">
              <strong>演示模式暂无工作流数据</strong>
              <span>连接后端（桌面应用或 oms-web）后，这里会显示已安装与远程注册表中的工作流。</span>
            </div>
          </section>
        </section>
      </div>
    );
  }

  return (
    <div className="skills-page">
      <section className="skills-workbench">
        <div className="skills-toolbar">
          <div className="searchbox compact">
            <Search size={16} />
            <input
              onChange={(event) => setQuery(event.target.value)}
              placeholder="搜索工作流、简介或标签"
              value={query}
            />
          </div>
          <div className="skills-toolbar-actions">
            {readonly ? (
              <button
                className="project-toolbar-action"
                onClick={pickImportFile}
                title="选择 zip 分享包上传到官方注册表"
                type="button"
              >
                <Upload size={14} />
                上传贡献
              </button>
            ) : (
              <button
                className="project-toolbar-action"
                onClick={() => setEditorState({ initial: null })}
                type="button"
              >
                新建工作流
              </button>
            )}
            {!readonly && (
              <button
                className="project-toolbar-action"
                disabled={checkingUpdates}
                onClick={() => void checkAllUpdates()}
                type="button"
              >
                {checkingUpdates ? "检查中…" : "检查全部更新"}
              </button>
            )}
            {!readonly && (
              <button className="project-toolbar-action" onClick={pickImportFile} type="button">
                导入分享包
              </button>
            )}
            <button
              className="icon-button plain"
              onClick={() => {
                void refreshInstalled();
                void refreshRemote(true);
              }}
              title="刷新"
              type="button"
            >
              <RefreshCw size={17} />
            </button>
            <input
              accept=".zip,application/zip"
              onChange={(event) => {
                const file = event.target.files?.[0];
                if (file) {
                  if (readonly) void handleContributeUpload(file);
                  else void handleImportFile(file);
                }
                event.target.value = "";
              }}
              ref={importInputRef}
              style={{ display: "none" }}
              type="file"
            />
          </div>
        </div>

        {error && (
          <div className="banner error" role="alert">
            <AlertTriangle size={17} />
            <span>{error}</span>
          </div>
        )}

        <div className="skills-summary">
          <span className="skills-summary-text">
            已安装工作流 · {installed.length} 个{busy ? ` · ${busy}…` : ""}
          </span>
        </div>

        <div className="skill-list-board" style={installedBoardStyle}>
          <div className="skill-table-head">
            <span>工作流</span>
            <span>信息</span>
            <span>状态</span>
          </div>
          <div className="skill-list">
            {filteredInstalled.map((item) => {
              const expanded = selectedSlug === item.slug;
              return (
                <Fragment key={item.slug}>
                  <InstalledRow
                    expanded={expanded}
                    item={item}
                    onSelect={() => void openDetail(item.slug)}
                    updateStatus={updateStates[item.slug]}
                  />
                  {expanded && (
                    detailLoading || !detail ? (
                      <div className="skill-detail">
                        <p className="muted">加载详情中…</p>
                      </div>
                    ) : (
                      <WorkflowDetailPanel
                        readonly={readonly}
                        busy={Boolean(busy)}
                        onCheckUpdate={() => void checkUpdate(item)}
                        onDelete={() => void remove(item)}
                        onEdit={() => void openEditor(item)}
                        onExport={() => void exportPackage(item)}
                        onPush={() => void push(item)}
                        onUse={() => setUseTarget(item)}
                        steps={detail.steps}
                        workflow={detail.workflow}
                      />
                    )
                  )}
                </Fragment>
              );
            })}
            {filteredInstalled.length === 0 && (
              <section className="agent-empty-state" aria-label="已安装工作流空状态">
                <ProjectEmptyVisual />
                <div className="agent-empty-copy">
                  <strong>{isFiltered ? "没有找到匹配的工作流" : "还没有已安装的工作流"}</strong>
                  <span>
                    {isFiltered
                      ? "换个关键词试试"
                      : "从下方远程注册表下载，或点击右上角「新建工作流」创建。"}
                  </span>
                </div>
                {isFiltered && (
                  <button className="secondary-button" onClick={() => setQuery("")} type="button">
                    清空搜索条件
                  </button>
                )}
              </section>
            )}
          </div>
        </div>

        <div className="skills-summary">
          <span className="skills-summary-text">
            远程注册表 · {remote ? `${downloadableCount} 个可下载` : "未加载"} · 来自 {registryLabel}
          </span>
          <button
            className="icon-button plain"
            disabled={remoteRefreshing}
            onClick={() => void refreshRemote(true)}
            title="刷新远程注册表"
            type="button"
          >
            <RefreshCw className={remoteRefreshing ? "spin" : ""} size={15} />
          </button>
        </div>

        <div className="skill-list-board" style={remoteBoardStyle}>
          <div className="skill-table-head">
            <span>工作流</span>
            <span>信息</span>
            <span>操作</span>
          </div>
          <div className="skill-list">
            {filteredRemote.map((item) => (
              <RemoteRow
                busy={Boolean(busy)}
                item={item}
                key={item.path}
                readonly={readonly}
                onContribute={() => void contribute(item)}
                onDownload={() => void download(item)}
              />
            ))}
            {remote === null && (
              <section className="agent-empty-state" aria-label="远程注册表空状态">
                <ProjectEmptyVisual />
                <div className="agent-empty-copy">
                  <strong>{remoteRefreshing ? "正在拉取远程注册表…" : "远程注册表暂未加载"}</strong>
                  <span>
                    {remoteError
                      ? remoteError
                      : "注册表地址来自「设置 → 数据 → 工作流注册表 URL」，请确认网络可用后重试。"}
                  </span>
                </div>
                {!remoteRefreshing && (
                  <button className="secondary-button" onClick={() => void refreshRemote(true)} type="button">
                    重新拉取
                  </button>
                )}
              </section>
            )}
            {remote !== null && filteredRemote.length === 0 && (
              <section className="agent-empty-state" aria-label="远程注册表空状态">
                <ProjectEmptyVisual />
                <div className="agent-empty-copy">
                  <strong>{isFiltered ? "没有找到匹配的工作流" : "注册表暂无可下载的工作流"}</strong>
                  <span>
                    {isFiltered
                      ? "换个关键词试试"
                      : `当前注册表（${registryLabel}）还没有条目，可在「设置 → 数据」更换注册表 URL。`}
                  </span>
                </div>
                {isFiltered && (
                  <button className="secondary-button" onClick={() => setQuery("")} type="button">
                    清空搜索条件
                  </button>
                )}
              </section>
            )}
          </div>
        </div>
      </section>

      {toast && <div className="toast" role="status">{toast}</div>}

      {editorState && (
        <WorkflowEditor
          busy={Boolean(busy)}
          initial={editorState.initial}
          librarySkills={librarySkills}
          onCancel={() => setEditorState(null)}
          onSave={saveEditor}
          skillLocks={skillLocks}
        />
      )}

      {useTarget && (
        <UseWorkflowSheet
          agents={agents}
          onApplied={handleApplied}
          onClose={() => setUseTarget(null)}
          workflow={useTarget}
        />
      )}
    </div>
  );
}

function InstalledRow({
  item,
  expanded,
  onSelect,
  updateStatus
}: {
  item: InstalledWorkflow;
  expanded: boolean;
  onSelect: () => void;
  /** check_workflow_updates 的该工作流状态；未拉取时为 undefined（保持既有「正常」）。 */
  updateStatus?: WorkflowUpdateStatus;
}) {
  return (
    <article className={`skill-row ${expanded ? "active" : ""}`} onClick={onSelect}>
      <button className="skill-row-main" type="button">
        <strong>
          <span className="skill-name-text">{item.name}</span>
          {item.hasPlaceholder && <em title="含占位步骤，使用时跳过">占位</em>}
          {expanded ? <ChevronDown size={15} /> : <ChevronRight size={15} />}
        </strong>
        <span className="skill-row-description">{item.description || item.slug}</span>
      </button>
      <div className="skill-reference-cell" title={item.author ? `作者：${item.author}` : item.slug}>
        <span>{item.stepCount} 步骤</span>
        <small>v{item.version || "—"}{item.author ? ` · ${item.author}` : ""}</small>
      </div>
      {item.error ? (
        <span className="skill-status-badge check" title={item.error}>
          损坏
        </span>
      ) : updateStatus ? (
        <UpdateBadge status={updateStatus} />
      ) : (
        <span className="skill-status-badge ok" title="解析正常">
          正常
        </span>
      )}
    </article>
  );
}

function UpdateBadge({ status }: { status: WorkflowUpdateStatus }) {
  const state = status.state;
  if (state.kind === "updateAvailable") {
    return (
      <span className="skill-status-badge check" title={`远程有新版本 v${state.remoteVersion}`}>
        有更新
      </span>
    );
  }
  if (state.kind === "modified") {
    return (
      <span className="skill-status-badge check" title="包含本地修改">
        已修改
      </span>
    );
  }
  if (state.kind === "local") {
    return (
      <span className="skill-status-badge ok" title="本地创建，无远程来源">
        本地
      </span>
    );
  }
  return (
    <span className="skill-status-badge ok" title="与远程注册表一致">
      最新
    </span>
  );
}

function RemoteRow({
  item,
  busy,
  readonly,
  onContribute,
  onDownload
}: {
  item: RemoteWorkflowSummary;
  busy: boolean;
  readonly: boolean;
  onContribute: () => void;
  onDownload: () => void;
}) {
  return (
    <article className="skill-row">
      <div className="skill-row-main">
        <strong>
          <span className="skill-name-text">{item.name}</span>
          {item.tags.slice(0, 2).map((tag) => (
            <em key={tag} title={`标签：${tag}`}>
              {tag}
            </em>
          ))}
        </strong>
        <span className="skill-row-description">{item.description || item.slug}</span>
      </div>
      <div className="skill-reference-cell" title={item.author ? `作者：${item.author}` : item.slug}>
        <span>v{item.version}</span>
        <small>{item.author ?? item.slug}</small>
      </div>
      {readonly ? (
        <span
          className={`skill-status-badge ${item.installed ? "ok" : "check"}`}
          title="只读模式：可浏览与导出，安装/贡献请连接本地后端"
        >
          {item.installed ? "已安装" : "可下载"}
        </span>
      ) : item.installed ? (
        <button
          className="secondary-button compact"
          disabled={busy}
          onClick={(event) => {
            event.stopPropagation();
            onContribute();
          }}
          title="一键贡献到官方注册表"
          type="button"
        >
          <Upload size={14} />
          贡献
        </button>
      ) : (
        <button
          className="secondary-button compact"
          disabled={busy}
          onClick={(event) => {
            event.stopPropagation();
            onDownload();
          }}
          title="下载并安装此工作流"
          type="button"
        >
          <Download size={14} />
          下载
        </button>
      )}
    </article>
  );
}

/** check_workflow_updates 结果按 slug 建索引，供 InstalledRow 徽标 O(1) 查找。 */
function indexBySlug(statuses: WorkflowUpdateStatus[]): Record<string, WorkflowUpdateStatus> {
  const index: Record<string, WorkflowUpdateStatus> = {};
  for (const entry of statuses) index[entry.slug] = entry;
  return index;
}

/** 注册表 URL → 仓库主页（去 .git 后缀），无配置时返回 null。 */
function registryHomeUrl(url?: string): string | null {
  const trimmed = url?.trim();
  if (!trimmed) return null;
  return trimmed.replace(/\.git$/, "").replace(/\/+$/, "");
}

/** File → base64（读 Data URL 并剥前缀；Web 壳导入分享包的读侧）。 */
function readFileAsBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(new Error(`读取文件失败：${file.name}`));
    reader.onload = () => {
      const dataUrl = typeof reader.result === "string" ? reader.result : "";
      const comma = dataUrl.indexOf(",");
      resolve(comma >= 0 ? dataUrl.slice(comma + 1) : dataUrl);
    };
    reader.readAsDataURL(file);
  });
}

function filterWorkflows<T extends { name: string; slug: string; description: string; tags: string[]; author?: string }>(
  items: T[],
  query: string
) {
  const needle = query.trim().toLowerCase();
  if (!needle) return items;
  return items.filter((item) =>
    [item.name, item.slug, item.description, item.author ?? "", item.tags.join(" ")]
      .join(" ")
      .toLowerCase()
      .includes(needle)
  );
}

/** 把 get_workflow_detail 的元组嵌套 statuses 归一化为详情页直接消费的形状。 */
function normalizeDetailSteps(detail: WorkflowDetail): WorkflowDetailStep[] {
  return detail.workflow.steps.map((step, index) => ({
    name: step.name,
    group: step.group,
    description: step.description,
    skills: (detail.statuses[index] ?? []).map(([view, status]) => ({
      kind: view.kind,
      slug: view.slug,
      sourceUrl: view.sourceUrl,
      skillPath: view.skillPath,
      placeholder: view.placeholder,
      status
    }))
  }));
}

function registrySourceLabel(url?: string) {
  const trimmed = url?.trim();
  if (!trimmed) return DEFAULT_REGISTRY_LABEL;
  const match = trimmed.match(/github\.com[/:]([^/]+\/[^/.]+)/);
  if (match) return match[1];
  return trimmed;
}

function reasonMessage(reason: unknown) {
  return reason instanceof Error ? reason.message : String(reason);
}
