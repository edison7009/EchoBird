import type { ReactNode } from 'react';
import './ScanProgress.css';

export function ScanProgress({
  progress,
  label,
  connectingLabel,
  ariaLabel,
  layout = 'compact',
}: {
  progress: number | null;
  label?: ReactNode;
  connectingLabel: ReactNode;
  ariaLabel: string;
  layout?: 'compact' | 'action' | 'settings';
}) {
  const value = progress === null ? undefined : Math.min(1, Math.max(0, progress));
  return (
    <>
      <div
        className={`h-1 rounded-full overflow-hidden bg-cyber-border/30 ${layout === 'action' ? 'w-64 self-center' : layout === 'settings' ? 'col-start-2 row-span-2 row-start-1 mt-[26px] w-full self-start' : ''}`}
        role="progressbar"
        aria-label={ariaLabel}
        aria-valuemin={0}
        aria-valuemax={1}
        aria-valuenow={value}
      >
        <div
          className="free-model-scan-progress h-full rounded-full bg-cyber-accent"
          style={{ width: `${Math.max(value ?? 0, 0.04) * 100}%` }}
        />
      </div>
      <div
        className={`text-cyber-text-secondary ${layout === 'settings' ? 'col-start-1 row-start-2 min-h-10 pl-[22px] text-[12px] leading-5' : `text-[11px] truncate ${layout === 'action' ? 'h-4 w-full text-center leading-4' : 'mt-2'}`}`}
        role="status"
        aria-live="polite"
      >
        {label || (
          <>
            {connectingLabel}
            <span className="free-model-scan-dots" aria-hidden="true">
              <span>.</span>
              <span>.</span>
              <span>.</span>
            </span>
          </>
        )}
      </div>
    </>
  );
}
