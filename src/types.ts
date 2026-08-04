export type Settings = {
  libraryPath: string;
  projectFolders: string[];
  customRoots: CustomRoot[];
  showRawPaths: boolean;
  language: string;
  workflowRegistryUrl?: string;
  skillRegistryUrl?: string;
  githubUsername?: string;
  /** 仅 save 入参方向：用户新输入的 token。返回体永不携带该键（裁剪为 hasGithubToken）。 */
  githubToken?: string;
  /** 仅 save 入参方向：true = 显式清除已存 token（优先级高于 githubToken 替换）。 */
  clearGithubToken?: boolean;
  /** 仅返回方向：当前是否已配置 token。 */
  hasGithubToken?: boolean;
};

export type CustomRoot = {
  id: string;
  label: string;
  path: string;
};

export type AgentDetectionSource = {
  kind: string;
  label: string;
  path: string;
  exists: boolean;
};

export type AgentRecord = {
  id: string;
  label: string;
  globalRoots: string[];
  projectRoots: string[];
  activeSignals: string[];
  cliNames: string[];
  appPaths: string[];
  symlinkSupport: boolean;
  priority: number;
  installed: boolean;
  status: string;
  detectionSources: AgentDetectionSource[];
  skillRoots: ResolvedRoot[];
  skillEntryCount: number;
};

export type ResolvedRoot = {
  agentId: string;
  agentLabel: string;
  scope: string;
  path: string;
  exists: boolean;
  active: boolean;
  orphaned: boolean;
};

export type ProjectWorkspaceAgentRoot = {
  agentId: string;
  agentLabel: string;
  path: string;
  skillCount: number;
};

export type ProjectWorkspaceCandidate = {
  name: string;
  path: string;
  agentRoots: ProjectWorkspaceAgentRoot[];
  skillCount: number;
  alreadyLinked: boolean;
};

export type SkillFrontmatter = {
  name?: string;
  description?: string;
  license?: string;
  allowedTools: string[];
  metadata: Record<string, string>;
};

export type SkillIssue = {
  code: string;
  severity: string;
  message: string;
  path?: string;
  agentId?: string;
};

export type SkillInstallation = {
  id: string;
  agentId: string;
  agentLabel: string;
  scope: string;
  rootPath: string;
  entryPath: string;
  realPath?: string;
  symlinkTarget?: string;
  isSymlink: boolean;
  brokenSymlink: boolean;
  hash?: string;
  frontmatter?: SkillFrontmatter;
  status: string;
  issues: SkillIssue[];
};

export type SkillRecord = {
  id: string;
  selectionKey?: string;
  slug: string;
  displayName: string;
  description?: string;
  canonicalStatus: string;
  canonicalPath?: string;
  canonicalHash?: string;
  installations: SkillInstallation[];
  missingAgents: string[];
  issues: SkillIssue[];
  conflict: boolean;
};

export type InventorySnapshot = {
  agents: AgentRecord[];
  roots: ResolvedRoot[];
  skills: SkillRecord[];
  issues: SkillIssue[];
  scannedAt: string;
  appDataPath: string;
  libraryPath: string;
};

export type SkillContent = {
  path: string;
  title: string;
  frontmatter?: SkillFrontmatter;
  content: string;
  markdownBody: string;
};

export type SkillLockEntry = {
  source?: string;
  sourceType?: string;
  sourceUrl?: string;
  skillPath?: string;
  installedAt?: string;
  updatedAt?: string;
};

export type SkillUpdateCheck = {
  status: string;
  message?: string;
  localHash?: string;
  remoteHash?: string;
};

export type InstallationRef = {
  installationId: string;
  entryPath: string;
  slug: string;
};

export type AgentTarget = {
  agentId: string;
  scope?: string;
  projectPath?: string;
};

export type SyncReplacement = {
  agentId: string;
  skillId: string;
  targetPath: string;
};

export type SyncOperation = {
  id: string;
  opType: string;
  status: string;
  sourcePath?: string;
  targetPath?: string;
  backupPath?: string;
  message: string;
  agentId?: string;
  skillId?: string;
};

export type SyncPlan = {
  planId: string;
  kind: string;
  riskLevel: string;
  operations: SyncOperation[];
  preconditions: string[];
  blockedConflicts: string[];
  createdAt: string;
};

export type ApplyResult = {
  planId: string;
  appliedOperations: string[];
  skippedOperations: string[];
  errors: string[];
  inventoryRefreshRecommended: boolean;
};

// ===== 工作流（round-2，镜像 src-tauri workflow.rs / workflow_registry.rs / workflow_use.rs）=====

