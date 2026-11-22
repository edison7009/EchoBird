import { RoutingHint } from '../../components/RoutingToggle';

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
        <CapabilityToggleButton
          key={label}
          label={label}
          checked={checked}
          disabled={disabled}
          onChange={onChange}
        />
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

function CapabilityToggleButton({
  label,
  checked,
  onChange,
  disabled,
  compact = false,
}: Omit<CapabilityToggle, 'hint'> & { disabled: boolean; compact?: boolean }) {
  return (
    <button
      type="button"
      aria-pressed={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={compact ? 'capability-switch is-compact' : 'capability-switch'}
    >
      <span className="min-w-0 truncate">{label}</span>
      <span aria-hidden="true" className="capability-switch-box" />
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
  return (
    <div className="flex flex-wrap items-center gap-2">
      {toggles.map((toggle) => (
        <CapabilityToggleButton
          key={toggle.id ?? toggle.label}
          {...toggle}
          disabled={disabled}
          compact
        />
      ))}
    </div>
  );
}
