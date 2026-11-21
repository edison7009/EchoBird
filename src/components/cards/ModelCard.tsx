// ModelCard component

import React from 'react';
import { KeyRound, Pencil, RefreshCw, Trash2 } from 'lucide-react';
import { useBrandColour } from '../../utils/brandColour';
import '../brandCards.css';
import './ModelCard.css';
import { useConfirm } from '../ConfirmDialog';
import { useI18n } from '../../hooks/useI18n';
import type { ModelUsageData } from '../../api/tauri';
import type { TKey } from '../../i18n/types';
import { useUsageClock } from '../../hooks/useUsageClock';
import { LatencyIndicator } from '../LatencyIndicator';
import { modelLatencyColor } from '../../utils/modelLatency';
import type { UsageQuota } from '../../api/models';
import {
  balanceLabelKey,
  compactCountdown,
  formatQuotaPercent,
  primaryQuota,
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
  isPinging?: boolean; // currently pinging
  usageData?: ModelUsageData; // usage quota data
  onEdit?: () => void; // edit callback
  onDelete?: () => void; // delete callback
  onRefresh?: () => void; // refresh usage callback
  isRefreshingUsage?: boolean; // usage refresh in progress
  onAccessKey?: () => void; // open the usage access modal (Volcengine AK/SK, Zhipu team)
  akSkMissing?: boolean; // AK/SK not yet configured -> show hint
  dragHandle?: React.ReactNode;
}

/** A quota's own name: "5h", "7 days", or the balance label. */
const quotaName = (quota: UsageQuota, t: (key: TKey) => string): string =>
  quota.balance != null
    ? t(balanceLabelKey(quota))
    : quota.period
      ? t(quotaPeriodKeys[quota.period])
      : t('model.quota.used');

