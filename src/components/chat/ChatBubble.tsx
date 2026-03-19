// ChatBubble — social-style chat bubbles, NO markdown rendering
// Left: AI (white bg, black text). Right: User (solid cyan/green).
import { Paperclip, KeyRound, Image as ImageIcon } from 'lucide-react';
import { getModelIcon } from '../cards/ModelCard';
import { useI18n } from '../../hooks/useI18n';

export type BubbleRole = 'user' | 'assistant' | 'system' | 'error' | 'working' | 'retry' | 'skeleton';

export interface BubbleChip {
    type: 'file' | 'image' | 'model';
    name: string;
    /** For model chips: model provider name to look up icon */
    modelId?: string;
    /** For image chips: base64 preview data URL */
    preview?: string;
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
        .replace(/\n{3,}/g, '\n\n')            // collapse 3+ consecutive newlines → 2
        .trim();
}

// ── Icon-only readonly chips below user bubble ───────────────────────────────
// Mirrors PendingChipsRow color semantics, but without text or remove button.
const BASE_CHIP = 'flex items-center justify-center w-6 h-6 rounded border flex-shrink-0';
const CHIP_MODEL = `${BASE_CHIP} bg-cyber-accent/10 border-cyber-accent/40`;
const CHIP_FILE  = `${BASE_CHIP} bg-cyber-bg/60 border-cyber-text-muted/30`;

function ReadonlyChips({ chips }: { chips: BubbleChip[] }) {
    if (!chips.length) return null;
    return (
        <div className="flex flex-wrap gap-1 mt-1.5">
            {chips.map((c, i) => {
                if (c.type === 'model') {
                    const icon = getModelIcon(c.name, c.modelId || '');
                    return (
                        <span key={i} className={CHIP_MODEL} title={c.name}>
                            {icon
                                ? <img src={icon} alt="" className="w-4 h-4" />
                                : <KeyRound size={12} className="text-cyber-accent" />}
                        </span>
                    );
                }

                if (c.type === 'image') {
                    return (
                        <span key={i} className={CHIP_FILE} title={c.name}>
                            {c.preview
                                ? <img src={c.preview} alt={c.name} className="w-4 h-4 object-cover rounded" />
                                : <ImageIcon size={12} className="text-cyber-text-muted" />}
                        </span>
                    );
                }
                // file
                return (
                    <span key={i} className={CHIP_FILE} title={c.name}>
                        <Paperclip size={12} className="text-cyber-text-muted" />
                    </span>
                );
            })}
        </div>
    );
}

// ── Animated streaming indicator ─────────────────────────────────────────────
function InputDots() {
    const { t } = useI18n();
    const col = '#F0EDE8';
    return (
        <span className="inline-flex items-center gap-2">
            <span className="font-sans font-medium text-sm" style={{ color: col }}>{t('common.inputting')}</span>
            <span className="inline-flex gap-[3px]">
                {[0,1,2].map(i => (
                    <span key={i}
                        className="inline-block w-1 h-1 rounded-full"
                        style={{ backgroundColor: col, animation: 'dotPulse 1.2s ease-in-out infinite', animationDelay: `${i * 0.2}s` }}
                    />
                ))}
            </span>
        </span>
    );
}

// ── Content truncation ───────────────────────────────────────────────────────
const CONTENT_LIMIT = 2000;
const truncate = (text: string) =>
    text.length > CONTENT_LIMIT ? text.slice(0, CONTENT_LIMIT) + '…' : text;

