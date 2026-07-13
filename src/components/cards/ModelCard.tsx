// ModelCard component

import React, { useState, useEffect } from 'react';
import { useConfirm } from '../ConfirmDialog';
import { useI18n } from '../../hooks/useI18n';
import type { ModelUsageData } from '../../api/tauri';
import type { TKey } from '../../i18n/types';

// Smart icon detection — match model name/ID to icon file
export const getModelIcon = (name: string, modelId?: string): string | null => {
  const text = `${name} ${modelId || ''}`.toLowerCase();

  // Matching rules: keywords -> icon file
  const iconMap: [string[], string][] = [
    [['qwen', '通义', 'tongyi'], 'qwen'],
    [['claude', 'anthropic', 'sonnet', 'opus', 'haiku'], 'claude'],
    [['gpt', 'openai', 'chatgpt', 'o1', 'o3'], 'chatgpt'],
    [['gemma'], 'google'],
    [['gemini', 'palm'], 'gemini'],
    [['deepseek'], 'deepseek'],
    [['mistral', 'mixtral'], 'mistral'],
    [['minimax'], 'minimax'],
    [['grok', 'x.ai'], 'grok'],
    [['groq'], 'groq'],
    [['kimi', 'moonshot'], 'kimi'],
    [['glm', 'zhipu', '智谱', 'z.ai', 'bigmodel'], 'glm'],
    [['ernie', 'wenxin', '文心'], 'ernie'],
    [['hunyuan', '混元'], 'hunyuan'],
    [['cohere', 'command'], 'cohere'],
    [['perplexity', 'pplx'], 'perplexity'],
    [['together'], 'together'],
    [['volcengine', 'volces', '火山', 'ark.cn-beijing'], 'volcengine'],
    [['byteplus', 'bytepluses'], 'byteplus'],
    [['doubao', '豆包', 'bytedance'], 'bytedance'],
    [['xiaomi', '小米', 'mimo'], 'xiaomi'],
    [['nemotron', 'nvidia'], 'nemotron'],
    [['stepfun', 'step', '阶跃'], 'stepfun'],
    [['granite', 'ibm'], 'granite'],
    [['meta'], 'meta'],
    [['openrouter'], 'openrouter'],
    [['worldrouter'], 'worldrouter'],
    [['b.ai', 'bai'], 'b-ai'],
    [['agnes'], 'agnes'],
    // Resellers (pure compute providers that host third-party models, e.g.
    // Compshare/UCloud 优云智算, CC Vibe) go LAST. A model card carries a modelId that
    // identifies the actual model brand (glm/kimi/deepseek/minimax), and the
    // model logo must win. The vendor logo only matches when the modelId has no
    // recognized brand — such as provider rows, which pass modelId=''. Model ID
    // and vendor are separate concerns; do not move resellers above model brands.
    [['compshare', '优云智算', '优云'], 'compshare'],
    [['ccvibe', 'cc vibe', 'cc-vibe'], 'ccvibe'],
  ];

  for (const [keywords, icon] of iconMap) {
    if (keywords.some((kw) => text.includes(kw))) {
      if (icon === 'worldrouter') return './icons/models/worldrouter.png';
      if (icon === 'b-ai') return './icons/models/b-ai.ico';
      if (icon === 'agnes') return './icons/models/agnes.png';
      if (icon === 'compshare') return './icons/models/compshare.png';
      if (icon === 'ccvibe') return './icons/models/ccvibe.png';
      if (icon === 'byteplus') return './icons/models/byteplus.png';
      return `./icons/models/${icon}.svg`;
    }
  }
  return null;
};

