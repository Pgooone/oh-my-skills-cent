import { Info } from "lucide-react";
import { useEffect, useRef, useState, type CSSProperties } from "react";
import type { SyncOperation } from "../../types";
import { detailPrimaryLine, type PlanDetail } from "./syncPlanDetails";

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
    const panelWidth = estimateDetailPanelWidth(details, window.innerWidth - viewportMargin * 2);
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
  }, [open, details]);

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

function estimateDetailPanelWidth(details: PlanDetail[], maxViewport: number) {
  const measure = (text: string) => {
    let width = 0;
    for (const char of text) {
      width += char.charCodeAt(0) > 255 ? 12 : 7;
    }
    return width;
  };

  let longest = measure("本次没有需处理项");
  for (const item of details) {
    longest = Math.max(
      longest,
      measure(detailPrimaryLine(item)),
      measure(item.summary),
      item.canIncludeReplacement ? measure("统一为中心库软链接") + 24 : 0
    );
  }

  const contentWidth = Math.ceil(longest + 44);
  return Math.min(Math.max(contentWidth, 260), Math.min(420, maxViewport));
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
  const countParts = [
    blockedItems.length > 0 ? `需处理 ${blockedItems.length}` : null,
    attentionItems.length > 0 ? `需注意 ${attentionItems.length}` : null
  ].filter(Boolean);

  return (
    <div className="plan-detail-panel" role="dialog" aria-label="同步明细" style={style}>
      {!hasDetails ? (
        <div className="plan-detail-empty">
          <strong>本次没有需处理项</strong>
          <span>所有目标都可以按预览执行。</span>
        </div>
      ) : (
        <>
          {countParts.length > 0 && (
            <div className="plan-detail-count-bar">{countParts.join(" · ")}</div>
          )}
          {blockedItems.length > 0 && (
            <PlanDetailGroup title="需处理" items={blockedItems} onIncludeReplacement={onIncludeReplacement} busy={busy} />
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
  return (
    <div className="plan-detail-group">
      <strong className="plan-detail-group-title">{title}</strong>
      <div className="plan-detail-items">
        {items.map((item) => (
          <div
            className={`plan-detail-item ${item.kind}`}
            key={`${item.kind}-${item.agentLabel}-${item.skillId}-${item.label}-${item.path ?? ""}-${item.summary}`}
          >
            <div className="plan-detail-item-main">
              <div className="plan-detail-item-primary">{detailPrimaryLine(item)}</div>
              <p className="plan-detail-item-summary">{item.summary}</p>
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
  );
}