// ── Main ChatBubble ───────────────────────────────────────────────────────────
export function ChatBubble({ role, content, variant, chips = [], isStreaming = false, subContent }: ChatBubbleProps) {
    const { t } = useI18n();

    // ── Skeleton: pulsing placeholder bars for lazy-load ──
    if (role === 'skeleton') {
        return (
            <div className="flex justify-start mb-4">
                <div className="max-w-[55%] rounded-xl px-4 py-3 space-y-2" style={{ background: '#2A2A2A' }}>
                    {[80, 60, 40].map((w, i) => (
                        <div key={i} className="h-3 rounded-full animate-pulse" style={{ width: `${w}%`, background: 'rgba(255,255,255,0.12)' }} />
                    ))}
                </div>
            </div>
        );
    }

    // ── System: centered muted ──
    if (role === 'system') {
        return (
            <div className="flex justify-center my-4">
                <span className="text-cyber-text-muted/50 text-xs font-mono">{content}</span>
            </div>
        );
    }

    // ── Error: centered red ──
    if (role === 'error') {
        return (
            <div className="flex flex-col items-center gap-0.5 my-4">
                <span className="text-red-400 text-xs font-mono text-center">{content}</span>
                {subContent && <span className="text-red-400/60 text-[11px] font-mono text-center">{subContent}</span>}
            </div>
        );
    }

    // ── Retry: centered yellow ──
    if (role === 'retry') {
        return (
            <div className="flex justify-center my-4">
                <span className="text-yellow-400/70 text-xs font-mono text-center">{content}</span>
            </div>
        );
    }

    // ── Working: centered dots ──
    if (role === 'working') {
        return (
            <div className="flex justify-center my-4">
                <InputDots />
            </div>
        );
    }

    // ── AI bubble (left) — white bg, black text, plain text ──
    if (role === 'assistant') {
        // MotherAgent uses `<chat>` protocol. Channels receives clean text from the Agent OS plugin.
        const chatMatch = variant === 'mother' ? content.match(/<chat>([\s\S]*?)(?:<\/chat>|$)/i) : null;
        
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
                    className="relative max-w-[62%] rounded-xl px-3 py-2 text-sm leading-snug font-sans font-medium"
                    style={{
                        background: '#2A2A2A',
                        color: '#F0EDE8',
                        boxShadow: '0 1px 4px rgba(0,0,0,0.25)',
                    }}
                >
                    {/* Rounded SVG tail — mostly inside bubble, tip sticks out 5px */}
                    <svg width="8" height="14" viewBox="0 0 8 14" style={{ position:'absolute', left:'-5px', top:'10px', overflow:'visible' }}>
                        <path d="M8,2 C8,1 7.2,0.4 6.5,1 L1.5,6 C0.8,6.6 0.8,7.4 1.5,8 L6.5,13 C7.2,13.6 8,13 8,12 Z" fill="#2A2A2A"/>
                    </svg>
                    {(isStreaming && !finalText)
                        ? <InputDots />
                        : <>
                            <p className="whitespace-pre-line" style={{ overflowWrap: 'anywhere', wordBreak: 'break-word' }}>{truncate(finalText)}</p>
                            {isStreaming && <div className="mt-1"><InputDots /></div>}
                          </>
                    }
                </div>
            </div>
        );
    }

    // ── User bubble (right) — solid color, dark text ──
    const tailColor = variant === 'mother' ? '#00D4FF' : '#00FF9D';
    return (
        <div className="flex flex-col items-end mb-4">
            <div className="flex justify-end max-w-[62%]">
                <div className={`relative flex-1 rounded-xl px-3 py-2 text-sm leading-snug whitespace-pre-line font-sans font-medium ${USER_BUBBLE[variant]}`} style={{ overflowWrap: 'anywhere', wordBreak: 'break-word' }}>
                    {/* Rounded SVG tail — mostly inside bubble, tip sticks out 5px right */}
                    <svg width="8" height="14" viewBox="0 0 8 14" style={{ position:'absolute', right:'-5px', top:'10px', overflow:'visible' }}>
                        <path d="M0,2 C0,1 0.8,0.4 1.5,1 L6.5,6 C7.2,6.6 7.2,7.4 6.5,8 L1.5,13 C0.8,13.6 0,13 0,12 Z" fill={tailColor}/>
                    </svg>
                    {truncate(content)}
                </div>
            </div>
            {chips.length > 0 && <ReadonlyChips chips={chips} />}
        </div>
    );
}
