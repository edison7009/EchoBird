import { useEffect, useMemo, useRef, useState } from 'react';
import { useToast } from '../../components/Toast';
import { useI18n } from '../../hooks/useI18n';
import { useNavigationStore } from '../../stores/navigationStore';
import { useRewardClaimStore } from '../../stores/rewardClaimStore';
import { usageIslandVisible, useUsageIslandStore } from '../../stores/usageIslandStore';
import { accountCenterProviders } from '../AccountCenter/accountCenterData';
import { readCachedAccounts } from '../AccountCenter/useAccountCardCache';
import { useAppManager } from '../AppManager/context';
import { useModelNexus } from '../ModelNexus/context';
import { accountSources, modelSources, type UsageSource } from './usageIslandData';

// Leaves time for saved accounts and models to load.
const FIRST_REFRESH_DELAY = 5000;

type Navigation = ReturnType<typeof useNavigationStore.getState>;

const onUsageIslandPage = (navigation: Navigation) =>
  navigation.developerMode &&
  navigation.activePage === 'developer' &&
  navigation.developerTab === 'usageIsland';

export function useUsageIslandActive() {
  return useNavigationStore(onUsageIslandPage);
}

export function useUsageSources(active: boolean): UsageSource[] {
  const context = useAppManager();
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

/** Only one caller may pass `timed`: the page while closed, the bridge while open. */
export function useUsageIslandRefresh(items: UsageSource[], timed: boolean, startSoon = false) {
  const { t } = useI18n();
  const { showToast } = useToast();
  const refreshSeconds = useUsageIslandStore((state) => state.refreshSeconds);
  const [refreshing, setRefreshing] = useState(false);
  const refreshPending = useRef(false);
  const latestItems = useRef(items);
  useEffect(() => {
    latestItems.current = items;
  });
  const live = () => onUsageIslandPage(useNavigationStore.getState()) || usageIslandVisible();
  const refresh = async (manual: boolean) => {
    if (refreshPending.current || !latestItems.current.length) return;
    if (!useRewardClaimStore.getState().tryStart()) return;
    refreshPending.current = true;
    setRefreshing(true);
    const failures = new Set<string>();
    try {
      for (const { key } of latestItems.current) {
        if (!live()) break;
        const source = latestItems.current.find((item) => item.key === key);
        try {
          await source?.refresh?.(manual, () => failures.add(key));
        } catch {
          failures.add(key);
        }
      }
      if (manual && failures.size && live())
        showToast(
          'error',
          t('accountCenter.batchRefreshFailed').replace('{n}', String(failures.size))
        );
    } finally {
      useRewardClaimStore.getState().finish();
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
    if (!timed || !refreshSeconds) return;
    const run = () => void latestRefresh.current(false);
    const first = startSoon ? setTimeout(run, FIRST_REFRESH_DELAY) : undefined;
    const timer = setInterval(run, refreshSeconds * 1000);
    return () => {
      clearTimeout(first);
      clearInterval(timer);
    };
  }, [timed, startSoon, refreshSeconds]);
  return { refreshing, refresh };
}
