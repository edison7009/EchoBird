// ModelCard component

import React, { useState, useEffect } from 'react';
import { useConfirm } from '../ConfirmDialog';
import { useI18n } from '../../hooks/useI18n';

// Smart icon detection — match model name/ID to icon file
export const getModelIcon = (name: string, modelId?: string): string | null => {
    const text = `${name} ${modelId || ''}`.toLowerCase();

    // Matching rules: keywords -> icon file
    const iconMap: [string[], string][] = [
        [['qwen', '通义', 'tongyi'], 'qwen'],
        [['claude', 'anthropic', 'sonnet', 'opus', 'haiku'], 'claude'],
        [['gpt', 'openai', 'chatgpt', 'o1', 'o3'], 'chatgpt'],
        [['gemini', 'google', 'palm'], 'gemini'],
        [['deepseek'], 'deepseek'],
        [['mistral', 'mixtral'], 'mistral'],
        [['grok', 'xai'], 'grok'],
        [['groq'], 'groq'],
        [['kimi', 'moonshot'], 'kimi'],
        [['glm', 'zhipu', '智谱'], 'glm'],
        [['ernie', 'wenxin', '文心'], 'ernie'],
        [['hunyuan', '混元'], 'hunyuan'],
        [['minimax'], 'minimax'],
        [['cohere', 'command'], 'cohere'],
        [['perplexity', 'pplx'], 'perplexity'],
        [['together'], 'together'],
        [['doubao', '豆包', 'bytedance'], 'bytedance'],
        [['xiaomi', '小米', 'mimo'], 'xiaomi'],
        [['nemotron', 'nvidia'], 'nemotron'],
    ];

    for (const [keywords, icon] of iconMap) {
        if (keywords.some(kw => text.includes(kw))) {
            return `./icons/models/${icon}.svg`;
        }
    }
    return null;
};

// Card skeleton (loading state)
export const ModelCardSkeleton = () => (
    <div className="h-48 p-4 border border-cyber-border bg-black/80 rounded-card shadow-cyber-card animate-pulse">
        <div className="h-3 w-16 bg-cyber-border rounded mb-2"></div>
        <div className="h-5 w-32 bg-cyber-border rounded mb-4"></div>
        <div className="space-y-2">
            <div className="h-3 w-full bg-cyber-border/50 rounded"></div>
            <div className="h-3 w-3/4 bg-cyber-border/50 rounded"></div>
            <div className="h-3 w-1/2 bg-cyber-border/50 rounded"></div>
        </div>
        <div className="mt-4 flex gap-2">
            <div className="h-5 w-14 bg-cyber-border/30 rounded"></div>
            <div className="h-5 w-14 bg-cyber-border/30 rounded"></div>
        </div>
    </div>
);

// ModelCard props interface
export interface ModelCardProps {
    id: string;
    name: string;
    type: string;            // provider / category
    baseUrl?: string;        // API endpoint (OpenAI)
    anthropicUrl?: string;   // API endpoint (Anthropic)
    modelId?: string;        // model ID (provider-defined)
    hasProxy?: boolean;      // proxy enabled
    latency?: number;        // latency in ms, undefined = untested
    protocols?: ('openai' | 'anthropic')[];  // supported API protocols
    openaiTested?: boolean;      // OpenAI protocol tested
    anthropicTested?: boolean;   // Anthropic protocol tested
    isPinging?: boolean;     // currently pinging (shows decode animation)
    selected?: boolean;
    onClick?: () => void;
    onEdit?: () => void;     // edit callback
    onDelete?: () => void;   // delete callback
    onProtocolClick?: (protocol: 'openai' | 'anthropic') => void;  // protocol tag click
}

// Matrix decode animation — characters scramble then lock in sequence
const MATRIX_CHARS = 'ｱｲｳｴｵｶｷｸｹｺｻｼｽｾｿﾀﾁﾂﾃﾄﾅﾆﾇﾈﾉﾊﾋﾌﾍﾎﾏﾐﾑﾒﾓﾔﾕﾖﾗﾘﾙﾚﾛﾜﾝ0123456789';
const TARGET_TEXT = 'ECHOBIRD';

// Generate random character
const randomChar = () => MATRIX_CHARS[Math.floor(Math.random() * MATRIX_CHARS.length)];

