interface ToggleSwitchProps {
  checked: boolean;
  onChange: (value: boolean) => void;
  ariaLabel?: string;
  disabled?: boolean;
  pending?: boolean;
  className?: string;
}

export function ToggleSwitch({
  checked,
  onChange,
  ariaLabel,
  disabled = false,
  pending = false,
  className = '',
}: ToggleSwitchProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={ariaLabel}
      aria-busy={pending || undefined}
      disabled={disabled || pending}
      onClick={() => onChange(!checked)}
      className={`relative inline-flex h-[24px] w-[40px] shrink-0 items-center rounded-full outline-none transition-colors duration-150 motion-reduce:transition-none focus-visible:ring-2 focus-visible:ring-cyber-accent focus-visible:ring-offset-2 focus-visible:ring-offset-cyber-bg ${checked ? 'bg-[rgb(var(--switch-on-rgb))]' : 'bg-cyber-elevated shadow-[inset_0_0_0_1px_rgb(var(--border-rgb))]'} ${className}`}
    >
      <span
        aria-hidden="true"
        className={`absolute left-[3px] top-[3px] h-[18px] w-[18px] rounded-full bg-white shadow-[0_1px_3px_rgba(0,0,0,0.3)] transition-transform duration-150 motion-reduce:transition-none ${checked ? 'translate-x-[16px]' : 'translate-x-0'}`}
      />
    </button>
  );
}
