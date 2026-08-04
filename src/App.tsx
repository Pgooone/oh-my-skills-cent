import { AlertTriangle } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { AgentDiscoveryEmptyState } from "./components/AgentDiscoveryEmptyState";
import { SettingsSheet } from "./components/SettingsSheet";
import { TabButton } from "./components/TabButton";
import { callApi, hasRealBackend, probeRealBackend } from "./lib/api";
import { demoBatchPlan, demoInventory, demoSkillLocks } from "./lib/demoData";
import { askConfirm, pickDirectory } from "./lib/shell";
import { aggregateSkillsBySlug, compactPath, failedUpdateCheck, isCentralLibraryReference, isRegistrySource, projectSkillsForFolder, quickMigrationSourcesForSkills, samePath, skillsShUpdateSource, syncSourcesForSkills } from "./lib/skillUtils";
import type { QuickMigrationMethod, SkillWorkspace, SyncMode, View } from "./uiTypes";
import { SkillsView } from "./views/SkillsView";
import { SyncView } from "./views/SyncView";
import { WorkflowsView } from "./views/WorkflowsView";
import type {
  AgentTarget,
  ApplyResult,
  InventorySnapshot,
  ProjectWorkspaceCandidate,
  RegistrySkillUpdate,
  Settings as AppSettings,
  SkillLockEntry,
  SkillRecord,
  SkillUpdateCheck,
  SyncPlan,
  SyncReplacement
} from "./types";

const defaultSettings: AppSettings = {
  libraryPath: "",
  projectFolders: [],
  customRoots: [],
  showRawPaths: false,
  language: "zh-CN"
};

const appLogo = new URL("../oms_logo.svg", import.meta.url).href;
const selectionKeySeparator = "\u0000";

