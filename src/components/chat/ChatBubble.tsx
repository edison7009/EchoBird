// ChatBubble — modern AI conversation style.
// Assistant: full-width markdown, no bubble (plain text on the page bg).
// User: subtle right-aligned card. Streaming caret while the agent types.
import { Paperclip, KeyRound, Image as ImageIcon } from 'lucide-react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { getModelIcon } from '../cards/ModelCard';
import { useI18n } from '../../hooks/useI18n';
import { mdComponents } from '../../pages/MotherAgent/mdComponents';

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
    /** Kept for API compatibility — only Mother Agent renders here now. */
    variant?: 'mother';
    chips?: BubbleChip[];
    isStreaming?: boolean;
    subContent?: string;
}

// ── Icon-only readonly chips below user message ───────────────────────────────
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
                        <span key={i} className={CHIP_MODEL}>
                            {icon
                                ? <img src={icon} alt="" className="w-4 h-4" />
                                : <KeyRound size={12} className="text-cyber-accent" />}
                        </span>
                    );
                }
                if (c.type === 'image') {
                    return (
                        <span key={i} className={CHIP_FILE}>
                            {c.preview
                                ? <img src={c.preview} alt={c.name} className="w-4 h-4 object-cover rounded" />
                                : <ImageIcon size={12} className="text-cyber-text-muted" />}
                        </span>
                    );
                }
                return (
                    <span key={i} className={CHIP_FILE}>
                        <Paperclip size={12} className="text-cyber-text-muted" />
                    </span>
                );
            })}
        </div>
    );
}

// ── "Inputting..." indicator before the first delta arrives ───────────────────
function InputDots() {
    const { t } = useI18n();
    const col = '#F0EDE8';
    return (
        <span className="inline-flex items-center gap-2">
            <svg viewBox="0 0 24 24" style={{ width: 12, height: 12, flexShrink: 0, animation: 'agentHeartbeat 0.9s ease-in-out infinite' }} aria-hidden="true">
                <path d="M12 21.593c-5.63-5.539-11-10.297-11-14.402 0-3.791 3.068-5.191 5.281-5.191 1.312 0 4.151.501 5.719 4.457 1.59-3.968 4.464-4.447 5.726-4.447 2.54 0 5.274 1.621 5.274 5.181 0 4.069-5.136 8.625-11 14.402z" fill="#FF5252" />
            </svg>
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

// ── Content size guard (markdown blow-up protection) ──────────────────────────
const CONTENT_LIMIT = 12000;
const truncate = (text: string) =>
    text.length > CONTENT_LIMIT ? text.slice(0, CONTENT_LIMIT) + '…' : text;

// Strip the agent's internal scaffolding while keeping the user-facing markdown.
// `<think>...</think>` is private reasoning. `<chat>...</chat>` is a wrapper the
// system prompt asks the agent to emit; we keep the inside but drop the tags.
function cleanAgentText(content: string): string {
    return content
        .replace(/<think>[\s\S]*?<\/think>/gi, '')
        .replace(/<think>[\s\S]*$/gi, '')
        .replace(/<\/?chat[^>]*>/gi, '')
        .trim();
}

// ── Main ChatBubble ───────────────────────────────────────────────────────────
export function ChatBubble({ role, content, chips = [], isStreaming = false, subContent }: ChatBubbleProps) {
    const { t } = useI18n();

    if (role === 'skeleton') {
        return (
            <div className="mb-6 space-y-2">
                {[80, 60, 40].map((w, i) => (
                    <div key={i} className="h-3 rounded-full animate-pulse" style={{ width: `${w}%`, background: 'rgba(255,255,255,0.08)' }} />
                ))}
            </div>
        );
    }

    if (role === 'system') {
        return (
            <div className="flex justify-center my-4">
                <span className="text-cyber-text-muted/50 text-xs font-mono">{content}</span>
            </div>
        );
    }

    if (role === 'error') {
        return (
            <div className="flex flex-col items-center gap-0.5 my-4">
                <span className="text-red-400 text-xs font-mono text-center">{content}</span>
                {subContent && <span className="text-red-400/60 text-[11px] font-mono text-center">{subContent}</span>}
            </div>
        );
    }

    if (role === 'retry') {
        return (
            <div className="flex justify-center my-4">
                <span className="text-yellow-400/70 text-xs font-mono text-center">{content}</span>
            </div>
        );
    }

    if (role === 'working') {
        return (
            <div className="flex justify-start my-4">
                <InputDots />
            </div>
        );
    }

    // ── Assistant: full-width markdown, no bubble ─────────────────────────────
    if (role === 'assistant') {
        const cleaned = cleanAgentText(content);

        // Empty + not streaming → don't render a ghost row
        if (!cleaned && !isStreaming) return null;

        return (
            <div className="mb-6 text-sm leading-relaxed text-cyber-text-primary font-sans">
                {cleaned ? (
                    <div className="markdown-body">
                        <ReactMarkdown remarkPlugins={[remarkGfm]} components={mdComponents}>
                            {truncate(cleaned)}
                        </ReactMarkdown>
                        {isStreaming && (
                            <span
                                className="inline-block w-1.5 h-4 ml-0.5 align-text-bottom bg-cyber-accent"
                                style={{ animation: 'caretBlink 1s steps(2) infinite' }}
                            />
                        )}
                    </div>
                ) : (
                    <InputDots />
                )}
            </div>
        );
    }

    // ── User: subtle right-aligned card, plain text ──────────────────────────
    return (
        <div className="flex flex-col items-end mb-6">
            <div
                className="max-w-[78%] rounded-lg px-3.5 py-2 text-sm leading-relaxed font-sans whitespace-pre-line text-cyber-text-primary border border-cyber-accent/25 bg-cyber-accent/5"
                style={{ overflowWrap: 'anywhere', wordBreak: 'break-word' }}
            >
                {truncate(content)}
            </div>
            {chips.length > 0 && <ReadonlyChips chips={chips} />}
        </div>
    );
}