export type Workflow = {
  name: string;
  slug: string;
  version: string;
  description: string;
  author?: string;
  tags: string[];
  icon?: string;
  groups: WorkflowGroup[];
  steps: WorkflowStep[];
};

export type WorkflowGroup = {
  id: string;
  name: string;
};

export type WorkflowStep = {
  name: string;
  group: string;
  description: string;
  skills: StepSkill[];
};

/** StepSkill 为 untagged 枚举：SkillRef 或 { placeholder }，靠 "placeholder" in skill 判别。 */
export type StepSkill = SkillRef | WorkflowStepPlaceholder;

export type WorkflowStepPlaceholder = {
  placeholder: string;
};

export type SkillRef = {
  /** v1 仅 "github" */
  sourceType: string;
  sourceUrl: string;
  slug: string;
  skillPath?: string;
};

export type InstalledWorkflow = {
  slug: string;
  name: string;
  version: string;
  description: string;
  author?: string;
  tags: string[];
  icon?: string;
  stepCount: number;
  hasPlaceholder: boolean;
  error?: string;
};

export type RemoteWorkflowSummary = {
  slug: string;
  name: string;
  version: string;
  description: string;
  author?: string;
  tags: string[];
  icon?: string;
  path: string;
  installed: boolean;
};

/** StepSkillStatus 的 serde 外部标签形状："ready" | "missing" | { placeholder: string } */
export type StepSkillStatus = "ready" | "missing" | { placeholder: string };

export type StepSkillView = {
  /** "ref" | "placeholder" */
  kind: string;
  slug?: string;
  sourceUrl?: string;
  skillPath?: string;
  placeholder?: string;
};

/** get_workflow_detail 返回的每步视图（UI 归一化后的形状，与 workflow.steps 对齐）。 */
export type WorkflowDetailStep = {
  name: string;
  group: string;
  description: string;
  skills: WorkflowDetailSkill[];
};

export type WorkflowDetailSkill = {
  kind: string;
  slug?: string;
  sourceUrl?: string;
  skillPath?: string;
  placeholder?: string;
  status: StepSkillStatus;
};

/**
 * get_workflow_detail 的线形状：statuses 与 workflow.steps 对齐的
 * [StepSkillView, StepSkillStatus] 元组嵌套（Rust Vec<Vec<(view, status)>> 直转）。
 */
export type WorkflowDetail = {
  workflow: Workflow;
  statuses: [StepSkillView, StepSkillStatus][][];
};

/** preview_use_workflow 的输出形态（OutputForm serde camelCase）。 */
export type OutputForm = "entryManifest" | "packagedSkill";

// ===== 工作流 v2（round-3，镜像 workflow_update.rs / workflow_share.rs / workflow_push.rs）=====

/** check_workflow_updates 的返回体（WorkflowUpdateStatus serde camelCase）。 */
export type WorkflowUpdateStatus = {
  slug: string;
  state: WorkflowUpdateState;
  localVersion?: string;
  remoteVersion?: string;
};

/** WorkflowUpdateState 为 internally tagged 枚举，判别字段 `kind`（local / upToDate / updateAvailable / modified）。 */
export type WorkflowUpdateState =
  | { kind: "local" }
  | { kind: "upToDate" }
  | { kind: "updateAvailable"; remoteVersion: string }
  | { kind: "modified"; remoteChanged: boolean; remoteVersion?: string };

/** export_workflow_package 的返回体（ExportPackage serde camelCase）。 */
export type ExportPackage = {
  filename: string;
  base64: string;
};

/** import_workflow_package 的返回体（ImportResult serde camelCase，hadSource=包内带来源记录）。 */
export type ImportResult = {
  slug: string;
  hadSource: boolean;
};

/** push_workflow_to_registry 的返回体（PushResult serde camelCase）。 */
export type PushResult = {
  commitHash: string;
  registryUrl: string;
};

/** ContributeOutcome 为 internally tagged 枚举，判别字段 `status`（noToken / needFork / ready）。 */
export type ContributeOutcome =
  | { status: "noToken" }
  | { status: "needFork"; forkPageUrl: string }
  | { status: "ready"; compareUrl: string; branch: string };

// ===== skill 注册表（round-3，镜像 src-tauri skill_registry.rs / workflow_push.rs）=====

/** list_remote_skills 的返回体（RemoteSkillSummary serde camelCase）。 */
export type RemoteSkillSummary = {
  slug: string;
  name: string;
  version: string;
  description: string;
  author?: string;
  tags: string[];
  icon?: string;
  path: string;
  installed: boolean;
};

/** check_registry_skill_updates 的返回体（RegistrySkillUpdate serde camelCase，批量一次 clone）。 */
export type RegistrySkillUpdate = {
  slug: string;
  updateAvailable: boolean;
  remoteVersion?: string;
};
