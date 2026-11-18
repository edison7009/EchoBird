// AI-career overview and per-family dashboards.

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { RefreshCw } from 'lucide-react';
import { PageActionButton } from '../../components/PageActionButton';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { useI18n } from '../../hooks/useI18n';
import { useAiCareerStore } from '../../stores/aiCareerStore';
import { useNavigationStore } from '../../stores/navigationStore';
import {
  aiCareerHeatmap,
  aiCareerTokenBytes,
  aiCareerDailyTokens,
  getAvatar,
  setAvatar,
} from '../../api/aiCareer';
import {
  deriveStats,
  entriesToBuckets,
  formatCompact,
  localDayKey,
  type DayBuckets,
} from './heatmapData';
import { ContributionHeatmap } from './ContributionHeatmap';
import { DailyActivityChart } from './DailyActivityChart';
import { AiCareerPanel } from './AiCareerPanel';

// Keep the existing labeled cumulative-token estimate for summary cards.
const TOKENS_PER_BYTE = 12;
const EMPTY_BUCKETS: DayBuckets = { messages: new Map(), sessions: new Map() };
const DEFAULT_AVATAR = '/default-avatar.webp';

interface TokenResult {
  key: string;
  failed: boolean;
}

export function AiCareerMain() {
  const { t } = useI18n();
  const selectedFamily = useAiCareerStore((s) => s.selectedFamily);
  const chartMetric = useAiCareerStore((s) => s.chartMetric);
  const setChartMetric = useAiCareerStore((s) => s.setChartMetric);
  const refreshKey = useAiCareerStore((s) => s.refreshKey);
  const refresh = useAiCareerStore((s) => s.refresh);
  const setRefreshing = useAiCareerStore((s) => s.setRefreshing);
  const cacheDashboard = useAiCareerStore((s) => s.cacheDashboard);
  const cacheDailyTokens = useAiCareerStore((s) => s.cacheDailyTokens);
  const activePage = useNavigationStore((s) => s.activePage);
  const [settledKey, setSettledKey] = useState('');
  const [failedKey, setFailedKey] = useState('');
  const [tokenResults, setTokenResults] = useState<Record<string, TokenResult>>({});
  const [tokenRetry, setTokenRetry] = useState(0);
  const visitRef = useRef(0);
  const tokenRequestRef = useRef('');
  const explicitTokenRetryRef = useRef(false);
  const [avatar, setAvatarUrl] = useState<string | null>(null);
  const scope = selectedFamily ?? 'overview';
  const currentKey = scope + '::' + refreshKey;
  const snapshot = useAiCareerStore((s) => s.cachedScopes[scope]);
  const cachedDashboard = snapshot?.dashboard;
  const dashboard = useMemo(
    () =>
      cachedDashboard
        ? {
            buckets: entriesToBuckets(cachedDashboard.heatmapEntries),
            tokenBytes: cachedDashboard.tokenBytes,
          }
        : undefined,
    [cachedDashboard]
  );
  const buckets = dashboard?.buckets ?? EMPTY_BUCKETS;
  const tokenResult = tokenResults[scope];
  const tokenDays = useMemo(() => {
    const days = new Map<string, { input: number; output: number }>();
    for (const entry of snapshot?.dailyTokens?.entries ?? []) {
      const key = localDayKey(new Date(entry.ts * 1000));
      const previous = days.get(key);
      days.set(key, {
        input: (previous?.input ?? 0) + entry.input_tokens,
        output: (previous?.output ?? 0) + entry.output_tokens,
      });
    }
    return days;
  }, [snapshot?.dailyTokens]);

  useEffect(() => {
    visitRef.current += 1;
    if (activePage !== 'aiCareer') return;
    let cancelled = false;
    const explicit = useAiCareerStore.getState().refreshing;
    const cached = useAiCareerStore.getState().cachedScopes[scope]?.dashboard;
    Promise.all([aiCareerHeatmap(selectedFamily), aiCareerTokenBytes(selectedFamily)])
      .then(([entries, tokenBytes]) => {
        if (cancelled) return;
        cacheDashboard(selectedFamily, entries, tokenBytes);
        setFailedKey('');
        setSettledKey(currentKey);
      })
      .catch(() => {
        if (cancelled) return;
        setFailedKey(explicit || !cached ? currentKey : '');
        setSettledKey(currentKey);
      });
    return () => {
      cancelled = true;
    };
  }, [activePage, selectedFamily, scope, currentKey, cacheDashboard]);

  useEffect(() => {
    if (activePage !== 'aiCareer' || chartMetric !== 'tokens') return;
    const requestKey = `${visitRef.current}::${currentKey}::${tokenRetry}`;
    if (tokenRequestRef.current === requestKey) return;
    tokenRequestRef.current = requestKey;
    let cancelled = false;
    let settled = false;
    const explicit = useAiCareerStore.getState().refreshing || explicitTokenRetryRef.current;
    explicitTokenRetryRef.current = false;
    const cached = useAiCareerStore.getState().cachedScopes[scope]?.dailyTokens;
    aiCareerDailyTokens(selectedFamily)
      .then((data) => {
        settled = true;
        if (cancelled) return;
        cacheDailyTokens(selectedFamily, data);
        setTokenResults((previous) => ({
          ...previous,
          [scope]: { key: currentKey, failed: false },
        }));
      })
      .catch(() => {
        settled = true;
        if (cancelled) return;
        setTokenResults((previous) => ({
          ...previous,
          [scope]: { key: currentKey, failed: explicit || !cached },
        }));
      });
    return () => {
      cancelled = true;
      if (!settled && tokenRequestRef.current === requestKey) tokenRequestRef.current = '';
    };
  }, [activePage, chartMetric, selectedFamily, scope, currentKey, tokenRetry, cacheDailyTokens]);

  useEffect(() => {
    if (
      activePage === 'aiCareer' &&
      settledKey === currentKey &&
      (chartMetric === 'messages' || tokenResult?.key === currentKey)
    )
      setRefreshing(false);
  }, [activePage, chartMetric, currentKey, settledKey, tokenResult?.key, setRefreshing]);

  useEffect(() => {
    let cancelled = false;
    getAvatar()
      .then((value) => {
        if (!cancelled) setAvatarUrl(value);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  const stats = useMemo(() => deriveStats(buckets), [buckets]);
  const handleEditAvatar = useCallback(async () => {
    try {
      const picked = await openDialog({
        multiple: false,
        filters: [{ name: 'Image', extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif', 'bmp'] }],
      });
      if (typeof picked !== 'string') return;
      await setAvatar(picked);
      setAvatarUrl(await getAvatar());
    } catch {
      // The picker was cancelled or the image could not be read.
    }
  }, []);

  const estTokens = Math.round((dashboard?.tokenBytes ?? 0) * TOKENS_PER_BYTE);
  const statCards: ReadonlyArray<{ label: string; value: string }> = [
    { label: t('aiCareer.stat.sessions'), value: formatCompact(stats.totalSessions) },
    { label: t('aiCareer.stat.messages'), value: formatCompact(stats.totalMessages) },
    { label: t('aiCareer.stat.tokens'), value: formatCompact(estTokens) },
    { label: t('aiCareer.stat.activeDays'), value: formatCompact(stats.activeDays) },
    { label: t('aiCareer.stat.longestStreak'), value: formatCompact(stats.longestStreak) },
  ];

  return (
    <div
      key={scope}
      className="min-w-0 min-h-0 flex-1 overflow-y-auto slim-scroll pr-1 cursor-default"
    >
      <div
        className={
          'max-w-3xl mx-auto min-h-full flex flex-col items-center gap-8 py-2 ' +
          (selectedFamily === null ? 'justify-center' : '')
        }
      >
        {selectedFamily === null && (
          <button type="button" onClick={handleEditAvatar} className="group flex-shrink-0">
            <div className="w-40 h-40 rounded-full overflow-hidden border border-cyber-border/60 bg-cyber-surface flex items-center justify-center transition-opacity group-hover:opacity-85">
              <img
                src={avatar || DEFAULT_AVATAR}
                alt={t('aiCareer.editAvatar')}
                className="w-full h-full object-cover"
                onError={(e) => {
                  const img = e.currentTarget;
                  if (!img.src.endsWith('/brand/bird.png')) img.src = '/brand/bird.png';
                }}
              />
            </div>
          </button>
        )}

        {failedKey === currentKey && (
          <div role="alert" className="w-full flex items-center justify-between gap-3 text-sm">
            <span className="text-cyber-text-secondary">{t('aiCareer.statsLoadFailed')}</span>
            <button
              type="button"
              onClick={refresh}
              className="px-3 py-1.5 rounded-md border border-cyber-border text-cyber-text"
            >
              {t('aiCareer.retry')}
            </button>
          </div>
        )}

        <div className="grid grid-cols-5 gap-3 w-full flex-shrink-0">
          {statCards.map((card) => (
            <div
              key={card.label}
              className="flex flex-col items-center justify-center gap-1.5 rounded-xl border border-transparent bg-cyber-surface/60 px-2 py-4"
            >
              <span className="text-3xl font-bold text-cyber-text tabular-nums">
                {dashboard ? card.value : '—'}
              </span>
              <span className="text-xs text-cyber-text-secondary text-center leading-tight">
                {card.label}
              </span>
            </div>
          ))}
        </div>

        {selectedFamily === null && (
          <div className="w-full flex-shrink-0 -mb-2">
            <ContributionHeatmap buckets={buckets} refreshKey={refreshKey} />
          </div>
        )}
        <div className="w-full flex-shrink-0">
          {activePage === 'aiCareer' && (
            <DailyActivityChart
              buckets={buckets}
              metric={chartMetric}
              onMetricChange={setChartMetric}
              tokenDays={tokenDays}
              tokenStatus={
                chartMetric !== 'tokens'
                  ? 'idle'
                  : tokenResult?.key === currentKey && tokenResult.failed
                    ? 'error'
                    : snapshot?.dailyTokens
                      ? 'ready'
                      : 'loading'
              }
              onTokenRetry={() => {
                explicitTokenRetryRef.current = true;
                setTokenResults((previous) => ({
                  ...previous,
                  [scope]: { key: '', failed: false },
                }));
                setTokenRetry((previous) => previous + 1);
              }}
            />
          )}
        </div>
        {selectedFamily !== null && activePage === 'aiCareer' && (
          <AiCareerPanel key={selectedFamily} family={selectedFamily} />
        )}
      </div>
    </div>
  );
}

export function AiCareerTitleActions() {
  const { t } = useI18n();
  const refresh = useAiCareerStore((s) => s.refresh);
  const refreshing = useAiCareerStore((s) => s.refreshing);
  return (
    <PageActionButton
      icon={RefreshCw}
      spinning={refreshing}
      onClick={refresh}
      pending={refreshing}
      disabled={refreshing}
    >
      {t('btn.refresh')}
    </PageActionButton>
  );
}
