// ChatBubble — social-style chat bubbles, NO markdown rendering
// Left: AI (white bg, black text). Right: User (solid cyan/green).
import { useI18n } from '../../hooks/useI18n';

export type BubbleRole = 'user' | 'assistant' | 'system' | 'error' | 'working' | 'retry';

export interface BubbleChip {
    type: 'file' | 'model' | 'skill';
    name: string;
}

export interface ChatBubbleProps {
    role: BubbleRole;
    content: string;
    variant: 'mother' | 'channels';
    chips?: BubbleChip[];
    isStreaming?: boolean;
    subContent?: string;
}

// ── User bubble colors (solid fill, dark text — same as nav active state) ────
const USER_BUBBLE = {
    mother:   'bg-[#00D4FF] text-[#1C1C1E]',
    channels: 'bg-[#00FF9D] text-[#1C1C1E]',
} as const;

// ── Strip common markdown symbols for plain-text display ─────────────────────
function stripMarkdown(text: string): string {
    return text
        .replace(/^#{1,6}\s+/gm, '')          // ## headers
        .replace(/\*\*\*(.+?)\*\*\*/g, '$1')  // ***bold italic***
        .replace(/\*\*(.+?)\*\*/g, '$1')       // **bold**
        .replace(/\*(.+?)\*/g, '$1')           // *italic*
        .replace(/`{3}[\s\S]*?`{3}/g, '')      // ```code blocks```
        .replace(/`([^`]+)`/g, '$1')           // `inline code`
        .replace(/^[-*]\s+/gm, '• ')           // - list → bullet
        .replace(/^\d+\.\s+/gm, '')            // 1. ordered list → remove number
        .replace(/\[([^\]]+)\]\([^)]+\)/g, '$1') // [link](url) → link text
        .trim();
}

// ── Chip styles ───────────────────────────────────────────────────────────────
const CHIP_STYLES = {
    file:  'bg-white/20 border-white/30 text-[#0a0f1a]',
    model: 'bg-white/20 border-white/30 text-[#0a0f1a]',
    skill: 'bg-white/20 border-white/30 text-[#0a0f1a]',
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

// ── Animated streaming dots ───────────────────────────────────────────────────
function InputDots({ dark }: { dark?: boolean }) {
    const { t } = useI18n();
    const col = dark ? '#888' : '#DED9D2';
    return (
        <span className="inline-flex items-center gap-2">
            <span className="font-sans font-semibold text-sm" style={{ color: col }}>{t('common.inputting')}</span>
            <span className="inline-flex gap-[3px]">
                {[0,1,2].map(i => (
                    <span key={i}
                        className="inline-block w-1.5 h-1.5 rounded-full"
                        style={{ backgroundColor: col, animation: 'dotPulse 1.2s ease-in-out infinite', animationDelay: `${i * 0.2}s` }}
                    />
                ))}
            </span>
        </span>
    );
}

// ── Main ChatBubble ───────────────────────────────────────────────────────────
export function ChatBubble({ role, content, variant, chips = [], isStreaming = false, subContent }: ChatBubbleProps) {

    // ── System: centered muted ──
    if (role === 'system') {
        return (
            <div className="flex justify-center my-1">
                <span className="text-cyber-text-muted/50 text-xs font-mono">{content}</span>
            </div>
        );
    }

    // ── Error: centered red ──
    if (role === 'error') {
        return (
            <div className="flex flex-col items-center gap-0.5 my-1">
                <span className="text-red-400 text-xs font-mono text-center">{content}</span>
                {subContent && <span className="text-red-400/60 text-[11px] font-mono text-center">{subContent}</span>}
            </div>
        );
    }

    // ── Retry: centered yellow ──
    if (role === 'retry') {
        return (
            <div className="flex justify-center my-1">
                <span className="text-yellow-400/70 text-xs font-mono text-center">{content}</span>
            </div>
        );
    }

    // ── Working: centered dots ──
    if (role === 'working') {
        return (
            <div className="flex justify-center my-2">
                <InputDots />
            </div>
        );
    }

    // ── AI bubble (left) — white bg, black text, plain text ──
    if (role === 'assistant') {
        // Priority: <chat> tag → else strip <think> → show remainder → strip markdown symbols
        const chatMatch = content.match(/<chat>([\s\S]*?)<\/chat>/i);
        const rawText = chatMatch
            ? chatMatch[1].trim()
            : content
                .replace(/<think>[\s\S]*?<\/think>/gi, '')
                .replace(/<think>[\s\S]*/gi, '')
                .trim();
        const finalText = stripMarkdown(rawText);

        // Don't render an empty bubble shell
        if (!finalText && !isStreaming) return null;

        return (
            <div className="flex justify-start mb-4">
                <div
                    className="max-w-[75%] rounded-2xl rounded-tl-sm px-4 py-3 text-base leading-relaxed font-sans font-semibold"
                    style={{
                        background: '#2A2A2A',
                        color: '#DED9D2',
                        boxShadow: 'inset 0 1px 0 rgba(255,255,255,0.06), 0 2px 12px rgba(0,0,0,0.30), 0 0 0 1px rgba(255,255,255,0.05)',
                    }}
                >
                    {(isStreaming && !finalText)
                        ? <InputDots />
                        : <p className="break-words whitespace-pre-wrap">{finalText}</p>
                    }
                </div>
            </div>
        );
    }

    // ── User bubble (right) — solid color, dark text ──
    return (
        <div className="flex flex-col items-end mb-4">
            <div className={`max-w-[75%] rounded-2xl rounded-tr-sm px-4 py-3 text-base leading-relaxed whitespace-pre-wrap break-words font-sans font-semibold ${USER_BUBBLE[variant]}`}>
                {content}
            </div>
            {chips.length > 0 && <ReadonlyChips chips={chips} />}
        </div>
    );
}
