import { Info } from "lucide-react";
import { useEffect, useRef, useState, type CSSProperties } from "react";
import { compactPath } from "../../lib/skillUtils";
import type { SyncOperation } from "../../types";
import { groupDetailsBySkill, type PlanDetail } from "./syncPlanDetails";

export function PlanInfoDisclosure({
  details,
  onIncludeReplacement,
  busy
}: {
  details: PlanDetail[];
  onIncludeReplacement: (operation: SyncOperation) => void;
  busy: boolean;
}) {
  const buttonRef = useRef<HTMLButtonElement>(null);
  const wrapRef = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  const [panelStyle, setPanelStyle] = useState<CSSProperties>();

  function updatePanelPosition() {
    const button = buttonRef.current;
    if (!button) return;

    const viewportMargin = 16;
    const buttonRect = button.getBoundingClientRect();
    const panelWidth = Math.min(560, Math.max(280, window.innerWidth - viewportMargin * 2));
    const minLeft = viewportMargin;
    const maxLeft = Math.max(minLeft, window.innerWidth - panelWidth - viewportMargin);
    const preferredLeft = buttonRect.left;
    const left = Math.min(Math.max(preferredLeft, minLeft), maxLeft);
    const maxHeight = Math.min(380, Math.max(180, buttonRect.top - viewportMargin));

    setPanelStyle({
      left,
      bottom: window.innerHeight - buttonRect.top + 10,
      width: panelWidth,
      maxHeight
    });
  }

  useEffect(() => {
    if (!open) return undefined;
    updatePanelPosition();
    const onDocClick = (e: MouseEvent) => {
      if (wrapRef.current && !wrapRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    const onEsc = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    window.addEventListener("resize", updatePanelPosition);
    document.addEventListener("mousedown", onDocClick);
    document.addEventListener("keydown", onEsc);
    return () => {
      window.removeEventListener("resize", updatePanelPosition);
      document.removeEventListener("mousedown", onDocClick);
      document.removeEventListener("keydown", onEsc);
    };
  }, [open]);

  return (
    <div className="plan-info-wrap" ref={wrapRef}>
      <button
        ref={buttonRef}
        className={`plan-info-button ${open ? "active" : ""}`}
        type="button"
        aria-label="查看同步明细"
        aria-expanded={open}
        onClick={() => {
          setOpen((current) => {
            const next = !current;
            if (!current) window.requestAnimationFrame(updatePanelPosition);
            return next;
          });
        }}
      >
        <Info size={14} />
      </button>
      {open && (
        <PlanDetailPanel details={details} onIncludeReplacement={onIncludeReplacement} busy={busy} style={panelStyle} />
      )}
    </div>
  );
}

function PlanDetailPanel({
  details,
  onIncludeReplacement,
  busy,
  style
}: {
  details: PlanDetail[];
  onIncludeReplacement: (operation: SyncOperation) => void;
  busy: boolean;
  style?: CSSProperties;
}) {
  const blockedItems = details.filter((item) => item.kind === "blocked");
  const attentionItems = details.filter((item) => item.kind === "attention");
  const hasDetails = details.length > 0;

  return (
    <div className="plan-detail-panel" role="dialog" aria-label="同步明细" style={style}>
      {!hasDetails ? (
        <div className="plan-detail-empty">
          <strong>本次没有异常项</strong>
          <span>所有目标都可以按预览执行。</span>
        </div>
      ) : (
        <>
          {blockedItems.length > 0 && (
            <PlanDetailGroup title="需要处理" items={blockedItems} onIncludeReplacement={onIncludeReplacement} busy={busy} />
          )}
          {attentionItems.length > 0 && (
            <PlanDetailGroup title="需注意" items={attentionItems} onIncludeReplacement={onIncludeReplacement} busy={busy} />
          )}
        </>
      )}
    </div>
  );
}

function PlanDetailGroup({
  title,
  items,
  onIncludeReplacement,
  busy
}: {
  title: string;
  items: PlanDetail[];
  onIncludeReplacement: (operation: SyncOperation) => void;
  busy: boolean;
}) {
  const grouped = groupDetailsBySkill(items);
  return (
    <div className="plan-detail-group">
      <strong className="plan-detail-group-title">{title}</strong>
      {grouped.map(([skillId, skillItems]) => (
        <div className="plan-detail-skill" key={`${title}-${skillId}`}>
          <div className="plan-detail-skill-title">{skillId}</div>
          <div className="plan-detail-items">
            {skillItems.map((item) => (
              <div className={`plan-detail-item ${item.kind}`} key={`${item.kind}-${item.agentLabel}-${item.skillId}-${item.label}-${item.path ?? ""}`}>
                <div className="plan-detail-item-main">
                  <div className="plan-detail-item-meta">
                    <span className="plan-detail-item-label">{item.label}</span>
                    <span className="plan-detail-item-agent">{item.agentLabel}</span>
                  </div>
                  <strong className="plan-detail-item-title">{item.title}</strong>
                  <p className="plan-detail-item-body">{item.body}</p>
                  {item.path && <code className="plan-detail-item-path" title={item.path}>{compactPath(item.path)}</code>}
                  {item.backupPath && <code className="plan-detail-item-path" title={item.backupPath}>备份到 {compactPath(item.backupPath)}</code>}
                </div>
                {item.canIncludeReplacement && item.operation && (
                  <button className="secondary-button compact" disabled={busy} type="button" onClick={() => onIncludeReplacement(item.operation!)}>
                    统一为中心库软链接
                  </button>
                )}
              </div>
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}
