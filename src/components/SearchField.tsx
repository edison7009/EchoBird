import { Search, X } from 'lucide-react';

export function SearchField({
  value,
  onChange,
  label,
  clearLabel,
  disabled = false,
}: {
  value: string;
  onChange: (value: string) => void;
  label: string;
  clearLabel: string;
  disabled?: boolean;
}) {
  return (
    <div className="page-search">
      <Search size={16} className="shrink-0" aria-hidden="true" />
      <input
        aria-label={label}
        placeholder={label}
        value={value}
        disabled={disabled}
        onChange={(event) => {
          if (!disabled) onChange(event.target.value);
        }}
      />
      <button
        type="button"
        disabled={disabled || !value}
        aria-label={clearLabel}
        aria-hidden={!value || undefined}
        tabIndex={value ? undefined : -1}
        className={`shrink-0 ${value ? '' : 'invisible'}`}
        onClick={() => {
          if (!disabled && value) onChange('');
        }}
      >
        <X size={15} aria-hidden="true" />
      </button>
    </div>
  );
}
