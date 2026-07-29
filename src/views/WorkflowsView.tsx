import { AlertTriangle, ChevronDown, ChevronRight, Download, RefreshCw, Search } from "lucide-react";
import { Fragment, useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { ProjectEmptyVisual } from "../components/EmptyStateVisuals";
import { UseWorkflowSheet } from "../components/workflow/UseWorkflowSheet";
import { WorkflowDetailPanel } from "../components/workflow/WorkflowDetailPanel";
import { WorkflowEditor } from "../components/workflow/WorkflowEditor";
import { callApi, hasRealBackend } from "../lib/api";
import { askConfirm } from "../lib/shell";
import type {
  AgentRecord,
  ApplyResult,
  InstalledWorkflow,
  RemoteWorkflowSummary,
  Settings as AppSettings,
  SkillLockEntry,
  SkillRecord,
  Workflow,
  WorkflowDetail,
  WorkflowDetailStep
} from "../types";

const installedBoardStyle = { "--skill-table-columns": "minmax(260px, 1fr) 180px 90px" } as CSSProperties;
const remoteBoardStyle = { "--skill-table-columns": "minmax(260px, 1fr) 180px 100px" } as CSSProperties;

const DEFAULT_REGISTRY_LABEL = "Pgooone/oh-my-skills-workflows";

/**
 * 工作流视图：已安装 / 远程注册表两区列表 + 搜索。
 * 数据自管（经 callApi），视觉复用 SkillsView 的列表语言；演示模式只给空态。
 */
export function WorkflowsView({
  agents,
  librarySkills,
  skillLocks,
  settings,
  onRequestScan
}: {
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
  const detailRunRef = useRef(0);
  const bootRef = useRef(false);

  useEffect(() => {
    if (bootRef.current) return;
    bootRef.current = true;
    if (!realBackend) return;
    // 仅首挂载加载一次；后续刷新走 toolbar 与远程区的刷新按钮。
    void refreshInstalled();
    void refreshRemote(false);
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
            <button
              className="project-toolbar-action"
              onClick={() => setEditorState({ initial: null })}
              type="button"
            >
              新建工作流
            </button>
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
                  />
                  {expanded && (
                    detailLoading || !detail ? (
                      <div className="skill-detail">
                        <p className="muted">加载详情中…</p>
                      </div>
                    ) : (
                      <WorkflowDetailPanel
                        busy={Boolean(busy)}
                        onDelete={() => void remove(item)}
                        onEdit={() => void openEditor(item)}
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
  onSelect
}: {
  item: InstalledWorkflow;
  expanded: boolean;
  onSelect: () => void;
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
      ) : (
        <span className="skill-status-badge ok" title="解析正常">
          正常
        </span>
      )}
    </article>
  );
}

function RemoteRow({
  item,
  busy,
  onDownload
}: {
  item: RemoteWorkflowSummary;
  busy: boolean;
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
      {item.installed ? (
        <span className="skill-status-badge ok" title="已安装到本地">
          已安装
        </span>
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