export const MatrixDecode = ({ duration = 2000 }: { duration?: number }) => {
    // Initialize with random chars immediately
    const [chars, setChars] = useState<string[]>(() =>
        Array(TARGET_TEXT.length).fill(0).map(() => randomChar())
    );
    const [locked, setLocked] = useState<boolean[]>(Array(TARGET_TEXT.length).fill(false));

    useEffect(() => {
        // Calculate lock interval for each character
        const totalSteps = TARGET_TEXT.length;
        // Reserve 20% of time for final state, allocate remaining 80% for sequential locking
        const stepInterval = (duration * 0.8) / totalSteps;
        // Character scramble speed (min 30ms to stay visible)
        const tickRate = Math.max(30, stepInterval / 2);

        // Random character rolling
        const interval = setInterval(() => {
            setChars(prev => prev.map((_, i) =>
                locked[i] ? TARGET_TEXT[i] : randomChar()
            ));
        }, tickRate);

        // Lock characters one by one
        const lockTimers = TARGET_TEXT.split('').map((_, i) =>
            setTimeout(() => {
                setLocked(prev => {
                    const next = [...prev];
                    next[i] = true;
                    return next;
                });
                setChars(prev => {
                    const next = [...prev];
                    next[i] = TARGET_TEXT[i];
                    return next;
                });
            }, (duration * 0.2) + (i * stepInterval)) // Initial delay 20%
        );

        return () => {
            clearInterval(interval);
            lockTimers.forEach(t => clearTimeout(t));
        };
    }, [duration]);

    return (
        <span className="font-mono inline-flex gap-[2px] text-xs">
            {chars.map((char, i) => (
                <span
                    key={i}
                    className={`inline-block transition-all ${locked[i]
                        ? 'text-cyber-accent'
                        : 'text-green-500 opacity-80'
                        }`}
                    style={{
                        transitionDuration: `${Math.max(50, duration / 20)}ms`,
                        textShadow: locked[i]
                            ? '0 0 8px rgba(0, 255, 136, 0.8)'
                            : '0 0 4px rgba(0, 255, 0, 0.5)'
                    }}
                >
                    {char}
                </span>
            ))}
        </span>
    );
};

