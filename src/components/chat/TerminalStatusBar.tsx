// TerminalStatusBar — ticker tape pseudo-terminal strip
// Content scrolls continuously from left to left (doubling creates seamless loop).
// No content → "思考中" dotPulse idle.

interface TerminalStatusBarProps {
    toolName?: string;       // tool currently being called
    textContent?: string;    // intermediate assistant text (before <chat> reply)
    isVisible: boolean;      // showProcess toggle
    isProcessing: boolean;   // AI is running
}

export function TerminalStatusBar({ toolName, textContent, isVisible, isProcessing }: TerminalStatusBarProps) {
    if (!isVisible || !isProcessing) return null;

    const label = toolName ? `⚡ ${toolName}` : textContent || '';
    const hasContent = label.length > 0;

    // Speed: short text scrolls faster (more exciting), long text slower but readable
    // 20 chars → ~2.5s, 80 chars → ~5s, capped 2-8s
    const duration = `${Math.min(8, Math.max(2, label.length * 0.06 + 1.5)).toFixed(1)}s`;

    const textColor = toolName ? '#5ecfff' : 'rgba(180,210,255,0.5)';

    return (
        <div className="h-7 flex items-center overflow-hidden bg-cyber-terminal/50 border-t border-cyber-border/10 select-none">
            {hasContent ? (
                // Overflow container — clips the scrolling text
                <div className="flex-1 overflow-hidden h-full flex items-center">
                    {/* Doubled label for seamless -50% loop */}
                    <span
                        key={label}
                        className="marquee-text text-xs font-mono"
                        style={{
                            '--marquee-duration': duration,
                            color: textColor,
                        } as React.CSSProperties}
                    >
                        {label}&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;
                        {label}&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;
                    </span>
                </div>
            ) : (
                // Idle: "思考中" + dotPulse dots
                <span className="inline-flex items-center gap-0.5 px-3">
                    <span className="text-cyber-text-muted/50 text-xs font-mono mr-1">思考中</span>
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
