// ModelCard component

import React from 'react';
import { KeyRound, Pencil, RefreshCw, Trash2 } from 'lucide-react';
import { useBrandColour } from '../../utils/brandColour';
import '../brandCards.css';
import { useConfirm } from '../ConfirmDialog';
import { useI18n } from '../../hooks/useI18n';
import type { ModelUsageData } from '../../api/tauri';
import type { TKey } from '../../i18n/types';
import { useUsageClock } from '../../hooks/useUsageClock';
import { LatencyIndicator } from '../LatencyIndicator';
import { modelLatencyColor } from '../../utils/modelLatency';
import {
  balanceLabelKey,
  formatQuotaPercent,
  quotaPercent,
  quotaPeriodKeys,
} from '../../utils/modelUsage';
import { quotaColorClasses, quotaTone } from '../../utils/quotaColors';

// Smart icon detection — match model name/ID to icon file
export const getModelIcon = (name: string, modelId?: string): string | null => {
  const nameText = name.toLowerCase();
  const modelIdText = (modelId || '').toLowerCase();

  // Provider/directory rows do not pass a real modelId. In that case the
  // vendor name must win over model-family words embedded in the title.
  if (!modelIdText) {
    if (['open design', 'opendesign'].some((kw) => nameText.includes(kw))) {
      return './icons/tools/opendesign.png';
    }
    if (['ccvibe', 'cc vibe', 'cc-vibe'].some((kw) => nameText.includes(kw))) {
      return './icons/models/ccvibe.png';
    }
  }

  const text = `${nameText} ${modelIdText}`;

  // Matching rules: keywords -> icon file
  const iconMap: [string[], string][] = [
    // Qwen model (紫色): model id (qwen3.8-max-preview etc.) or「通义」.
    // MUST precede the platform rule — the platform row's text is CJK「千问」
    // (no `qwen` token), and the token-plan URL carries no `qwen` substring, so
    // this rule only fires when a real qwen modelId is present (config tab).
    [['qwen', '通义', 'tongyi'], 'qwen'],
    // Qwen platform (白底): platform name「千问AI平台」or token-plan/bailian
    // host. Catches the usage-tab URL and the right-panel provider row, which
    // have no modelId and thus miss the model rule above.
    [['千问', 'qianwenai', 'maas.aliyuncs', 'bailian'], 'qianwen'],
    [['claude', 'anthropic', 'sonnet', 'opus', 'haiku'], 'claude'],
    [['gpt', 'openai', 'chatgpt', 'o1', 'o3'], 'chatgpt'],
    [['gemma'], 'google'],
    [['gemini', 'palm'], 'gemini'],
    [['deepseek'], 'deepseek'],
    [['mistral', 'mixtral'], 'mistral'],
    [['minimax'], 'minimax'],
    [['grok', 'x.ai'], 'grok'],
    [['groq'], 'groq'],
    [['cerebras'], 'cerebras'],
    [['modelscope'], 'modelscope'],
    [['ollama'], 'ollama'],
    [['sambanova', 'samba nova'], 'sambanova'],
    [['scaleway'], 'scaleway'],
    [['typhoon', 'opentyphoon'], 'typhoon'],
    [['w&b', 'wandb', 'weights & biases'], 'wandb'],
    [['kimi', 'moonshot'], 'kimi'],
    [['longcat', '美团', 'meituan', '龙猫'], 'longcat'],
    [['glm', 'zhipu', '智谱', 'z.ai', 'bigmodel'], 'glm'],
    [['ernie', 'wenxin', '文心', 'qianfan', '千帆', 'baidu'], 'ernie'],
    [['hunyuan', '混元', 'hy', 'tencent', '腾讯'], 'hunyuan'],
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
    [['agnes'], 'agnes'],
    // OpenCode (Zen / Go) — a gateway hosting many vendors' models, matched
    // by the provider-row name. The model brand in the id must win, so this
    // stays below the model-brand rules.
    [['opencode', 'open code'], 'opencode'],
    [['kilo'], 'kilo'],
    // Resellers (pure compute providers that host third-party models, e.g.
    // CC Vibe) go LAST. A model card carries a modelId that identifies the
    // actual model brand (glm/kimi/deepseek/minimax), and the model logo must
    // win. The vendor logo only matches when the modelId has no recognized
    // brand — such as provider rows, which pass modelId=''. Model ID and vendor
    // are separate concerns; do not move resellers above model brands.
    [['siliconflow', 'siliconcloud', '硅基流动'], 'siliconcloud'],
    [['huawei', 'modelarts'], 'huaweicloud'],
    [['iflytek', 'xf-yun', 'astron', '讯飞'], 'iflytek'],
    [['ant ling', 'ant-ling', '百灵'], 'antling'],
    [['novita'], 'novita'],
    [['fireworks'], 'fireworks'],
    [['baseten'], 'baseten'],
    [['nous'], 'nousresearch'],
    [['ccvibe', 'cc vibe', 'cc-vibe'], 'ccvibe'],
    // 88API — token aggregation relay hosting many model brands; matches on
    // its own name only, so it never outranks a recognized model brand above.
    [['88api'], '88api'],
    [['grooroute'], 'grooroute'],
    [['sidrune', 'fluxion'], 'sidrune'],
  ];

  for (const [keywords, icon] of iconMap) {
    if (keywords.some((kw) => text.includes(kw))) {
      if (icon === 'kilo') return './icons/tools/kilo.png';
      if (icon === 'ccvibe') return './icons/models/ccvibe.png';
      if (icon === '88api') return './icons/models/88api.png';
      if (icon === 'grooroute') return './icons/models/grooroute.png';
      if (icon === 'byteplus') return './icons/models/byteplus.png';
      if (icon === 'qianwen') return './icons/models/qianwen.png';
      if (icon === 'meta') return './icons/models/meta.png';
      return `./icons/models/${icon}.svg`;
    }
  }
  return null;
};

