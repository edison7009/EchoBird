import type { ReactNode } from 'react';
import { Box, ExternalLink, LoaderCircle, Plus, X } from 'lucide-react';

export function DirectoryRow({
  name,
  url,
  iconSrc,
  secondary,
  onOpen,
  openLabel,
  add,
}: {
  name: string;
  url?: string;
  iconSrc?: string | null;
  secondary?: ReactNode;
  onOpen: () => void;
  openLabel: string;
  add?: {
    onClick: () => void;
    label: string;
    disabled?: boolean;
    busy?: boolean;
    onCancel?: () => void;
    cancelLabel?: string;
  };
}) {
  const canCancel = !!(add?.busy && add.onCancel);
  const hostname = (() => {
    try {
      return new URL(url!).hostname;
    } catch {
      return url ?? '—';
    }
  })();
  return (
    <div className="relative flex h-14 shrink-0 items-stretch rounded-card border-2 border-transparent overflow-hidden bg-cyber-surface/60">
      {add && (
        <button
          type="button"
          onClick={add.onClick}
          aria-label={add.label}
          disabled={add.disabled || add.busy}
          className="group/left flex-1 min-h-0 bg-gradient-to-r from-transparent to-transparent hover:from-cyber-text/15 hover:to-transparent transition-[background-image] duration-200 disabled:opacity-50"
        />
      )}
      <button
        type="button"
        onClick={onOpen}
        aria-label={openLabel}
        disabled={!url}
        className="group/right flex-1 min-h-0 bg-gradient-to-l from-transparent to-transparent hover:from-cyber-text/15 hover:to-transparent transition-[background-image] duration-200 disabled:opacity-50"
      />
      <div className="pointer-events-none absolute inset-0 flex items-center gap-3 px-3">
        {add && (
          <span
            className={`flex-shrink-0 text-cyber-text-muted ${add.disabled ? 'opacity-40' : ''}`}
          >
            {add.busy ? (
              <LoaderCircle size={22} className="animate-spin" aria-hidden="true" />
            ) : (
              <Plus
                size={22}
                strokeWidth={2.5}
                className="group-hover/left:text-cyber-text group-hover/left:scale-110 transition-all"
              />
            )}
          </span>
        )}
        <div className="flex-shrink-0">
          {iconSrc ? (
            <img
              src={iconSrc}
              alt=""
              className="w-8 h-8"
              onError={(event) => {
                event.currentTarget.style.display = 'none';
              }}
            />
          ) : (
            <div className="w-8 h-8 flex items-center justify-center text-cyber-text">
              <Box size={32} />
            </div>
          )}
        </div>
        <div className="flex-1 min-w-0 grid grid-cols-[minmax(0,1fr)] auto-rows-[16px] items-center">
          <div className="text-sm font-bold truncate leading-none">{name}</div>
          <div
            className={`text-[10px] text-cyber-text-secondary leading-tight ${canCancel ? 'flex min-w-0 items-center gap-1' : 'truncate opacity-70'}`}
          >
            {canCancel ? (
              <>
                <span className="min-w-0 truncate opacity-70">{secondary ?? hostname}</span>
                <button
                  type="button"
                  aria-label={add?.cancelLabel}
                  onClick={(event) => {
                    event.stopPropagation();
                    add?.onCancel?.();
                  }}
                  className="pointer-events-auto relative flex h-4 w-4 flex-shrink-0 items-center justify-center rounded-full text-cyber-text-muted hover:text-cyber-text focus-visible:outline focus-visible:outline-1"
                >
                  <X size={12} aria-hidden="true" />
                </button>
              </>
            ) : (
              (secondary ?? hostname)
            )}
          </div>
        </div>
        <ExternalLink
          size={18}
          strokeWidth={2.25}
          className="flex-shrink-0 text-cyber-text-muted group-hover/right:text-cyber-text group-hover/right:scale-110 transition-all"
        />
      </div>
    </div>
  );
}
