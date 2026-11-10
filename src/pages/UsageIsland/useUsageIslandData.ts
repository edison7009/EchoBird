import { useEffect, useMemo, useRef, useState } from 'react';
import { create } from 'zustand';
import { getSmartRouterActivity } from '../../api/smartRouter';
import { useToast } from '../../components/Toast';
import { useI18n } from '../../hooks/useI18n';
import { useNavigationStore } from '../../stores/navigationStore';
import { useRewardClaimStore } from '../../stores/rewardClaimStore';
import { usageIslandVisible, useUsageIslandStore } from '../../stores/usageIslandStore';
import { accountCenterProviders } from '../AccountCenter/accountCenterData';
import { readCachedAccounts } from '../AccountCenter/useAccountCardCache';
import { useAccounts } from '../../features/accounts/context';
import { useModelNexus } from '../ModelNexus/context';
import {
  accountSourceKey,
  accountSources,
  modelSources,
  type UsageSource,
  type UsageIslandAction,
} from './usageIslandData';

// Leaves time for saved accounts and models to load.
const FIRST_REFRESH_DELAY = 5000;
// Sites limit these queries, so a timed refresh skips a failing source's next turn.
const FAILURE_COOLDOWN = 10 * 60_000;

type Navigation = ReturnType<typeof useNavigationStore.getState>;

const onUsageIslandPage = (navigation: Navigation) =>
  navigation.developerMode &&
  navigation.activePage === 'apps' &&
  navigation.appsTab === 'usageIsland';

export function useUsageIslandActive() {
  return useNavigationStore(onUsageIslandPage);
}

export function useUsageSources(active: boolean): UsageSource[] {
  const context = useAccounts();
  const models = useModelNexus();
  const { t, locale } = useI18n();
  // Read on entry: the account page owns this cache and keeps writing it.
  const cached = useMemo(() => (active ? readCachedAccounts(locale) : {}), [active, locale]);
  return [
    ...accountSources(accountCenterProviders(context, t, locale), cached),
    ...modelSources(
      models.userModels,
      models.modelUsageData,
      models.refreshingUsageIds,
      models.refreshSingleUsage,
      t
    ),
  ];
}

export function useUsageIslandItems(sources: UsageSource[]) {
  const selected = useUsageIslandStore((state) => state.sources);
  return selected.flatMap((key) => sources.filter((source) => source.key === key).slice(0, 1));
}

/**
 * Only one caller may pass `timed`: the page while closed, the bridge while open. With
 * `ticked` the caller drives the interval itself, as `refresh(false)`.
 */
function normalizedAccountProvider(provider: string) {
  return (
    (
      {
        claudeCode: 'claudecode',
        workBuddy: 'workbuddy',
        workBuddyAi: 'workbuddyai',
        qoderCn: 'qodercn',
        codeArts: 'codearts',
      } as Record<string, string>
    )[provider] ?? provider
  );
}

function sameAccountSource(a: UsageSource, b: UsageSource) {
  return (
    a.kind === 'account' &&
    b.kind === 'model' &&
    a.providerId != null &&
    b.accountSource != null &&
    normalizedAccountProvider(a.providerId) ===
      normalizedAccountProvider(b.accountSource.provider) &&
    a.key ===
      accountSourceKey(
        normalizedAccountProvider(b.accountSource.provider),
        b.accountSource.accountId
      )
  );
}

export function useUsageIslandSmartActivity(enabled: boolean, items: UsageSource[]) {
  const [activeKeys, setActiveKeys] = useState<Set<string>>(() => new Set());
  const latestItems = useRef(items);
  const signature = items.map((item) => `${item.key}:${item.candidateId ?? ''}`).join('|');
  useEffect(() => {
    latestItems.current = items;
  });
  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    let pending = false;
    const poll = async () => {
      if (pending) return;
      pending = true;
      try {
        const activity = await getSmartRouterActivity();
        if (cancelled) return;
        const candidateIds = new Set(
          activity.activeCandidateIds ??
            (activity.active && activity.candidateId ? [activity.candidateId] : [])
        );
        const activeModels = latestItems.current.filter(
          (item) => item.kind === 'model' && item.candidateId && candidateIds.has(item.candidateId)
        );
        const active = new Set(activeModels.map((item) => item.key));
        for (const item of latestItems.current) {
          if (activeModels.some((model) => sameAccountSource(item, model))) active.add(item.key);
        }
        setActiveKeys(active);
      } catch {
        if (!cancelled) setActiveKeys(new Set());
      } finally {
        pending = false;
      }
    };
    void poll();
    const timer = window.setInterval(() => void poll(), 750);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [enabled, signature]);
  return enabled ? activeKeys : new Set<string>();
}

/** One usage batch at a time across the settings page and the floating-island
 *  bridge: both run in the main window, and a page refresh must not overlap the
 *  island's timed refresh (or the reverse). Timed runs skip while it is held. */
export const useUsageIslandBatchStore = create<{ running: boolean }>(() => ({ running: false }));

