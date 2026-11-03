import type { ModelUsageData } from '../../api/models';
import type { ModelConfig } from '../../api/types';
import { getModelIcon } from '../../components/cards';
import { isSmartRouterId } from '../../utils/smartRouter';
import {
  balanceLabelKey,
  formatQuotaPercent,
  primaryQuota,
  quotaPercent,
  quotaPeriodKeys,
} from '../../utils/modelUsage';
import type { TKey } from '../../i18n';
import type { UsageIslandStyle } from '../../stores/usageIslandStore';
import type { AccountCardData, AccountProvider } from '../AccountCenter/accountCenterData';
import type { CachedAccount } from '../AccountCenter/useAccountCardCache';
import type { UsageRefreshOptions } from '../ModelNexus/context';

type RefreshError = (error: unknown) => void;

export interface UsageLine {
  label: string;
  // Remaining allowance; null keeps an unknown quota from reading as empty.
  percent: number | null;
  value: string;
  /** Unix seconds, matching QuotaCountdown. */
  resetAt: number | null;
}

export interface UsageIslandItem {
  kind: 'account' | 'model';
  key: string;
  name: string;
  detail: string;
  icon: string | null;
  percent: number | null;
  value: string;
  lines: UsageLine[];
}

export interface UsageSource extends UsageIslandItem {
  /** Absent while the source is cached only or already busy. */
  refresh?: (manual: boolean, onError: RefreshError) => Promise<unknown>;
}

const NO_VALUE = '—';
const MAX_LINES = 2;
// Large token balances arrive scaled, with the scale in their unit.
const TOKEN_SCALE = /^([KMB]) Tokens$/;
// One decimal is kept, so these already read as the next unit.
const ROUNDS_TO_MILLION = 999_950;
const ROUNDS_TO_BILLION = 999_950_000;
const unknownLine: UsageLine = { label: '', percent: null, value: NO_VALUE, resetAt: null };

export const accountSourceKey = (providerId: string, accountId: string) =>
  `account:${providerId}:${accountId}`;
export const modelSourceKey = (modelId: string) => `model:${modelId}`;

/** Fits a balance in the logo slot, keeping any currency mark around the number. */
export function compactValue(value: string) {
  const parts = /^([^\d-]*)(-?[\d,]*\.?\d+)(.*)$/.exec(value.trim());
  if (!parts) return value;
  const [, mark, digits, suffix] = parts;
  // Chinese locales write US dollars as "US$"; the slot only has room for "$".
  const prefix = mark.replace(/^US\$/, '$');
  const amount = Number(digits.replace(/,/g, ''));
  const size = Math.abs(amount);
  const scaled = (divisor: number, unit: string) =>
    `${Number((amount / divisor).toFixed(1))}${unit}`;
  const short =
    size >= ROUNDS_TO_BILLION
      ? scaled(1e9, 'B')
      : size >= ROUNDS_TO_MILLION
        ? scaled(1e6, 'M')
        : size >= 1e3
          ? scaled(1e3, 'K')
          : size >= 100
            ? String(Math.round(amount))
            : digits;
  return `${prefix}${short}${suffix}`;
}

function withLines(lines: UsageLine[]) {
  const shown = (lines.length ? lines.slice(0, MAX_LINES) : [unknownLine]).map((line) => ({
    ...line,
    percent: line.percent === null ? null : Math.max(0, Math.min(100, Math.round(line.percent))),
  }));
  return { percent: shown[0].percent, value: shown[0].value, lines: shown };
}

function accountLines(account: Pick<AccountCardData, 'metrics' | 'authorizationFailed'>) {
  if (account.authorizationFailed) return withLines([]);
  const known = account.metrics.filter((metric) => metric.value !== NO_VALUE);
  return withLines(
    (known.length ? known : account.metrics).map((metric) => ({
      label: metric.label,
      percent: metric.percent ?? null,
      value: metric.value,
      resetAt: metric.resetAt ?? null,
    }))
  );
}

function modelLines(t: (key: TKey) => string, usage?: ModelUsageData) {
  const balances = usage?.quotas.filter((quota) => quota.balance != null) ?? [];
  if (balances.length)
    return withLines(
      balances.map((quota) => ({
        ...unknownLine,
        label: t(balanceLabelKey(quota)),
        // The island hides balance labels, so a minus sign marks money already spent.
        value: `${quota.spent && quota.balance! > 0 ? '-' : ''}${quota.balance!.toFixed(2)}${
          TOKEN_SCALE.exec(quota.balanceUnit ?? '')?.[1] ?? ''
        }`,
      }))
    );
  const primary = usage && primaryQuota(usage);
  if (!primary) return withLines([]);
  const quotas = usage.quotas.filter((quota) => quota !== primary && quota.balance == null);
  return withLines(
    [primary, ...quotas].map((quota) => ({
      label: quota.period ? t(quotaPeriodKeys[quota.period]) : '',
      percent: quotaPercent(quota),
      value: formatQuotaPercent(quota),
      // Legacy quotas may carry placeholder reset times without a known period.
      resetAt: quota.period && quota.resetAt > 0 ? quota.resetAt / 1000 : null,
    }))
  );
}

