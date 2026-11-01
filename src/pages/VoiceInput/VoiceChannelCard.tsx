import type { ReactNode } from 'react';
import { Box, Trash2 } from 'lucide-react';

export function VoiceChannelCard({
  index,
  label,
  filled,
  name,
  detail,
  icon,
  disabled,
  active,
  handle,
  overlay,
  removeLabel,
  onRemove,
}: {
  index: number;
  label: string;
  filled: boolean;
  name: string;
  detail: string;
  icon: string | null;
  disabled: boolean;
  active: boolean;
  handle?: ReactNode;
  overlay: boolean;
  removeLabel: string;
  onRemove: () => void;
}) {
  return (
    <div
      role={overlay ? undefined : 'listitem'}
      aria-label={label}
      aria-hidden={overlay || undefined}
      className={
        'free-model-route-node voice-channel relative min-h-[80px] rounded-card flex items-center' +
        (active ? ' is-active' : '') +
        (overlay ? ' is-dragging shadow-2xl' : '')
      }
    >
      <div
        aria-hidden="true"
        className="pointer-events-none absolute inset-0 overflow-hidden rounded-[inherit]"
      >
        <span className="free-model-route-priority is-compact absolute font-mono font-semibold">
          {index + 1}
        </span>
      </div>
      {handle && (
        <span className="voice-channel-drag-handle absolute inset-0 rounded-[inherit]">
          {handle}
        </span>
      )}
      {filled && (
        <div className="relative z-10 pointer-events-none flex w-full items-center gap-2.5">
          {icon ? (
            <img src={icon} alt="" className="h-12 w-12 shrink-0 object-contain" />
          ) : (
            <Box size={48} className="shrink-0 text-cyber-text-secondary" />
          )}
          <div className="flex min-w-0 flex-1 flex-col gap-1">
            <div className="flex h-5 min-w-0 items-center gap-2">
              <span className="min-w-0 flex-1 text-sm !leading-5 font-semibold text-cyber-text truncate">
                {name}
              </span>
            </div>
            <div className="flex min-w-0 items-center gap-1">
              <span className="min-w-0 flex-1 text-xs text-cyber-text-secondary truncate">
                {detail}
              </span>
            </div>
          </div>
          <button
            type="button"
            className="voice-channel-remove pointer-events-auto"
            aria-label={removeLabel}
            disabled={disabled}
            onClick={onRemove}
          >
            <Trash2 size={18} />
          </button>
        </div>
      )}
    </div>
  );
}
