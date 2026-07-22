import { Check, X } from "lucide-react";
import { useEffect, useState } from "react";
import { AgentIcon, StatusPill } from "./shared";
import { agentSignalSummary, agentSkillCount, compactPath } from "../lib/skillUtils";
import type { AgentRecord, InventorySnapshot, Settings as AppSettings, SkillRecord } from "../types";

export function SettingsSheet({
  settings,
  inventory,
  agents = [],
  skills = [],
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

  const installedCount = agents.length;
  const skillsForCount = skills.length ? skills : (inventory?.skills ?? []);
  const appDataPath = inventory?.appDataPath || "";

  useEffect(() => {
    const onEsc = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onEsc);
    return () => document.removeEventListener("keydown", onEsc);
  }, [onClose]);

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
              {installedCount > 0 ? (
                <div className="settings-list settings-agent-list" role="list">
                  {agents.map((agent) => {
                    const count = agentSkillCount(agent.id, skillsForCount);
                    const signal = agentSignalSummary(agent);
                    return (
                      <div className="settings-row settings-agent-row" key={agent.id} role="listitem">
                        <div className="settings-agent-identity">
                          <AgentIcon agent={agent} />
                          <div className="settings-row-copy">
                            <strong>{agent.label}</strong>
                            <span>{signal || "未检测到 CLI / App / 插件信号"}</span>
                          </div>
                        </div>
                        <div className="settings-agent-meta">
                          <span className="settings-agent-count">
                            <strong>{count}</strong>
                            <small>Skills</small>
                          </span>
                          <StatusPill status={agent.status} />
                        </div>
                      </div>
                    );
                  })}
                </div>
              ) : (
                <div className="settings-agent-empty">暂未发现本地有可用 Agent</div>
              )}
              <p className="settings-agent-hint">已发现 {installedCount} 个已安装 Agent。</p>
            </div>
          )}
        </div>

        {settingsTab === "data" ? (
          <footer className="sheet-actions">
            <button className="secondary-button" onClick={onClose} type="button">取消</button>
            <button className="primary-button" onClick={onSave} type="button">
              <Check size={16} />
              保存
            </button>
          </footer>
        ) : (
          <footer className="sheet-actions sheet-actions-single">
            <button className="primary-button" onClick={onClose} type="button">关闭</button>
          </footer>
        )}
      </aside>
    </div>
  );
}
