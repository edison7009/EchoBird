import { ViewModeTabGroup } from './ViewModeTabGroup';

export function ViewModeTabs<T extends string>({
  value,
  options,
  onChange,
  fullWidth = false,
  dividers = false,
}: {
  fullWidth?: boolean;
  dividers?: boolean;
  value: T | null;
  options: readonly { value: T; label: string }[];
  onChange: (value: T) => void;
}) {
  return (
    <ViewModeTabGroup
      className={`view-mode-tabs flex border border-cyber-border rounded-button overflow-hidden ${dividers ? 'view-mode-dividers' : ''} ${fullWidth ? 'w-full' : ''}`}
    >
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          onClick={() => onChange(option.value)}
          aria-pressed={value === option.value}
          className={`view-mode-tab ${fullWidth ? 'flex-1 min-w-0' : ''} px-3 py-1.5 text-sm cjk-btn transition-[color] ${
            value === option.value
              ? 'bg-cyber-elevated text-cyber-text'
              : 'text-cyber-text-secondary hover:text-cyber-text'
          }`}
        >
          {option.label}
        </button>
      ))}
    </ViewModeTabGroup>
  );
}
