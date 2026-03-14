import { useEffect, useRef } from 'react';

// ── TerminalStatusBar ─────────────────────────────────────────────────────────
// Single-line pseudo-terminal strip shown above the input box while AI is working.
// Displays the current tool call name, slides in from right.
// Closed by the existing showProcess toggle button.

interface TerminalStatusBarProps {
    /** Name of the tool currently being called, or undefined when thinking */
    toolName?: string;
    /** Whether the strip is visible (controlled by showProcess toggle) */
    isVisible: boolean;
    /** Whether AI is currently processing */
    isProcessing: boolean;
}

export function TerminalStatusBar({ toolName, isVisible, isProcessing }: TerminalStatusBarProps) {
    const textRef = useRef<HTMLSpanElement>(null);
    const prevTool = useRef<string | undefined>(undefined);

    // Trigger slide-in animation when toolName changes
    useEffect(() => {
        if (!textRef.current) return;
        if (toolName !== prevTool.current) {
            prevTool.current = toolName;
            textRef.current.classList.remove('animate-slide-from-right');
            void textRef.current.offsetWidth; // reflow
            textRef.current.classList.add('animate-slide-from-right');
        }
    }, [toolName]);

    if (!isVisible || !isProcessing) return null;

    return (
        <div className="h-7 flex items-center px-3 overflow-hidden bg-cyber-terminal/50 border-t border-cyber-border/10 text-xs font-mono text-cyber-text-muted/70 select-none">
            <span className="text-cyber-accent/50 mr-2 flex-shrink-0">→</span>
            <span ref={textRef} className="truncate animate-slide-from-right">
                {toolName
                    ? <><span className="text-cyber-accent-secondary/80">{toolName}</span><span className="ml-1 animate-pulse">▋</span></>
                    : <span className="animate-pulse">思考中 ▋</span>
                }
            </span>
        </div>
    );
}
