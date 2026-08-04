import { Check, Plus, Trash2, X } from "lucide-react";
import { useEffect, useState } from "react";
import { AgentIcon, StatusPill } from "./shared";
import { pickDirectory } from "../lib/shell";
import { agentSignalSummary, compactPath } from "../lib/skillUtils";
import type { AgentRecord, InventorySnapshot, Settings as AppSettings, SkillRecord } from "../types";

/** 注册表 URL 校验（评审门 F3）：留空合法（官方缺省）；非空必须是 GitHub 仓库
 * 地址（owner/repo），userinfo 与非 GitHub 来源一并拒绝。与后端
 * normalize_github_url 同规则的前置拦截。 */
function registryUrlError(value: string | undefined): string | null {
  const trimmed = (value ?? "").trim();
  if (!trimmed) return null;
  const stripped = trimmed.replace(/\/+$/, "").replace(/\.git$/, "");
  const prefix = ["https://github.com/", "git@github.com:", "github.com/"].find((item) =>
    stripped.startsWith(item)
  );
  const path = prefix ? stripped.slice(prefix.length) : stripped;
  if (path.includes("@") || (!prefix && stripped.includes("://"))) {
    return "注册表 URL 仅支持 GitHub 仓库，且不允许携带账号信息";
  }
  const parts = path.split("/").filter(Boolean);
  return parts.length === 2 ? null : "注册表 URL 需为 GitHub 仓库地址（owner/repo）";
}

const urlErrorStyle = { color: "#b42318", fontSize: 12 } as const;

