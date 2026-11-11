// RemoteModelSelector — Minimal model dropdown for Mother Agent
// Text + arrow, no background/border, hover shows soft bg, dropdown opens upward
import React, { useState, useRef, useEffect, useLayoutEffect } from 'react';
import { ChevronDown, Loader2, Check, Waypoints } from 'lucide-react';

export interface ModelOption {
  id: string;
  name: string;
  icon?: string | null; // icon path from getModelIcon()
  iconKind?: 'router';
}

/** A non-model option appended below the models list (after a divider).
 *  Used by Mother Agent to inject Claude Code as a parasite engine choice
 *  alongside regular models — picking it activates parasite mode rather
 *  than selecting a chat model. */
export interface ExtraOption extends ModelOption {
  /** When true the row renders as a non-clickable hint (e.g. CC not installed). */
  disabled?: boolean;
  /** Short suffix label appended after the name when disabled, e.g. "未安装". */
  disabledLabel?: string;
}

interface RemoteModelSelectorProps {
  models: ModelOption[];
  /** May reference a model id or an extra option id. */
  currentModelId: string | null;
  loading: boolean;
  onSelect: (modelId: string) => void;
  placeholder?: string;
  /** Items rendered below the models with a divider above them. Disabled
   *  extras don't fire onSelect but still appear in the menu so the user
   *  knows the option exists and what it'd take to enable it. */
  extras?: ExtraOption[];
}

export const RemoteModelSelector: React.FC<RemoteModelSelectorProps> = ({
  models,
  currentModelId,
  loading,
  onSelect,
  placeholder = 'Select model',
  extras,
}) => {
  const [isOpen, setIsOpen] = useState(false);
  const [menuBounds, setMenuBounds] = useState({ maxWidth: 200, maxHeight: 480 });
  const containerRef = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    if (!isOpen) return;
    const resizeMenu = () => {
      const anchor = containerRef.current?.getBoundingClientRect();
      if (!anchor) return;
      const boundary = containerRef.current?.closest('.overflow-hidden')?.getBoundingClientRect();
      const left = Math.max(0, boundary?.left ?? 0) + 8;
      const top = Math.max(0, boundary?.top ?? 0) + 8;
      setMenuBounds({
        maxWidth: Math.max(0, Math.min(anchor.right, window.innerWidth - 8) - left),
        maxHeight: Math.max(0, Math.min(480, anchor.top - top - 4)),
      });
    };
    resizeMenu();
    window.addEventListener('resize', resizeMenu);
    return () => window.removeEventListener('resize', resizeMenu);
  }, [isOpen]);

  // Close on click outside
  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(event.target as Node)) {
        setIsOpen(false);
      }
    };
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, []);

  // Close on Escape
  useEffect(() => {
    const handleEsc = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setIsOpen(false);
    };
    if (isOpen) {
      document.addEventListener('keydown', handleEsc);
      return () => document.removeEventListener('keydown', handleEsc);
    }
  }, [isOpen]);

  // Search both the regular models AND the extras so the trigger label
  // reflects whichever the parent currently considers "active" (a model id
  // for normal use, or e.g. "claudecode" when parasite mode is on).
  const currentModel =
    models.find((m) => m.id === currentModelId) || extras?.find((e) => e.id === currentModelId);
  const displayText = currentModel?.name || placeholder;

  // Shared composer controls use the same quiet hover surface.
  const triggerClass =
    'composer-tool-trigger flex h-8 max-w-full items-center gap-1.5 px-2 py-1 text-xs font-sans transition-colors';
  const spinClass = 'animate-spin text-cyber-text/70';
  const checkClass = 'flex-shrink-0 ml-1 text-cyber-text';

  const renderIcon = (option: ModelOption, sizeClass: string, iconSize: number) => {
    if (option.iconKind === 'router') {
      return <Waypoints size={iconSize} className="flex-shrink-0 text-cyber-accent" />;
    }
    if (!option.icon) return null;
    return (
      <img
        src={option.icon}
        alt=""
        className={`${sizeClass} flex-shrink-0`}
        onError={(e) => {
          (e.target as HTMLImageElement).style.display = 'none';
        }}
      />
    );
  };

  return (
    <div ref={containerRef} className="relative min-w-0">
      {/* Trigger button — icon + text + arrow, no bg/border */}
      <button
        type="button"
        onClick={() => !loading && setIsOpen(!isOpen)}
        disabled={loading}
        data-open={isOpen || undefined}
        className={triggerClass}
      >
        {loading ? (
          <Loader2 size={12} className={spinClass} />
        ) : (
          <>
            {currentModel && renderIcon(currentModel, 'w-3.5 h-3.5', 14)}
            <span className="truncate max-w-[140px]">{displayText}</span>
            <ChevronDown
              size={12}
              className={`h-3 w-3 flex-shrink-0 opacity-60 ${isOpen ? 'rotate-180' : ''}`}
            />
          </>
        )}
      </button>

      {/* Dropdown — opens upward */}
      {isOpen && (models.length > 0 || (extras && extras.length > 0)) && (
        <div
          style={{
            minWidth: Math.min(200, menuBounds.maxWidth),
            maxWidth: menuBounds.maxWidth,
            maxHeight: menuBounds.maxHeight,
          }}
          className="app-tooltip selection-popover model-selection-menu absolute bottom-full mb-1 right-0 w-max overflow-y-auto p-1.5
                    animate-in fade-in slide-in-from-bottom-2 duration-150
                    z-50"
        >
          {models.map((model) => (
            <div
              key={model.id}
              onClick={() => {
                if (model.id !== currentModelId) {
                  onSelect(model.id);
                }
                setIsOpen(false);
              }}
              data-selected={model.id === currentModelId || undefined}
              className="selection-menu-row flex items-center gap-2 px-3 py-2 text-xs font-sans transition-colors"
            >
              <span className="min-w-0 flex-1 whitespace-normal break-words">{model.name}</span>
              {model.id === currentModelId && <Check size={12} className={checkClass} />}
            </div>
          ))}
          {models.length === 0 && (!extras || extras.length === 0) && (
            <div className="px-3 py-2 text-xs text-cyber-text-secondary font-sans">
              No models configured
            </div>
          )}
          {extras && extras.length > 0 && models.length > 0 && (
            // Theme-aware divider — cyber-text-muted is a CSS variable
            // that flips polarity between themes (light grey on dark
            // backgrounds, dark grey on light backgrounds), so 40%
            // opacity reads as a consistent subtle line in both modes.
            // The dark-only groove (dark line + white highlight) we had
            // before only worked in dark mode and looked off in light.
            <div className="mx-3 my-1.5 h-px bg-cyber-text-muted/40" />
          )}
          {extras?.map((extra) => {
            const isCurrent = extra.id === currentModelId;
            const rowBase =
              'selection-menu-row flex items-center gap-2 px-3 py-2 text-xs font-sans transition-colors';
            const rowState = extra.disabled ? 'opacity-50' : '';
            return (
              <div
                key={extra.id}
                aria-disabled={extra.disabled || undefined}
                data-selected={isCurrent || undefined}
                onClick={() => {
                  if (extra.disabled || isCurrent) return;
                  onSelect(extra.id);
                  setIsOpen(false);
                }}
                className={`${rowBase} ${rowState}`}
              >
                <span className="min-w-0 flex-1 whitespace-normal break-words">{extra.name}</span>
                {extra.disabled && extra.disabledLabel && (
                  <span className="flex-shrink-0 text-[10px] text-cyber-text-muted">
                    {extra.disabledLabel}
                  </span>
                )}
                {isCurrent && !extra.disabled && <Check size={12} className={checkClass} />}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
};