// ModelCard component: the model and its usage on one card. The quota that
// runs out first is the headline, its bar is the card's bottom edge, and any
// other quotas sit to the right of the headline.
export const ModelCard = React.memo(
  ({
    name,
    type: _type,
    baseUrl,
    anthropicUrl,
    modelId,
    latency,
    isPinging = false,
    usageData,
    onEdit,
    onDelete,
    onRefresh,
    isRefreshingUsage = false,
    onAccessKey,
    akSkMissing,
    dragHandle,
  }: ModelCardProps) => {
    const confirm = useConfirm();
    const { t } = useI18n();

    const quotas = usageData?.quotas ?? [];
    const primary = usageData ? primaryQuota(usageData) : undefined;
    const heroBalance = primary ? undefined : quotas.find((quota) => quota.balance != null);
    const others = quotas.filter((quota) => quota !== primary && quota !== heroBalance);
    const remaining = primary ? quotaPercent(primary) : null;
    const tone = quotaTone(remaining);
    const headline = primary
      ? formatQuotaPercent(primary)
      : (heroBalance?.balance?.toFixed(2) ?? '—');

    const now = useUsageClock(!!primary && primary.resetAt > 0);

    // The model's logo (from its ID, so a reseller shows the model it serves),
    // or the provider's, from the address, when the ID names no known brand.
    const shownIcon = getModelIcon('', modelId) ?? getModelIcon('', baseUrl || anthropicUrl || '');
    const brand = useBrandColour(shownIcon);
    const logoClass = brand.neutral && brand.glyph ? ' is-neutral-logo' : '';
    const hostname = (() => {
      const url = baseUrl || anthropicUrl;
      if (!url) return '';
      try {
        return new URL(url).hostname;
      } catch {
        return url;
      }
    })();

    return (
      <div
        className="brand-card model-card relative flex h-40 flex-col px-4 pb-[0.9rem] pt-[0.85rem] cursor-default"
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
        {/* The headline quota's bar is the card's bottom edge. */}
        {primary && (
          <div
            role="progressbar"
            aria-label={quotaName(primary, t)}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={remaining ?? undefined}
            className="model-card-edge"
          >
            <div
              className={`quota-progress-fill h-full bg-gradient-to-r ${quotaColorClasses[tone].gradient}`}
              style={{ width: `${remaining ?? 0}%` }}
            />
          </div>
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
          {(onAccessKey || onRefresh || onEdit || onDelete) && (
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
          )}
        </div>

        {/* Model ID, with the latency on the right, and its address under it. */}
        <div className="brand-card-latency mt-1 flex h-5 min-w-0 items-center gap-2 text-xs">
          <span className="min-w-0 flex-1 truncate font-mono text-[13px] font-semibold text-cyber-text/90">
            {modelId || '-'}
          </span>
          {isPinging ? (
            <LatencyIndicator />
          ) : latency === -2 ? (
            <span className="font-bold text-red-500">{t('model.timedOut')}</span>
          ) : latency === -1 ? (
            <span className="font-bold text-red-500">Error</span>
          ) : latency !== undefined ? (
            <span className={`font-bold tabular-nums ${modelLatencyColor(latency)}`}>
              {latency}ms
            </span>
          ) : (
            <span className="text-cyber-text-muted">{t('model.notTested')}</span>
          )}
        </div>

        {/* Where the model is served from. */}
        {hostname && (
          <div className="truncate text-[11px] leading-4 text-cyber-text-muted">{hostname}</div>
        )}

        {/* The headline: what is left of the quota that runs out first, or the balance. */}
        <div className="mt-auto flex min-w-0 items-end gap-2.5">
          <span
            className={`model-card-hero${!primary && !heroBalance ? ' text-cyber-text-muted' : tone === 'critical' ? ` ${quotaColorClasses.critical.text}` : ''}`}
          >
            {headline}
          </span>
          <span className="model-card-side">
            {primary ? (
              <>
                <span>
                  {/* A quota without a period reports what was used, not what is left. */}
                  {primary.period
                    ? `${quotaName(primary, t)} ${t('model.quota.remaining')}`
                    : quotaName(primary, t)}
                </span>
                {primary.resetAt > 0 && <span>{compactCountdown(primary.resetAt, now)}</span>}
              </>
            ) : heroBalance ? (
              <>
                <span>{quotaName(heroBalance, t)}</span>
                <b>{heroBalance.balanceUnit || 'CNY'}</b>
              </>
            ) : (
              <span>{akSkMissing ? t('model.akSkRequired') : t('model.noUsageData')}</span>
            )}
          </span>
          {/* The other quotas, on the right, each with a short bar. */}
          {others.length > 0 && (
            <span className="model-card-others">
              {others.map((quota, idx) =>
                quota.balance != null ? (
                  <span key={idx} className="model-card-other-balance">
                    {quotaName(quota, t)} <b>{quota.balance.toFixed(2)}</b>{' '}
                    {quota.balanceUnit || 'CNY'}
                  </span>
                ) : (
                  <span key={idx} className="model-card-other">
                    <span className="model-card-other-name">{quotaName(quota, t)}</span>
                    <span
                      role="progressbar"
                      aria-label={quotaName(quota, t)}
                      aria-valuemin={0}
                      aria-valuemax={100}
                      aria-valuenow={quotaPercent(quota) ?? undefined}
                      className="model-card-mini"
                    >
                      <span
                        className={`quota-progress-fill block h-full bg-gradient-to-r ${quotaColorClasses[quotaTone(quotaPercent(quota))].gradient}`}
                        style={{ width: `${quotaPercent(quota) ?? 0}%` }}
                      />
                    </span>
                    <b
                      className={
                        quotaTone(quotaPercent(quota)) === 'critical'
                          ? quotaColorClasses.critical.text
                          : undefined
                      }
                    >
                      {formatQuotaPercent(quota)}
                    </b>
                  </span>
                )
              )}
            </span>
          )}
        </div>
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
      'isPinging',
      'isRefreshingUsage',
      'akSkMissing',
      'dragHandle',
    ];
    const p = prev as unknown as Record<string, unknown>;
    const n = next as unknown as Record<string, unknown>;
    for (const k of keys) {
      if (p[k] !== n[k]) return false;
    }
    // Compare usageData (shallow)
    if (p.usageData !== n.usageData) return false;
    // An action shows only while its callback is given.
    for (const k of ['onEdit', 'onDelete', 'onRefresh', 'onAccessKey'] as const) {
      if (!prev[k] !== !next[k]) return false;
    }
    return true;
  }
);