export function SettingsSheet({
  readonly,
  settings,
  inventory,
  agents = [],
  onChange,
  onClose,
  onSave
}: {
  /** 只读模式（DD §8.5）：隐藏设置保存与添加路径（目录浏览入口）。 */
  readonly: boolean;
  settings: AppSettings;
  inventory: InventorySnapshot | null;
  agents?: AgentRecord[];
  skills?: SkillRecord[];
  onChange: (settings: AppSettings) => void;
  onClose: () => void;
  onSave: () => void;
}) {
  const [settingsTab, setSettingsTab] = useState<"data" | "agents">("data");
  const appDataPath = inventory?.appDataPath || "";
  const customRoots = settings.customRoots ?? [];
  const workflowRegistryUrlError = registryUrlError(settings.workflowRegistryUrl);
  const skillRegistryUrlError = registryUrlError(settings.skillRegistryUrl);
  const hasRegistryUrlError = Boolean(workflowRegistryUrlError || skillRegistryUrlError);

  useEffect(() => {
    const onEsc = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onEsc);
    return () => document.removeEventListener("keydown", onEsc);
  }, [onClose]);

  async function addCustomRoot() {
    const selected = await pickDirectory("选择要扫描的 Skills 路径");
    if (!selected?.trim()) return;
    pushCustomRoot(selected.trim());
  }

  function pushCustomRoot(path: string) {
    const exists = customRoots.some((root) => root.path === path);
    if (exists) return;
    const label = path.split(/[/\\]/).filter(Boolean).pop() || "自定义路径";
    onChange({
      ...settings,
      customRoots: [
        ...customRoots,
        {
          id: crypto.randomUUID(),
          label,
          path
        }
      ]
    });
  }

  function removeCustomRoot(id: string) {
    onChange({
      ...settings,
      customRoots: customRoots.filter((root) => root.id !== id)
    });
  }

  function updateCustomRootLabel(id: string, label: string) {
    onChange({
      ...settings,
      customRoots: customRoots.map((root) => (root.id === id ? { ...root, label } : root))
    });
  }

  return (
    <div className="sheet-backdrop" onClick={onClose}>
      <aside className="settings-sheet" role="dialog" aria-modal="true" aria-label="设置" onClick={(e) => e.stopPropagation()}>
        <header className="settings-header">
          <div className="settings-header-top">
            <h1>设置</h1>
            <button className="settings-close" onClick={onClose} title="关闭" type="button">
              <X size={16} />
            </button>
          </div>
          <div className="settings-tabs" role="tablist" aria-label="设置分类">
            <button
              role="tab"
              aria-selected={settingsTab === "data"}
              className={settingsTab === "data" ? "active" : ""}
              onClick={() => setSettingsTab("data")}
              type="button"
            >
              数据
            </button>
            <button
              role="tab"
              aria-selected={settingsTab === "agents"}
              className={settingsTab === "agents" ? "active" : ""}
              onClick={() => setSettingsTab("agents")}
              type="button"
            >
              Agent
            </button>
          </div>
        </header>

        <div className="settings-content">
          {settingsTab === "data" && (
            <div className="settings-list" role="list">
              <div className="settings-row settings-row-stack" role="listitem">
                <div className="settings-row-copy">
                  <strong>中心库</strong>
                  <span>保存规范 Skill 副本；同步时从这里链接或复制到目标 Agent。</span>
                </div>
                <input
                  className="settings-path-input"
                  value={settings.libraryPath}
                  onChange={(event) => onChange({ ...settings, libraryPath: event.target.value })}
                  spellCheck={false}
                />
              </div>

              <div className="settings-row settings-row-stack" role="listitem">
                <div className="settings-row-copy">
                  <strong>工作流注册表 URL</strong>
                  <span>远程工作流列表来源的 Git 仓库地址；留空使用官方注册表，修改后下次刷新远程列表生效。</span>
                </div>
                <input
                  className="settings-path-input"
                  value={settings.workflowRegistryUrl ?? ""}
                  onChange={(event) => onChange({ ...settings, workflowRegistryUrl: event.target.value })}
                  placeholder="https://github.com/Pgooone/oh-my-skills-workflows.git"
                  spellCheck={false}
                  aria-invalid={Boolean(workflowRegistryUrlError)}
                />
                {workflowRegistryUrlError && <span style={urlErrorStyle}>{workflowRegistryUrlError}</span>}
              </div>

              <div className="settings-row settings-row-stack" role="listitem">
                <div className="settings-row-copy">
                  <strong>Skill 注册表 URL</strong>
                  <span>远程 Skill 列表来源的 Git 仓库地址；留空使用官方注册表，修改后下次刷新远程列表生效。</span>
                </div>
                <input
                  className="settings-path-input"
                  value={settings.skillRegistryUrl ?? ""}
                  onChange={(event) => onChange({ ...settings, skillRegistryUrl: event.target.value })}
                  placeholder="https://github.com/Pgooone/oh-my-skills-skills.git"
                  spellCheck={false}
                  aria-invalid={Boolean(skillRegistryUrlError)}
                />
                {skillRegistryUrlError && <span style={urlErrorStyle}>{skillRegistryUrlError}</span>}
              </div>

              <div className="settings-row settings-row-stack" role="listitem">
                <div className="settings-row-copy">
                  <strong>GitHub 用户名</strong>
                  <span>一键贡献时定位你的 fork 仓库（github.com/&lt;用户名&gt;/…）。</span>
                </div>
                <input
                  className="settings-path-input"
                  value={settings.githubUsername ?? ""}
                  onChange={(event) => onChange({ ...settings, githubUsername: event.target.value })}
                  spellCheck={false}
                />
              </div>

              <div className="settings-row settings-row-stack" role="listitem">
                <div className="settings-row-copy">
                  <strong>GitHub Token</strong>
                  <span>
                    推送注册表与一键贡献时使用；以明文存储于本机 settings.json（与 gh CLI 同级），环境变量
                    OMS_GITHUB_TOKEN 优先。
                  </span>
                </div>
                <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
                  <input
                    className="settings-path-input"
                    type="password"
                    value={settings.githubToken ?? ""}
                    onChange={(event) =>
                      onChange({ ...settings, githubToken: event.target.value, clearGithubToken: false })
                    }
                    placeholder={settings.hasGithubToken ? "已配置（为安全起见不显示），输入以替换" : "未配置"}
                    spellCheck={false}
                    autoComplete="new-password"
                  />
                  {settings.hasGithubToken && !settings.clearGithubToken && (
                    <button
                      className="secondary-button"
                      onClick={() => onChange({ ...settings, githubToken: undefined, clearGithubToken: true })}
                      type="button"
                    >
                      清除
                    </button>
                  )}
                </div>
                {settings.clearGithubToken && <span style={urlErrorStyle}>保存后将清除已配置的 token。</span>}
              </div>

              <div className="settings-row" role="listitem">
                <div className="settings-row-copy">
                  <strong>显示原始文件路径</strong>
                  <span>在 Skill 详情中展示未折叠的完整路径。</span>
                </div>
                <button
                  aria-checked={settings.showRawPaths}
                  className={`settings-toggle ${settings.showRawPaths ? "on" : ""}`}
                  onClick={() => onChange({ ...settings, showRawPaths: !settings.showRawPaths })}
                  role="switch"
                  type="button"
                >
                  <i />
                </button>
              </div>

              <div className="settings-row" role="listitem">
                <div className="settings-row-copy">
                  <strong>应用数据</strong>
                  <span>计划、缓存与同步历史所在目录。</span>
                </div>
                <code className="settings-inline-path" title={appDataPath || undefined}>
                  {appDataPath ? compactPath(appDataPath) : "尚未扫描"}
                </code>
              </div>
            </div>
          )}

          {settingsTab === "agents" && (
            <div className="settings-agents-pane">
              <section className="settings-block">
                <div className="settings-block-heading">
                  <h2>内置支持的 Agent</h2>
                  <p>Oh My Skills 默认识别的工具；安装状态来自本机扫描。</p>
                </div>
                {agents.length > 0 ? (
                  <div className="settings-list settings-agent-list" role="list">
                    {agents.map((agent) => {
                      const signal = agentSignalSummary(agent);
                      return (
                        <div className="settings-row settings-agent-row" key={agent.id} role="listitem">
                          <div className="settings-agent-identity">
                            <AgentIcon agent={agent} />
                            <div className="settings-row-copy">
                              <strong>{agent.label}</strong>
                              <span>
                                {agent.installed
                                  ? signal || "已安装"
                                  : "未检测到安装"}
                              </span>
                            </div>
                          </div>
                          <StatusPill status={agent.installed ? "installed" : "not-installed"} />
                        </div>
                      );
                    })}
                  </div>
                ) : (
                  <div className="settings-agent-empty">尚未扫描到 Agent 列表，请先返回主界面重新扫描。</div>
                )}
              </section>

              <section className="settings-block">
                <div className="settings-block-heading settings-block-heading-row">
                  <div>
                    <h2>自定义扫描路径</h2>
                    <p>添加额外的 Skills 根目录，用于扫描未内置的 Agent 或自定义位置。</p>
                  </div>
                  {!readonly && (
                    <button className="settings-text-button" onClick={() => void addCustomRoot()} type="button">
                      <Plus size={14} />
                      添加路径
                    </button>
                  )}
                </div>

                {customRoots.length > 0 ? (
                  <div className="settings-list" role="list">
                    {customRoots.map((root) => (
                      <div className="settings-row settings-custom-root-row" key={root.id} role="listitem">
                        <div className="settings-custom-root-fields">
                          <input
                            className="settings-label-input"
                            value={root.label}
                            onChange={(event) => updateCustomRootLabel(root.id, event.target.value)}
                            placeholder="显示名称"
                            spellCheck={false}
                          />
                          <code className="settings-custom-root-path" title={root.path}>
                            {compactPath(root.path)}
                          </code>
                        </div>
                        <button
                          className="meta-icon-button danger"
                          onClick={() => removeCustomRoot(root.id)}
                          title="移除此路径"
                          type="button"
                        >
                          <Trash2 size={14} />
                        </button>
                      </div>
                    ))}
                  </div>
                ) : (
                  <div className="settings-agent-empty settings-custom-empty">
                    还没有自定义路径。添加后会在下次扫描时纳入。
                  </div>
                )}
              </section>
            </div>
          )}
        </div>

        <footer className="sheet-actions">
          <button className="secondary-button" onClick={onClose} type="button">取消</button>
          {!readonly && (
            <button
              className="primary-button"
              onClick={onSave}
              type="button"
              disabled={hasRegistryUrlError}
              title={hasRegistryUrlError ? "请先修正注册表 URL" : undefined}
            >
              <Check size={16} />
              保存
            </button>
          )}
        </footer>
      </aside>
    </div>
  );
}
