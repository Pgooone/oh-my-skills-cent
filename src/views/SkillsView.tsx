import { AlertTriangle, Check, ChevronDown, ChevronLeft, ChevronRight, Download, FolderOpen, Github, RefreshCw, Search, Trash2, Upload, XCircle } from "lucide-react";
import { Fragment, useEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { AgentEmptyVisual, ProjectEmptyVisual } from "../components/EmptyStateVisuals";
import { AgentBadge, AgentIcon, Coverage, IssueList, SkillState } from "../components/shared";
import { callApi, hasRealBackend } from "../lib/api";
import { openUrl, revealPath } from "../lib/shell";
import { agentSkillCount, centralLibraryReferenceSummary, compactPath, isCentralLibraryReference, isRegistryTracked, projectName, projectStats, samePath, skillListStatus, skillSourceSummary } from "../lib/skillUtils";
import type { AgentRecord, ContributeOutcome, ProjectWorkspaceCandidate, RemoteSkillSummary, Settings as AppSettings, SkillLockEntry, SkillRecord, SkillUpdateCheck } from "../types";
import type { SkillWorkspace } from "../uiTypes";

export function SkillsView({
  agents,
  skills,
  allSkills,
  sourceSkills,
  skillLocks,
  skillUpdateChecks,
  updatingSkillIds,
  workspace,
  projectFolders,
  selectedProjectFolder,
  discoveredProjects,
  discoveryBasePath,
  discovering,
  selectedSkill,
  selectedSkillIds,
  selectedSkills,
  query,
  agentFilter,
  settings,
  removing,
  onQuery,
  onAgentFilter,
  onWorkspace,
  onSelectProject,
  onSelectSkill,
  onToggleSkill,
  onUpdateSkill,
  onAdoptSelected,
  onQuickSyncSelected,
  onRemoveSelected,
  onRemovePaths,
  onClearSelection,
  onRefresh,
  onAddProject,
  onDiscoverProjects,
  onCloseDiscovery,
  onLinkDiscoveredProject,
  onRemoveProject
}: {
  agents: AgentRecord[];
  skills: SkillRecord[];
  allSkills: SkillRecord[];
  sourceSkills: SkillRecord[];
  skillLocks: Record<string, SkillLockEntry>;
  skillUpdateChecks: Record<string, SkillUpdateCheck>;
  updatingSkillIds: Set<string>;
  workspace: SkillWorkspace;
  projectFolders: string[];
  selectedProjectFolder: string | null;
  discoveredProjects: ProjectWorkspaceCandidate[];
  discoveryBasePath: string | null;
  discovering: boolean;
  selectedSkill: SkillRecord | null;
  selectedSkillIds: Set<string>;
  selectedSkills: SkillRecord[];
  query: string;
  agentFilter: string;
  settings: AppSettings;
  removing: boolean;
  onQuery: (value: string) => void;
  onAgentFilter: (value: string) => void;
  onWorkspace: (value: SkillWorkspace) => void;
  onSelectProject: (folder: string) => void;
  onSelectSkill: (id: string | null) => void;
  onToggleSkill: (id: string) => void;
  onUpdateSkill: (skill: SkillRecord) => void;
  onAdoptSelected: () => void;
  onQuickSyncSelected: () => void;
  onRemoveSelected: () => void;
  onRemovePaths: (skill: SkillRecord, paths: string[]) => void;
  onClearSelection: () => void;
  onRefresh: () => void;
  onAddProject: () => void;
  onDiscoverProjects: () => void;
  onCloseDiscovery: () => void;
  onLinkDiscoveredProject: (path: string) => void;
  onRemoveProject: (folder: string) => void;
}) {
  const [agentMenuOpen, setAgentMenuOpen] = useState(false);
  const [searchOpen, setSearchOpen] = useState(false);
  const [projectScrollState, setProjectScrollState] = useState({ left: false, right: false });
  const agentMenuRef = useRef<HTMLDivElement>(null);
  const projectBarRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!agentMenuOpen) return undefined;
    const onDocClick = (e: MouseEvent) => {
      if (agentMenuRef.current && !agentMenuRef.current.contains(e.target as Node)) {
        setAgentMenuOpen(false);
      }
    };
    const onEsc = (e: KeyboardEvent) => {
      if (e.key === "Escape") setAgentMenuOpen(false);
    };
    document.addEventListener("mousedown", onDocClick);
    document.addEventListener("keydown", onEsc);
    return () => {
      document.removeEventListener("mousedown", onDocClick);
      document.removeEventListener("keydown", onEsc);
    };
  }, [agentMenuOpen]);

  const updateProjectScrollState = () => {
    const element = projectBarRef.current;
    if (!element) {
      setProjectScrollState({ left: false, right: false });
      return;
    }

    const maxScroll = element.scrollWidth - element.clientWidth;
    setProjectScrollState({
      left: element.scrollLeft > 2,
      right: element.scrollLeft < maxScroll - 2
    });
  };

  useEffect(() => {
    window.requestAnimationFrame(updateProjectScrollState);
  }, [projectFolders, selectedProjectFolder, workspace]);

  useEffect(() => {
    window.addEventListener("resize", updateProjectScrollState);
    return () => window.removeEventListener("resize", updateProjectScrollState);
  }, []);

  function scrollProjectBar(direction: "left" | "right") {
    const element = projectBarRef.current;
    if (!element) return;
    element.scrollBy({
      left: direction === "left" ? -340 : 340,
      behavior: "smooth"
    });
    window.setTimeout(updateProjectScrollState, 260);
  }

  const selectedAgentLabel = agentFilter === "all"
    ? "全部 Agent"
    : agents.find((agent) => agent.id === agentFilter)?.label ?? "全部 Agent";
  const isProjectWorkspace = workspace === "project";
  const isLibraryWorkspace = workspace === "library";
  const tabSummary = workspaceSummary(workspace, sourceSkills.length, selectedProjectFolder);
  const hasProjectWorkspaces = projectFolders.length > 0;
  const isProjectNoWorkspace = isProjectWorkspace && !hasProjectWorkspaces;
  const isFiltered = Boolean(query.trim()) || agentFilter !== "all";
  const emptyTitle = isFiltered
    ? "没有找到匹配的 Skills"
    : isProjectWorkspace
      ? hasProjectWorkspaces
        ? "这个项目还没有项目级 Skills"
        : "尚未关联项目工作区"
      : isLibraryWorkspace
        ? "中心库还没有 Skills"
        : "还没有全局 Skills";
  const emptyBody = isFiltered
    ? "换个关键词试试"
    : isProjectWorkspace
      ? hasProjectWorkspaces
        ? "可以从中心库同步到当前项目，或创建某个 Agent 的项目 skills 目录。"
        : "选择一个项目根目录后，Oh My Skills 会自动检测该项目下各 Agent 的项目级 Skills。"
      : isLibraryWorkspace
        ? "从全局或项目工作区选择 Skill 导入中心库后，这里会显示可统一分发的规范副本。"
        : "重新扫描或从中心库同步到某个 Agent 后，这里会显示机器级生效的 Skills。";
  const selectedCount = selectedSkills.length;
  const recentSelectedSkills = selectedSkills.slice(-2);
  const extraSelectedCount = Math.max(0, selectedCount - recentSelectedSkills.length);

  return (
    <div className="skills-page">
      <section className="skills-workbench">
        <div className="skills-toolbar">
          <div className="scope-tabs workspace-tabs" role="tablist" aria-label="Skills 工作区">
            {(["global", "project", "library"] as SkillWorkspace[]).map((scope) => (
              <button
                className={workspace === scope ? "active" : ""}
                key={scope}
                onClick={() => onWorkspace(scope)}
                role="tab"
                type="button"
                aria-selected={workspace === scope}
              >
                {workspaceLabel(scope)}
              </button>
            ))}
          </div>

          <div className="skills-toolbar-actions">
            {isProjectWorkspace ? (
              <div className="project-toolbar-actions">
                <button className="project-toolbar-action" onClick={onAddProject} type="button">
                  关联项目
                </button>
                <button className="project-toolbar-action" onClick={onDiscoverProjects} type="button">
                  扫描发现
                </button>
              </div>
            ) : (
              <>
                {searchOpen && (
                  <div className="searchbox compact">
                    <Search size={16} />
                    <input
                      autoFocus
                      value={query}
                      onChange={(event) => onQuery(event.target.value)}
                      placeholder="搜索 Skill、简介或 Agent"
                    />
                  </div>
                )}
                <button
                  className={`icon-button plain ${searchOpen ? "active" : ""}`}
                  onClick={() => {
                    setAgentMenuOpen(false);
                    setSearchOpen((open) => !open);
                  }}
                  title="搜索"
                  type="button"
                >
                  <Search size={18} />
                </button>
                <button className="icon-button plain" onClick={onRefresh} title="重新扫描" type="button">
                  <RefreshCw size={17} />
                </button>
                <div className="agent-menu-wrap" ref={agentMenuRef}>
                  <button
                    className={`agent-menu-trigger ${agentMenuOpen ? "open" : ""}`}
                    onClick={() => {
                      setSearchOpen(false);
                      setAgentMenuOpen((open) => !open);
                    }}
                    type="button"
                  >
                    <span>{selectedAgentLabel}</span>
                    <ChevronDown size={14} />
                  </button>
                  {agentMenuOpen && (
                    <div className="agent-menu" role="menu">
                      <button
                        className={agentFilter === "all" ? "active" : ""}
                        onClick={() => {
                          onAgentFilter("all");
                          setAgentMenuOpen(false);
                        }}
                        type="button"
                      >
                        <span className="check-col">{agentFilter === "all" && <Check size={13} />}</span>
                        <span className="menu-label">全部 Agent</span>
                        <strong>{sourceSkills.length}</strong>
                      </button>
                      {agents.map((agent) => (
                        <button
                          className={agentFilter === agent.id ? "active" : ""}
                          key={agent.id}
                          onClick={() => {
                            onAgentFilter(agent.id);
                            setAgentMenuOpen(false);
                          }}
                          type="button"
                        >
                          <span className="check-col">{agentFilter === agent.id && <Check size={13} />}</span>
                          <span className="menu-label">{agent.label}</span>
                          <strong>{agentSkillCount(agent.id, sourceSkills)}</strong>
                        </button>
                      ))}
                    </div>
                  )}
                </div>
              </>
            )}
          </div>
        </div>
        {!isProjectNoWorkspace && (
          <div className="skills-summary">
            <span className="skills-summary-text">{tabSummary}</span>
          </div>
        )}

        {isProjectWorkspace && (discovering || discoveryBasePath || discoveredProjects.length > 0) && (
          <section className="discovery-panel">
            <div className="discovery-heading">
              <span>
                <strong>扫描发现</strong>
                {discoveryBasePath && <small>{discoveryBasePath}</small>}
              </span>
              <button className="icon-button plain" onClick={onCloseDiscovery} title="关闭扫描发现" type="button">
                <XCircle size={18} />
              </button>
            </div>
            <div className="discovery-list">
              {discoveredProjects.map((candidate) => (
                <article className="discovery-card" key={candidate.path}>
                  <span>
                    <strong>{candidate.name}</strong>
                    <code>{candidate.path}</code>
                  </span>
                  <div className="discovery-agents">
                    {candidate.agentRoots.map((root) => (
                      <AgentBadge label={`${root.agentLabel} · ${root.skillCount}`} status="linked" key={`${candidate.path}-${root.agentId}`} />
                    ))}
                  </div>
                  <button
                    className="secondary-button"
                    disabled={candidate.alreadyLinked}
                    onClick={() => onLinkDiscoveredProject(candidate.path)}
                    type="button"
                  >
                    <Check size={16} />
                    {candidate.alreadyLinked ? "已关联" : "关联"}
                  </button>
                </article>
              ))}
            </div>
          </section>
        )}

        {isProjectWorkspace && hasProjectWorkspaces && (
          <div className="project-workspace-shell">
            {projectScrollState.left && (
              <button
                className="project-scroll-button left"
                onClick={() => scrollProjectBar("left")}
                title="向左滑动"
                type="button"
              >
                <ChevronLeft size={17} />
              </button>
            )}
            <div
              className="project-workspace-bar"
              aria-label="已关联项目工作区"
              onScroll={updateProjectScrollState}
              ref={projectBarRef}
            >
              {projectFolders.map((folder) => {
                const stats = projectStats(folder, allSkills);
                const active = selectedProjectFolder === folder;
                return (
                  <button
                    className={`project-chip ${active ? "active" : ""}`}
                    key={folder}
                    onClick={() => onSelectProject(folder)}
                    type="button"
                  >
                    <span>
                      <strong>{projectName(folder)}</strong>
                      <small>{folder}</small>
                    </span>
                    <em>{stats.skillCount} Skills</em>
                    <span
                      aria-hidden="true"
                      className="project-chip-close"
                      onClick={(event) => {
                        event.stopPropagation();
                        onRemoveProject(folder);
                      }}
                      title="取消关联"
                    >
                      <XCircle size={15} />
                    </span>
                  </button>
                );
              })}
            </div>
            {projectScrollState.right && (
              <button
                className="project-scroll-button right"
                onClick={() => scrollProjectBar("right")}
                title="向右滑动"
                type="button"
              >
                <ChevronRight size={17} />
              </button>
            )}
          </div>
        )}

        {!isProjectNoWorkspace && (
          <div className={`skill-list-board ${selectedCount > 0 ? "has-selection-bar" : ""}`}>
            <div className="skill-table-head">
              <span />
              <span>Skill</span>
              <span>{isLibraryWorkspace ? "引用位置" : "Agent 覆盖"}</span>
              <span>状态</span>
            </div>

            <div className="skill-list">
              {skills.map((skill) => {
                const expanded = selectedSkill?.id === skill.id;
                return (
                  <Fragment key={skill.id}>
                    <SkillRow
                      skill={skill}
                      agents={agents}
                      skillLocks={skillLocks}
                      registryUrl={settings.skillRegistryUrl}
                      active={expanded}
                      checked={selectedSkillIds.has(skill.id)}
                      updateCheck={skillUpdateChecks[skill.id]}
                      updating={updatingSkillIds.has(skill.id)}
                      workspace={workspace}
                      onSelect={() => onSelectSkill(expanded ? null : skill.id)}
                      onToggle={() => onToggleSkill(skill.id)}
                      onUpdate={() => onUpdateSkill(skill)}
                    />
                    {expanded && (
                      <SkillDetail
                        skill={skill}
                        settings={settings}
                        skillLocks={skillLocks}
                        workspace={workspace}
                        removing={removing}
                        onRemovePath={onRemovePaths}
                      />
                    )}
                  </Fragment>
                );
              })}
              {skills.length === 0 && (
                <SkillsListEmptyState
                  title={emptyTitle}
                  body={emptyBody}
                  workspace={workspace}
                  isFiltered={isFiltered}
                  onClearFilters={() => {
                    onQuery("");
                    onAgentFilter("all");
                  }}
                />
              )}
            </div>
          </div>
        )}

        {isProjectNoWorkspace && !(discovering || discoveryBasePath || discoveredProjects.length > 0) && (
          <ProjectWorkspaceEmptyState onAddProject={onAddProject} onDiscoverProjects={onDiscoverProjects} />
        )}
      </section>

      <SkillRegistrySection
        realBackend={hasRealBackend()}
        registryUrl={settings.skillRegistryUrl}
        onRefresh={onRefresh}
      />

      {selectedCount > 0 && (
        <div className="selection-action-bar" role="region" aria-label="已选 Skills 操作">
          <div className="selection-summary">
            <div className="selection-names">
              {recentSelectedSkills.map((skill) => (
                <span className="selection-name-chip" key={skill.id} title={skill.displayName}>
                  {skill.displayName}
                </span>
              ))}
              {extraSelectedCount > 0 && <span className="selection-extra">+{extraSelectedCount}</span>}
            </div>
            <span className="selection-count">{selectedCount} 个 Skill</span>
            <button className="selection-clear" onClick={onClearSelection} type="button">
              取消全选
            </button>
          </div>
          <div className="selection-actions button-pair">
            <button
              className="secondary-button large selection-bar-action"
              disabled={removing}
              onClick={onRemoveSelected}
              type="button"
            >
              {removing ? "移除中…" : "移除"}
            </button>
            <button className="secondary-button large selection-bar-action" onClick={onAdoptSelected} type="button">
              {isLibraryWorkspace ? "从中心库同步" : "导入中心库"}
            </button>
            <button className="primary-button large selection-bar-action" onClick={onQuickSyncSelected} type="button">
              快速同步
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

function workspaceLabel(workspace: SkillWorkspace) {
  if (workspace === "global") return "全局工作区";
  if (workspace === "project") return "项目工作区";
  return "中心库工作区";
}

function workspaceSummary(workspace: SkillWorkspace, skillCount: number, selectedProjectFolder: string | null) {
  if (workspace === "project") {
    return selectedProjectFolder
      ? `管理 ${projectName(selectedProjectFolder)} 内生效的 Agent Skills，已发现 ${skillCount} 个。`
      : "关联一个项目工作区后，可以管理该项目内各 Agent 生效的 Skills。";
  }

  if (workspace === "library") {
    return `管理中心库里的规范 Skill 副本，已发现 ${skillCount} 个。`;
  }

  return `管理这台机器上各 Agent 的全局 Skills，已发现 ${skillCount} 个。`;
}

function SkillsListEmptyState({
  title,
  body,
  workspace,
  isFiltered,
  onClearFilters
}: {
  title: string;
  body: string;
  workspace: SkillWorkspace;
  isFiltered: boolean;
  onClearFilters: () => void;
}) {
  return (
    <section className="agent-empty-state" aria-label="Skills 列表空状态">
      {isFiltered || workspace !== "global" ? <ProjectEmptyVisual /> : <AgentEmptyVisual />}
      <div className="agent-empty-copy">
        <strong>{title}</strong>
        <span>{body}</span>
      </div>
      {isFiltered && (
        <button className="secondary-button" onClick={onClearFilters} type="button">
          清空搜索条件
        </button>
      )}
    </section>
  );
}

function ProjectWorkspaceEmptyState({
  onAddProject,
  onDiscoverProjects
}: {
  onAddProject: () => void;
  onDiscoverProjects: () => void;
}) {
  return (
    <section className="project-empty-state" aria-label="项目工作区空状态">
      <ProjectEmptyVisual />

      <div className="agent-empty-copy project-empty-copy">
        <strong>尚未关联项目工作区</strong>
        <span>关联项目根目录后，这里会显示该项目内各 Agent 生效的 Skills。</span>
      </div>

      <div className="empty-actions project-empty-actions">
        <button
          className="agent-empty-button"
          onClick={onAddProject}
          title="手动选择一个包含 Skills 的项目目录"
          type="button"
        >
          <span>关联项目</span>
        </button>
        <button
          className="secondary-button"
          onClick={onDiscoverProjects}
          title="从上级目录自动查找一个或多个包含 Skills 的项目"
          type="button"
        >
          扫描发现
        </button>
      </div>
    </section>
  );
}

function SkillRow({
  skill,
  agents,
  skillLocks,
  registryUrl,
  active,
  checked,
  updateCheck,
  updating,
  workspace,
  onSelect,
  onToggle,
  onUpdate
}: {
  skill: SkillRecord;
  agents: AgentRecord[];
  skillLocks: Record<string, SkillLockEntry>;
  registryUrl?: string;
  active: boolean;
  checked: boolean;
  updateCheck?: SkillUpdateCheck;
  updating: boolean;
  workspace: SkillWorkspace;
  onSelect: () => void;
  onToggle: () => void;
  onUpdate: () => void;
}) {
  return (
    <article className={`skill-row ${active ? "active" : ""}`} onClick={onSelect}>
      <label
        className={`select-checkbox ${checked ? "checked" : ""}`}
        onClick={(event) => {
          event.stopPropagation();
        }}
        title="选择同步"
      >
        <input
          aria-label={`选择同步 ${skill.displayName}`}
          checked={checked}
          onChange={onToggle}
          type="checkbox"
        />
        <span>{checked && <Check size={14} />}</span>
      </label>
      <button className="skill-row-main" onClick={onSelect} type="button">
        <strong>
          <span className="skill-name-text">{skill.displayName}</span>
          <SourceOwnerTag skill={skill} skillLocks={skillLocks} />
          {isRegistryTracked(skill, skillLocks, registryUrl) && (
            <em title="来源：skill 注册表">注册表</em>
          )}
          {active ? <ChevronDown size={15} /> : <ChevronRight size={15} />}
        </strong>
        <span className="skill-row-description">{skill.description || skill.slug}</span>
      </button>
      {workspace === "library"
        ? <SkillReferenceCell skill={skill} />
        : <SkillAgentStack skill={skill} agents={agents} />}
      <SkillStatusCell
        skill={skill}
        skillLocks={skillLocks}
        updateCheck={updateCheck}
        updating={updating}
        onUpdate={onUpdate}
      />
    </article>
  );
}

function SkillStatusCell({
  skill,
  skillLocks,
  updateCheck,
  updating,
  onUpdate
}: {
  skill: SkillRecord;
  skillLocks: Record<string, SkillLockEntry>;
  updateCheck?: SkillUpdateCheck;
  updating: boolean;
  onUpdate: () => void;
}) {
  const status = skillListStatus(skill, skillLocks, updateCheck);
  const title = updateCheck?.message ?? status.title;

  if (status.kind === "update") {
    return (
      <button
        className={`skill-status-badge ${status.kind}`}
        disabled={updating}
        onClick={(event) => {
          event.stopPropagation();
          onUpdate();
        }}
        title={title}
        type="button"
      >
        {updating ? "更新中" : status.label}
      </button>
    );
  }

  return (
    <span className={`skill-status-badge ${status.kind}`} title={title}>
      {status.label}
    </span>
  );
}

function SourceOwnerTag({ skill, skillLocks }: { skill: SkillRecord; skillLocks: Record<string, SkillLockEntry> }) {
  const source = skillSourceSummary(skill, skillLocks);
  if (!source.owner) return null;
  return <em title={source.detail}>{source.owner}</em>;
}

function SkillAgentStack({ skill, agents }: { skill: SkillRecord; agents: AgentRecord[] }) {
  const installedAgentById = new Map(agents.map((agent) => [agent.id, agent]));
  const uniqueAgents = Array.from(
    new Map(skill.installations.map((installation) => {
      const agent = installedAgentById.get(installation.agentId);
      return agent ? [installation.agentId, agent] as const : null;
    }).filter((item): item is readonly [string, AgentRecord] => Boolean(item))).values()
  );
  const knownAgents = uniqueAgents.slice(0, 5);
  const extra = Math.max(0, uniqueAgents.length - knownAgents.length);

  return (
    <div className="skill-agent-stack" aria-label="已安装 Agent">
      {knownAgents.map((agent) => (
        <AgentIcon agent={agent} key={agent.id} />
      ))}
      {extra > 0 && <span className="agent-extra">+{extra}</span>}
      {knownAgents.length === 0 && <span className="muted">未安装</span>}
    </div>
  );
}

function SkillReferenceCell({ skill }: { skill: SkillRecord }) {
  const summary = centralLibraryReferenceSummary(skill);
  const detail = summary.total === 0
    ? "暂未发现指向中心库副本的引用位置"
    : [
        summary.global > 0 ? `全局 ${summary.global}` : "",
        summary.project > 0 ? `项目 ${summary.project}` : ""
      ].filter(Boolean).join(" · ");

  return (
    <div className={`skill-reference-cell ${summary.total === 0 ? "empty" : ""}`} title={detail}>
      {summary.total === 0 ? (
        <span>未引用</span>
      ) : (
        <span>{summary.total} 个位置</span>
      )}
      {summary.total > 0 && <small>{detail}</small>}
    </div>
  );
}

function SkillDetail({
  skill,
  settings,
  skillLocks,
  workspace,
  removing,
  onRemovePath
}: {
  skill: SkillRecord;
  settings: AppSettings;
  skillLocks: Record<string, SkillLockEntry>;
  workspace: SkillWorkspace;
  removing: boolean;
  onRemovePath?: (skill: SkillRecord, paths: string[]) => void;
}) {
  const source = skillSourceSummary(skill, skillLocks);
  const pathSections = skillPathSections(skill, workspace);
  const removableLabels = removablePathLabels(workspace);

  return (
    <div className="skill-detail">
      {pathSections.map((section) => (
        <DetailField label={section.label} key={section.label}>
          <PathList
            paths={section.paths}
            showRawPaths={settings.showRawPaths}
            canRemove={Boolean(onRemovePath) && removableLabels.has(section.label)}
            removing={removing}
            onRemovePath={onRemovePath ? (paths) => onRemovePath(skill, paths) : undefined}
          />
        </DetailField>
      ))}

      <DetailField label="描述">
        <p>{skill.description || skill.slug}</p>
      </DetailField>

      {source.githubUrl && (
        <DetailField label="来源">
          <code title={source.githubUrl}>{source.detail}</code>
          <button
            className="meta-icon-button"
            onClick={(event) => {
              event.stopPropagation();
              if (source.githubUrl) openUrl(source.githubUrl);
            }}
            title="打开 GitHub 仓库"
            type="button"
          >
            <Github size={15} />
          </button>
        </DetailField>
      )}

      {skill.issues.length > 0 && (
        <DetailField label="问题">
          <IssueList issues={skill.issues} language={settings.language} />
        </DetailField>
      )}
    </div>
  );
}

type DetailPath = { id: string; path: string };
type DetailPathSection = { label: string; paths: DetailPath[] };

function removablePathLabels(workspace: SkillWorkspace) {
  if (workspace === "global") return new Set(["全局路径"]);
  if (workspace === "project") return new Set(["项目路径"]);
  // Library: central copy (cascades refs when deleted) + individual reference paths.
  return new Set(["中心库路径", "引用位置"]);
}

function skillPathSections(skill: SkillRecord, workspace: SkillWorkspace): DetailPathSection[] {
  if (workspace === "library") {
    return compactSections([
      {
        label: "中心库路径",
        paths: skill.canonicalPath ? [{ id: `library:${skill.canonicalPath}`, path: skill.canonicalPath }] : []
      },
      {
        label: "引用位置",
        paths: uniqueInstallationPaths(skill.installations.filter((installation) => isCentralLibraryReference(skill, installation)))
      }
    ]);
  }

  if (workspace === "global") {
    // Global workspace only lists machine-level install paths; the central library
    // copy is managed in the library workspace and should not clutter this detail.
    return compactSections([
      {
        label: "全局路径",
        paths: uniqueInstallationPaths(skill.installations.filter((installation) => installation.scope === "global"))
      }
    ]);
  }

  // Project workspace focuses on project-level installs only.
  return compactSections([
    {
      label: "项目路径",
      paths: uniqueInstallationPaths(skill.installations.filter((installation) => installation.scope === "project"))
    }
  ]);
}

function compactSections(sections: DetailPathSection[]) {
  return sections.filter((section) => section.paths.length > 0);
}

function uniqueInstallationPaths(installations: SkillRecord["installations"]) {
  const paths: DetailPath[] = [];
  for (const installation of installations) {
    if (!installation.entryPath) continue;
    if (paths.some((item) => samePath(item.path, installation.entryPath))) continue;
    paths.push({
      id: installation.id,
      path: installation.entryPath
    });
  }
  return paths;
}

function PathList({
  paths,
  showRawPaths,
  canRemove = false,
  removing = false,
  onRemovePath
}: {
  paths: DetailPath[];
  showRawPaths: boolean;
  canRemove?: boolean;
  removing?: boolean;
  onRemovePath?: (paths: string[]) => void;
}) {
  return (
    <div className="detail-path-list">
      {paths.map((item) => (
        <div className="detail-path-row" key={item.id}>
          <code title={item.path}>{showRawPaths ? item.path : compactPath(item.path)}</code>
          <button
            className="meta-icon-button"
            onClick={(event) => {
              event.stopPropagation();
              revealPath(item.path);
            }}
            title="打开路径（不存在或断链时打开上一级）"
            type="button"
          >
            <FolderOpen size={15} />
          </button>
          {canRemove && onRemovePath && (
            <button
              className="meta-icon-button danger"
              disabled={removing}
              onClick={(event) => {
                event.stopPropagation();
                onRemovePath([item.path]);
              }}
              title="删除此路径"
              type="button"
            >
              <Trash2 size={14} />
            </button>
          )}
        </div>
      ))}
    </div>
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

const skillRemoteBoardStyle = { "--skill-table-columns": "minmax(260px, 1fr) 180px 100px" } as CSSProperties;

/**
 * Skill 注册表远程区（镜像 WorkflowsView 远程区）：列表 / 下载（进中心库）/
 * 贡献（contribute_skill）+ 来源标签。数据自管（经 callApi）；演示模式只给空态。
 */
function SkillRegistrySection({
  realBackend,
  registryUrl,
  onRefresh
}: {
  realBackend: boolean;
  registryUrl?: string;
  onRefresh: () => void;
}) {
  const [remote, setRemote] = useState<RemoteSkillSummary[] | null>(null);
  const [remoteError, setRemoteError] = useState<string | null>(null);
  const [remoteRefreshing, setRemoteRefreshing] = useState(false);
  const [busy, setBusy] = useState("");
  const [toast, setToast] = useState<string | null>(null);
  const bootRef = useRef(false);

  useEffect(() => {
    if (!toast) return undefined;
    const timer = window.setTimeout(() => setToast(null), 3000);
    return () => window.clearTimeout(timer);
  }, [toast]);

  // 仅首挂载加载一次；后续刷新走刷新按钮（镜像 WorkflowsView）。
  useEffect(() => {
    if (bootRef.current) return;
    bootRef.current = true;
    if (realBackend) void refreshRemote(false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function refreshRemote(refresh: boolean) {
    setRemoteRefreshing(true);
    setRemoteError(null);
    try {
      const list = await callApi<RemoteSkillSummary[]>("list_remote_skills", { refresh });
      setRemote(list);
    } catch (reason) {
      setRemote(null);
      setRemoteError(reasonMessage(reason));
    } finally {
      setRemoteRefreshing(false);
    }
  }

  async function download(item: RemoteSkillSummary) {
    setBusy(`下载 ${item.name}`);
    setRemoteError(null);
    try {
      await callApi("download_skill", { path: item.path });
      setRemote((current) =>
        current?.map((entry) => (entry.path === item.path ? { ...entry, installed: true } : entry)) ?? current
      );
      setToast(`已安装 ${item.name} 到中心库`);
      // 中心库新增内容 → 重新扫描（更新触发链随之刷新 registry 更新提示）。
      onRefresh();
    } catch (reason) {
      setRemoteError(reasonMessage(reason));
    } finally {
      setBusy("");
    }
  }

  /** 一键贡献：按返回体 status 字段三分支（noToken→开注册表主页引导 / needFork→开 fork 页 / ready→开 compare URL）。 */
  async function contribute(item: RemoteSkillSummary) {
    setBusy(`贡献 ${item.name}`);
    setRemoteError(null);
    try {
      const outcome = await callApi<ContributeOutcome>("contribute_skill", { slug: item.slug });
      if (outcome.status === "noToken") {
        const home = registryHomeUrl(registryUrl);
        if (home) openUrl(home);
        setToast(`未配置 GitHub Token：已打开注册表主页（${registryLabel}）的贡献指南，也可在「设置 → 数据」配置 Token 后重试`);
      } else if (outcome.status === "needFork") {
        openUrl(outcome.forkPageUrl);
        setToast("请先在 fork 页面创建你的 fork，再重新贡献");
      } else {
        openUrl(outcome.compareUrl);
        setToast(`已推送贡献分支 ${outcome.branch}，请创建 PR`);
      }
    } catch (reason) {
      setRemoteError(reasonMessage(reason));
    } finally {
      setBusy("");
    }
  }

  const registryLabel = registrySourceLabel(registryUrl);
  const downloadableCount = (remote ?? []).filter((item) => !item.installed).length;

  return (
    <>
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

      {remoteError && (
        <div className="banner error" role="alert">
          <AlertTriangle size={17} />
          <span>{remoteError}</span>
        </div>
      )}

      <div className="skill-list-board" style={skillRemoteBoardStyle}>
        <div className="skill-table-head">
          <span>Skill</span>
          <span>信息</span>
          <span>操作</span>
        </div>
        <div className="skill-list">
          {(remote ?? []).map((item) => (
            <SkillRemoteRow
              busy={Boolean(busy)}
              item={item}
              key={item.path}
              onContribute={() => void contribute(item)}
              onDownload={() => void download(item)}
            />
          ))}
          {!realBackend && (
            <section className="agent-empty-state" aria-label="远程注册表空状态">
              <ProjectEmptyVisual />
              <div className="agent-empty-copy">
                <strong>演示模式暂无注册表数据</strong>
                <span>连接后端（桌面应用或 oms-web）后，这里会显示注册表中的可下载 Skills。</span>
              </div>
            </section>
          )}
          {realBackend && remote === null && (
            <section className="agent-empty-state" aria-label="远程注册表空状态">
              <ProjectEmptyVisual />
              <div className="agent-empty-copy">
                <strong>{remoteRefreshing ? "正在拉取远程注册表…" : "远程注册表暂未加载"}</strong>
                <span>
                  {remoteError
                    ? remoteError
                    : "注册表地址来自「设置 → 数据 → Skill 注册表 URL」，请确认网络可用后重试。"}
                </span>
              </div>
              {!remoteRefreshing && (
                <button className="secondary-button" onClick={() => void refreshRemote(true)} type="button">
                  重新拉取
                </button>
              )}
            </section>
          )}
          {realBackend && remote !== null && remote.length === 0 && (
            <section className="agent-empty-state" aria-label="远程注册表空状态">
              <ProjectEmptyVisual />
              <div className="agent-empty-copy">
                <strong>注册表暂无可下载的 Skills</strong>
                <span>{`当前注册表（${registryLabel}）还没有条目，可在「设置 → 数据」更换注册表 URL。`}</span>
              </div>
            </section>
          )}
        </div>
      </div>

      {toast && <div className="toast" role="status">{toast}</div>}
    </>
  );
}

function SkillRemoteRow({
  item,
  busy,
  onContribute,
  onDownload
}: {
  item: RemoteSkillSummary;
  busy: boolean;
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
      {item.installed ? (
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
          title="下载并安装到中心库"
          type="button"
        >
          <Download size={14} />
          下载
        </button>
      )}
    </article>
  );
}

const DEFAULT_SKILL_REGISTRY_LABEL = "Pgooone/oh-my-skills-skills";

/** 注册表 URL → 仓库主页（去 .git 后缀），无配置时返回 null。 */
function registryHomeUrl(url?: string): string | null {
  const trimmed = url?.trim();
  if (!trimmed) return null;
  return trimmed.replace(/\.git$/, "").replace(/\/+$/, "");
}

function registrySourceLabel(url?: string) {
  const trimmed = url?.trim();
  if (!trimmed) return DEFAULT_SKILL_REGISTRY_LABEL;
  const match = trimmed.match(/github\.com[/:]([^/]+\/[^/.]+)/);
  if (match) return match[1];
  return trimmed;
}

function reasonMessage(reason: unknown) {
  return reason instanceof Error ? reason.message : String(reason);
}