export default function App() {
  const [settings, setSettings] = useState<AppSettings>(defaultSettings);
  const [draftSettings, setDraftSettings] = useState<AppSettings>(defaultSettings);
  const [inventory, setInventory] = useState<InventorySnapshot | null>(null);
  const [skillLocks, setSkillLocks] = useState<Record<string, SkillLockEntry>>({});
  const [view, setView] = useState<View>("skills");
  const [skillWorkspace, setSkillWorkspace] = useState<SkillWorkspace>("global");
  const [query, setQuery] = useState("");
  const [agentFilter, setAgentFilter] = useState("all");
  const [selectedProjectFolder, setSelectedProjectFolder] = useState<string | null>(null);
  const [discoveredProjects, setDiscoveredProjects] = useState<ProjectWorkspaceCandidate[]>([]);
  const [discoveryBasePath, setDiscoveryBasePath] = useState<string | null>(null);
  const [selectedSkillId, setSelectedSkillId] = useState<string | null>(null);
  const [selectedSkillIds, setSelectedSkillIds] = useState<Set<string>>(new Set());
  const [syncQueuedSkillIds, setSyncQueuedSkillIds] = useState<Set<string>>(new Set());
  const [skillUpdateChecks, setSkillUpdateChecks] = useState<Record<string, SkillUpdateCheck>>({});
  const [updatingSkillIds, setUpdatingSkillIds] = useState<Set<string>>(new Set());
  const [removing, setRemoving] = useState(false);
  const [syncPlan, setSyncPlan] = useState<SyncPlan | null>(null);
  const [syncPlanProjectFolders, setSyncPlanProjectFolders] = useState<string[]>([]);
  const [applyResult, setApplyResult] = useState<ApplyResult | null>(null);
  const [syncMode, setSyncMode] = useState<SyncMode>("quick");
  /** Survives skills ↔ sync tab switches; resets only when the app restarts. */
  const [syncSelectedTargetIds, setSyncSelectedTargetIds] = useState<Set<string>>(() => new Set());
  const [busy, setBusy] = useState("启动中");
  const [error, setError] = useState<string | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [hasScanned, setHasScanned] = useState(false);
  const [previouslyScanned, setPreviouslyScanned] = useState(false);
  const bootStartedRef = useRef(false);
  const discoveryRunRef = useRef(0);

  useEffect(() => {
    if (bootStartedRef.current) return;
    bootStartedRef.current = true;
    void boot();
  }, []);

  useEffect(() => {
    if (!toast) return undefined;
    const timer = window.setTimeout(() => setToast(null), 3000);
    return () => window.clearTimeout(timer);
  }, [toast]);

  const agents = useMemo(
    () =>
      [...(inventory?.agents ?? [])].sort((left, right) => {
        if (left.installed !== right.installed) return left.installed ? -1 : 1;
        return left.label.localeCompare(right.label, undefined, { sensitivity: "base" });
      }),
    [inventory?.agents]
  );
  const installedAgents = useMemo(() => agents.filter((agent) => agent.installed), [agents]);
  const installedAgentIds = useMemo(() => new Set(installedAgents.map((agent) => agent.id)), [installedAgents]);
  const allSkills = useMemo(() => aggregateSkillsBySlug(inventory?.skills ?? []), [inventory?.skills]);
  const projectFolders = settings.projectFolders;

  useEffect(() => {
    setSelectedProjectFolder((current) => {
      if (current && projectFolders.includes(current)) return current;
      return projectFolders[0] ?? null;
    });
  }, [projectFolders]);

  useEffect(() => {
    if (agentFilter !== "all" && !installedAgentIds.has(agentFilter)) {
      setAgentFilter("all");
    }
  }, [agentFilter, installedAgentIds]);

  const globalSkills = useMemo(
    () => allSkills.filter((skill) => skill.installations.some((item) => item.scope === "global")),
    [allSkills]
  );

  const projectSkills = useMemo(
    () => projectSkillsForFolder(allSkills, selectedProjectFolder),
    [allSkills, selectedProjectFolder]
  );

  const librarySkills = useMemo(
    () => allSkills.filter((skill) => skill.canonicalStatus === "imported"),
    [allSkills]
  );

  const visibleSourceSkills = skillWorkspace === "project"
    ? projectSkills
    : skillWorkspace === "library"
      ? librarySkills
      : globalSkills;

  const filteredSkills = useMemo(() => {
    const needle = query.trim().toLowerCase();

    return visibleSourceSkills.filter((skill) => {
      if (agentFilter !== "all" && !skill.installations.some((item) => item.agentId === agentFilter)) {
        return false;
      }

      if (!needle) return true;
      const haystack = [
        skill.displayName,
        skill.slug,
        skill.description ?? "",
        skill.installations.map((item) => item.agentLabel).join(" ")
      ]
        .join(" ")
        .toLowerCase();
      return haystack.includes(needle);
    });
  }, [agentFilter, query, visibleSourceSkills]);

  const selectedSkill = useMemo(
    () => selectedSkillId ? filteredSkills.find((skill) => skill.id === selectedSkillId) ?? null : null,
    [filteredSkills, selectedSkillId]
  );

  const scopedSelectedSkillIds = useMemo(
    () => new Set(
      visibleSourceSkills
        .filter((skill) => selectedSkillIds.has(selectionKeyFor(skillWorkspace, selectedProjectFolder, skill.id)))
        .map((skill) => skill.id)
    ),
    [selectedProjectFolder, selectedSkillIds, skillWorkspace, visibleSourceSkills]
  );

  const selectedSkills = useMemo(
    () => selectedSkillsForKeys(selectedSkillIds, allSkills, globalSkills, projectFolders, librarySkills),
    [allSkills, globalSkills, librarySkills, projectFolders, selectedSkillIds]
  );

  const queuedSkills = useMemo(
    () => selectedSkillsForKeys(syncQueuedSkillIds, allSkills, globalSkills, projectFolders, librarySkills),
    [allSkills, globalSkills, librarySkills, projectFolders, syncQueuedSkillIds]
  );

  async function boot() {
    setBusy("读取上次扫描");
    setError(null);
    await probeRealBackend();
    if (!hasRealBackend()) {
      setSettings(defaultSettings);
      setDraftSettings(defaultSettings);
      setSkillLocks(demoSkillLocks);
      setInventory(demoInventory);
      setHasScanned(false);
      const hadDemoData = (demoInventory?.agents?.length ?? 0) > 0 || (demoInventory?.skills?.length ?? 0) > 0;
      setPreviouslyScanned(hadDemoData);
      setSelectedSkillId(null);
      setBusy("");
      return;
    }
    try {
      const [loaded, locks, cachedInventory] = await Promise.all([
        callApi<AppSettings>("get_settings"),
        readSkillLocks(),
        readInventoryCache()
      ]);
      setSettings(loaded);
      setDraftSettings(loaded);
      setSkillLocks(locks);
      setInventory(cachedInventory);
      setHasScanned(false);
      const hadData = (cachedInventory?.agents?.length ?? 0) > 0 || (cachedInventory?.skills?.length ?? 0) > 0;
      setPreviouslyScanned(hadData);
      setSelectedSkillId((current) => {
        if (current && cachedInventory?.skills.some((skill) => skill.id === current)) return current;
        return null;
      });
      setSelectedSkillIds((current) => {
        if (!cachedInventory) return new Set();
        return filterValidSelectionKeys(current, aggregateSkillsBySlug(cachedInventory.skills), loaded.projectFolders);
      });
      setSyncQueuedSkillIds((current) => {
        if (!cachedInventory) return new Set();
        return filterValidSelectionKeys(current, aggregateSkillsBySlug(cachedInventory.skills), loaded.projectFolders);
      });
      setBusy("");
    } catch (reason) {
      setError(String(reason));
      setBusy("");
    }
  }

  async function refreshInventory(projectFoldersForSelection = settings.projectFolders) {
    setBusy("扫描本机 Agent 与 Skills");
    setError(null);
    if (!hasRealBackend()) {
      setInventory(demoInventory);
      setSkillLocks(demoSkillLocks);
      setSkillUpdateChecks({});
      setHasScanned(true);
      setPreviouslyScanned(true);
      setBusy("");
      return;
    }
    try {
      const [locks, next] = await Promise.all([
        readSkillLocks(),
        callApi<InventorySnapshot>("scan_inventory", {
          options: { includeOrphaned: false }
        })
      ]);
      setSkillLocks(locks);
      setInventory(next);
      setSkillUpdateChecks({});
      setHasScanned(true);
      setPreviouslyScanned(true);
      setSelectedSkillId((current) => {
        if (current && next.skills.some((skill) => skill.id === current)) return current;
        return null;
      });
      setSelectedSkillIds((current) => {
        return filterValidSelectionKeys(current, aggregateSkillsBySlug(next.skills), projectFoldersForSelection);
      });
      setSyncQueuedSkillIds((current) => {
        return filterValidSelectionKeys(current, aggregateSkillsBySlug(next.skills), projectFoldersForSelection);
      });
      // 触发链接线（DD §8.5 门-B6）：扫描数据就绪后刷新更新检查，不阻塞扫描完成。
      // 必须位于 setSkillUpdateChecks({}) 重置之后，避免结果被清零。
      void refreshSkillsShUpdateChecks(aggregateSkillsBySlug(next.skills), locks);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy("");
    }
  }

  async function refreshSkillLocks() {
    setSkillLocks(await readSkillLocks());
  }

  async function readSkillLocks() {
    if (!hasRealBackend()) {
      return demoSkillLocks;
    }
    return callApi<Record<string, SkillLockEntry>>("read_skill_lock");
  }

  async function readInventoryCache() {
    if (!hasRealBackend()) {
      return demoInventory;
    }
    return callApi<InventorySnapshot | null>("read_inventory_cache");
  }

  async function previewSkillsSync(skills = queuedSkills, targets: AgentTarget[] = [], replacements: SyncReplacement[] = []) {
    const sources = syncSourcesForSkills(skills);
    if (sources.length === 0) return;
    const plannedProjectFolders = projectFoldersFromTargets(targets);
    setBusy("生成同步预览");
    setError(null);
    setApplyResult(null);
    if (!hasRealBackend()) {
      setSyncPlan(demoBatchPlan(skills, targets, "batch-sync"));
      setSyncPlanProjectFolders(plannedProjectFolders);
      setView("sync");
      setBusy("");
      return;
    }
    try {
      const plan = await callApi<SyncPlan>("preview_batch_sync", {
        sources,
        targets,
        replacements
      });
      setSyncPlan(plan);
      setSyncPlanProjectFolders(plannedProjectFolders);
      setView("sync");
    } catch (reason) {
      setSyncPlanProjectFolders([]);
      setError(String(reason));
    } finally {
      setBusy("");
    }
  }

  async function previewQuickMigration(skills = queuedSkills, method: QuickMigrationMethod, targets: AgentTarget[] = []) {
    const sources = quickMigrationSourcesForSkills(skills);
    if (sources.length === 0) return;
    const plannedProjectFolders = projectFoldersFromTargets(targets);
    setBusy("生成同步预览");
    setError(null);
    setApplyResult(null);
    if (!hasRealBackend()) {
      setSyncPlan(demoBatchPlan(skills, targets, "batch-quick-migrate"));
      setSyncPlanProjectFolders(plannedProjectFolders);
      setView("sync");
      setBusy("");
      return;
    }
    try {
      const plan = await callApi<SyncPlan>("preview_batch_quick_migration", {
        sources,
        targets,
        method
      });
      setSyncPlan(plan);
      setSyncPlanProjectFolders(plannedProjectFolders);
      setView("sync");
    } catch (reason) {
      setSyncPlanProjectFolders([]);
      setError(String(reason));
    } finally {
      setBusy("");
    }
  }

  async function applyPlan() {
    if (!syncPlan) return;
    const projectFoldersToRegister = syncPlanProjectFolders;
    setBusy("执行同步计划");
    setError(null);
    if (!hasRealBackend()) {
      setApplyResult({
        planId: syncPlan.planId,
        appliedOperations: syncPlan.operations.map((operation) => operation.id),
        skippedOperations: [],
        errors: [],
        inventoryRefreshRecommended: false
      });
      setBusy("");
      return;
    }
    try {
      const result = await callApi<ApplyResult>("apply_sync_plan", {
        planId: syncPlan.planId
      });
      setApplyResult(result);
      let projectFoldersForRefresh = settings.projectFolders;
      if (result.errors.length === 0 && projectFoldersToRegister.length > 0) {
        const nextProjectFolders = mergeProjectFolders(settings.projectFolders, projectFoldersToRegister);
        if (nextProjectFolders.length !== settings.projectFolders.length) {
          const saved = await callApi<AppSettings>("save_settings", {
            settings: {
              ...settings,
              projectFolders: nextProjectFolders
            }
          });
          setSettings(saved);
          setDraftSettings(saved);
          projectFoldersForRefresh = saved.projectFolders;
          setToast(`已关联 ${nextProjectFolders.length - settings.projectFolders.length} 个项目工作区`);
        }
      }
      if (result.inventoryRefreshRecommended && result.errors.length === 0) {
        await refreshInventory(projectFoldersForRefresh);
      }
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy("");
    }
  }

  async function saveSettings() {
    setBusy("保存设置");
    setError(null);
    try {
      const saved = await callApi<AppSettings>("save_settings", { settings: draftSettings });
      setSettings(saved);
      setDraftSettings(saved);
      setSettingsOpen(false);
      await refreshInventory(saved.projectFolders);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy("");
    }
  }

  async function saveProjectFolders(projectFolders: string[], busyLabel: string) {
    const nextSettings = {
      ...settings,
      projectFolders
    };
    setBusy(busyLabel);
    setError(null);
    try {
      if (hasRealBackend()) {
        const saved = await callApi<AppSettings>("save_settings", { settings: nextSettings });
        setSettings(saved);
        setDraftSettings(saved);
      } else {
        setSettings(nextSettings);
        setDraftSettings(nextSettings);
      }
      await refreshInventory(nextSettings.projectFolders);
      return nextSettings;
    } catch (reason) {
      setError(String(reason));
      return null;
    } finally {
      setBusy("");
    }
  }

  async function addProjectPath(path: string) {
    const valid = await validateProjectWorkspacePath(path);
    if (!valid) {
      setToast("该项目没有 skills，暂时无法添加");
      return;
    }

    const projectFolders = Array.from(new Set([...settings.projectFolders, path]));
    const saved = await saveProjectFolders(projectFolders, "关联项目工作区");
    if (!saved) return;
    setSelectedProjectFolder(path);
    switchSkillWorkspace("project");
    setView("skills");
    setDiscoveredProjects((current) =>
      current.map((candidate) => candidate.path === path ? { ...candidate, alreadyLinked: true } : candidate)
    );
  }

  async function validateProjectWorkspacePath(path: string) {
    if (settings.projectFolders.some((folder) => samePath(folder, path))) return true;
    if (!hasRealBackend()) return true;

    setBusy("检查项目 Skills");
    setError(null);
    try {
      const candidates = await callApi<ProjectWorkspaceCandidate[]>("discover_project_workspaces", {
        basePath: path
      });
      return candidates.some((candidate) => samePath(candidate.path, path) && candidate.skillCount > 0);
    } catch (reason) {
      setError(String(reason));
      return false;
    } finally {
      setBusy("");
    }
  }

  async function addProjectWorkspace() {
    const selected = await pickDirectory("关联项目工作区");
    if (typeof selected !== "string") return;
    await addProjectPath(selected);
  }

  async function chooseSyncProject() {
    if (!hasRealBackend()) {
      return "/Users/example/Projects/demo-project";
    }

    return pickDirectory("选择要同步的项目");
  }

  async function discoverProjectWorkspaces() {
    const selected = await pickDirectory("扫描发现项目工作区");
    if (typeof selected !== "string") return;
    const runId = discoveryRunRef.current + 1;
    discoveryRunRef.current = runId;
    setBusy("扫描发现项目工作区");
    setError(null);
    switchSkillWorkspace("project");
    setView("skills");
    setDiscoveryBasePath(selected);
    try {
      if (!hasRealBackend()) {
        if (discoveryRunRef.current !== runId) return;
        setDiscoveredProjects([]);
        setDiscoveryBasePath(null);
        setToast("该项目没有 skills，暂时无法添加");
        return;
      }
      const candidates = await callApi<ProjectWorkspaceCandidate[]>("discover_project_workspaces", {
        basePath: selected
      });
      if (discoveryRunRef.current !== runId) return;
      if (candidates.length === 0) {
        setDiscoveredProjects([]);
        setDiscoveryBasePath(null);
        setToast("该项目没有 skills，暂时无法添加");
        return;
      }
      setDiscoveredProjects(candidates);
    } catch (reason) {
      if (discoveryRunRef.current === runId) {
        setError(String(reason));
      }
    } finally {
      if (discoveryRunRef.current === runId) {
        setBusy("");
      }
    }
  }

  function closeProjectDiscovery() {
    discoveryRunRef.current += 1;
    setDiscoveredProjects([]);
    setDiscoveryBasePath(null);
    setBusy((current) => current === "扫描发现项目工作区" ? "" : current);
  }

  async function removeProjectWorkspace(folder: string) {
    const nextProjectFolders = settings.projectFolders.filter((item) => item !== folder);
    const saved = await saveProjectFolders(nextProjectFolders, "移除项目工作区");
    if (!saved) return;
    if (selectedProjectFolder === folder) {
      setSelectedProjectFolder(saved.projectFolders[0] ?? null);
    }
    setDiscoveredProjects((current) =>
      current.map((candidate) => candidate.path === folder ? { ...candidate, alreadyLinked: false } : candidate)
    );
  }

  function toggleSkill(id: string) {
    const selectionKey = selectionKeyFor(skillWorkspace, selectedProjectFolder, id);
    setSelectedSkillIds((current) => {
      const next = new Set(current);
      if (next.has(selectionKey)) next.delete(selectionKey);
      else next.add(selectionKey);
      return next;
    });
  }

  function openSelectedSkillsSync(mode: SyncMode) {
    if (selectedSkillIds.size === 0) return;
    setSyncQueuedSkillIds((current) => new Set([...current, ...selectedSkillIds]));
    setView("sync");
    setSyncMode(mode);
  }

  function clearSelectedSkills() {
    setSelectedSkillIds(new Set());
  }

  function switchSkillWorkspace(workspace: SkillWorkspace) {
    if (skillWorkspace !== workspace) {
      setSelectedSkillIds(new Set());
    }
    setSkillWorkspace(workspace);
  }

  async function refreshSkillsShUpdateChecks(skills: SkillRecord[], locks: Record<string, SkillLockEntry>) {
    if (!hasRealBackend() || skills.length === 0) return;
    const registryTrackedSkills: SkillRecord[] = [];
    for (const skill of skills) {
      const source = skillsShUpdateSource(skill, locks);
      if (!source) continue;
      // 检查分流（DD §8.5）：lock.sourceUrl 归一化 == skillRegistryUrl → 收集后
      // 走批量 check_registry_skill_updates（一次 clone 覆盖全部注册表条目，
      // 避免逐 skill N 倍 clone）；其余走既有单条 check_skills_sh_update。
      if (isRegistrySource(source.sourceUrl, settings.skillRegistryUrl)) {
        registryTrackedSkills.push(skill);
        continue;
      }
      setSkillUpdateChecks((current) => {
        if (current[skill.id]) return current;
        return { ...current, [skill.id]: { status: "checking" } };
      });
      try {
        const result = await callApi<SkillUpdateCheck>("check_skills_sh_update", {
          slug: skill.slug,
          entryPath: source.installation.entryPath,
          sourceUrl: source.sourceUrl,
          skillPath: source.lock.skillPath ?? null
        });
        setSkillUpdateChecks((current) => ({ ...current, [skill.id]: result }));
      } catch (reason) {
        setSkillUpdateChecks((current) => ({
          ...current,
          [skill.id]: failedUpdateCheck(reason)
        }));
      }
    }
    if (registryTrackedSkills.length === 0) return;
    try {
      const updates = await callApi<RegistrySkillUpdate[]>("check_registry_skill_updates");
      const updateBySlug = new Map(updates.map((entry) => [entry.slug, entry]));
      for (const skill of registryTrackedSkills) {
        const update = updateBySlug.get(skill.slug);
        if (!update) continue;
        setSkillUpdateChecks((current) => ({
          ...current,
          [skill.id]: update.updateAvailable
            ? { status: "available", message: update.remoteVersion ? `远程有新版本 ${update.remoteVersion}` : undefined }
            : { status: "current" }
        }));
      }
    } catch (reason) {
      for (const skill of registryTrackedSkills) {
        setSkillUpdateChecks((current) => ({
          ...current,
          [skill.id]: failedUpdateCheck(reason)
        }));
      }
    }
  }

  async function updateSkillsShSkill(skill: SkillRecord) {
    const source = skillsShUpdateSource(skill, skillLocks);
    if (!source) return;
    setBusy(`更新 ${skill.displayName}`);
    setError(null);
    setUpdatingSkillIds((current) => new Set(current).add(skill.id));
    try {
      // 执行分流（DD §8.5）：registry 来源走 update_registry_skill——既有
      // update_skills_sh_skill 有 is_agents_skill_path 守卫，中心库路径必拒；
      // 其余来源维持既有 command。
      if (isRegistrySource(source.sourceUrl, settings.skillRegistryUrl)) {
        await callApi<void>("update_registry_skill", { slug: skill.slug });
        setSkillUpdateChecks((current) => ({ ...current, [skill.id]: { status: "current" } }));
      } else {
        const result = await callApi<SkillUpdateCheck>("update_skills_sh_skill", {
          slug: skill.slug,
          entryPath: source.installation.entryPath,
          sourceUrl: source.sourceUrl,
          skillPath: source.lock.skillPath ?? null
        });
        setSkillUpdateChecks((current) => ({ ...current, [skill.id]: result }));
      }
      await refreshInventory();
    } catch (reason) {
      setError(String(reason));
      setSkillUpdateChecks((current) => ({
        ...current,
        [skill.id]: failedUpdateCheck(reason)
      }));
    } finally {
      setUpdatingSkillIds((current) => {
        const next = new Set(current);
        next.delete(skill.id);
        return next;
      });
      setBusy("");
    }
  }

  async function removeSkillPaths(paths: string[], options?: { confirmMessage?: string }) {
    const uniquePaths = uniqueSkillPaths(paths);
    if (uniquePaths.length === 0 || removing) return;

    const preview = uniquePaths
      .slice(0, 8)
      .map((path) => compactPath(path))
      .join("\n");
    const more = uniquePaths.length > 8 ? `\n…另有 ${uniquePaths.length - 8} 个路径` : "";
    const defaultMessage = uniquePaths.length === 1
      ? `确定删除以下路径吗？\n\n${preview}\n\n此操作不可撤销。`
      : `确定删除以下 ${uniquePaths.length} 个路径吗？\n\n${preview}${more}\n\n此操作不可撤销。`;
    const confirmed = await askConfirm(options?.confirmMessage ?? defaultMessage, "确认移除");
    if (!confirmed) return;

    setRemoving(true);
    setBusy(uniquePaths.length === 1 ? "正在移除路径" : `正在移除 ${uniquePaths.length} 个路径`);
    setError(null);
    try {
      if (!hasRealBackend()) {
        setToast("演示模式无法删除本机路径");
        return;
      }
      const result = await callApi<{ removed: string[]; failed: { path: string; error: string }[] }>(
        "remove_skill_entries",
        { paths: uniquePaths }
      );
      if (result.failed.length > 0) {
        setError(result.failed.map((item) => `${compactPath(item.path)}：${item.error}`).join("；"));
      }
      if (result.removed.length > 0) {
        setToast(
          result.removed.length === 1
            ? `已移除 ${compactPath(result.removed[0])}`
            : `已移除 ${result.removed.length} 个路径`
        );
      }
      await refreshInventory();
    } catch (reason) {
      setError(String(reason));
    } finally {
      setRemoving(false);
      setBusy("");
    }
  }

  async function removeSelectedWorkspaceSkills() {
    const paths = selectedSkills.flatMap((skill) => removablePathsForSkill(skill, skillWorkspace));
    const libraryCascade = skillWorkspace === "library"
      && selectedSkills.some((skill) => libraryReferencePaths(skill).length > 0);
    const confirmMessage = libraryCascade
      ? `确定移除已选 ${selectedSkills.length} 个 Skill 的中心库副本，并一并删除其引用位置吗？\n\n共 ${uniqueSkillPaths(paths).length} 个路径。此操作不可撤销。`
      : undefined;
    await removeSkillPaths(paths, { confirmMessage });
  }

  function removeDetailPaths(skill: SkillRecord, paths: string[]) {
    const expanded = paths.flatMap((path) => expandRemovalPaths(skill, path, skillWorkspace));
    const hitsLibrary = skillWorkspace === "library"
      && skill.canonicalPath
      && paths.some((path) => samePath(path, skill.canonicalPath!));
    const refCount = libraryReferencePaths(skill).length;
    const confirmMessage = hitsLibrary && refCount > 0
      ? `确定删除中心库路径，并一并删除 ${refCount} 个引用位置吗？\n\n${uniqueSkillPaths(expanded).map((path) => compactPath(path)).join("\n")}\n\n此操作不可撤销。`
      : undefined;
    void removeSkillPaths(expanded, { confirmMessage });
  }

  return (
    <main className="app-shell">
      <header className="top-nav">
        <div className="tab-bar" aria-label="主导航">
          <button
            className="nav-avatar"
            onClick={() => setSettingsOpen(true)}
            title="设置"
            type="button"
          >
            <img src={appLogo} alt="Oh My Skills" />
          </button>
          <TabButton active={view === "skills"} onClick={() => setView("skills")}>
            发现 Skills
          </TabButton>
          <TabButton active={view === "sync"} onClick={() => setView("sync")}>
            同步 Skills
          </TabButton>
          <TabButton active={view === "workflows"} onClick={() => setView("workflows")}>
            工作流
          </TabButton>
        </div>


      </header>

      {error && (
        <div className="banner error">
          <AlertTriangle size={17} />
          <span>{error}</span>
        </div>
      )}

      {toast && <div className="toast" role="status">{toast}</div>}

      <section className="content-frame">
        {view === "skills" && !hasScanned ? (
          <div className="agents-page empty-state-page">
            <AgentDiscoveryEmptyState
              busy={busy}
              previouslyScanned={previouslyScanned}
              onScan={() => void refreshInventory()}
              onSkip={() => setHasScanned(true)}
            />
          </div>
        ) : view === "skills" ? (
          <SkillsView
            agents={installedAgents}
            skills={filteredSkills}
            allSkills={allSkills}
            sourceSkills={visibleSourceSkills}
            skillLocks={skillLocks}
            skillUpdateChecks={skillUpdateChecks}
            updatingSkillIds={updatingSkillIds}
            workspace={skillWorkspace}
            projectFolders={projectFolders}
            selectedProjectFolder={selectedProjectFolder}
            discoveredProjects={discoveredProjects}
            discoveryBasePath={discoveryBasePath}
            discovering={busy === "扫描发现项目工作区"}
            selectedSkill={selectedSkill}
            selectedSkillIds={scopedSelectedSkillIds}
            selectedSkills={selectedSkills}
            query={query}
            agentFilter={agentFilter}
            settings={settings}
            removing={removing}
            onQuery={setQuery}
            onAgentFilter={setAgentFilter}
            onWorkspace={(workspace) => {
              switchSkillWorkspace(workspace);
              setSelectedSkillId(null);
              setQuery("");
              setAgentFilter("all");
            }}
            onSelectProject={(folder) => {
              setSelectedProjectFolder(folder);
              setSelectedSkillId(null);
              setQuery("");
              setAgentFilter("all");
            }}
            onSelectSkill={setSelectedSkillId}
            onToggleSkill={toggleSkill}
            onUpdateSkill={updateSkillsShSkill}
            onAdoptSelected={() => openSelectedSkillsSync("managed")}
            onQuickSyncSelected={() => openSelectedSkillsSync("quick")}
            onRemoveSelected={() => void removeSelectedWorkspaceSkills()}
            onRemovePaths={(skill, paths) => removeDetailPaths(skill, paths)}
            onClearSelection={clearSelectedSkills}
            onRefresh={() => void refreshInventory()}
            onAddProject={() => void addProjectWorkspace()}
            onDiscoverProjects={() => void discoverProjectWorkspaces()}
            onCloseDiscovery={closeProjectDiscovery}
            onLinkDiscoveredProject={(path) => void addProjectPath(path)}
            onRemoveProject={(folder) => void removeProjectWorkspace(folder)}
          />
        ) : null}

        {view === "sync" && (
          <SyncView
            agents={installedAgents.length ? installedAgents : agents}
            queuedSkills={queuedSkills}
            settings={settings}
            plan={syncPlan}
            applyResult={applyResult}
            busy={Boolean(busy)}
            syncMode={syncMode}
            onSyncModeChange={setSyncMode}
            selectedTargetIds={syncSelectedTargetIds}
            onSelectedTargetIdsChange={setSyncSelectedTargetIds}
            onRemoveSkill={(id) => {
              setSyncQueuedSkillIds((current) => {
                const next = new Set(current);
                next.delete(id);
                if (next.size === current.size) {
                  for (const key of current) {
                    if (selectionSkillId(key) === id) next.delete(key);
                  }
                }
                return next;
              });
            }}
            onPreviewGlobal={(targets, replacements) => void previewSkillsSync(queuedSkills, targets, replacements)}
            onPreviewProject={(targets, replacements) => void previewSkillsSync(queuedSkills, targets, replacements)}
            onPreviewQuick={(method, targets) => void previewQuickMigration(queuedSkills, method, targets)}
            onChooseProject={chooseSyncProject}
            onApply={() => void applyPlan()}
            onGoSkills={() => setView("skills")}
          />
        )}

        {view === "workflows" && (
          <WorkflowsView
            agents={installedAgents.length ? installedAgents : agents}
            librarySkills={librarySkills}
            skillLocks={skillLocks}
            settings={settings}
            onRequestScan={() => void refreshInventory()}
          />
        )}
      </section>

      {settingsOpen && (
        <SettingsSheet
          settings={draftSettings}
          inventory={inventory}
          agents={agents}
          onChange={setDraftSettings}
          onClose={() => {
            setDraftSettings(settings);
            setSettingsOpen(false);
          }}
          onSave={() => void saveSettings()}
        />
      )}
    </main>
  );
}

