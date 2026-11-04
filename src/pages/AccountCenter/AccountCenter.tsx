import { useEffect, useRef, useState, type ReactNode } from 'react';
import {
  DndContext,
  PointerSensor,
  KeyboardSensor,
  closestCenter,
  DragOverlay,
  useSensor,
  useSensors,
  type DragEndEvent,
  type DragStartEvent,
} from '@dnd-kit/core';
import {
  SortableContext,
  rectSortingStrategy,
  sortableKeyboardCoordinates,
  arrayMove,
} from '@dnd-kit/sortable';
import { Gift, GripVertical, LoaderCircle, RefreshCw, Users } from 'lucide-react';
import { useAppManager } from '../AppManager/context';
import {
  AccountSectionActions,
  AccountSectionRewardButton,
  AccountSectionWarmUpButton,
} from '../AppManager/AccountSectionPrimitives';
import {
  useWorkBuddyClaimedToday,
  workBuddyClaimedToday,
} from '../AppManager/workBuddyDailyCredits';
import { useNavigationStore } from '../../stores/navigationStore';
import { useToolsStore } from '../../stores/toolsStore';
import { useRewardClaimStore } from '../../stores/rewardClaimStore';
import { useAccountCardCache } from './useAccountCardCache';
import { ZCodeAccountProviderChoice } from '../AppManager/ZCodeAccountSection';
import { KimiAccountRegionChoice } from '../AppManager/KimiAccountSection';
import { MinimaxAccountRegionChoice } from '../AppManager/MinimaxAccountSection';
import { DirectoryRow } from '../../components/DirectoryRow';
import { SortableCard } from '../../components/SortableCard';
import { AccountPrivacyButton } from '../../components/AccountPrivacyButton';
import { PageActionButton } from '../../components/PageActionButton';
import { ACCOUNT_NAME_MASK, useAccountPrivacyStore } from '../../stores/accountPrivacyStore';
import { QuotaCountdown } from '../AppManager/QuotaCountdown';
import { accountPlanLabel } from '../AppManager/accountPlanLabel';
import { useI18n } from '../../hooks/useI18n';
import { useUsageClock } from '../../hooks/useUsageClock';
import * as api from '../../api/tauri';
import { accountError, isAccountAuthorizationError } from '../../utils/accountError';
import { quotaColorClasses, quotaTone } from '../../utils/quotaColors';
import {
  accountCenterProviders,
  type AccountCardData,
  type AccountProvider,
} from './accountCenterData';
import '../ModelNexus/ModelNexus.css';

