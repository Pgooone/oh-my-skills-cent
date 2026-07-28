import { ArrowUp, Check, Folder, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { callApi } from "../lib/api";

/**
 * Web 模式目录选择器（D3 目录选择替代）：面包屑 + 上级 + 目录列表（仅目录）。
 * 单击选中行，双击下钻；「选择此目录」返回选中行路径（未选中则返回当前路径）。
 * 数据来自 `POST /api/commands/list_dir`，浏览范围受服务端 D7-R1 宽松 jail 约束。
 */

interface DirEntry {
  name: string;
  path: string;
  isDir: boolean;
}

interface ListDirResponse {
  path: string;
  parent: string | null;
  entries: DirEntry[];
}

interface Crumb {
  label: string;
  path: string;
}

function buildBreadcrumbs(path: string): Crumb[] {
  const normalized = path.replace(/\\/g, "/");
  const isUnixAbsolute = normalized.startsWith("/");
  const parts = normalized.split("/").filter((part) => part.length > 0);
  const crumbs: Crumb[] = [];
  parts.forEach((part, index) => {
    // Windows 盘符段（如 "C:"）本身即盘符根。
    if (index === 0 && !isUnixAbsolute && /^[A-Za-z]:$/.test(part)) {
      crumbs.push({ label: part, path: `${part}/` });
      return;
    }
    if (crumbs.length > 0) {
      const prefix = crumbs[crumbs.length - 1].path.replace(/\/+$/, "");
      crumbs.push({ label: part, path: `${prefix}/${part}` });
      return;
    }
    crumbs.push({ label: part, path: `${isUnixAbsolute ? "/" : ""}${part}` });
  });
  if (crumbs.length === 0 && isUnixAbsolute) {
    crumbs.push({ label: "/", path: "/" });
  }
  return crumbs;
}

export function DirPicker({
  title,
  onSelect,
  onCancel
}: {
  title: string;
  onSelect: (path: string) => void;
  onCancel: () => void;
}) {
  const [current, setCurrent] = useState<ListDirResponse | null>(null);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const navigate = useCallback(async (path?: string) => {
    setLoading(true);
    setError(null);
    setSelectedPath(null);
    try {
      const result = await callApi<ListDirResponse>("list_dir", path ? { path } : {});
      setCurrent(result);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void navigate();
  }, [navigate]);

  useEffect(() => {
    const onEsc = (event: KeyboardEvent) => {
      if (event.key === "Escape") onCancel();
    };
    document.addEventListener("keydown", onEsc);
    return () => document.removeEventListener("keydown", onEsc);
  }, [onCancel]);

  const directories = current?.entries.filter((entry) => entry.isDir) ?? [];
  const crumbs = current ? buildBreadcrumbs(current.path) : [];

  function confirmSelection() {
    const chosen = selectedPath ?? current?.path;
    if (chosen) onSelect(chosen);
  }

  return (
    <div className="sheet-backdrop" onClick={onCancel}>
      <aside
        aria-label={title}
        aria-modal="true"
        className="settings-sheet dir-picker"
        onClick={(event) => event.stopPropagation()}
        role="dialog"
      >
        <header className="settings-header">
          <div className="settings-header-top">
            <h1>{title}</h1>
            <button className="settings-close" onClick={onCancel} title="关闭" type="button">
              <X size={16} />
            </button>
          </div>
          {current && (
            <nav aria-label="当前路径" className="dir-picker-breadcrumb">
              {crumbs.map((crumb, index) => (
                <span className="dir-picker-crumb" key={crumb.path}>
                  {index > 0 && <span className="dir-picker-crumb-sep">/</span>}
                  <button onClick={() => void navigate(crumb.path)} type="button">
                    {crumb.label}
                  </button>
                </span>
              ))}
            </nav>
          )}
        </header>

        <div className="settings-content dir-picker-content">
          {error && <div className="dir-picker-error">{error}</div>}
          {!error && loading && <div className="dir-picker-hint">加载中…</div>}
          {!error && !loading && current && (
            <div className="settings-list dir-picker-list" role="listbox" aria-label="目录列表">
              {current.parent && (
                <button
                  className="dir-picker-row dir-picker-up"
                  onClick={() => void navigate(current.parent ?? undefined)}
                  type="button"
                >
                  <ArrowUp size={14} />
                  <span>..</span>
                </button>
              )}
              {directories.map((entry) => (
                <button
                  aria-selected={selectedPath === entry.path}
                  className={`dir-picker-row ${selectedPath === entry.path ? "selected" : ""}`}
                  key={entry.path}
                  onClick={() => setSelectedPath(entry.path)}
                  onDoubleClick={() => void navigate(entry.path)}
                  role="option"
                  title={`${entry.path}（双击进入）`}
                  type="button"
                >
                  <Folder size={14} />
                  <span>{entry.name}</span>
                </button>
              ))}
              {directories.length === 0 && (
                <div className="dir-picker-hint">此目录下没有子目录</div>
              )}
            </div>
          )}
        </div>

        <footer className="sheet-actions">
          <button className="secondary-button" onClick={onCancel} type="button">
            取消
          </button>
          <button
            className="primary-button"
            disabled={!current && !selectedPath}
            onClick={confirmSelection}
            type="button"
          >
            <Check size={16} />
            选择此目录
          </button>
        </footer>
      </aside>
    </div>
  );
}
