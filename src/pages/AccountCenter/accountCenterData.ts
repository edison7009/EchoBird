import { accountPluginAvailable, communityAccountApplications } from '../../hooks/useAccountPlugin';
import type { AccountDirectoryContextType } from '../../features/accounts/context';
import type { ManagedAccount } from '../AppManager/useManagedAccounts';
import type { AntigravityQuota, TraeAccount, WorkBuddyAccount } from '../../api/tauri';
import type { TKey } from '../../i18n';
import { codexPlanLabel } from '../AppManager/codexPlanLabel';
import { isFreePlan, minimaxPlanLabel, qoderPlanLabel } from '../AppManager/accountPlanLabel';
import {
  MINIMAX_REWARD_CACHE_SECONDS,
  minimaxCreditBuckets,
  minimaxRewardStatus,
} from '../AppManager/minimaxAccountDisplay';
import {
  qoderExpiry,
  qoderQuotaLabel,
  qoderRewardStatus,
  qoderSubscriptionExpiry,
} from '../AppManager/qoderAccountDisplay';
import { traeClaimedToday, traeClaimUnavailable } from '../AppManager/traeDailyCredits';
import { clineBalanceLabel } from '../AppManager/clineAccountDisplay';
import {
  qwenWorkIdentity,
  qwenWorkCreditBalances,
  qwenWorkPlanLabel,
} from '../AppManager/qwenWorkAccountDisplay';
import { kimiQuotaLabel } from '../AppManager/KimiAccountSection';
import { codeArtsRemainingPercent } from '../AppManager/codeArtsAccountDisplay';
import {
  dumateCreditBalances,
  dumatePlanLabel,
  dumateRewardStatus,
} from '../AppManager/dumateAccountDisplay';
import { devinQuotaLabel } from '../AppManager/devinAccountDisplay';
import { doubaoRemainingPercent, doubaoWindowLabel } from '../AppManager/doubaoAccountDisplay';
import { openAccountPlugins } from '../AppManager/openAccountPlugins';

export interface AccountMetric {
  label: string;
  value: string;
  percent?: number | null;
  resetAt?: number | null;
  resetLabel?: string;
  showProgress?: boolean;
  balanceState?: 'available' | 'empty';
}

export interface AccountCardData {
  id: string;
  identity: string;
  detail?: string;
  plan?: string | null;
  subscriptionEndAt?: number | null;
  metrics: AccountMetric[];
  refreshing: boolean;
  pending?: boolean;
  /** A cached row whose account plugin is no longer installed. */
  cachedOnly?: boolean;
  authorizationFailed: boolean;
  refresh: (onError?: (error: unknown) => void) => Promise<void>;
  claim?: (onError?: (error: unknown) => void) => Promise<unknown>;
  warmUp?: (onError?: (error: unknown) => void) => Promise<unknown>;
  claimDisabled?: boolean;
  dailyClaimedAt?: number | null;
  dailyClaimed?: boolean;
  claimStatusExpiresAt?: number;
  showUnclaimed?: boolean;
  remove: () => void;
}

export interface AccountProvider {
  id: string;
  name: string;
  icon: string;
  website?: string;
  installed: boolean;
  /** The application's account plugin is not installed or enabled. */
  pluginMissing?: boolean;
  busy: boolean;
  loading: boolean;
  loaded?: boolean;
  readFailed?: boolean;
  mutationRevision?: number;
  remainingSeconds: number;
  waitingLabel?: string;
  /** Replaces "Add Account" for providers whose accounts are added differently. */
  addLabel?: string;
  add: () => void;
  cancelLogin: () => void;
  accounts: AccountCardData[];
}

interface AccountGroup<A> {
  accounts: A[];
  busy: boolean;
  loading: boolean;
  loaded?: boolean;
  readFailed?: boolean;
  mutationRevision?: number;
  remainingSeconds: number;
  awaitingClientExit?: boolean;
  refreshing: Set<string>;
  authorizationFailedIds: Set<string>;
  add: () => Promise<void>;
  cancelLogin: () => void;
  refresh: (
    account: A,
    operation?: (account: A) => Promise<A>,
    onError?: (error: unknown) => void
  ) => Promise<unknown>;
  remove: (account: A) => Promise<void>;
}

