import { useEffect, useMemo, useRef, useState } from 'react';
import { useToast } from '../../components/Toast';
import { useI18n } from '../../hooks/useI18n';
import { useNavigationStore } from '../../stores/navigationStore';
import { useRewardClaimStore } from '../../stores/rewardClaimStore';
import { usageIslandVisible, useUsageIslandStore } from '../../stores/usageIslandStore';
import { accountCenterProviders } from '../AccountCenter/accountCenterData';
import { readCachedAccounts } from '../AccountCenter/useAccountCardCache';
import { useAccounts } from '../../features/accounts/context';
import { useModelNexus } from '../ModelNexus/context';
import { accountSources, modelSources, type UsageSource } from './usageIslandData';

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
export function useUsageIslandRefresh(
  items: UsageSource[],
  timed: boolean,
  startSoon = false,
  ticked = false
) {
  const { t } = useI18n();
  const { showToast } = useToast();
  const refreshSeconds = useUsageIslandStore((state) => state.refreshSeconds);
  const [refreshing, setRefreshing] = useState(false);
  const hasItems = items.length > 0;
  const refreshPending = useRef(false);
  const cooldowns = useRef(new Map<string, number>());
  const latestItems = useRef(items);
  useEffect(() => {
    latestItems.current = items;
  });
  const live = () => onUsageIslandPage(useNavigationStore.getState()) || usageIslandVisible();
  const refresh = async (manual: boolean) => {
    if (refreshPending.current || !latestItems.current.length) return;
    // Timed refreshes only give way: holding this lock would starve claims and account batches.
    const rewards = useRewardClaimStore.getState();
    if (manual ? !rewards.tryStart() : rewards.running) return;
    refreshPending.current = true;
    setRefreshing(true);
    const failures = new Set<string>();
    try {
      for (const { key } of latestItems.current) {
        if (!live()) break;
        if (!manual && (cooldowns.current.get(key) ?? 0) > Date.now()) continue;
        const source = latestItems.current.find((item) => item.key === key);
        const failed = () => {
          failures.add(key);
          cooldowns.current.set(key, Date.now() + FAILURE_COOLDOWN);
        };
        try {
          await source?.refresh?.(manual, failed);
        } catch {
          failed();
        }
        if (!failures.has(key)) cooldowns.current.delete(key);
      }
      if (manual && failures.size && live())
        showToast(
          'error',
          t('accountCenter.batchRefreshFailed').replace('{n}', String(failures.size))
        );
    } finally {
      if (manual) useRewardClaimStore.getState().finish();
      refreshPending.current = false;
      setRefreshing(false);
    }
  };
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
  return { refreshing, refresh };
}
