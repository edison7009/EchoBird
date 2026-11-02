export const quotaColorClasses = {
  healthy: {
    track: 'border-green-500/20',
    fill: 'border-green-500',
    gradient: 'from-green-500 to-green-500/70',
    text: 'text-green-500 [[data-theme=light]_&]:text-green-800',
  },
  low: {
    track: 'border-cyber-warning/20',
    fill: 'border-cyber-warning',
    gradient: 'from-cyber-warning to-cyber-warning/70',
    text: 'text-cyber-warning [[data-theme=light]_&]:text-yellow-800',
  },
  critical: {
    track: 'border-cyber-error/20',
    fill: 'border-cyber-error',
    gradient: 'from-cyber-error to-cyber-error/70',
    text: 'text-cyber-error [[data-theme=light]_&]:text-red-700',
  },
  unknown: {
    track: 'border-cyber-border/30',
    fill: 'border-cyber-border',
    gradient: 'from-cyber-border to-cyber-border/70',
    text: 'text-cyber-text',
  },
};

export function quotaTone(percent: number | null | undefined): keyof typeof quotaColorClasses {
  if (percent == null || !Number.isFinite(percent)) return 'unknown';
  if (percent > 30) return 'healthy';
  if (percent > 10) return 'low';
  return 'critical';
}

/** Rounded first, so a quota reads the same color as the whole percent shown for it. */
export function quotaTextClass(percent: number | null | undefined): string {
  return quotaColorClasses[quotaTone(percent == null ? percent : Math.round(percent))].text;
}