// Accounts are managed in EchoBird, so refresh, claim and delete work without the
// client. These providers sign in through the client itself, so adding an
// account still needs it installed.
const LOGIN_NEEDS_CLIENT = [
  'grok',
  'copilot',
  'devin',
  'dumate',
  'qwenworkcn',
  'qwenwork',
  'doubaowork',
  'dola',
];

export function accountCenterProviders(
  context: AccountDirectoryContextType,
  t: (key: TKey) => string,
  locale: string
): AccountProvider[] {
  const quota = (
    percent?: number | null,
    resetAt?: number | null,
    label = t('accountCenter.quota')
  ): AccountMetric => ({
    label,
    value: percent == null ? '—' : `${Math.round(percent)}%`,
    percent,
    resetAt,
    showProgress: true,
  });
  const number = (value?: number | null) =>
    value == null ? '—' : new Intl.NumberFormat(locale, { maximumFractionDigits: 2 }).format(value);
  const windowLabel = (index: number) =>
    t('accountCenter.window').replace('{n}', String(index + 1));
  const minimum = (quotas: AntigravityQuota[], prefix: string) =>
    quotas
      .filter((q) => q.name.startsWith(prefix))
      .reduce<
        AntigravityQuota | undefined
      >((lowest, q) => (!lowest || q.remainingPercent < lowest.remainingPercent ? q : lowest), undefined);
  const credits = (
    label: string,
    remaining?: number | null,
    total?: number | null,
    resetAt?: number | null,
    currency?: string | null
  ): AccountMetric => ({
    label,
    value: remaining != null && currency === 'USD' ? `$${number(remaining)}` : number(remaining),
    resetAt,
    percent:
      remaining != null && total != null && total > 0 ? (remaining / total) * 100 : undefined,
  });
  const creditMetrics = (account: WorkBuddyAccount | TraeAccount): AccountMetric[] => {
    const metrics: AccountMetric[] = [];
    if (account.baseRemaining != null)
      metrics.push(
        credits(
          t('agent.baseCredits'),
          account.baseRemaining,
          account.baseTotal,
          account.baseResetAt
        )
      );
    if (account.rewardRemaining != null) {
      const reward = credits(
        t('agent.rewardCredits'),
        account.rewardRemaining,
        account.rewardTotal,
        account.rewardNextExpiryAt
      );
      if (account.rewardNextExpiryAt != null)
        reward.resetLabel = t('agent.rewardCreditsNextExpiry');
      metrics.push(reward);
    }
    if (account.addonRemaining != null)
      metrics.push(credits(t('agent.purchasedCredits'), account.addonRemaining));
    return metrics.length
      ? metrics
      : [credits(t('agent.credits'), account.remaining, account.total, account.expiresAt)];
  };
  const provider = <A extends ManagedAccount>(
    id: string,
    name: string,
    icon: string,
    toolIds: string[],
    group: AccountGroup<A>,
    describe: (
      account: A
    ) => Pick<
      AccountCardData,
      | 'identity'
      | 'detail'
      | 'plan'
      | 'metrics'
      | 'subscriptionEndAt'
      | 'claim'
      | 'warmUp'
      | 'claimDisabled'
      | 'dailyClaimedAt'
      | 'dailyClaimed'
      | 'claimStatusExpiresAt'
      | 'showUnclaimed'
    >
  ): AccountProvider => {
    const installed = context.detectedTools.some(
      (tool) => toolIds.includes(tool.id) && tool.installed
    );
    return {
      id,
      name,
      icon,
      website: context.detectedTools.find((tool) => toolIds.includes(tool.id) && tool.website)
        ?.website,
      installed,
      pluginMissing: communityAccountApplications.includes(id) && !accountPluginAvailable(id),
      busy: group.busy,
      loading: group.loading,
      loaded: group.loaded,
      readFailed: group.readFailed,
      mutationRevision: group.mutationRevision,
      remainingSeconds: group.remainingSeconds,
      waitingLabel: undefined,
      add: () => {
        if (communityAccountApplications.includes(id) && !accountPluginAvailable(id)) {
          void openAccountPlugins().catch((error) => context.setApplyError(String(error)));
        } else if (!installed && LOGIN_NEEDS_CLIENT.includes(id)) {
          context.setApplyError(t('accountCenter.installClientFirst').replace('{name}', name));
        } else {
          void group.add();
        }
      },
      cancelLogin: group.cancelLogin,
      accounts: group.accounts.map((account) => {
        const summary = describe(account);
        const free = isFreePlan(summary.plan);
        return {
          id: account.id,
          ...summary,
          subscriptionEndAt:
            free && summary.subscriptionEndAt == null ? undefined : summary.subscriptionEndAt,
          metrics:
            id !== 'trae' &&
            id !== 'traecn' &&
            id !== 'kiro' &&
            free &&
            summary.metrics.every(
              (metric) => metric.value === '—' && metric.percent == null && metric.resetAt == null
            )
              ? []
              : summary.metrics,
          refreshing: group.refreshing.has(account.id),
          authorizationFailed: group.authorizationFailedIds.has(account.id),
          refresh: async (onError) => {
            await group.refresh(account, undefined, onError);
          },
          remove: () => void group.remove(account),
        };
      }),
    };
  };
  return [
    provider(
      'codex',
      'ChatGPT / Codex',
      '/icons/tools/codex.svg',
      ['chatgptdesktop', 'codex'],
      {
        accounts: context.codexAccounts,
        busy: context.isAddingCodexAccount,
        loading: context.isLoadingCodexAccounts,
        loaded: context.codexAccountsLoaded,
        readFailed: context.codexAccountsReadFailed,
        mutationRevision: context.codexAccountsMutationRevision,
        remainingSeconds: context.codexOAuthRemainingSeconds,
        refreshing: context.refreshingCodexAccountIds,
        authorizationFailedIds: context.codexAuthorizationFailedIds,
        add: context.addCodexAccount,
        cancelLogin: context.cancelCodexLogin,
        refresh: context.refreshCodexAccountQuota,
        remove: context.deleteCodexAccount,
      },
      (account) => {
        const plan = codexPlanLabel(account.plan);
        const windows = account.quotaWindows ?? [];
        const metrics =
          windows.length && windows.every((w) => w.label === '5h' || w.label === '7d')
            ? (plan === 'Plus' ? ['5h', '7d'] : ['7d']).map((label) => {
                const window = windows.find((w) => w.label === label);
                return quota(window?.remainingPercent, window?.resetAt, label);
              })
            : windows.length
              ? windows
                  .filter((w) => w.label !== '5h' || plan === 'Plus')
                  .map((w, i) => quota(w.remainingPercent, w.resetAt, w.label || windowLabel(i)))
              : [quota(account.quotaPercent, account.quotaResetAt)];
        return {
          identity: account.email,
          plan,
          subscriptionEndAt:
            account.plan?.trim().toLowerCase() === 'free'
              ? undefined
              : (account.subscriptionEndAt ?? null),
          metrics,
        };
      }
    ),
    provider(
      'claudecode',
      'Claude Code',
      '/icons/tools/claudecode.svg',
      ['claudecode'],
      context.claudeCodeAccounts,
      (account) => ({
        identity: account.email,
        plan: account.plan,
        metrics: [
          quota(account.fiveHour?.remainingPercent, account.fiveHour?.resetAt, '5h'),
          quota(account.sevenDay?.remainingPercent, account.sevenDay?.resetAt, '7d'),
        ],
        warmUp:
          accountPluginAvailable('claudecode', 'account.warmup') && !isFreePlan(account.plan)
            ? (onError) => context.claudeCodeAccounts.warmUp(account, onError)
            : undefined,
      })
    ),
    {
      // Claude Desktop cannot be signed in from here; it saves snapshots instead.
      ...provider(
        'claudedesktop',
        context.detectedTools.find((tool) => tool.id === 'claudedesktop')?.name ?? 'Claude Desktop',
        '/icons/tools/claudedesktop.svg',
        ['claudedesktop'],
        context.claudeDesktopAccounts,
        (account) => ({
          identity: account.email,
          plan: account.plan,
          metrics: [
            quota(account.fiveHour?.remainingPercent, account.fiveHour?.resetAt, '5h'),
            quota(account.sevenDay?.remainingPercent, account.sevenDay?.resetAt, '7d'),
          ],
          warmUp:
            accountPluginAvailable('claudedesktop', 'account.warmup') && !isFreePlan(account.plan)
              ? (onError) => context.claudeDesktopAccounts.warmUp(account, onError)
              : undefined,
        })
      ),
      addLabel: t('agent.accountSnapshot'),
      waitingLabel: t('agent.accountSnapshotSaving'),
    },
    provider(
      'dsh',
      'DeepSeek Harness',
      '/icons/tools/dsh.png',
      ['dsh'],
      context.deepSeekAccounts,
      (account) => ({
        identity: account.name,
        metrics: account.balances?.length
          ? account.balances.map((balance) => ({
              label: t('model.balance'),
              value: new Intl.NumberFormat(locale, {
                style: 'currency',
                currency: balance.currency,
                maximumFractionDigits: 2,
              }).format(balance.amount),
            }))
          : [{ label: t('model.balance'), value: account.balances == null ? '—' : '0' }],
      })
    ),
    ...(['workbuddy', 'workbuddyai'] as const).map((edition) =>
      provider(
        edition,
        edition === 'workbuddy' ? 'WorkBuddy' : 'WorkBuddy AI',
        `/icons/tools/${edition}.png`,
        [edition],
        context.workBuddyAccountGroups[edition],
        (account) => ({
          identity: account.name,
          plan: account.plan,
          metrics: creditMetrics(account),
          ...(edition === 'workbuddy'
            ? {
                claim: (onError) =>
                  context.workBuddyAccountGroups.workbuddy.claimDaily(account, onError),
                dailyClaimedAt: account.dailyClaimedAt,
              }
            : {}),
        })
      )
    ),
    provider(
      'kiro',
      'Kiro',
      '/icons/tools/kirocrew.svg',
      ['kirocrew', 'kirocli'],
      context.kiroAccounts,
      (account) => ({
        identity: account.name,
        plan: account.plan,
        metrics: account.quota.length
          ? account.quota.map((quota) =>
              credits(
                t(
                  quota.label === 'credits'
                    ? 'agent.baseCredits'
                    : quota.label === 'trial' || quota.label === 'bonus'
                      ? 'agent.rewardCredits'
                      : 'agent.credits'
                ),
                quota.remaining,
                quota.total,
                quota.resetAt
              )
            )
          : [credits(t('agent.credits'), undefined, undefined)],
      })
    ),
    ...(['trae', 'traecn'] as const).map((edition) =>
      provider(
        edition,
        edition === 'trae' ? 'TRAE' : 'TRAE CN',
        `/icons/tools/${edition}.svg`,
        [edition],
        context.traeAccountGroups[edition],
        (account) => {
          const metrics =
            edition === 'traecn'
              ? creditMetrics(account)
              : [
                  credits(
                    t('accountCenter.quota'),
                    account.remaining,
                    account.total,
                    account.expiresAt,
                    account.currency
                  ),
                ];
          if (edition === 'traecn') {
            for (const metric of metrics) {
              if (
                metric.label === t('agent.baseCredits') &&
                account.baseRemaining === 0 &&
                account.baseTotal === 0
              )
                metric.percent = 0;
              if (metric.label === t('agent.rewardCredits') && account.rewardNextExpiryAt != null) {
                metric.resetAt = account.rewardNextExpiryAt;
                metric.resetLabel = t('agent.rewardCreditsNextExpiry');
              }
            }
          }
          return {
            identity: account.name,
            plan: account.plan,
            subscriptionEndAt: account.subscriptionEndAt ?? undefined,
            metrics,
            ...(edition === 'traecn'
              ? {
                  claim: async (onError) => {
                    const group = context.traeAccountGroups.traecn;
                    if (traeClaimUnavailable(account, group.accounts)) return;
                    const updated = await group.claimDaily(account, onError);
                    return updated != null && traeClaimedToday(updated.deviceClaimedAt);
                  },
                  claimDisabled:
                    context.traeAccountGroups.traecn.refreshing.size > 0 ||
                    traeClaimUnavailable(account, context.traeAccountGroups.traecn.accounts),
                  dailyClaimedAt: account.dailyClaimedAt,
                  showUnclaimed: account.rewardEnabled === true,
                }
              : {}),
          };
        }
      )
    ),
    provider(
      'antigravity',
      'Antigravity',
      '/icons/tools/antigravitydesktop.png',
      ['antigravitydesktop', 'antigravity'],
      context.antigravityAccounts,
      (account) => ({
        identity: account.email,
        plan: account.plan,
        metrics: ['gemini', 'claude'].map((family) => {
          const q = minimum(account.quotas, family);
          return quota(q?.remainingPercent, q?.resetAt, family === 'gemini' ? 'Gemini' : 'Claude');
        }),
      })
    ),
    provider(
      'cursor',
      'Cursor',
      '/icons/tools/cursor.svg',
      ['cursor', 'cursorcli'],
      context.cursorAccounts,
      (account) => ({
        identity: account.email,
        plan: account.usage?.plan,
        metrics: [quota(account.usage?.remainingPercent, account.usage?.resetAt)],
      })
    ),
    provider(
      'minimax',
      'MiniMax',
      '/icons/tools/minimaxdesktop.svg',
      ['minimaxcode', 'minimaxdesktop'],
      context.minimaxAccounts,
      (account) => ({
        identity: account.email,
        detail: t(account.region === 'cn' ? 'agent.minimaxRegionCn' : 'agent.minimaxRegionGlobal'),
        plan: minimaxPlanLabel(account.plan),
        subscriptionEndAt: isFreePlan(minimaxPlanLabel(account.plan))
          ? undefined
          : account.plan
            ? (account.subscriptionEndAt ?? null)
            : undefined,
        claim: (onError) => context.minimaxAccounts.claimDaily(account, onError),
        dailyClaimed: minimaxRewardStatus(account) === 'claimed',
        claimDisabled: minimaxRewardStatus(account) === 'unavailable',
        claimStatusExpiresAt: account.dailyReward
          ? account.dailyReward.checkedAt + MINIMAX_REWARD_CACHE_SECONDS
          : undefined,
        metrics: [
          ...(account.quota?.windows ?? []).map((window) =>
            quota(
              Math.max(0, Math.min(100, 100 - window.usedPercent)),
              window.resetsAt,
              window.name
            )
          ),
          ...(minimaxCreditBuckets(account.quota).length
            ? minimaxCreditBuckets(account.quota).map((bucket) => ({
                ...credits(t(bucket.label), bucket.remaining, bucket.total, bucket.expiresAt),
                showProgress: true,
              }))
            : account.quota?.credits != null || !account.quota?.windows.length
              ? [credits(t('agent.credits'), account.quota?.credits)]
              : []),
        ],
      })
    ),
    ...(context.doubaoAccountGroups
      ? (['doubaowork', 'dola'] as const).map((edition) =>
          provider(
            edition,
            edition === 'doubaowork' ? '豆包工作' : 'Dola',
            `/icons/tools/${edition}.png`,
            [edition],
            context.doubaoAccountGroups[edition],
            (account) => ({
              identity: account.name || account.email || account.id,
              plan: account.plan,
              subscriptionEndAt: account.expiresAt,
              claim:
                account.dailyReward && ['available', 'claimed'].includes(account.dailyReward.status)
                  ? (onError) => context.doubaoAccountGroups[edition].claimDaily(account, onError)
                  : undefined,
              dailyClaimed: account.dailyReward?.status === 'claimed',
              metrics:
                account.quota?.windows.length || account.quota?.bonus
                  ? [
                      ...(account.quota.windows ?? []).map((window) => ({
                        ...credits(
                          doubaoWindowLabel(window, account.quota!.windows, t('agent.baseCredits')),
                          window.remaining,
                          window.total,
                          window.resetAt
                        ),
                        value: window.unlimited
                          ? '∞'
                          : window.remaining != null
                            ? number(window.remaining)
                            : doubaoRemainingPercent(window) == null
                              ? '—'
                              : `${Math.round(doubaoRemainingPercent(window)!)}%`,
                        percent: doubaoRemainingPercent(window),
                        showProgress: true,
                      })),
                      ...(account.quota.bonus
                        ? [
                            {
                              ...credits(
                                t('agent.rewardCredits'),
                                account.quota.bonus.remaining,
                                account.quota.bonus.total,
                                account.quota.bonus.expiresAt
                              ),
                              percent: doubaoRemainingPercent(account.quota.bonus),
                              resetLabel: t('agent.rewardCreditsNextExpiry'),
                              showProgress: true,
                            },
                          ]
                        : []),
                    ]
                  : [credits(t('agent.credits'), account.remaining, account.total)],
            })
          )
        )
      : []),
    ...(context.dumateAccounts
      ? [
          provider(
            'dumate',
            'DuMate',
            '/icons/tools/dumate.svg',
            ['dumate'],
            context.dumateAccounts,
            (account) => {
              const balances = dumateCreditBalances(account);
              return {
                identity: account.name || account.id,
                plan: dumatePlanLabel(account, t),
                subscriptionEndAt: account.expiresAt ?? undefined,
                claim: account.enterprise
                  ? undefined
                  : (onError) => context.dumateAccounts.claimDaily(account, onError),
                dailyClaimed: dumateRewardStatus(account) === 'claimed',
                claimDisabled: dumateRewardStatus(account) === 'unavailable',
                claimStatusExpiresAt: account.dailyReward?.nextClaimAt,
                metrics: [
                  [t('agent.baseCredits'), balances.package] as const,
                  [t('agent.rewardCredits'), balances.reward] as const,
                ].map(([label, balance]) => ({
                  ...credits(label, balance.remaining, balance.total, balance.expiresAt),
                  showProgress: true,
                  balanceState:
                    balance.remaining == null
                      ? undefined
                      : balance.remaining > 0
                        ? ('available' as const)
                        : ('empty' as const),
                })),
              };
            }
          ),
        ]
      : []),
    ...(['qwenworkcn', 'qwenwork'] as const).map((edition) =>
      provider(
        edition,
        edition === 'qwenworkcn' ? '千问办公' : 'QwenWork',
        `/icons/tools/${edition}.svg`,
        [edition],
        context.qwenWorkAccountGroups[edition],
        (account) => {
          const balances = qwenWorkCreditBalances(account);
          const metric = (
            label: string,
            remaining: number | null,
            total: number | null,
            resetAt?: number | null
          ): AccountMetric => ({
            ...credits(label, remaining, total, resetAt),
            showProgress: true,
            balanceState: remaining == null ? undefined : remaining > 0 ? 'available' : 'empty',
          });
          return {
            identity: qwenWorkIdentity(account),
            plan: qwenWorkPlanLabel(account, t),
            subscriptionEndAt: account.expiresAt ?? undefined,
            metrics: [
              metric(t('agent.baseCredits'), balances.package.remaining, balances.package.total),
              ...(balances.reward
                ? [
                    metric(
                      t('agent.rewardCredits'),
                      balances.reward.remaining,
                      balances.reward.total,
                      balances.reward.expiresAt
                    ),
                  ]
                : []),
            ],
          };
        }
      )
    ),
    ...(['qoder', 'qodercn'] as const).map((edition) =>
      provider(
        edition,
        edition === 'qoder' ? 'Qoder' : 'Qoder CN',
        '/icons/tools/qoder.svg',
        [edition],
        context.qoderAccountGroups[edition],
        (account) => ({
          identity: account.name || account.email || account.id,
          plan: qoderPlanLabel(account.plan, account.edition, t),
          subscriptionEndAt: qoderSubscriptionExpiry(account),
          claim: (onError) => context.qoderAccountGroups[edition].claimDaily(account, onError),
          dailyClaimed: qoderRewardStatus(account) === 'claimed',
          claimDisabled: qoderRewardStatus(account) === 'unavailable',
          claimStatusExpiresAt: account.dailyReward?.resetAt,
          metrics: account.quotaWindows.length
            ? account.quotaWindows.map((window) => ({
                ...credits(
                  qoderQuotaLabel(window.name, t),
                  window.remaining,
                  window.total,
                  qoderExpiry(window.expiresAt)
                ),
                showProgress: true,
              }))
            : [
                {
                  ...credits(t('accountCenter.quota'), account.remaining, account.total),
                  showProgress: true,
                },
              ],
        })
      )
    ),
    {
      ...provider(
        'zcode',
        'ZCode',
        '/icons/tools/zcode.png',
        ['zcode'],
        context.zcodeAccounts,
        (account) => ({
          identity: account.email,
          detail: account.provider === 'bigmodel' ? 'BigModel' : 'Z.ai',
          plan:
            account.plan === 'Trial'
              ? t('agent.zcodeTrial')
              : account.plan === 'No Plan'
                ? t('agent.zcodeNoPlan')
                : account.plan?.replace(/^ZCode\s+/i, ''),
          subscriptionEndAt:
            ['trial', 'free', 'no plan'].includes(account.plan?.toLowerCase() ?? '') ||
            (!account.plan && account.subscriptionEndAt == null)
              ? undefined
              : (account.subscriptionEndAt ?? null),
          metrics: account.quotaWindows?.length
            ? account.quotaWindows.map((w, i) =>
                quota(w.remainingPercent, w.resetAt, windowLabel(i))
              )
            : [quota(account.remainingPercent, account.resetAt)],
        })
      ),
      website:
        context.zcodeAccounts.provider === 'bigmodel'
          ? 'https://bigmodel.cn/glm-coding'
          : 'https://z.ai/subscribe',
    },
    {
      ...provider(
        'kimi',
        'Kimi',
        '/icons/tools/kimidesktop.png',
        ['kimicode', 'kimidesktop'],
        context.kimiAccounts,
        (account) => {
          const metrics = account.quotaWindows.map((window) =>
            quota(window.remainingPercent, window.resetAt, kimiQuotaLabel(window.name, t))
          );
          if (account.wallet)
            metrics.push({
              ...credits(t('model.balance'), account.wallet.remaining, account.wallet.total),
              value: account.wallet.currency
                ? new Intl.NumberFormat(locale, {
                    style: 'currency',
                    currency: account.wallet.currency,
                  }).format(account.wallet.remaining)
                : number(account.wallet.remaining),
            });
          return {
            identity: account.email,
            detail: account.region === 'cn' ? 'kimi.com' : 'kimi.ai',
            plan: account.plan,
            metrics: metrics.length ? metrics : [quota()],
          };
        }
      ),
      website:
        context.kimiAccounts.region === 'cn'
          ? 'https://www.kimi.com/code/'
          : 'https://www.kimi.ai/code/',
    },
    provider(
      'grok',
      'Grok Build',
      '/icons/tools/grok.svg',
      ['grok'],
      context.grokAccounts,
      (account) => ({ identity: account.email, plan: account.plan, metrics: [] })
    ),
    provider(
      'grokbot',
      'Grok Bot',
      '/icons/tools/grokbot.png',
      ['grokbot'],
      context.grokBotAccounts,
      (account) => ({
        identity: account.email,
        plan:
          account.usage?.plan?.trim().toLowerCase() === 'grok bot plan'
            ? null
            : account.usage?.plan,
        metrics: [quota(account.usage?.remainingPercent, account.usage?.resetAt)],
      })
    ),
    ...(context.pluginAccounts?.provider
      ? [
          provider(
            `plugin:${context.pluginAccounts.provider.application}`,
            context.pluginAccounts.provider.name,
            context.pluginAccounts.provider.icon,
            context.pluginAccounts.provider.applications,
            context.pluginAccounts,
            (account) => ({
              identity: account.email,
              plan: account.plan,
              metrics: [
                credits(
                  t('model.balance'),
                  account.remaining ?? undefined,
                  account.total ?? undefined
                ),
              ],
            })
          ),
        ]
      : []),
    ...(['manus', 'cue'] as const).map((tool) =>
      provider(
        tool,
        tool === 'cue' ? 'Cue' : 'Manus',
        `/icons/tools/${tool}.png`,
        [tool],
        tool === 'cue' ? context.cueAccounts : context.manusAccounts,
        (account) => ({
          identity: account.email,
          plan: account.plan,
          subscriptionEndAt:
            !isFreePlan(account.plan) && 'subscriptionEndAt' in account
              ? (account.subscriptionEndAt ?? undefined)
              : undefined,
          metrics:
            tool === 'cue' && 'weekly' in account && account.weekly
              ? [quota(account.weekly.remainingPercent, account.weekly.resetAt, '7d')]
              : [
                  {
                    label: t('agent.credits'),
                    value: number('credits' in account ? account.credits?.total : null),
                  },
                ],
        })
      )
    ),
    provider(
      'kilo',
      'Kilo',
      '/icons/tools/kilo.png',
      ['kilo', 'kilodesktop'],
      context.kiloAccounts,
      (account) => ({
        identity: account.email,
        plan: account.plan,
        metrics: [
          {
            label: t('model.balance'),
            value:
              account.balance == null
                ? '—'
                : new Intl.NumberFormat(locale, {
                    style: 'currency',
                    currency: 'USD',
                    maximumFractionDigits: 2,
                  }).format(account.balance),
          },
        ],
      })
    ),
    {
      ...provider(
        'cline',
        'Cline',
        '/icons/tools/clinedesktop.png',
        ['cline', 'clinedesktop'],
        context.clineAccounts,
        (account) => ({
          identity: account.email || account.name || account.id,
          plan: account.plan,
          subscriptionEndAt: account.subscriptionEndAt ?? undefined,
          metrics: [
            { label: t('model.balance'), value: clineBalanceLabel(account.balanceMicros, locale) },
          ],
        })
      ),
      waitingLabel:
        context.clineAccounts.busy && context.clineAccounts.login?.userCode
          ? `${context.clineAccounts.login.userCode} · ${t('agent.waitingForBrowser')}`
          : undefined,
    },
    provider(
      'copilot',
      'GitHub Copilot',
      '/icons/tools/copilot.png',
      ['copilot', 'copilotdesktop'],
      context.copilotAccounts,
      (account) => ({
        identity: account.login,
        detail: account.host !== 'github.com' ? account.host : undefined,
        plan: account.plan,
        metrics: [
          {
            label: t('accountCenter.quota'),
            value: number(account.remaining),
            resetAt: account.resetAt,
            percent:
              account.remaining != null && account.total != null && account.total > 0
                ? (account.remaining / account.total) * 100
                : undefined,
            showProgress: true,
          },
        ],
      })
    ),
    provider(
      'opendesign',
      'Open Design',
      '/icons/tools/opendesign.png',
      ['opendesign'],
      context.openDesignAccounts,
      (account) => ({
        identity: account.email,
        plan: account.plan,
        subscriptionEndAt: account.subscriptionEndAt,
        metrics: account.quotaWindows.length
          ? account.quotaWindows.map((window) =>
              quota(window.remainingPercent, window.resetAt, window.label)
            )
          : [credits(t('model.balance'), account.balanceUsd, undefined, undefined, 'USD')],
      })
    ),
    ...(context.devinAccounts
      ? [
          provider(
            'devin',
            'Devin',
            '/icons/tools/devin.png',
            ['devin', 'devincli'],
            context.devinAccounts,
            (account) => ({
              identity: account.email,
              plan: account.plan,
              subscriptionEndAt: account.subscriptionEndAt,
              metrics: account.quotaWindows.length
                ? account.quotaWindows.map((window) =>
                    quota(window.remainingPercent, window.resetAt, devinQuotaLabel(window.label))
                  )
                : [credits(t('model.balance'), account.balanceUsd, undefined, undefined, 'USD')],
            })
          ),
        ]
      : []),
    ...(context.codeArtsAccounts
      ? [
          provider(
            'codearts',
            'CodeArts',
            '/icons/tools/codearts.png',
            ['codearts'],
            context.codeArtsAccounts,
            (account) => {
              const percent = codeArtsRemainingPercent(account);
              return {
                identity: account.email,
                plan: account.plan,
                subscriptionEndAt: !isFreePlan(account.plan)
                  ? (account.subscriptionEndAt ?? undefined)
                  : undefined,
                dailyClaimedAt: account.dailyClaimedAt,
                metrics: [
                  {
                    label: t('agent.availableCredits'),
                    value: number(account.balance),
                    percent,
                    showProgress: true,
                  },
                ],
                claim: (onError) => context.codeArtsAccounts.claimDaily(account, onError),
              };
            }
          ),
        ]
      : []),
  ];
}