export function AccountCard({
  provider,
  account,
  dailyClaimed = workBuddyClaimedToday(account.dailyClaimedAt),
  dragHandle,
}: {
  provider: AccountProvider;
  account: AccountCardData;
  dailyClaimed?: boolean;
  dragHandle?: ReactNode;
}) {
  const { t } = useI18n();
  const hidden = useAccountPrivacyStore((state) => state.hidden);
  const identity = hidden ? ACCOUNT_NAME_MASK : account.identity;
  const now = useUsageClock(
    account.subscriptionEndAt != null ||
      account.claimStatusExpiresAt != null ||
      account.metrics.some((metric) => Boolean(metric.resetLabel))
  );
  const hasSubscriptionExpiry =
    account.subscriptionEndAt != null && account.subscriptionEndAt * 1000 > now;
  const currentClaimStatus =
    account.claimStatusExpiresAt == null || account.claimStatusExpiresAt * 1000 > now;
  const claimed = currentClaimStatus && (account.dailyClaimed ?? dailyClaimed);
  const name = [provider.name, account.detail].filter(Boolean).join(' · ');
  const missingUsage = account.metrics.every(
    (metric) => metric.value === '—' && metric.percent == null && metric.resetAt == null
  );
  const singleMetric = account.metrics.length === 1;
  const balanceOnly =
    singleMetric &&
    account.metrics.every(
      (metric) => metric.percent == null && metric.resetAt == null && metric.balanceState == null
    );
  const evenlySpaced = !account.authorizationFailed && account.metrics.length === 2 && !balanceOnly;
  const compactMetrics =
    ['minimax', 'doubaowork', 'dola'].includes(provider.id) && account.metrics.length > 3;
  return (
    <article
      aria-label={`${name} ${identity}`}
      className="flex h-44 flex-col gap-2 rounded-card border border-transparent bg-cyber-surface/60 p-3 transition-colors hover:bg-cyber-elevated/70"
    >
      <div className={`flex items-center ${dragHandle ? 'gap-1.5' : 'gap-2.5'}`}>
        <span className="flex flex-shrink-0 items-center gap-1.5">
          {dragHandle && <span className="-ml-1 flex h-6 items-center">{dragHandle}</span>}
          <img src={provider.icon} alt="" className="h-6 w-6 flex-shrink-0 object-contain" />
        </span>
        <span className="min-w-0 flex-1 truncate text-sm font-bold">{name}</span>
        <span className="flex flex-shrink-0 items-center gap-1.5 text-cyber-text-muted">
          {account.claim && (
            <AccountSectionRewardButton
              email={identity}
              claimed={claimed}
              disabled={
                account.pending ||
                provider.busy ||
                account.refreshing ||
                (currentClaimStatus && account.claimDisabled)
              }
              onClaim={() => void account.claim!()}
            />
          )}
          {account.warmUp && (
            <AccountSectionWarmUpButton
              email={identity}
              disabled={account.pending || account.refreshing}
              onWarmUp={() => void account.warmUp!()}
            />
          )}
          <AccountSectionActions
            email={identity}
            refreshing={account.refreshing}
            disabled={account.pending}
            deleteDisabled={account.pending && !account.cachedOnly}
            onRefresh={account.refresh}
            onDelete={account.remove}
          />
        </span>
      </div>
      <div
        className={
          evenlySpaced ? 'grid min-h-0 flex-1 grid-rows-[30%_35%_35%] items-center' : 'contents'
        }
      >
        <div className="flex min-w-0 items-center gap-3">
          <h3 className="min-w-0 flex-1 truncate text-base font-bold">{identity}</h3>
          {(account.plan || hasSubscriptionExpiry) && (
            <span className="flex max-w-[132px] flex-shrink-0 items-center gap-2 text-xs text-cyber-text-secondary">
              {hasSubscriptionExpiry && (
                <span className="flex flex-shrink-0 items-center text-[10px] leading-3 [&>span]:w-auto [&>span]:leading-3">
                  <QuotaCountdown
                    resetAt={account.subscriptionEndAt}
                    small
                    label={t('accountCenter.subscription')}
                  />
                </span>
              )}
              {account.plan && (
                <span className="min-w-0 truncate leading-normal">
                  {provider.id === 'minimax' ? account.plan : accountPlanLabel(account.plan)}
                </span>
              )}
            </span>
          )}
        </div>
        <div
          className={
            evenlySpaced
              ? 'contents'
              : `flex min-h-0 flex-1 flex-col justify-center ${compactMetrics ? 'gap-0' : account.metrics.length === 2 ? 'gap-4' : 'gap-2'} ${balanceOnly || account.authorizationFailed ? 'items-center text-center' : ''}`
          }
        >
          {account.authorizationFailed ? (
            <div className="flex min-w-0 flex-col items-center gap-2 text-center">
              <span role="status" className="text-sm font-semibold text-cyber-error">
                {t('accountCenter.authFailed')}
              </span>
              <button
                type="button"
                aria-label={`${t('accountCenter.reauthorize')} ${provider.name}`}
                disabled={provider.busy || account.pending}
                onClick={provider.add}
                className="inline-flex h-7 max-w-full items-center gap-1.5 rounded-button border border-cyber-border px-3 text-xs font-normal text-cyber-text transition-colors enabled:hover:bg-cyber-text/10 disabled:text-cyber-text-muted disabled:opacity-50"
              >
                <RefreshCw
                  size={13}
                  className={provider.busy ? 'animate-spin' : ''}
                  aria-hidden="true"
                />
                <span className="truncate">
                  {provider.busy
                    ? (provider.waitingLabel ?? t('agent.waitingForBrowser')).replace(
                        '{seconds}',
                        String(provider.remainingSeconds)
                      )
                    : t('accountCenter.reauthorize')}
                </span>
              </button>
            </div>
          ) : missingUsage ? (
            <span
              role="status"
              className={`${evenlySpaced ? 'row-span-2 flex h-full items-center justify-center ' : ''}text-center text-sm text-cyber-text-secondary`}
            >
              {t('model.noUsageData')}
            </span>
          ) : (
            account.metrics.map((metric, index) => {
              const colors =
                quotaColorClasses[
                  metric.percent == null && metric.balanceState
                    ? metric.balanceState === 'available'
                      ? 'healthy'
                      : 'critical'
                    : quotaTone(metric.percent)
                ];
              const loneBalance =
                balanceOnly && singleMetric && metric.label === t('model.balance');
              const hideLabel =
                loneBalance ||
                provider.id === 'dsh' ||
                (singleMetric &&
                  [t('accountCenter.quota'), t('agent.credits')].includes(metric.label));
              return (
                <div
                  key={index}
                  className={`flex w-full flex-shrink-0 flex-col ${compactMetrics ? 'gap-0' : 'gap-1'}`}
                >
                  <div
                    className={
                      balanceOnly
                        ? 'space-y-1'
                        : 'account-card-metric-row flex h-4 items-center gap-2'
                    }
                  >
                    <span
                      className={
                        balanceOnly ? 'contents' : 'flex min-w-0 flex-1 items-center gap-2'
                      }
                    >
                      {!hideLabel && (
                        <span className="block min-w-0 truncate text-xs text-cyber-text-secondary">
                          {metric.label}
                        </span>
                      )}
                      {(provider.id === 'workbuddy' ||
                        (provider.id === 'traecn' && account.showUnclaimed)) &&
                        !claimed &&
                        metric.label === t('agent.rewardCredits') && (
                          <span className="flex-shrink-0 text-[10px] font-semibold leading-[16px] text-cyber-text">
                            {t('agent.dailyCreditsUnclaimed')}
                          </span>
                        )}
                      {metric.resetAt != null &&
                        (!metric.resetLabel || metric.resetAt > now / 1000) && (
                          <QuotaCountdown
                            resetAt={metric.resetAt}
                            compact
                            small
                            label={metric.resetLabel ?? t('accountCenter.reset')}
                          />
                        )}
                    </span>
                    <span
                      aria-label={hideLabel ? `${metric.label} ${metric.value}` : undefined}
                      className={`block ${metric.value === '—' ? 'min-w-0 truncate' : 'flex-shrink-0'} ${loneBalance ? 'font-bold' : 'font-semibold'} tabular-nums ${balanceOnly ? 'text-2xl' : 'text-xs'} ${metric.percent != null || metric.balanceState ? colors.text : ''}`}
                    >
                      {loneBalance && <span className="mr-2">{metric.label}</span>}
                      {metric.value === '—' ? t('model.noUsageData') : metric.value}
                    </span>
                  </div>
                  {metric.percent != null ? (
                    <div
                      role="progressbar"
                      aria-label={`${metric.label} ${identity}`}
                      aria-valuemin={0}
                      aria-valuemax={100}
                      aria-valuenow={Math.max(0, Math.min(100, metric.percent))}
                      className={`relative -my-px h-0 flex-shrink-0 rounded-full border-t-[6px] ${colors.track}`}
                    >
                      <div
                        className={`quota-progress-fill absolute bottom-0 left-0 h-0 rounded-full border-t-[6px] ${colors.fill}`}
                        style={{ width: `${Math.max(0, Math.min(100, metric.percent))}%` }}
                      />
                    </div>
                  ) : metric.showProgress && metric.balanceState && metric.value !== '—' ? (
                    <div
                      aria-hidden="true"
                      className={`-my-px h-0 flex-shrink-0 rounded-full border-t-[6px] ${metric.balanceState === 'available' ? colors.fill : colors.track}`}
                    />
                  ) : metric.showProgress && metric.value !== '—' && !balanceOnly ? (
                    <div
                      aria-hidden="true"
                      className="-my-px h-0 flex-shrink-0 rounded-full border-t-[6px] border-cyber-border"
                    />
                  ) : null}
                </div>
              );
            })
          )}
        </div>
      </div>
    </article>
  );
}