type DemoQuota = [label: string, percent: number, resetHours: number];

const demoSources: (Omit<UsageIslandItem, 'percent' | 'value' | 'lines'> & {
  quotas?: DemoQuota[];
})[] = [
  {
    kind: 'account',
    key: 'demo:chatgpt',
    name: 'ChatGPT',
    detail: 'alex@example.com',
    icon: '/icons/tools/codex.svg',
    quotas: [
      ['5h', 82, 2.4],
      ['7d', 64, 90],
    ],
  },
  {
    kind: 'account',
    key: 'demo:claude',
    name: 'Claude Code',
    detail: 'sam@example.com',
    icon: '/icons/tools/claudecode.svg',
    quotas: [
      ['5h', 46, 1.2],
      ['7d', 27, 40],
    ],
  },
  {
    kind: 'model',
    key: 'demo:deepseek',
    name: 'DeepSeek',
    detail: 'deepseek-v4',
    icon: '/icons/models/deepseek.svg',
  },
  {
    kind: 'account',
    key: 'demo:kimi',
    name: 'Kimi',
    detail: 'kim@example.com',
    icon: '/icons/models/kimi.svg',
    quotas: [
      ['5h', 91, 4],
      ['7d', 73, 100],
    ],
  },
  {
    kind: 'model',
    key: 'demo:glm',
    name: 'GLM',
    detail: 'glm-5',
    icon: '/icons/models/glm.svg',
    quotas: [
      ['5h', 8, 0.5],
      ['7d', 54, 70],
    ],
  },
  {
    kind: 'model',
    key: 'demo:minimax',
    name: 'MiniMax',
    detail: 'minimax-m3',
    icon: '/icons/models/minimax.svg',
    quotas: [
      ['5h', 100, 5],
      ['7d', 88, 120],
    ],
  },
];
const DEMO_BALANCE = '12.40';
// The stage has a fixed height, so each style previews what fits it.
const demoCounts: Record<UsageIslandStyle, number> = { row: 3, column: 2, dial: 6, line: 2 };

export function usageIslandDemoItems(style: UsageIslandStyle, now: number): UsageIslandItem[] {
  return demoSources.slice(0, demoCounts[style]).map(({ quotas, ...source }) => ({
    ...source,
    ...withLines(
      quotas
        ? quotas.map(([label, percent, resetHours]) => ({
            label,
            percent,
            value: `${percent}%`,
            resetAt: now + resetHours * 3600,
          }))
        : [{ ...unknownLine, value: DEMO_BALANCE }]
    ),
  }));
}

export function accountSources(
  providers: AccountProvider[],
  cached: Record<string, CachedAccount[]>
): UsageSource[] {
  return providers.flatMap((provider) => {
    const settled = provider.loaded ?? !provider.loading;
    const live = new Set(provider.accounts.map((account) => account.id));
    // Cached rows only stand in until the saved accounts have been read.
    const stale =
      settled && !provider.readFailed
        ? []
        : (cached[provider.id] ?? []).filter((account) => !live.has(account.id));
    const source = (
      account: CachedAccount | AccountCardData,
      refresh?: UsageSource['refresh']
    ): UsageSource => ({
      kind: 'account',
      key: accountSourceKey(provider.id, account.id),
      name: provider.name,
      detail: account.identity,
      icon: provider.icon,
      ...accountLines(account),
      refresh,
    });
    return [
      ...provider.accounts.map((account) =>
        source(
          account,
          provider.busy || account.pending || account.refreshing
            ? undefined
            : (_manual, onError) => account.refresh(onError)
        )
      ),
      ...stale.map((account) => source(account)),
    ];
  });
}

export function modelSources(
  models: ModelConfig[],
  usage: Record<string, ModelUsageData>,
  refreshing: Set<string>,
  refresh: (modelId: string, options?: UsageRefreshOptions) => Promise<void>,
  t: (key: TKey) => string
): UsageSource[] {
  return models
    .filter(
      (model) =>
        model.internalId !== 'local-server' &&
        !isSmartRouterId(model.internalId) &&
        model.modelType !== 'DEMO'
    )
    .map((model) => ({
      kind: 'model',
      key: modelSourceKey(model.internalId),
      name: model.name,
      detail: model.modelId || '',
      icon: getModelIcon(model.name, model.modelId),
      ...modelLines(t, usage[model.internalId]),
      refresh: refreshing.has(model.internalId)
        ? undefined
        : (manual, onError) => refresh(model.internalId, { silent: !manual, onError }),
    }));
}
