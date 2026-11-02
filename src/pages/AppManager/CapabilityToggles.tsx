import { RoutingHint } from '../../components/RoutingToggle';

export interface CapabilityToggle {
  label: string;
  hint: string;
  checked: boolean;
  onChange: (value: boolean) => void;
}

// Independent on/off buttons, so each one carries its own highlight instead of
// sharing a sliding tab indicator. All hints merge into one right-aligned glyph.
export function CapabilityToggles({
  toggles,
  disabled = false,
}: {
  toggles: CapabilityToggle[];
  disabled?: boolean;
}) {
  const merged = toggles.length > 1;
  return (
    <div className="flex items-center gap-2 px-3">
      {toggles.map(({ label, checked, onChange }) => (
        <button
          key={label}
          type="button"
          aria-pressed={checked}
          aria-label={label}
          disabled={disabled}
          onClick={() => onChange(!checked)}
          className={`view-mode-tab h-[24px] min-w-[44px] rounded-full px-[10px] text-xs font-medium whitespace-nowrap border outline-none transition-[color,background-color] focus-visible:ring-2 focus-visible:ring-cyber-accent disabled:opacity-50 ${
            checked
              ? 'border-transparent text-cyber-text'
              : 'border-cyber-border text-cyber-text-secondary enabled:hover:border-transparent enabled:hover:bg-cyber-elevated enabled:hover:text-cyber-text'
          }`}
        >
          {label}
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