export function AccountCenterMain() {
  const context = useAppManager();
  const hidden = useAccountPrivacyStore((state) => state.hidden);
  const { t, locale } = useI18n();
  const activePage = useNavigationStore((state) => state.activePage);
  const toolsReady = useToolsStore((state) => state.hasScanned);
  const { providers, loading } = useAccountCardCache(accountCenterProviders(context, t, locale), {
    locale,
    active: activePage === 'accounts',
    toolsReady,
    scanning: context.isScanning,
  });
  const claimedToday = useWorkBuddyClaimedToday(
    activePage === 'accounts' &&
      providers.some((provider) => provider.accounts.some((account) => account.claim))
  );
  const { accountCardOrder: accountOrder, setAccountCardOrder: setAccountOrder } = context;
  const orderIndex = new Map(accountOrder.map((id, index) => [id, index]));
  const accounts = providers
    .flatMap((provider) =>
      provider.accounts.map((account) => ({
        id: `${provider.id}:${account.id}`,
        provider,
        account,
      }))
    )
    .sort((a, b) => (orderIndex.get(a.id) ?? Infinity) - (orderIndex.get(b.id) ?? Infinity));
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates })
  );
  const [activeDragId, setActiveDragId] = useState<string | null>(null);
  const [activeDragWidth, setActiveDragWidth] = useState(0);
  const gridRef = useRef<HTMLDivElement>(null);
  const handleDragStart = (event: DragStartEvent) => {
    const id = String(event.active.id);
    setActiveDragId(id);
    const el = Array.from(gridRef.current?.children ?? []).find(
      (node) => node.getAttribute('data-sortable-card') === id
    );
    setActiveDragWidth(el?.getBoundingClientRect().width ?? 0);
  };
  const handleDragEnd = (event: DragEndEvent) => {
    setActiveDragId(null);
    if (useNavigationStore.getState().activePage !== 'accounts' || loading) return;
    const oldIndex = accounts.findIndex((item) => item.id === event.active.id);
    const newIndex = accounts.findIndex((item) => item.id === event.over?.id);
    if (oldIndex < 0 || newIndex < 0 || oldIndex === newIndex) return;
    const next = arrayMove(accounts, oldIndex, newIndex).map((item) => item.id);
    setAccountOrder(next);
    try {
      localStorage.setItem('echobird_account_card_order', JSON.stringify(next));
    } catch {
      /* Keep this session's order if local storage is unavailable. */
    }
  };
  useEffect(() => {
    return useNavigationStore.subscribe((state, previous) => {
      if (state.activePage !== previous.activePage) setActiveDragId(null);
    });
  }, []);
  const activeDragAccount = accounts.find((item) => item.id === activeDragId);
  return accounts.length ? (
    <DndContext
      key={activePage}
      sensors={sensors}
      collisionDetection={closestCenter}
      onDragStart={handleDragStart}
      onDragEnd={handleDragEnd}
      onDragCancel={() => setActiveDragId(null)}
      autoScroll={false}
    >
      <SortableContext items={accounts.map((item) => item.id)} strategy={rectSortingStrategy}>
        <div ref={gridRef} className="model-nexus-grid grid gap-4 pb-2">
          {accounts.map(({ id, provider, account }) => (
            <SortableCard
              key={id}
              id={id}
              dragLabel={`${t('model.dragSort')} ${hidden ? ACCOUNT_NAME_MASK : account.identity}`}
              disabled={loading || activePage !== 'accounts'}
            >
              {(handle) => (
                <AccountCard
                  provider={provider}
                  account={account}
                  dailyClaimed={claimedToday(account.dailyClaimedAt)}
                  dragHandle={handle}
                />
              )}
            </SortableCard>
          ))}
        </div>
      </SortableContext>
      <DragOverlay>
        {activeDragAccount && (
          <div style={{ width: activeDragWidth }} className="pointer-events-none shadow-2xl">
            <AccountCard
              provider={activeDragAccount.provider}
              account={activeDragAccount.account}
              dailyClaimed={claimedToday(activeDragAccount.account.dailyClaimedAt)}
              dragHandle={
                <span aria-hidden="true" className="p-0.5 text-cyber-text-muted/60">
                  <GripVertical size={14} />
                </span>
              }
            />
          </div>
        )}
      </DragOverlay>
    </DndContext>
  ) : loading ? (
    <div aria-label={t('accountCenter.loading')} className="model-nexus-grid grid gap-4">
      {[0, 1, 2].map((index) => (
        <div key={index} className="h-44 animate-pulse rounded-card bg-cyber-surface/60" />
      ))}
    </div>
  ) : (
    <div className="flex h-full min-h-52 flex-col items-center justify-center gap-3 text-cyber-text-secondary">
      <Users size={28} aria-hidden="true" />
      <span className="text-sm">{t('accountCenter.empty')}</span>
    </div>
  );
}

