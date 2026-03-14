import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import type { Components } from 'react-markdown';

// ── Markdown renderer (same as existing channel-markdown style) ──────────────
const mdComponents: Components = {
    p:      ({ children }) => <p className="mb-1 last:mb-0 break-words">{children}</p>,
    code:   ({ children, className }) => {
        const isBlock = className?.includes('language-');
        return isBlock
            ? <pre className="bg-black/40 rounded p-2 my-1 overflow-x-auto text-xs"><code>{children}</code></pre>
            : <code className="bg-black/30 rounded px-1 text-cyber-accent/80 text-xs">{children}</code>;
    },
    ul:     ({ children }) => <ul className="list-disc list-inside mb-1 space-y-0.5">{children}</ul>,
    ol:     ({ children }) => <ol className="list-decimal list-inside mb-1 space-y-0.5">{children}</ol>,
    li:     ({ children }) => <li className="break-words">{children}</li>,
    a:      ({ children, href }) => <a href={href} target="_blank" rel="noreferrer" className="text-cyber-accent underline">{children}</a>,
    strong: ({ children }) => <strong className="font-bold text-cyber-text">{children}</strong>,
    em:     ({ children }) => <em className="italic opacity-80">{children}</em>,
    h1:     ({ children }) => <h1 className="text-base font-bold mb-1 text-cyber-text">{children}</h1>,
    h2:     ({ children }) => <h2 className="text-sm font-bold mb-1 text-cyber-text">{children}</h2>,
    h3:     ({ children }) => <h3 className="text-xs font-bold mb-0.5 text-cyber-text">{children}</h3>,
    blockquote: ({ children }) => <blockquote className="border-l-2 border-cyber-border/40 pl-2 my-1 text-cyber-text-muted/70 italic">{children}</blockquote>,
};

// ── Types ────────────────────────────────────────────────────────────────────

export type BubbleRole = 'user' | 'assistant' | 'system' | 'error' | 'working' | 'retry';

export interface BubbleChip {
    type: 'file' | 'model' | 'skill';
    name: string;
}

export interface ChatBubbleProps {
    role: BubbleRole;
    content: string;
    variant: 'mother' | 'channels';
    chips?: BubbleChip[];        // displayed below user bubble (read-only)
    isStreaming?: boolean;       // show ●●● animation when AI is generating
    subContent?: string;         // secondary line (e.g. connection hint below error)
}

// ── Color tokens per variant ─────────────────────────────────────────────────

const USER_BUBBLE = {
    mother:   'bg-[#00D4FF] text-[#0a0f1a]',
    channels: 'bg-[#00FF9D] text-[#0a0f1a]',
} as const;

// ── Streaming dots ────────────────────────────────────────────────────────────

function StreamingDots() {
    return (
        <span className="inline-flex gap-[3px] items-center">
            {[0, 1, 2].map(i => (
                <span
                    key={i}
                    className="inline-block w-1.5 h-1.5 rounded-full bg-cyber-text-muted/60"
                    style={{ animation: 'dotPulse 1.2s ease-in-out infinite', animationDelay: `${i * 0.2}s` }}
                />
            ))}
        </span>
    );
}

// ── Read-only chip label below user bubble ────────────────────────────────────

const CHIP_STYLES = {
    file:  'bg-cyber-bg border-cyber-text-muted/60 text-cyber-text-muted',
    model: 'bg-cyber-accent/5 border-cyber-accent/30 text-cyber-accent',
    skill: 'bg-cyber-warning/10 border-cyber-warning/30 text-cyber-warning',
} as const;

function ReadonlyChips({ chips }: { chips: BubbleChip[] }) {
    if (!chips.length) return null;
    return (
        <div className="flex flex-wrap gap-1.5 mt-1.5">
            {chips.map((c, i) => (
                <span key={i} className={`flex items-center gap-1 h-6 rounded px-2 text-[11px] font-mono border ${CHIP_STYLES[c.type]}`}>
                    {c.name}
                </span>
            ))}
        </div>
    );
}

// ── Main ChatBubble component ─────────────────────────────────────────────────

export function ChatBubble({ role, content, variant, chips = [], isStreaming = false, subContent }: ChatBubbleProps) {

    // ── System: centered muted gray ──
    if (role === 'system') {
        return (
            <div className="flex justify-center my-1">
                <span className="text-cyber-text-muted/50 text-xs font-mono">{content}</span>
            </div>
        );
    }

    // ── Error: centered red text (no bubble, no icon) ──
    if (role === 'error') {
        return (
            <div className="flex flex-col items-center gap-0.5 my-1">
                <span className="text-red-400 text-xs font-mono text-center">{content}</span>
                {subContent && <span className="text-red-400/60 text-[11px] font-mono text-center">{subContent}</span>}
            </div>
        );
    }

    // ── Retry warning: center yellow ──
    if (role === 'retry') {
        return (
            <div className="flex justify-center my-1">
                <span className="text-yellow-400/70 text-xs font-mono text-center">{content}</span>
            </div>
        );
    }

    // ── Working: centered animated dots ──
    if (role === 'working') {
        return (
            <div className="flex justify-center my-2">
                <StreamingDots />
            </div>
        );
    }

    // ── AI bubble (left) ──
    if (role === 'assistant') {
        // Priority 1: extract <chat>...</chat> — the AI's final user-facing reply
        const chatMatch = content.match(/<chat>([\s\S]*?)<\/chat>/i);
        // Priority 2: strip <think> blocks, show what remains
        const finalText = chatMatch
            ? chatMatch[1].trim()
            : content
                .replace(/<think>[\s\S]*?<\/think>/gi, '')   // complete think blocks
                .replace(/<think>[\s\S]*/gi, '')              // unclosed think (still thinking)
                .trim();

        return (
            <div className="flex justify-start mb-4">
                <div className="max-w-[75%] bg-white rounded-2xl rounded-tl-sm px-4 py-3 text-sm text-gray-900 leading-relaxed">
                    {(isStreaming && !finalText)
                        ? <span className="inline-flex items-center gap-2">
                            <span className="text-gray-400 font-mono text-sm">输入中</span>
                            <span className="inline-flex gap-[3px]">
                                {[0,1,2].map(i => (
                                    <span key={i} className="inline-block w-1.5 h-1.5 rounded-full bg-gray-400"
                                        style={{ animation: 'dotPulse 1.2s ease-in-out infinite', animationDelay: `${i * 0.2}s` }}
                                    />
                                ))}
                            </span>
                          </span>
                        : <div className="break-words channel-markdown">
                            <ReactMarkdown remarkPlugins={[remarkGfm]} components={mdComponents}>{finalText}</ReactMarkdown>
                          </div>
                    }
                </div>
            </div>
        );
    }

    // ── User bubble (right) ──
    return (
        <div className="flex flex-col items-end mb-4">
            <div className={`max-w-[75%] rounded-2xl rounded-tr-sm px-4 py-3 text-sm leading-relaxed whitespace-pre-wrap break-words ${USER_BUBBLE[variant]}`}>
                {content}
            </div>
            {chips.length > 0 && <ReadonlyChips chips={chips} />}
        </div>
    );
}