function selectionKeyFor(workspace: SkillWorkspace, projectPath: string | null, skillId: string) {
  return [
    workspace,
    workspace === "project" ? projectPath ?? "" : "",
    skillId
  ].join(selectionKeySeparator);
}

function selectionParts(key: string) {
  const [workspace, projectPath, skillId] = key.split(selectionKeySeparator);
  return {
    workspace: workspace as SkillWorkspace | undefined,
    projectPath: projectPath || null,
    skillId: skillId ?? key
  };
}

function selectionSkillId(key: string) {
  return selectionParts(key).skillId;
}

function removablePathsForSkill(skill: SkillRecord, workspace: SkillWorkspace) {
  if (workspace === "global") return scopedInstallationPaths(skill, "global");
  if (workspace === "project") return scopedInstallationPaths(skill, "project");
  // Library: central copy + all symlink references that point at it.
  return uniqueSkillPaths([
    ...(skill.canonicalPath ? [skill.canonicalPath] : []),
    ...libraryReferencePaths(skill)
  ]);
}

function expandRemovalPaths(skill: SkillRecord, path: string, workspace: SkillWorkspace) {
  if (
    workspace === "library"
    && skill.canonicalPath
    && samePath(path, skill.canonicalPath)
  ) {
    return uniqueSkillPaths([skill.canonicalPath, ...libraryReferencePaths(skill)]);
  }
  return [path];
}

