// ChatBubble — modern AI conversation style.
// Assistant: full-width markdown, no bubble (plain text on the page bg).
// User: subtle right-aligned card. Streaming caret while the agent types.
import { useEffect, useState } from 'react';
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

// ── Claude-Code-style "thinking" indicator ────────────────────────────────────
// Cycling asterisk glyph (forward + reverse) plus a random verb with ellipsis,
// rendered in theme green. Same shape as Claude Code's terminal spinner.
const SPINNER_GLYPHS = ['·', '✢', '*', '✶', '✻', '✽'];
const SPINNER_FRAMES = [...SPINNER_GLYPHS, ...[...SPINNER_GLYPHS].reverse()];
const SPINNER_VERBS_EN = [
    'Accomplishing', 'Architecting', 'Brewing', 'Bootstrapping', 'Calculating',
    'Cascading', 'Channelling', 'Cogitating', 'Composing', 'Computing',
    'Concocting', 'Considering', 'Cooking', 'Crafting', 'Crunching',
    'Cultivating', 'Deciphering', 'Deliberating', 'Doing', 'Effecting',
    'Envisioning', 'Forging', 'Formulating', 'Generating', 'Hatching',
    'Honing', 'Imagining', 'Incubating', 'Manifesting', 'Marinating',
    'Meditating', 'Mulling', 'Musing', 'Optimizing', 'Orchestrating',
    'Percolating', 'Plotting', 'Pondering', 'Processing', 'Reasoning',
    'Reticulating', 'Spelunking', 'Spinning', 'Stewing', 'Synthesizing',
    'Thinking', 'Tinkering', 'Transmuting', 'Unfurling', 'Vibing',
    'Working', 'Wrangling',
];
// Chinese verbs — keep the playful Claude-Code-ish vibe ("烹调中…", "揉捏中…").
const SPINNER_VERBS_ZH = [
    '思索', '琢磨', '酝酿', '烹调', '雕琢',
    '沉吟', '推敲', '冥想', '编织', '梳理',
    '揉捏', '锻造', '调和', '织梦', '谋划',
    '玩味', '端详', '神游', '钻研', '寻思',
    '烧脑', '挠头', '浸泡', '发酵', '熬煮',
    '炮制', '推演', '演算', '召唤', '拨弦',
    '咕嘟', '搅拌', '编排', '凝聚', '飘忽',
];

function InputDots() {
    const { locale } = useI18n();
    const isZh = locale.startsWith('zh');
    const verbs = isZh ? SPINNER_VERBS_ZH : SPINNER_VERBS_EN;
    const formatVerb = (v: string) => (isZh ? `正在${v}中…` : `${v}…`);
    const pickRandom = () => formatVerb(verbs[Math.floor(Math.random() * verbs.length)]);

    // Glyph cycle (·✢*✶✻✽ forward + reverse)
    const [frame, setFrame] = useState(0);
    useEffect(() => {
        const id = setInterval(() => setFrame(f => (f + 1) % SPINNER_FRAMES.length), 100);
        return () => clearInterval(id);
    }, []);

    // Typewriter cycle: show → erase → type a new verb → repeat
    const [target, setTarget] = useState<string>(pickRandom);
    const [shown, setShown] = useState<string>(target);
    const [phase, setPhase] = useState<'show' | 'erase' | 'type'>('show');

    // When locale flips, immediately swap to a fresh verb in the new language.
    useEffect(() => {
        const next = pickRandom();
        setTarget(next);
        setShown(next);
        setPhase('show');
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [isZh]);

    useEffect(() => {
        if (phase === 'show') {
            const id = setTimeout(() => setPhase('erase'), 2800);
            return () => clearTimeout(id);
        }
        if (phase === 'erase') {
            if (shown.length === 0) {
                setTarget(pickRandom());
                setPhase('type');
                return;
            }
            const id = setTimeout(() => setShown(s => s.slice(0, -1)), 45);
            return () => clearTimeout(id);
        }
        // phase === 'type'
        if (shown.length >= target.length) {
            setPhase('show');
            return;
        }
        const id = setTimeout(() => setShown(target.slice(0, shown.length + 1)), 70);
        return () => clearTimeout(id);
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [phase, shown, target]);

    return (
        <span className="inline-flex items-center gap-2">
            <span className="spinner-glyph inline-block w-3 text-center font-mono text-base leading-none text-cyber-accent">
                {SPINNER_FRAMES[frame]}
            </span>
            <span className="inline-flex items-baseline font-mono text-sm">
                <span className="spinner-shimmer">{shown}</span>
                {/* Caret only appears while we're actively rewriting the verb
                    (erase/type). During the steady "show" phase it's hidden so
                    the line doesn't sit there with a blinking cursor on idle. */}
                {phase !== 'show' && (
                    <span
                        className="inline-block w-[0.5em] h-[1em] ml-0.5 bg-cyber-accent self-center"
                        style={{ animation: 'caretBlink 1s steps(2) infinite' }}
                    />
                )}
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
