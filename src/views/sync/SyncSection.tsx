import type { ReactNode } from "react";

export function SyncSection({
  number,
  title,
  titleHint,
  action,
  children
}: {
  number?: string;
  title: string;
  titleHint?: string;
  action?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="sync-section">
      <div className="sync-section-title">
        <div>
          {number && <span className="sync-section-number">{number}</span>}
          <strong>{title}</strong>
          {titleHint ? <span className="sync-section-title-hint">{titleHint}</span> : null}
          {action}
        </div>
      </div>
      {children}
    </section>
  );
}
