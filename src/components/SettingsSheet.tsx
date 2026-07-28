import { Check, Plus, Trash2, X } from "lucide-react";
import { useEffect, useState } from "react";
import { AgentIcon, StatusPill } from "./shared";
import { pickDirectory } from "../lib/shell";
import { agentSignalSummary, compactPath } from "../lib/skillUtils";
import type { AgentRecord, InventorySnapshot, Settings as AppSettings, SkillRecord } from "../types";

export function SettingsSheet({
  settings,
  inventory,
  agents = [],
  onChange,
  onClose,
  onSave
}: {
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
                  <button className="settings-text-button" onClick={() => void addCustomRoot()} type="button">
                    <Plus size={14} />
                    添加路径
                  </button>
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
          <button className="primary-button" onClick={onSave} type="button">
            <Check size={16} />
            保存
          </button>
        </footer>
      </aside>
    </div>
  );
}
