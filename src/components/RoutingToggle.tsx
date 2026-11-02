import { useEffect, useRef, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { ToggleSwitch } from './ToggleSwitch';

// A single routing toggle: label + switch + themed help glyph with an
// interactive tooltip. The tooltip stays open while the pointer is over the
// glyph OR the tooltip itself.
interface RoutingToggleProps {
  label: string;
  hideLabel?: boolean;
  hint?: string;
  className?: string;
  checked: boolean;
  disabled?: boolean;
  pending?: boolean;
  onChange: (v: boolean) => void;
}

export function RoutingToggle({
  label,
  hideLabel = false,
  hint,
  className = '',
  checked,
  disabled = false,
  pending = false,
  onChange,
}: RoutingToggleProps) {
  return (
    <div className={`flex items-center ${className}`}>
      {!hideLabel && (
        <span className="text-xs text-cyber-text-secondary mr-2 whitespace-nowrap">{label}</span>
      )}
      <ToggleSwitch
        checked={checked}
        onChange={onChange}
        ariaLabel={label}
        disabled={disabled}
        pending={pending}
        className={`${hideLabel ? '' : 'mr-2'} ${disabled ? 'disabled:opacity-50' : ''}`}
      />
      {hint && <RoutingHint hint={hint} />}
    </div>
  );
}

export function RoutingHint({ hint, content }: { hint: string; content?: ReactNode }) {
  const [open, setOpen] = useState(false);
  const anchorRef = useRef<HTMLSpanElement>(null);
  const tooltipRef = useRef<HTMLSpanElement>(null);
  const [position, setPosition] = useState<{
    left: number;
    top: number;
    caret: number;
    above: boolean;
  } | null>(null);
  useEffect(() => {
    if (!open) return;
    const place = () => {
      const anchor = anchorRef.current?.getBoundingClientRect();
      const tooltip = tooltipRef.current?.getBoundingClientRect();
      if (!anchor || !tooltip) return;
      const left = Math.max(
        8,
        Math.min(anchor.right - tooltip.width, window.innerWidth - tooltip.width - 8)
      );
      const above = anchor.bottom + 6 + tooltip.height > window.innerHeight - 8;
      const top = Math.max(8, above ? anchor.top - tooltip.height - 6 : anchor.bottom + 6);
      setPosition({
        left,
        top,
        above,
        caret: Math.max(8, Math.min(anchor.left + anchor.width / 2 - left - 4, tooltip.width - 16)),
      });
    };
    place();
    window.addEventListener('resize', place);
    window.addEventListener('scroll', place, true);
    return () => {
      window.removeEventListener('resize', place);
      window.removeEventListener('scroll', place, true);
    };
  }, [open, hint]);
  const closeTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Clear any pending close timer on unmount so it can't fire after teardown.
  useEffect(
    () => () => {
      if (closeTimer.current) clearTimeout(closeTimer.current);
    },
    []
  );

  const showTip = () => {
    if (closeTimer.current) clearTimeout(closeTimer.current);
    if (!open) setPosition(null);
    setOpen(true);
  };
  // Small grace delay so moving the pointer from "?" across the gap into the
  // tooltip doesn't dismiss it.
  const scheduleHide = () => {
    if (closeTimer.current) clearTimeout(closeTimer.current);
    closeTimer.current = setTimeout(() => setOpen(false), 160);
  };

  return (
    <span
      ref={anchorRef}
      className="relative inline-flex items-center"
      onMouseEnter={showTip}
      onMouseLeave={scheduleHide}
    >
      <span
        aria-label={hint}
        className="inline-flex h-5 w-5 items-center justify-center rounded-full bg-cyber-elevated font-sans text-xs font-medium leading-none text-cyber-text-secondary select-none hover:bg-cyber-accent/15 hover:text-cyber-accent transition-colors"
      >
        ?
      </span>
      {open &&
        createPortal(
          <span
            ref={tooltipRef}
            role="tooltip"
            onMouseEnter={showTip}
            onMouseLeave={scheduleHide}
            style={{
              left: position?.left ?? 0,
              top: position?.top ?? 0,
              visibility: position ? 'visible' : 'hidden',
              width: 'min(288px, calc(100vw - 16px))',
              maxHeight: 'calc(100vh - 16px)',
            }}
            className="fixed z-[10000] rounded border border-cyber-accent/40 bg-cyber-elevated px-3 py-2 text-[11px] leading-relaxed text-cyber-text shadow-cyber-card backdrop-blur-sm"
          >
            <span
              aria-hidden="true"
              style={{ left: position?.caret ?? 0 }}
              className={`absolute h-2 w-2 rotate-45 border-cyber-accent/40 bg-cyber-elevated ${position?.above ? '-bottom-1 border-r border-b' : '-top-1 border-l border-t'}`}
            />
            <span className="block overflow-y-auto" style={{ maxHeight: 'calc(100vh - 34px)' }}>
              {content ?? hint}
            </span>
          </span>,
          document.body
        )}
    </span>
  );
}