export function AccountCenterTitleActions() {
  const context = useAppManager();
  const { t, locale } = useI18n();
  const activePage = useNavigationStore((state) => state.activePage);
  const rewardBatchRunning = useRewardClaimStore((state) => state.running);
  const toolsReady = useToolsStore((state) => state.hasScanned) || context.detectedTools.length > 0;
  const providers = accountCenterProviders(context, t, locale);
  const latest = useRef(providers);
  const batch = useRef<symbol | null>(null);
  const [running, setRunning] = useState<'refresh' | 'claim' | null>(null);
  useEffect(() => {
    latest.current = providers;
  }, [providers]);
  useEffect(() => {
    const unsubscribe = useNavigationStore.subscribe((state, previous) => {
      if (state.activePage !== previous.activePage) {
        batch.current = null;
        setRunning(null);
      }
    });
    return () => {
      unsubscribe();
      batch.current = null;
    };
  }, []);
  const accounts = providers.flatMap((provider) =>
    provider.accounts.map((account) => ({ providerId: provider.id, account }))
  );
  const blocked =
    rewardBatchRunning ||
    !toolsReady ||
    context.isScanning ||
    context.isLaunching ||
    providers.some(
      (provider) =>
        (provider.loading && !provider.loaded) ||
        provider.busy ||
        provider.accounts.some((account) => account.refreshing)
    );
  const run = async (action: 'refresh' | 'claim') => {
    if (batch.current || blocked || activePage !== 'accounts' || !accounts.length) return;
    if (!useRewardClaimStore.getState().tryStart()) return;
    const request = Symbol();
    batch.current = request;
    setRunning(action);
    const authFailures = new Map<string, { name: string; identity: string }>();
    const otherFailures = new Map<string, { name: string; identity: string }>();
    const claimedProviders = new Set<string>();
    context.setApplyError(null);
    try {
      for (const target of accounts) {
        if (batch.current !== request || useNavigationStore.getState().activePage !== 'accounts')
          break;
        const provider = latest.current.find((provider) => provider.id === target.providerId);
        const account = provider?.accounts.find((account) => account.id === target.account.id);
        if (
          !provider ||
          (provider.loading && !provider.loaded) ||
          provider.busy ||
          !account ||
          account.pending ||
          account.refreshing
        )
          continue;
        const collectError = (error: unknown) => {
          const failures = isAccountAuthorizationError(error) ? authFailures : otherFailures;
          failures.set(`${provider.id}:${account.id}`, {
            name: [provider.name, account.detail].filter(Boolean).join(' · '),
            identity: account.identity,
          });
        };
        try {
          if (action === 'claim') {
            if (account.claim && !claimedProviders.has(provider.id)) {
              const deviceClaimed = await account.claim(collectError);
              if (deviceClaimed === true && batch.current === request)
                claimedProviders.add(provider.id);
            }
          } else {
            await account.refresh(collectError);
          }
        } catch (error) {
          collectError(error);
        }
      }
      if (batch.current === request && useNavigationStore.getState().activePage === 'accounts') {
        const hidden = useAccountPrivacyStore.getState().hidden;
        const summary = (
          [
            [authFailures, 'accountCenter.batchAuthFailed'],
            [
              otherFailures,
              action === 'claim'
                ? 'accountCenter.batchClaimFailed'
                : 'accountCenter.batchRefreshFailed',
            ],
          ] as const
        )
          .filter(([failures]) => failures.size > 0)
          .map(
            ([failures, key]) =>
              `${t(key).replace('{n}', String(failures.size))} (${Array.from(failures.values(), ({ name, identity }) => `${name} · ${hidden ? ACCOUNT_NAME_MASK : identity}`).join(', ')})`
          )
          .join(' ');
        if (summary) context.setApplyError(summary);
      }
    } finally {
      useRewardClaimStore.getState().finish();
      if (batch.current === request) {
        batch.current = null;
        setRunning(null);
      }
    }
  };
  return (
    <div className="ml-auto flex-shrink-0 flex items-center gap-3">
      <AccountPrivacyButton />
      <PageActionButton
        icon={running === 'claim' ? LoaderCircle : Gift}
        spinning={running === 'claim'}
        aria-label={t('accountCenter.claimAll')}
        disabled={blocked || running !== null}
        onClick={() => run('claim')}
      />
      <PageActionButton
        icon={RefreshCw}
        spinning={running === 'refresh'}
        aria-label={t('accountCenter.refreshAll')}
        disabled={blocked || running !== null}
        onClick={() => run('refresh')}
        className="font-mono"
      >
        {t('accountCenter.refreshAll')}
      </PageActionButton>
    </div>
  );
}

