import { useEffect, useId, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { useI18n } from '../../hooks/useI18n';
import { ViewModeTabGroup } from '../../components/ViewModeTabGroup';
import { CareerTooltip } from './CareerTooltip';
import { localDayKey, type DayBuckets } from './heatmapData';

const CHART_DAYS = 30;
const LABEL_DAYS = new Set([0, 7, 14, 21, CHART_DAYS - 1]);

function chartDays(buckets: DayBuckets, today: Date) {
  today.setHours(0, 0, 0, 0);
  return Array.from({ length: CHART_DAYS }, (_, i) => {
    const date = new Date(today);
    date.setDate(date.getDate() - (CHART_DAYS - 1 - i));
    const key = localDayKey(date);
    return { key, date, count: buckets.messages.get(key) ?? 0 };
  });
}

interface TipState {
  index: number;
  cx: number;
  top: number;
}

export function DailyActivityChart({
  buckets,
  metric,
  onMetricChange,
  tokenDays,
  tokenStatus,
  onTokenRetry,
}: {
  buckets: DayBuckets;
  metric: 'messages' | 'tokens';
  onMetricChange: (metric: 'messages' | 'tokens') => void;
  tokenDays: Map<string, { input: number; output: number }>;
  tokenStatus: 'idle' | 'loading' | 'ready' | 'error';
  onTokenRetry: () => void;
}) {
  const { t, locale } = useI18n();
  const chartId = useId();
  const today = localDayKey(new Date());
  const days = useMemo(() => chartDays(buckets, new Date(`${today}T00:00:00`)), [buckets, today]);
  const max = Math.max(
    1,
    ...days.map((day) => {
      const tokens = tokenDays.get(day.key);
      return metric === 'messages' ? day.count : tokens ? tokens.input + tokens.output : 0;
    })
  );
  const tipRef = useRef<HTMLDivElement>(null);
  const [tip, setTip] = useState<TipState | null>(null);
  const [tipLeft, setTipLeft] = useState(0);

  useLayoutEffect(() => {
    if (!tip || !tipRef.current) return;
    const half = tipRef.current.offsetWidth / 2;
    setTipLeft(Math.max(half + 4, Math.min(tip.cx, window.innerWidth - half - 4)));
  }, [tip, buckets, tokenDays, metric, locale]);

  useEffect(() => {
    if (!tip) return;
    const clear = () => setTip(null);
    window.addEventListener('scroll', clear, true);
    window.addEventListener('resize', clear);
    return () => {
      window.removeEventListener('scroll', clear, true);
      window.removeEventListener('resize', clear);
    };
  }, [tip]);

  const tipText = (index: number) => {
    const day = days[index];
    const date = day.date.toLocaleDateString(locale, {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
    });
    if (metric === 'messages') {
      return `${date} · ${t('aiCareer.messages').replace('{count}', day.count.toLocaleString(locale))}`;
    }
    const tokens = tokenDays.get(day.key);
    const input = tokens ? tokens.input.toLocaleString(locale) : '—';
    const output = tokens ? tokens.output.toLocaleString(locale) : '—';
    const total = tokens ? (tokens.input + tokens.output).toLocaleString(locale) : '—';
    return `${date} · ${t('aiCareer.input')} ${input} · ${t('aiCareer.output')} ${output} · ${t('aiCareer.total')} ${total}`;
  };

  const showTip = (index: number, target: HTMLElement) => {
    const cb = target.getBoundingClientRect();
    setTip({ index, cx: cb.left + cb.width / 2, top: cb.top - 4 });
  };

  return (
    <div className="relative w-full" onMouseLeave={() => setTip(null)}>
      <div className="h-10 mb-3 flex items-center justify-between gap-2">
        <div
          aria-hidden={metric !== 'tokens'}
          className={`flex items-center gap-3 text-xs text-cyber-text-secondary ${metric === 'tokens' ? '' : 'invisible'}`}
        >
          <span className="flex items-center gap-1.5">
            <span
              className="w-2.5 h-2.5 rounded-sm"
              style={{ background: 'rgb(var(--accent-rgb) / 0.90)' }}
            />
            {t('aiCareer.input')}
          </span>
          <span className="flex items-center gap-1.5">
            <span
              className="w-2.5 h-2.5 rounded-sm"
              style={{ background: 'rgb(var(--accent-rgb) / 0.48)' }}
            />
            {t('aiCareer.output')}
          </span>
        </div>
        <ViewModeTabGroup
          role="tablist"
          className="view-mode-tabs flex flex-shrink-0 border border-cyber-border rounded-button overflow-hidden"
        >
          {(['messages', 'tokens'] as const).map((mode, index, modes) => (
            <button
              key={mode}
              type="button"
              role="tab"
              id={`${chartId}-${mode}`}
              aria-selected={metric === mode}
              aria-controls={`${chartId}-panel`}
              tabIndex={metric === mode ? 0 : -1}
              onClick={() => {
                setTip(null);
                onMetricChange(mode);
              }}
              onKeyDown={(event) => {
                if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
                event.preventDefault();
                const next = event.key === 'Home' ? 0 : event.key === 'End' ? 1 : 1 - index;
                setTip(null);
                onMetricChange(modes[next]);
                const tabButtons =
                  event.currentTarget.parentElement?.querySelectorAll<HTMLButtonElement>(
                    '[role="tab"]'
                  );
                tabButtons?.[next]?.focus();
              }}
              className={`view-mode-tab px-3 py-1.5 text-sm cjk-btn transition-[color] ${
                metric === mode
                  ? 'bg-cyber-elevated text-cyber-text'
                  : 'text-cyber-text-secondary hover:text-cyber-text'
              }`}
            >
              {t(mode === 'messages' ? 'aiCareer.chartMessages' : 'aiCareer.chartTokens')}
            </button>
          ))}
        </ViewModeTabGroup>
      </div>
      <div
        role="tabpanel"
        id={`${chartId}-panel`}
        aria-labelledby={`${chartId}-${metric}`}
        aria-busy={metric === 'tokens' && (tokenStatus === 'idle' || tokenStatus === 'loading')}
        className="relative grid h-28 items-end gap-1 border-b border-cyber-border/50"
        style={{ gridTemplateColumns: `repeat(${CHART_DAYS}, minmax(0, 1fr))` }}
      >
        {days.map((day, index) => {
          const tokens = tokenDays.get(day.key);
          const total = tokens ? tokens.input + tokens.output : 0;
          return (
            <div
              key={day.key}
              role="img"
              aria-label={tipText(index)}
              tabIndex={0}
              onMouseEnter={(e) => showTip(index, e.currentTarget)}
              onFocus={(e) => showTip(index, e.currentTarget)}
              onBlur={() => setTip(null)}
              className="flex h-full items-end rounded-t-sm focus-visible:outline focus-visible:outline-1 focus-visible:outline-cyber-accent"
            >
              {metric === 'messages' ? (
                <div
                  className="w-full rounded-t-sm"
                  style={{
                    height: day.count > 0 ? `${Math.max(2, (day.count / max) * 100)}%` : 2,
                    background:
                      day.count > 0
                        ? 'rgb(var(--accent-rgb) / 0.82)'
                        : 'rgb(var(--text-primary-rgb) / 0.06)',
                  }}
                />
              ) : total > 0 && tokens ? (
                <div
                  className="w-full flex flex-col-reverse rounded-t-sm overflow-hidden"
                  style={{ height: `${(total / max) * 100}%` }}
                >
                  <div
                    style={{
                      height: `${(tokens.input / total) * 100}%`,
                      background: 'rgb(var(--accent-rgb) / 0.90)',
                    }}
                  />
                  <div
                    style={{
                      height: `${(tokens.output / total) * 100}%`,
                      background: 'rgb(var(--accent-rgb) / 0.48)',
                    }}
                  />
                </div>
              ) : (
                <div
                  className="w-full"
                  style={{ height: 2, background: 'rgb(var(--text-primary-rgb) / 0.06)' }}
                />
              )}
            </div>
          );
        })}
        {metric === 'tokens' && (tokenStatus === 'idle' || tokenStatus === 'loading') && (
          <div
            className="absolute inset-0 flex items-center justify-center pointer-events-none text-xs text-cyber-text-secondary"
            role="status"
          >
            {t('aiCareer.chartLoading')}
          </div>
        )}
        {metric === 'tokens' && tokenStatus === 'error' && (
          <div
            className="absolute inset-0 flex items-center justify-center gap-3 pointer-events-none text-xs text-cyber-text-secondary"
            role="alert"
          >
            <span>{t('aiCareer.tokenLoadFailed')}</span>
            <button
              type="button"
              onClick={onTokenRetry}
              className="pointer-events-auto px-2 py-1 rounded-md border border-cyber-border text-cyber-text"
            >
              {t('aiCareer.retry')}
            </button>
          </div>
        )}
      </div>
      <div
        className="mt-2 grid text-[10px] text-cyber-text-secondary"
        style={{ gridTemplateColumns: `repeat(${CHART_DAYS}, minmax(0, 1fr))` }}
      >
        {days.map((day, index) => (
          <span
            key={localDayKey(day.date)}
            className={
              index === 0
                ? 'justify-self-start whitespace-nowrap'
                : index === CHART_DAYS - 1
                  ? 'justify-self-end whitespace-nowrap'
                  : 'justify-self-center whitespace-nowrap'
            }
          >
            {LABEL_DAYS.has(index) &&
              day.date.toLocaleDateString(locale, { month: 'short', day: 'numeric' })}
          </span>
        ))}
      </div>
      {tip &&
        createPortal(
          <CareerTooltip
            ref={tipRef}
            text={tipText(tip.index)}
            left={tipLeft}
            top={tip.top}
            fixed
          />,
          document.body
        )}
    </div>
  );
}