function scopedInstallationPaths(skill: SkillRecord, scope: "global" | "project") {
  const paths: string[] = [];
  for (const installation of skill.installations) {
    if (installation.scope !== scope || !installation.entryPath) continue;
    if (paths.some((path) => samePath(path, installation.entryPath))) continue;
    paths.push(installation.entryPath);
  }
  return paths;
}

function libraryReferencePaths(skill: SkillRecord) {
  const paths: string[] = [];
  for (const installation of skill.installations) {
    if (!installation.entryPath || !isCentralLibraryReference(skill, installation)) continue;
    if (paths.some((path) => samePath(path, installation.entryPath))) continue;
    paths.push(installation.entryPath);
  }
  return paths;
}

function uniqueSkillPaths(paths: string[]) {
  const unique: string[] = [];
  for (const path of paths) {
    if (!path.trim()) continue;
    if (unique.some((item) => samePath(item, path))) continue;
    unique.push(path);
  }
  return unique;
}

function projectFoldersFromTargets(targets: AgentTarget[]) {
  return mergeProjectFolders(
    [],
    targets
      .filter((target) => target.scope === "project")
      .map((target) => target.projectPath?.trim() ?? "")
      .filter(Boolean)
  );
}

function mergeProjectFolders(current: string[], additions: string[]) {
  const next = [...current];
  for (const path of additions) {
    const trimmed = path.trim();
    if (!trimmed) continue;
    if (!next.some((existing) => samePath(existing, trimmed))) {
      next.push(trimmed);
    }
  }
  return next;
}

