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

const isUsageIslandPage = () => {
  const navigation = useNavigationStore.getState();
  return (
    navigation.developerMode &&
    navigation.activePage === 'developer' &&
    navigation.developerTab === 'usageIsland'
  );
};

export function useUsageIslandActive() {
  return useNavigationStore(
    (state) =>
      state.developerMode &&
      state.activePage === 'developer' &&
      state.developerTab === 'usageIsland'
  );
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
export function useUsageIslandRefresh(items: UsageSource[], timed: boolean) {
  const { t } = useI18n();
  const { showToast } = useToast();
  const refreshMinutes = useUsageIslandStore((state) => state.refreshMinutes);
  const [refreshing, setRefreshing] = useState(false);
  const refreshPending = useRef(false);
  const latestItems = useRef(items);
  useEffect(() => {
    latestItems.current = items;
  });
  const live = () => isUsageIslandPage() || usageIslandVisible();
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
  // The first tick waits a full interval, so opening the page never queries quota.
  useEffect(() => {
    if (!timed || !refreshMinutes) return;
    const timer = setInterval(() => void latestRefresh.current(false), refreshMinutes * 60_000);
    return () => clearInterval(timer);
  }, [timed, refreshMinutes]);
  return { refreshing, refresh };
}