// Card skeleton (loading state)
export const ModelCardSkeleton = () => (
  <div className="h-40 p-4 bg-cyber-surface/60 rounded-card animate-pulse">
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
  latency?: number; // ms; undefined = untested, -1 = failed, -2 = timed out
  protocols?: ('openai' | 'anthropic')[]; // supported API protocols
  openaiTested?: boolean; // OpenAI protocol tested
  anthropicTested?: boolean; // Anthropic protocol tested
  isPinging?: boolean; // currently pinging
  viewMode?: 'config' | 'usage'; // display mode
  usageData?: ModelUsageData; // usage quota data
  onEdit?: () => void; // edit callback
  onDelete?: () => void; // delete callback
  onRefresh?: () => void; // refresh usage callback (usage mode only)
  isRefreshingUsage?: boolean; // usage refresh in progress (usage mode only)
  onAccessKey?: () => void; // open AK/SK config modal (Volcengine usage mode)
  akSkMissing?: boolean; // AK/SK not yet configured -> show hint
  dragHandle?: React.ReactNode;
}

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
    viewMode = 'config',
    usageData,
    onEdit,
    onDelete,
    onRefresh,
    isRefreshingUsage = false,
    onAccessKey,
    akSkMissing,
    dragHandle,
  }: ModelCardProps) => {
    // Config tab: resolve icon from the MODEL ID only (platform name must not
    // leak in — otherwise a Qwen platform card configured with model id
    // `glm-5.2` matches the「千问」platform rule and shows qianwen.png instead
    // of glm.svg). The 用量 tab below resolves from baseUrl instead, so the
    // two tabs use two separate icon lookups (model-id vs vendor) by design.
    const iconPath = getModelIcon('', modelId);
    const confirm = useConfirm();
    const { t } = useI18n();

    const now = useUsageClock(
      viewMode === 'usage' &&
        !!usageData?.quotas.some((quota) => quota.balance == null && quota.resetAt > 0)
    );

    // Usage mode shows the provider's logo (from the address), config mode
    // the model's; the card takes its colour from the logo it shows.
    const usageIcon = viewMode === 'usage' ? getModelIcon('', baseUrl || anthropicUrl || '') : null;
    const shownIcon = viewMode === 'usage' ? usageIcon : iconPath;
    const brand = useBrandColour(shownIcon);
    const logoClass = brand.neutral && brand.glyph ? ' is-neutral-logo' : '';
    const hostname = (() => {
      const url = baseUrl || anthropicUrl;
      if (!url) return '-';
      try {
        return new URL(url).hostname;
      } catch {
        return url;
      }
    })();

    return (
      <div
        className="brand-card relative flex h-40 flex-col px-4 pb-[0.9rem] pt-[0.85rem] cursor-default"
        style={{ '--brand': brand.from } as React.CSSProperties}
      >
        {shownIcon && (
          <img
            src={shownIcon}
            alt=""
            draggable={false}
            className={`brand-card-mark${logoClass}`}
            onError={(event) => {
              event.currentTarget.style.display = 'none';
            }}
          />
        )}
        <div className="flex items-center gap-2">
          {dragHandle && <span className="-ml-1.5 flex">{dragHandle}</span>}
          {shownIcon && (
            <img
              src={shownIcon}
              alt=""
              draggable={false}
              className={`h-5 w-5 flex-shrink-0 object-contain${logoClass}`}
              onError={(event) => {
                event.currentTarget.style.display = 'none';
              }}
            />
          )}
          <div className="h-7 min-w-0 flex-1 truncate text-base font-bold leading-7">
            {name || <span className="invisible">-</span>}
          </div>
          {/* Actions as icons in one pill: edit / delete, or access key / refresh. */}
          {viewMode === 'config'
            ? (onEdit || onDelete) && (
                <div className="brand-card-actions">
                  {onEdit && (
                    <button
                      type="button"
                      aria-label={t('btn.edit')}
                      onClick={(e) => {
                        e.stopPropagation();
                        onEdit();
                      }}
                    >
                      <Pencil size={14} aria-hidden="true" />
                    </button>
                  )}
                  {onDelete && (
                    <button
                      type="button"
                      data-danger
                      aria-label={t('btn.delete')}
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
                      <Trash2 size={14} aria-hidden="true" />
                    </button>
                  )}
                </div>
              )
            : (onAccessKey || onRefresh) && (
                <div className="brand-card-actions">
                  {onAccessKey && (
                    <button
                      type="button"
                      aria-label={t('model.accessKey')}
                      onClick={(e) => {
                        e.stopPropagation();
                        onAccessKey();
                      }}
                    >
                      <KeyRound size={14} aria-hidden="true" />
                    </button>
                  )}
                  {onRefresh && (
                    <button
                      type="button"
                      disabled={isRefreshingUsage}
                      aria-busy={isRefreshingUsage}
                      aria-label={t('btn.refresh')}
                      onClick={(e) => {
                        e.stopPropagation();
                        if (!isRefreshingUsage) {
                          onRefresh();
                        }
                      }}
                    >
                      <RefreshCw
                        size={14}
                        className={isRefreshingUsage ? 'animate-spin' : ''}
                        aria-hidden="true"
                      />
                    </button>
                  )}
                </div>
              )}
        </div>

        {viewMode === 'usage' ? (
          // Usage: quota bars, or one large balance.
          <div className="flex flex-1 flex-col justify-end gap-2 pt-2">
            {usageData?.quotas && usageData.quotas.length > 0 ? (
              usageData.quotas.map((quota, idx) => {
                const remaining = quotaPercent(quota);
                const tone = quotaTone(remaining);
                const colors = quotaColorClasses[tone];
                return quota.balance !== undefined && quota.balance !== null ? (
                  <div key={idx} className="m-auto flex items-baseline justify-center gap-2">
                    <span className="text-xs text-cyber-text-secondary">
                      {t(balanceLabelKey(quota))}
                    </span>
                    <span className="text-[1.75rem] font-extrabold tabular-nums text-cyber-text">
                      {quota.balance.toFixed(2)}
                    </span>
                    <span className="text-xs font-bold text-cyber-text-secondary">
                      {quota.balanceUnit || 'CNY'}
                    </span>
                  </div>
                ) : (
                  <div key={idx} className="space-y-1">
                    <div className="flex items-baseline gap-2 text-xs">
                      {quota.period && (
                        <span className="flex-shrink-0 font-bold text-cyber-text">
                          {`${t(quotaPeriodKeys[quota.period])} ${t('model.quota.remaining')}`}
                        </span>
                      )}
                      <span className="min-w-0 flex-1 truncate text-[10.5px] text-cyber-text-secondary/85">
                        {quota.resetAt > 0 ? formatCountdown(quota.resetAt - now, t) : '—'}
                      </span>
                      <span
                        className={`brand-quota-value flex-shrink-0 font-extrabold tabular-nums${tone === 'critical' ? ' is-critical' : ''}`}
                      >
                        {formatQuotaPercent(quota)}
                      </span>
                    </div>
                    <div
                      role="progressbar"
                      aria-label={
                        quota.period ? t(quotaPeriodKeys[quota.period]) : t('model.quota.used')
                      }
                      aria-valuemin={0}
                      aria-valuemax={100}
                      aria-valuenow={remaining ?? undefined}
                      className={`relative h-0 flex-shrink-0 rounded-full border-t-[6px] ${colors.track}`}
                    >
                      <div
                        className={`quota-progress-fill absolute bottom-0 left-0 h-0 rounded-full border-t-[length:inherit] border-transparent bg-origin-border bg-gradient-to-r ${colors.gradient}`}
                        style={{ width: `${remaining ?? 0}%` }}
                      />
                    </div>
                  </div>
                );
              })
            ) : (
              <div className="m-auto text-center text-xs text-cyber-text-secondary">
                {akSkMissing ? t('model.akSkRequired') : t('model.noUsageData')}
              </div>
            )}
          </div>
        ) : (
          // Config: model, source and latency on labelled rows; the protocols below.
          <>
            <div className="mt-[0.7rem] flex min-w-0 items-baseline gap-1.5 font-mono text-[13.5px] font-semibold text-cyber-text/90">
              <span className="brand-card-label">{t('model.label')}</span>
              <span className="truncate">{modelId || '-'}</span>
            </div>
            <div className="mt-0.5 flex min-w-0 items-baseline gap-1.5 text-xs text-cyber-text-secondary/90">
              <span className="brand-card-label">{t('model.source')}</span>
              <span className="truncate">{hostname}</span>
            </div>
            <div className="brand-card-latency mt-0.5 flex min-w-0 items-center gap-1.5 text-xs">
              <span className="brand-card-label">{t('model.latency')}</span>
              {isPinging ? (
                <LatencyIndicator />
              ) : latency === -2 ? (
                <span className="font-bold text-red-500">{t('model.timedOut')}</span>
              ) : latency === -1 ? (
                <span className="font-bold text-red-500">Error</span>
              ) : latency !== undefined ? (
                <span className={`font-bold ${modelLatencyColor(latency)}`}>{latency}ms</span>
              ) : (
                <span className="text-cyber-text-muted">{t('model.notTested')}</span>
              )}
            </div>
            <div className="mt-auto flex min-w-0 items-center gap-1.5 overflow-hidden">
              {protocols.includes('openai') && (
                <span className={`brand-pill${openaiTested ? '' : ' is-untested'}`}>OpenAI</span>
              )}
              {protocols.includes('anthropic') && (
                <span className={`brand-pill${anthropicTested ? '' : ' is-untested'}`}>
                  Anthropic
                </span>
              )}
              {protocols.length === 0 && <span className="text-xs text-cyber-text-muted">-</span>}
            </div>
          </>
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
      'viewMode',
      'isRefreshingUsage',
      'akSkMissing',
      'dragHandle',
    ];
    const p = prev as unknown as Record<string, unknown>;
    const n = next as unknown as Record<string, unknown>;
    for (const k of keys) {
      if (p[k] !== n[k]) return false;
    }
    // Compare protocols array by value
    const pp = prev.protocols || [],
      np = next.protocols || [];
    if (pp.length !== np.length || pp.some((v, i) => v !== np[i])) return false;
    // Compare usageData (shallow)
    if (p.usageData !== n.usageData) return false;
    return true;
  }
);
