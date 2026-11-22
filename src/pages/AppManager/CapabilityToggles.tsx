import { useLayoutEffect, useRef, type CSSProperties } from 'react';
import { TAB_KEY_INSET } from '../../components/tabCanvas';
import { mountTabCanvas } from '../../components/useTabCanvas';
import { RoutingHint } from '../../components/RoutingToggle';

const keyStyle = { '--capability-key-inset': `${TAB_KEY_INSET}px` } as CSSProperties;

export interface CapabilityToggle {
  id?: string;
  label: string;
  hint: string;
  checked: boolean;
  onChange: (value: boolean) => void;
}

// Independent on/off tiles, as wide as their names: the name, then a box that
// turns blue with a check when on; the whole tile toggles it. All hints merge into one right-aligned glyph.
export function CapabilityToggles({
  toggles,
  disabled = false,
}: {
  toggles: CapabilityToggle[];
  disabled?: boolean;
}) {
  const merged = toggles.length > 1;
  return (
    <div data-disabled={disabled} className="capability-toggles flex items-center gap-2">
      {toggles.map(({ label, checked, onChange }) => (
        <button
          key={label}
          type="button"
          aria-pressed={checked}
          aria-label={label}
          disabled={disabled}
          onClick={() => onChange(!checked)}
          className="capability-switch"
        >
          <span>{label}</span>
          <span aria-hidden="true" className="capability-switch-box" />
        </button>
      ))}
      <span className="ml-auto inline-flex">
        <RoutingHint
          hint={
            merged
              ? toggles.map(({ label, hint }) => `${label}: ${hint}`).join(' ')
              : toggles[0].hint
          }
          content={
            merged ? (
              <span className="flex flex-col gap-2">
                {toggles.map(({ label, hint }) => (
                  <span key={label} className="block">
                    <span className="block font-semibold">{label}</span>
                    {hint}
                  </span>
                ))}
              </span>
            ) : undefined
          }
        />
      </span>
    </div>
  );
}

export function CapabilityToggleButton({
  label,
  checked,
  onChange,
  disabled = false,
}: Omit<CapabilityToggle, 'hint'> & { disabled?: boolean }) {
  return (
    <button
      type="button"
      aria-pressed={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className="capability-toggle relative isolate h-[24px] min-w-[44px] rounded-full px-[11px] text-xs font-semibold whitespace-nowrap text-cyber-text-secondary outline-none transition-colors focus-visible:ring-2 focus-visible:ring-cyber-accent"
      style={keyStyle}
    >
      {label}
    </button>
  );
}

export function CapabilitySelection({
  toggles,
  disabled = false,
}: {
  toggles: Omit<CapabilityToggle, 'hint'>[];
  disabled?: boolean;
}) {
  const root = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    if (root.current) return mountTabCanvas(root.current, undefined, true);
  }, []);
  return (
    <div
      ref={root}
      data-disabled={disabled}
      className="capability-toggles relative isolate flex flex-wrap items-center gap-2"
    >
      {toggles.map((toggle) => (
        <CapabilityToggleButton key={toggle.id ?? toggle.label} {...toggle} disabled={disabled} />
      ))}
    </div>
  );
}
