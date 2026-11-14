import { Search, X } from 'lucide-react';

export function SearchField({
  value,
  onChange,
  label,
  clearLabel,
}: {
  value: string;
  onChange: (value: string) => void;
  label: string;
  clearLabel: string;
}) {
  return (
    <div className="page-search">
      <Search size={16} aria-hidden="true" />
      <input
        aria-label={label}
        placeholder={label}
        value={value}
        onChange={(event) => onChange(event.target.value)}
      />
      {value && (
        <button type="button" aria-label={clearLabel} onClick={() => onChange('')}>
          <X size={15} aria-hidden="true" />
        </button>
      )}
    </div>
  );
}