// ModelCard component
export const ModelCard = React.memo(({
    name, type, baseUrl, anthropicUrl, modelId, hasProxy, latency,
    protocols = [], openaiTested = false, anthropicTested = false,
    isPinging = false, selected = false, isActive = false,
    onClick, onEdit, onDelete, onProtocolClick
}: ModelCardProps & { isActive?: boolean }) => {
    const iconPath = getModelIcon(name, modelId);
    const confirm = useConfirm();
    const { t } = useI18n();

    return (
        <div
            className={`h-48 p-4 border ${isActive
                ? 'border-cyber-accent shadow-[0_0_15px_rgba(0,255,157,0.3)] bg-cyber-accent/5'
                : selected
                    ? 'border-cyber-accent shadow-lg shadow-cyber-accent/20 bg-cyber-accent/5'
                    : 'border-cyber-border'
                } relative overflow-hidden rounded-card cursor-pointer transition-all hover:border-cyber-accent bg-black/80 shadow-cyber-card flex flex-col`}
            onClick={onClick}
        >
            {/* Action buttons — top right */}
            {(onEdit || onDelete) && (
                <div className="absolute top-2 right-2 flex gap-1.5">
                    {onDelete && (
                        <button
                            className="text-xs font-mono text-cyber-text-muted/70 hover:text-red-500 transition-colors"
                            onClick={async (e) => {
                                e.stopPropagation();
                                const ok = await confirm({
                                    title: t('model.deleteTitle'),
                                    message: t('model.deleteConfirm'),
                                    confirmText: t('btn.delete'),
                                    cancelText: t('btn.cancel'),
                                    type: 'danger'
                                });
                                if (ok) {
                                    onDelete();
                                }
                            }}
                        >
                            [{t('btn.delete')}]
                        </button>
                    )}
                    {onEdit && (
                        <button
                            className="text-xs font-mono text-cyber-text-muted/70 hover:text-cyber-accent transition-colors"
                            onClick={(e) => {
                                e.stopPropagation();
                                onEdit();
                            }}
                        >
                            [{t('btn.edit')}]
                        </button>
                    )}
                </div>
            )}
            <div className="text-xs text-cyber-text-secondary mb-1 tracking-widest uppercase font-mono min-h-[15px]">
                {baseUrl ? (/localhost|127\.0\.0\.1/.test(baseUrl) ? t('model.local') : t('model.cloud')) : <span>&nbsp;</span>}
            </div>
            <div className="text-lg font-bold mb-3 truncate h-7">{name || <span className="invisible">-</span>}</div>
            <div className="text-xs space-y-1.5 font-mono">
                <div className="flex items-center gap-1 truncate">
                    <span className="text-cyber-accent/60">{t('model.label')}:</span>
                    <span className="truncate text-cyber-accent/60">{modelId || '-'}</span>
                </div>
                {baseUrl && (
                    <div className="flex items-center gap-1 truncate">
                        <span className="text-cyber-accent/60">{t('model.source')}:</span>
                        <span className="truncate text-cyber-accent/40">{(() => {
                            try { return new URL(baseUrl).hostname; } catch { return baseUrl; }
                        })()}</span>
                    </div>
                )}

                <div className="flex items-center gap-1">
                    <span className="text-cyber-accent/60">{t('model.latency')}:</span>
                    {isPinging ? (
                        <MatrixDecode />
                    ) : latency === -1 ? (
                        <span className="text-red-500 font-bold">Error</span>
                    ) : latency !== undefined ? (
                        <span className={latency < 200 ? 'text-green-500' : latency < 500 ? 'text-yellow-500' : 'text-red-500'}>
                            {latency}ms
                        </span>
                    ) : (
                        <span className="text-cyber-text-muted/70 text-xs">{t('model.debugTesting')}</span>
                    )}
                    {hasProxy && (
                        <span className="text-[10px] ml-1 text-cyber-accent/70 font-mono">
                            ({t('model.tunnel')})
                        </span>
                    )}
                </div>
            </div>
            {/* Protocol tags */}
            <div className="mt-auto flex items-center gap-1.5 flex-wrap">
                {protocols.includes('openai') && (
                    <div
                        className="flex items-center gap-1.5 select-none group cursor-pointer"
                        onClick={() => onProtocolClick?.('openai')}
                    >
                        <span className="text-cyber-accent/30 font-mono text-xs transition-colors group-hover:text-cyber-accent/60">[</span>
                        <span className={`${openaiTested
                            ? 'font-bold font-mono tracking-widest text-cyber-accent text-[10px] drop-shadow-[0_0_5px_rgba(0,255,157,0.5)] animate-pulse'
                            : 'font-mono tracking-widest text-cyber-text-muted/70 text-[10px]'
                            }`}>
                            OPENAI
                        </span>
                        <span className="text-cyber-accent/30 font-mono text-xs transition-colors group-hover:text-cyber-accent/60">]</span>
                    </div>
                )}
                {protocols.includes('anthropic') && (
                    <div
                        className="flex items-center gap-1.5 select-none group cursor-pointer"
                        onClick={(e) => {
                            e.stopPropagation();
                            onProtocolClick?.('anthropic');
                        }}
                    >
                        <span className="text-cyber-accent/30 font-mono text-xs transition-colors group-hover:text-cyber-accent/60">[</span>
                        <span className={`${anthropicTested
                            ? 'font-bold font-mono tracking-widest text-cyber-accent text-[10px] drop-shadow-[0_0_5px_rgba(0,255,157,0.5)] animate-pulse'
                            : 'font-mono tracking-widest text-cyber-text-muted/70 text-[10px]'
                            }`}>
                            ANTHROPIC
                        </span>
                        <span className="text-cyber-accent/30 font-mono text-xs transition-colors group-hover:text-cyber-accent/60">]</span>
                    </div>
                )}
                {protocols.length === 0 && (
                    <span className="text-[9px] px-1.5 py-0.5 font-mono bg-cyber-text-muted/20 text-cyber-text-muted border border-cyber-text-muted/30">
                        CHECKING...
                    </span>
                )}
            </div>
            {/* Model icon bottom-right */}
            {iconPath && (
                <img
                    src={iconPath}
                    alt={name}
                    className={`absolute bottom-3 right-3 w-8 h-8 ${selected || isActive ? 'opacity-100' : 'opacity-60'}`}
                    onError={(e) => { (e.target as HTMLImageElement).style.display = 'none'; }}
                />
            )}
        </div>
    );
}, (prev, next) => {
    // Custom comparator: skip function props (new refs each render)
    const keys: (keyof ModelCardProps)[] = [
        'id', 'name', 'type', 'baseUrl', 'anthropicUrl', 'modelId',
        'hasProxy', 'latency', 'openaiTested', 'anthropicTested',
        'isPinging', 'selected',
    ];
    for (const k of keys) {
        if ((prev as any)[k] !== (next as any)[k]) return false;
    }
    if ((prev as any).isActive !== (next as any).isActive) return false;
    // Compare protocols array by value
    const pp = prev.protocols || [], np = next.protocols || [];
    if (pp.length !== np.length || pp.some((v, i) => v !== np[i])) return false;
    return true;
});
