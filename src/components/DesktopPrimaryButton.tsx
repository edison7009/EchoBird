import type { ButtonHTMLAttributes } from 'react';

export function DesktopPrimaryButton({
  muted,
  segment,
  className = '',
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { muted?: boolean; segment?: 'main' | 'end' }) {
  // Match the end segment width with leading padding to center the label across both segments.
  return (
    <button
      type="button"
      {...props}
      className={`h-14 text-lg font-bold font-mono tracking-widest transition-colors cjk-btn border ${
        segment === 'main'
          ? 'min-w-0 flex-1 rounded-l-lg border-r-0 pl-12 focus-visible:brightness-110'
          : segment === 'end'
            ? 'relative flex w-12 shrink-0 items-center justify-center rounded-r-lg border-l-0 enabled:hover:brightness-110 focus-visible:brightness-110'
            : 'w-full rounded-lg shadow-lg'
      } ${
        (muted ?? props.disabled)
          ? 'bg-cyber-border text-cyber-text-secondary border-transparent shadow-none'
          : 'bg-cyber-accent text-white border-cyber-accent hover:bg-cyber-accent-secondary hover:border-cyber-accent-secondary shadow-cyber-accent/30'
      } ${className}`}
    />
  );
}