// Card skeleton (loading state)
export const ModelCardSkeleton = () => (
  <div className="h-48 p-4 bg-cyber-surface rounded-card animate-pulse">
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
  type: string; // provider / category
  baseUrl?: string; // API endpoint (OpenAI)
  anthropicUrl?: string; // API endpoint (Anthropic)
  modelId?: string; // model ID (provider-defined)
  latency?: number; // latency in ms, undefined = untested
  protocols?: ('openai' | 'anthropic')[]; // supported API protocols
  openaiTested?: boolean; // OpenAI protocol tested
  anthropicTested?: boolean; // Anthropic protocol tested
  isPinging?: boolean; // currently pinging (shows decode animation)
  selected?: boolean;
  viewMode?: 'config' | 'usage'; // display mode
  usageData?: ModelUsageData; // usage quota data
  volcSsoExpired?: boolean; // show [二次验证] button (Volcengine SSO needed)
  onReauth?: () => void; // SSO re-login callback
  onClick?: () => void;
  onEdit?: () => void; // edit callback
  onDelete?: () => void; // delete callback
  onRefresh?: () => void; // refresh usage callback (usage mode only)
  onProtocolClick?: (protocol: 'openai' | 'anthropic') => void; // protocol tag click
}

// Matrix decode animation — characters scramble then lock in sequence
const MATRIX_CHARS = 'ｱｲｳｴｵｶｷｸｹｺｻｼｽｾｿﾀﾁﾂﾃﾄﾅﾆﾇﾈﾉﾊﾋﾌﾍﾎﾏﾐﾑﾒﾓﾔﾕﾖﾗﾘﾙﾚﾛﾜﾝ0123456789';
const TARGET_TEXT = 'ECHOBIRD';

// Format countdown time (ms to human readable) - i18n aware
const formatCountdown = (ms: number, t: (key: TKey) => string): string => {
  const totalSeconds = Math.floor(Math.max(0, ms) / 1000);
  const days = Math.floor(totalSeconds / 86400);
  const hours = Math.floor((totalSeconds % 86400) / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);

  if (days > 0) {
    return t('model.countdown.days')
      .replace('{d}', String(days))
      .replace('{h}', String(hours))
      .replace('{m}', String(minutes));
  } else if (hours > 0) {
    return t('model.countdown.hours').replace('{h}', String(hours)).replace('{m}', String(minutes));
  } else {
    return t('model.countdown.minutes').replace('{m}', String(minutes));
  }
};

// Generate random character
const randomChar = () => MATRIX_CHARS[Math.floor(Math.random() * MATRIX_CHARS.length)];

export const MatrixDecode = ({ duration = 2000 }: { duration?: number }) => {
  // Initialize with random chars immediately
  const [chars, setChars] = useState<string[]>(() =>
    Array(TARGET_TEXT.length)
      .fill(0)
      .map(() => randomChar())
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
      setChars((prev) => prev.map((_, i) => (locked[i] ? TARGET_TEXT[i] : randomChar())));
    }, tickRate);

    // Lock characters one by one
    const lockTimers = TARGET_TEXT.split('').map(
      (_, i) =>
        setTimeout(
          () => {
            setLocked((prev) => {
              const next = [...prev];
              next[i] = true;
              return next;
            });
            setChars((prev) => {
              const next = [...prev];
              next[i] = TARGET_TEXT[i];
              return next;
            });
          },
          duration * 0.2 + i * stepInterval
        ) // Initial delay 20%
    );

    return () => {
      clearInterval(interval);
      lockTimers.forEach((t) => clearTimeout(t));
    };
    // Animation is keyed on `duration`; it reads `locked` for the current
    // frame but must not restart when `locked` changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [duration]);

  return (
    <span className="font-mono inline-flex gap-[2px] text-xs">
      {chars.map((char, i) => (
        <span
          key={i}
          className={`inline-block transition-all ${
            locked[i] ? 'text-cyber-text' : 'text-green-500 opacity-80'
          }`}
          style={{
            transitionDuration: `${Math.max(50, duration / 20)}ms`,
            textShadow: locked[i]
              ? '0 0 8px rgba(0, 255, 136, 0.8)'
              : '0 0 4px rgba(0, 255, 0, 0.5)',
          }}
        >
          {char}
        </span>
      ))}
    </span>
  );
};

