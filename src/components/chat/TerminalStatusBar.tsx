import { useEffect, useRef } from 'react';

// ── TerminalStatusBar ─────────────────────────────────────────────────────────
// Single-line pseudo-terminal strip shown above the input box while AI is working.
// Shows tool calls (tool name) OR intermediate assistant text — whichever is latest.
// Closed by the existing showProcess toggle button.

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
    const textRef = useRef<HTMLSpanElement>(null);
    const prevContent = useRef<string>('');

    const displayText = toolName
        ? `→ ${toolName}`
        : textContent
            ? textContent
            : null;

    // Trigger slide-in animation when content changes
    useEffect(() => {
        if (!textRef.current || !displayText) return;
        if (displayText !== prevContent.current) {
            prevContent.current = displayText;
            textRef.current.classList.remove('animate-slide-from-right');
            void textRef.current.offsetWidth; // reflow
            textRef.current.classList.add('animate-slide-from-right');
        }
    }, [displayText]);

    if (!isVisible || !isProcessing) return null;

    return (
        <div className="h-7 flex items-center px-3 overflow-hidden bg-cyber-terminal/50 border-t border-cyber-border/10 text-xs font-mono text-cyber-text-muted/70 select-none">
            <span className="text-cyber-accent/40 mr-2 flex-shrink-0">▸</span>
            <span ref={textRef} className="truncate animate-slide-from-right">
                {displayText
                    ? <span className={toolName ? 'text-cyber-accent-secondary/80' : 'text-cyber-text-muted/60'}>{displayText}</span>
                    : <span className="animate-pulse text-cyber-text-muted/40">思考中 ▋</span>
                }
            </span>
        </div>
    );
}