export function AccountCenterPanel() {
  const context = useAppManager();
  const { t, locale } = useI18n();
  const providers = accountCenterProviders(context, t, locale).sort(
    (a, b) => Number(b.installed) - Number(a.installed)
  );
  const pending = providers.some((provider) => provider.busy);
  const openWebsite = async (website: string) => {
    try {
      await api.openExternal(website);
    } catch (error) {
      context.setApplyError(accountError(error, t));
    }
  };
  return (
    <div className="flex-1 px-2 pb-2 overflow-y-auto overflow-x-hidden">
      <section>
        <h3 className="px-1 py-2 flex items-center gap-2">
          <span className="flex-1 h-px bg-cyber-border/60" />
          <span className="text-[11px] font-semibold uppercase tracking-wider text-cyber-text-secondary whitespace-nowrap">
            {t('accountCenter.subscriptionProviders')}
          </span>
          <span className="flex-1 h-px bg-cyber-border/60" />
        </h3>
        <div className="flex h-12 items-center justify-center rounded-card bg-cyber-surface/60 text-xs text-cyber-text-muted">
          {t('accountCenter.noSubscriptionProviders')}
        </div>
      </section>
      <section>
        <h3 className="px-1 py-2 flex items-center gap-2">
          <span className="flex-1 h-px bg-cyber-border/60" />
          <span className="text-[11px] font-semibold uppercase tracking-wider text-cyber-text-secondary whitespace-nowrap">
            {t('agent.addCurrentAccount')}
          </span>
          <span className="flex-1 h-px bg-cyber-border/60" />
        </h3>
        <div className="space-y-2">
          {providers.map((provider) => {
            const disabled = context.isLaunching || (pending && !provider.busy);
            const hostname = provider.website ? new URL(provider.website).hostname : '—';
            return (
              <section key={provider.id} aria-label={provider.name}>
                <DirectoryRow
                  name={provider.name}
                  url={provider.website}
                  iconSrc={provider.icon}
                  onOpen={() => void openWebsite(provider.website!)}
                  openLabel={t('accountCenter.website').replace('{name}', provider.name)}
                  add={{
                    onClick: provider.add,
                    onCancel: provider.cancelLogin,
                    cancelLabel: t('btn.cancel'),
                    label: provider.busy
                      ? (provider.waitingLabel ?? t('agent.waitingForBrowser')).replace(
                          '{seconds}',
                          String(provider.remainingSeconds)
                        )
                      : `${provider.addLabel ?? t('agent.addCurrentAccount')} ${provider.name}`,
                    disabled,
                    busy: provider.busy,
                  }}
                  secondary={
                    provider.busy &&
                    provider.id === 'minimax' &&
                    context.minimaxAccounts.login?.userCode ? (
                      <span className="flex items-center gap-2">
                        <span>
                          {(provider.waitingLabel ?? t('agent.waitingForBrowser')).replace(
                            '{seconds}',
                            String(provider.remainingSeconds)
                          )}
                        </span>
                        <span className="font-mono">{context.minimaxAccounts.login.userCode}</span>
                      </span>
                    ) : provider.busy ? (
                      (provider.waitingLabel ?? t('agent.waitingForBrowser')).replace(
                        '{seconds}',
                        String(provider.remainingSeconds)
                      )
                    ) : !provider.installed ? (
                      `${hostname} · ${t('status.notInstalled')}`
                    ) : ['zcode', 'minimax', 'kimi'].includes(provider.id) ? (
                      <span className="flex items-center gap-2">
                        <span className="min-w-0 truncate">{hostname}</span>
                        <span className="pointer-events-auto relative flex-shrink-0">
                          {provider.id === 'minimax' ? (
                            <MinimaxAccountRegionChoice disabled={disabled} />
                          ) : provider.id === 'kimi' ? (
                            <KimiAccountRegionChoice disabled={disabled} />
                          ) : (
                            <ZCodeAccountProviderChoice disabled={disabled} />
                          )}
                        </span>
                      </span>
                    ) : undefined
                  }
                />
              </section>
            );
          })}
        </div>
      </section>
    </div>
  );
}