// ModelCard component
export const ModelCard = React.memo(
  ({
    name,
    type: _type,
    baseUrl,
    anthropicUrl,
    modelId,
    latency,
    protocols = [],
    openaiTested = false,
    anthropicTested = false,
    isPinging = false,
    selected = false,
    isActive = false,
    viewMode = 'config',
    usageData,
    volcSsoExpired,
    onReauth,
    onClick,
    onEdit,
    onDelete,
    onRefresh,
    onProtocolClick: _onProtocolClick,
  }: ModelCardProps & { isActive?: boolean }) => {
    const iconPath = getModelIcon(name, modelId);
    const confirm = useConfirm();
    const { t } = useI18n();

    // Real-time countdown update for usage mode
    const [, setTick] = useState(0);
    useEffect(() => {
      if (viewMode !== 'usage' || !usageData) return;
      const timer = setInterval(() => setTick((prev) => prev + 1), 60000); // Update every minute
      return () => clearInterval(timer);
    }, [viewMode, usageData]);

    return (
      <div
        className={`h-48 p-4 border bg-cyber-surface ${
          isActive || selected
            ? 'border-cyber-accent'
            : 'border-transparent hover:bg-cyber-elevated'
        } relative overflow-hidden rounded-card cursor-pointer transition-colors flex flex-col`}
        onClick={onClick}
      >
        {/* Action buttons — top right, different for config vs usage mode */}
        {viewMode === 'config' ? (
          // Config mode: [删除] [编辑]
          (onEdit || onDelete) && (
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
                      type: 'danger',
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
                  className="text-xs font-mono text-cyber-text-muted/70 hover:text-cyber-text transition-colors"
                  onClick={(e) => {
                    e.stopPropagation();
                    onEdit();
                  }}
                >
                  [{t('btn.edit')}]
                </button>
              )}
            </div>
          )
        ) : (
          // Usage mode: [二次验证] [刷新]
          <div className="absolute top-2 right-2 flex gap-1.5">
            {volcSsoExpired && onReauth && (
              <button
                className="text-xs font-mono text-cyber-text-muted/70 hover:text-cyber-accent transition-colors"
                onClick={(e) => {
                  e.stopPropagation();
                  onReauth();
                }}
              >
                [{t('model.reauth')}]
              </button>
            )}
            {onRefresh && !volcSsoExpired && (
              <button
                className="text-xs font-mono text-cyber-text-muted/70 hover:text-cyber-text transition-colors"
                onClick={(e) => {
                  e.stopPropagation();
                  onRefresh();
                }}
              >
                [{t('btn.refresh')}]
              </button>
            )}
          </div>
        )}
        <div className="text-xs text-cyber-text-secondary mb-1 tracking-widest uppercase font-mono min-h-[15px]">
          {(() => {
            const url = baseUrl || anthropicUrl;
            if (!url) return <span>&nbsp;</span>;
            return /localhost|127\.0\.0\.1/.test(url) ? t('model.local') : t('model.cloud');
          })()}
        </div>
        <div className="flex items-center gap-2 mb-3">
          {/* Show provider logo in usage mode (based on baseUrl) */}
          {viewMode === 'usage' &&
            (() => {
              const url = baseUrl || anthropicUrl || '';
              const providerIcon = getModelIcon('', url);
              return providerIcon ? (
                <img
                  src={providerIcon}
                  alt=""
                  className="w-6 h-6 flex-shrink-0"
                  onError={(e) => {
                    (e.target as HTMLImageElement).style.display = 'none';
                  }}
                />
              ) : null;
            })()}
          <div className="text-lg font-bold truncate h-7 flex-1">
            {name || <span className="invisible">-</span>}
          </div>
        </div>

        {/* Content area - switches based on viewMode */}
        {viewMode === 'usage' ? (
          // Usage mode - show quota bars or balance
          <div className="flex-1 flex flex-col justify-center space-y-2.5">
            {usageData?.quotas && usageData.quotas.length > 0 ? (
              usageData.quotas.map((quota, idx) => (
                <div key={idx} className="space-y-1">
                  {quota.balance !== undefined && quota.balance !== null ? (
                    // Balance display (for providers like DeepSeek) - centered, one line
                    <div className="flex items-center justify-center gap-2">
                      <span className="text-cyber-text font-bold text-2xl">
                        {quota.label || t('model.balance')}
                      </span>
                      <span className="text-cyber-text font-bold text-2xl">
                        {quota.balance.toFixed(2)} {quota.balanceUnit || 'CNY'}
                      </span>
                    </div>
                  ) : (
                    // Percentage progress bar (for quota-based providers)
                    <>
                      <div className="flex items-center justify-between text-xs">
                        <span className="text-cyber-text font-bold">
                          {quota.label || `${quota.percentage.toFixed(1)}%`}
                        </span>
                        <span className="text-cyber-text-muted text-[10px]">
                          {formatCountdown(
                            // eslint-disable-next-line react-hooks/purity
                            quota.resetAt - Date.now(),
                            t
                          )}
                        </span>
                      </div>
                      <div className="h-1.5 bg-cyber-border/30 rounded-full overflow-hidden">
                        <div
                          className="h-full bg-gradient-to-r from-cyber-accent to-cyber-accent/70 rounded-full transition-all duration-300"
                          style={{ width: `${quota.percentage}%` }}
                        />
                      </div>
                    </>
                  )}
                </div>
              ))
            ) : (
              <div className="text-center text-cyber-text-muted text-xs">
                {t('model.noUsageData')}
              </div>
            )}
          </div>
        ) : (
          // Config mode - show existing info
          <div className="text-xs space-y-1.5 font-mono">
            <div className="flex items-center gap-1 truncate">
              <span className="text-cyber-text/60">{t('model.label')}:</span>
              <span className="truncate text-cyber-text/60">{modelId || '-'}</span>
            </div>
            <div className="flex items-center gap-1 truncate">
              <span className="text-cyber-text/60">{t('model.source')}:</span>
              <span className="truncate text-cyber-text/60">
                {(() => {
                  const url = baseUrl || anthropicUrl;
                  if (!url) return '-';
                  try {
                    return new URL(url).hostname;
                  } catch {
                    return url;
                  }
                })()}
              </span>
            </div>

            <div className="flex items-center gap-1">
              <span className="text-cyber-text/60">{t('model.latency')}:</span>
              {isPinging ? (
                <MatrixDecode />
              ) : latency === -1 ? (
                <span className="text-red-500 font-bold">Error</span>
              ) : latency !== undefined ? (
                <span
                  className={
                    latency < 200
                      ? 'text-green-500'
                      : latency < 500
                        ? 'text-yellow-500'
                        : 'text-red-500'
                  }
                >
                  {latency}ms
                </span>
              ) : (
                <span className="text-cyber-text-muted/70 text-xs">{t('model.notTested')}</span>
              )}
            </div>

            {/* Protocol row */}
            <div className="flex items-center gap-1 truncate">
              <span className="truncate text-cyber-text/60">
                {protocols.includes('openai') && (
                  <span className={openaiTested ? 'text-cyber-text/60' : 'text-cyber-text/30'}>
                    [OpenAI]
                  </span>
                )}
                {protocols.includes('openai') && protocols.includes('anthropic') && ' '}
                {protocols.includes('anthropic') && (
                  <span className={anthropicTested ? 'text-cyber-text/60' : 'text-cyber-text/30'}>
                    [Anthropic]
                  </span>
                )}
                {protocols.length === 0 && '-'}
              </span>
            </div>
          </div>
        )}

        {/* Model icon bottom-right - only show in config mode */}
        {iconPath && viewMode === 'config' && (
          <img
            src={iconPath}
            alt={name}
            className={`absolute bottom-3 right-3 w-8 h-8 ${selected || isActive ? 'opacity-100' : 'opacity-60'}`}
            onError={(e) => {
              (e.target as HTMLImageElement).style.display = 'none';
            }}
          />
        )}
      </div>
    );
  },
  (prev, next) => {
    // Custom comparator: skip function props (new refs each render)
    const keys: (keyof ModelCardProps)[] = [
      'id',
      'name',
      'type',
      'baseUrl',
      'anthropicUrl',
      'modelId',
      'latency',
      'openaiTested',
      'anthropicTested',
      'isPinging',
      'selected',
      'viewMode',
    ];
    const p = prev as unknown as Record<string, unknown>;
    const n = next as unknown as Record<string, unknown>;
    for (const k of keys) {
      if (p[k] !== n[k]) return false;
    }
    if (p.isActive !== n.isActive) return false;
    // Compare protocols array by value
    const pp = prev.protocols || [],
      np = next.protocols || [];
    if (pp.length !== np.length || pp.some((v, i) => v !== np[i])) return false;
    // Compare usageData (shallow)
    if (p.usageData !== n.usageData) return false;
    return true;
  }
);