export function useUsageIslandRefresh(
  items: UsageSource[],
  timed: boolean,
  startSoon = false,
  ticked = false
) {
  const { t } = useI18n();
  const { showToast } = useToast();
  const refreshSeconds = useUsageIslandStore((state) => state.refreshSeconds);
  const [running, setRunning] = useState<UsageIslandAction | null>(null);
  const batchRunning = useRewardClaimStore((state) => state.running);
  const sharedRunning = useUsageIslandBatchStore((state) => state.running);
  const mounted = useRef(true);
  const generation = useRef(0);
  useEffect(() => {
    mounted.current = true;
    const stop = useNavigationStore.subscribe((state, previous) => {
      if (onUsageIslandPage(previous) && !onUsageIslandPage(state) && !usageIslandVisible())
        generation.current += 1;
    });
    return () => {
      mounted.current = false;
      generation.current += 1;
      stop();
    };
  }, []);
  const hasItems = items.length > 0;
  const batchPending = useRef(false);
  const cooldowns = useRef(new Map<string, number>());
  const latestItems = useRef(items);
  useEffect(() => {
    latestItems.current = items;
  });
  const live = () => onUsageIslandPage(useNavigationStore.getState()) || usageIslandVisible();
  const run = async (action: UsageIslandAction, manual = true) => {
    if (!mounted.current || !live()) return;
    if (batchPending.current || !latestItems.current.length) return;
    if (useUsageIslandBatchStore.getState().running) return;
    if (manual && latestItems.current.some((item) => item.busy)) return;
    // Timed refreshes only give way: holding this lock would starve claims and account batches.
    const rewards = useRewardClaimStore.getState();
    if (manual ? !rewards.tryStart() : rewards.running) return;
    const scope = generation.current;
    const current = () => mounted.current && generation.current === scope && live();
    batchPending.current = true;
    useUsageIslandBatchStore.setState({ running: true });
    setRunning(action);
    const failures = new Set<string>();
    const claimedProviders = new Set<string>();
    let completed = 0;
    const targets = latestItems.current.map((item) => item.key);
    try {
      for (const key of targets) {
        if (!current()) break;
        if (action === 'refresh' && !manual && (cooldowns.current.get(key) ?? 0) > Date.now())
          continue;
        const source = latestItems.current.find((item) => item.key === key);
        if (!source?.refresh || !useUsageIslandStore.getState().sources.includes(key)) continue;
        if (
          action === 'claim' &&
          (!source.claim || !source.canClaim || claimedProviders.has(source.providerId ?? key))
        )
          continue;
        if (action === 'warmUp' && !source.warmUp) continue;
        completed += 1;
        const failed = () => {
          failures.add(key);
          if (action === 'refresh') cooldowns.current.set(key, Date.now() + FAILURE_COOLDOWN);
        };
        try {
          const result =
            action === 'refresh'
              ? await source.refresh(manual, failed)
              : await source[action]?.(failed);
          if (action === 'claim' && result === true) claimedProviders.add(source.providerId ?? key);
        } catch {
          failed();
        }
        if (action === 'refresh' && !failures.has(key)) cooldowns.current.delete(key);
      }
      if (manual && failures.size && current()) {
        if (action === 'refresh')
          showToast(
            'error',
            t('accountCenter.batchRefreshFailed').replace('{n}', String(failures.size))
          );
        else
          showToast(
            'warning',
            t(
              action === 'claim'
                ? 'accountCenter.batchClaimSummary'
                : 'accountCenter.batchWarmUpSummary'
            )
              .replace('{success}', String(Math.max(0, completed - failures.size)))
              .replace('{failed}', String(failures.size))
          );
      }
    } finally {
      if (manual) useRewardClaimStore.getState().finish();
      batchPending.current = false;
      useUsageIslandBatchStore.setState({ running: false });
      if (mounted.current) setRunning(null);
    }
  };
  const refresh = (manual: boolean) => run('refresh', manual);
  const latestRefresh = useRef(refresh);
  useEffect(() => {
    latestRefresh.current = refresh;
  });
  // Opening the page never queries quota, so its first tick waits a full interval. The
  // floating island has nothing to show until then, as model usage lives in memory.
  useEffect(() => {
    if (!timed || ticked || !refreshSeconds) return;
    const timer = setInterval(() => void latestRefresh.current(false), refreshSeconds * 1000);
    return () => clearInterval(timer);
  }, [timed, ticked, refreshSeconds]);
  useEffect(() => {
    if (!timed || !startSoon || !refreshSeconds || !hasItems) return;
    const first = setTimeout(() => void latestRefresh.current(false), FIRST_REFRESH_DELAY);
    return () => clearTimeout(first);
  }, [timed, startSoon, refreshSeconds, hasItems]);
  return {
    refreshing: running === 'refresh',
    refresh,
    run,
    actions: {
      warmUp: items.some((item) => item.canWarmUp),
      claim: items.some((item) => item.canClaim),
      refresh: items.some((item) => Boolean(item.refresh)),
      blocked: batchRunning || sharedRunning || running !== null || items.some((item) => item.busy),
      running,
    },
  };
}
