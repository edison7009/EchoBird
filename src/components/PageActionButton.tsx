import type { ComponentProps } from 'react';
import type { LucideIcon } from 'lucide-react';

export function PageActionButton({
  icon: Icon,
  spinning = false,
  children,
  disabled,
  pending = spinning,
  className = '',
  ...props
}: ComponentProps<'button'> & { icon?: LucideIcon; spinning?: boolean; pending?: boolean }) {
  return (
    <button
      type="button"
      {...props}
      disabled={disabled || pending}
      aria-busy={pending || props['aria-busy'] || undefined}
      className={`page-action-button flex h-9 shrink-0 items-center justify-center rounded-button text-sm outline-none ${disabled && !pending ? 'disabled:text-cyber-text-muted disabled:opacity-50' : ''} ${children ? 'gap-1.5 px-3' : 'w-9'} ${className}`}
    >
      {Icon && (
        <Icon
          size={16}
          className={`shrink-0 ${spinning ? 'animate-spin' : ''}`}
          aria-hidden="true"
        />
      )}
      {children}
    </button>
  );
}