function selectedSkillsForKeys(
  keys: Set<string>,
  allSkills: SkillRecord[],
  globalSkills: SkillRecord[],
  projectFolders: string[],
  librarySkills: SkillRecord[]
) {
  const selected: SkillRecord[] = [];

  for (const key of keys) {
    const { workspace, projectPath, skillId } = selectionParts(key);
    const skill = skillForSelection(workspace, projectPath, skillId, allSkills, globalSkills, projectFolders, librarySkills);
    if (skill) selected.push({ ...skill, selectionKey: key });
  }

  return selected;
}

function filterValidSelectionKeys(keys: Set<string>, allSkills: SkillRecord[], projectFolders: string[]) {
  const globalSkills = allSkills.filter((skill) => skill.installations.some((item) => item.scope === "global"));
  const librarySkills = allSkills.filter((skill) => skill.canonicalStatus === "imported");
  const valid = new Set<string>();

  for (const key of keys) {
    const { workspace, projectPath, skillId } = selectionParts(key);
    if (skillForSelection(workspace, projectPath, skillId, allSkills, globalSkills, projectFolders, librarySkills)) {
      valid.add(key);
    }
  }

  return valid;
}

function skillForSelection(
  workspace: SkillWorkspace | undefined,
  projectPath: string | null,
  skillId: string,
  allSkills: SkillRecord[],
  globalSkills: SkillRecord[],
  projectFolders: string[],
  librarySkills: SkillRecord[]
) {
  if (workspace === "global") {
    return globalSkills.find((skill) => skill.id === skillId) ?? null;
  }

  if (workspace === "library") {
    return librarySkills.find((skill) => skill.id === skillId) ?? null;
  }

  if (workspace === "project" && projectPath && projectFolders.includes(projectPath)) {
    return projectSkillsForFolder(allSkills, projectPath).find((skill) => skill.id === skillId) ?? null;
  }

  return null;
}
