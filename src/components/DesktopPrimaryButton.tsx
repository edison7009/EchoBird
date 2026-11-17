import type { ButtonHTMLAttributes } from 'react';

export function DesktopPrimaryButton({
  muted,
  className = '',
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { muted?: boolean }) {
  return (
    <button
      type="button"
      {...props}
      className={`w-full h-14 text-lg font-bold font-mono tracking-widest transition-colors rounded-lg cjk-btn border shadow-lg ${
        (muted ?? props.disabled)
          ? 'bg-cyber-border text-cyber-text-secondary border-transparent shadow-none'
          : 'bg-cyber-accent text-white border-cyber-accent hover:bg-cyber-accent-secondary hover:border-cyber-accent-secondary shadow-cyber-accent/30'
      } ${className}`}
    />
  );
}
