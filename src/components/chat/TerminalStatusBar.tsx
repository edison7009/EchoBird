// TerminalStatusBar — ticker tape pseudo-terminal strip
// Text scrolls continuously from right to left (marquee) while AI is working.
// Speed adjusts by text length so short tool names fly by fast, long text still readable.

interface TerminalStatusBarProps {
    /** Name of the tool currently being called */
    toolName?: string;
    /** Intermediate assistant text (working text before final <chat> reply) */
    textContent?: string;
    /** Whether the strip is visible (controlled by showProcess toggle) */
    isVisible: boolean;
    /** Whether AI is currently processing */
    isProcessing: boolean;
}

export function TerminalStatusBar({ toolName, textContent, isVisible, isProcessing }: TerminalStatusBarProps) {
    if (!isVisible || !isProcessing) return null;

    // Priority: tool name > working text
    const hasContent = toolName || textContent;
    const label = toolName ? `⚡ ${toolName}` : textContent || '';

    // Speed: shorter text = faster scroll (more thrilling), long text = readable
    // base 4s for ~40 chars, scales up. Min 2s, max 8s.
    const duration = hasContent
        ? `${Math.min(8, Math.max(2, label.length * 0.1))}s`
        : undefined;

    return (
        <div className="h-7 overflow-hidden relative flex items-center bg-cyber-terminal/50 border-t border-cyber-border/10 select-none">
            {hasContent ? (
                // Marquee: text scrolls right → left continuously
                <span
                    key={label}                    // remount = restart animation on new content
                    className="marquee-text text-xs font-mono"
                    style={{
                        '--marquee-duration': duration,
                        color: toolName ? 'rgba(var(--color-cyber-accent-secondary), 0.8)' : 'rgba(200,220,255,0.55)',
                    } as React.CSSProperties}
                >
                    {label}&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;{label}&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;{label}
                </span>
            ) : (
                // Idle: dotPulse animated dots
                <span className="inline-flex items-center gap-0.5 px-3">
                    <span className="text-cyber-text-muted/50 text-xs font-mono mr-1">输入中</span>
                    {[0, 1, 2].map(i => (
                        <span
                            key={i}
                            className="inline-block w-1 h-1 rounded-full bg-cyber-text-muted/40"
                            style={{ animation: 'dotPulse 1.2s ease-in-out infinite', animationDelay: `${i * 0.2}s` }}
                        />
                    ))}
                </span>
            )}
        </div>
    );
}
