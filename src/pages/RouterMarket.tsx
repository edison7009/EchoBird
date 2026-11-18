import { ArrowDownRight, ArrowUpRight, RefreshCw } from 'lucide-react';
import type * as api from '../api/routerStatistics';
import { MiniSelect } from '../components/MiniSelect';
import { ViewModeTabs } from '../components/ViewModeTabs';
import { useI18n } from '../hooks/useI18n';
import type { StatisticsDisplayRow } from './routerStatisticsData';
import {
  baselineValue,
  marketChange,
  marketModels,
  marketValue,
  type MarketMetric,
} from './routerMarketData';

type Props = {
  snapshot: api.RouterStatisticsSnapshot | null;
  catalog: api.RouterStatisticsCatalog | null;
  rows: StatisticsDisplayRow[];
  current: StatisticsDisplayRow | undefined;
  hidden: boolean;
  metric: MarketMetric;
  modelId: string;
  setSelected: (id: string) => void;
};
function useFormat() {
  const { t, locale } = useI18n();
  const number = (n: number) => n.toLocaleString(locale || 'en', { maximumFractionDigits: 1 });
  const value = (n: number | null, metric: MarketMetric) =>
    n === null
      ? '—'
      : metric === 'cost'
        ? new Intl.NumberFormat(locale || 'en', {
            style: 'currency',
            currency: 'USD',
            maximumFractionDigits: 6,
          }).format(n)
        : number(n);
  const name = (r: StatisticsDisplayRow, hidden: boolean) =>
    hidden ? '******' : r.name || t('statistics.archived');
  return { t, locale, number, value, name };
}
function Change({
  current,
  baseline,
  metric,
}: {
  current: number | null;
  baseline: number | null;
  metric: MarketMetric;
}) {
  const { number } = useFormat();
  const change = marketChange(current, baseline, metric);
  if (change === null) return <span className="rm-change rm-unknown">—</span>;
  const Icon = change >= 0 ? ArrowUpRight : ArrowDownRight;
  return (
    <span className={`rm-change ${change < 0 ? 'rm-down' : 'rm-up'}`}>
      <Icon size={13} />
      {number(Math.abs(change))}%
    </span>
  );
}
function modelFor(props: Props, row: StatisticsDisplayRow) {
  const models = marketModels(props.snapshot, props.catalog, row.account);
  return (
    (row.id === props.current?.id ? models.find((m) => m.id === props.modelId) : undefined) ??
    models[0]
  );
}
export function RouterMarketOverview(props: Props) {
  const { t, value, name } = useFormat();
  if (!props.rows.length) return null;
  return (
    <div className="rm-tickers" data-count={Math.min(props.rows.length, 4)}>
      {props.rows.slice(0, 4).map((row) => {
        const model = modelFor(props, row);
        const current = model ? marketValue(model.metrics, props.metric) : null;
        return (
          <button
            key={row.id}
            className="rm-ticker"
            aria-pressed={row.id === props.current?.id}
            onClick={() => props.setSelected(row.id)}
          >
            <span className="rm-ticker-name">{name(row, props.hidden)}</span>
            <span className="rm-ticker-quote">
              <b>{value(current, props.metric)}</b>
              <Change
                current={current}
                baseline={baselineValue(props.snapshot, model, props.metric)}
                metric={props.metric}
              />
            </span>
            <small>
              {model?.name || t('statistics.archived')} ·{' '}
              {t(props.metric === 'speed' ? 'statistics.outputSpeed' : 'statistics.referenceCost')}
            </small>
          </button>
        );
      })}
    </div>
  );
}
export function RouterMarketWatchlist(props: Props & { showSingle?: boolean }) {
  const { t, value, name } = useFormat();
  if (!props.rows.length || (props.rows.length < 2 && !props.showSingle)) return null;
  return (
    <section className="rm-watchlist" aria-label={t('statistics.watchlist')}>
      <h4>
        {t('statistics.watchlist')} <small>{props.rows.length}</small>
      </h4>
      {props.rows.map((row) => {
        const model = modelFor(props, row);
        return (
          <button
            className="rm-watch-row"
            key={row.id}
            aria-pressed={row.id === props.current?.id}
            onClick={() => props.setSelected(row.id)}
          >
            <span>
              <b>{name(row, props.hidden)}</b>
              <small>{model?.name || '—'}</small>
            </span>
            <span>
              {value(model ? marketValue(model.metrics, props.metric) : null, props.metric)}
            </span>
            <Change
              current={model ? marketValue(model.metrics, props.metric) : null}
              baseline={baselineValue(props.snapshot, model, props.metric)}
              metric={props.metric}
            />
          </button>
        );
      })}
    </section>
  );
}
export function RouterMarketChart(
  props: Props & {
    days: string;
    setMetric: (metric: MarketMetric) => void;
    setModelId: (id: string) => void;
    refreshPrices: () => void;
    priceLoading: boolean;
    priceError: boolean;
  }
) {
  const { t, number, value, name } = useFormat();
  const models = marketModels(props.snapshot, props.catalog, props.current?.account);
  const model = models.find((m) => m.id === props.modelId) ?? models[0];
  const current = model ? marketValue(model.metrics, props.metric) : null;
  const baseline = baselineValue(props.snapshot, model, props.metric);
  const points = (model?.trend ?? []).map((p) => ({
    index: (p.day - (props.snapshot?.today ?? 0) + Number(props.days) - 1) * 24 + p.hour,
    value: marketValue(p.metrics, props.metric),
  }));
  const valid = points.filter((p): p is { index: number; value: number } => p.value !== null);
  const values = [...valid.map((p) => p.value), ...(baseline === null ? [] : [baseline])];
  const low = values.length ? Math.min(...values) * 0.85 : 0;
  const high = values.length ? Math.max(low + 1e-9, Math.max(...values) * 1.12) : 1;
  const x = (index: number) => 5 + (index / Math.max(1, Number(props.days) * 24 - 1)) * 790;
  const y = (n: number) => 159 - ((n - low) / (high - low)) * 145;
  const path = valid
    .map(
      (p, i) => `${i && p.index === valid[i - 1].index + 1 ? 'L' : 'M'}${x(p.index)},${y(p.value)}`
    )
    .join(' ');
  const sampleCount = model
    ? props.metric === 'speed'
      ? model.metrics.speedSamples
      : model.metrics.pricedReports
    : 0;
  const baselineSamples = model
    ? props.metric === 'speed'
      ? model.baseline.speedSamples
      : model.baseline.pricedReports
    : 0;
  return (
    <section className="rm-chart-panel">
      <div className="rm-panel-heading">
        <span>
          <b>{props.current ? name(props.current, props.hidden) : t('statistics.allGroups')}</b>
          <small>{model?.name || '—'}</small>
        </span>
        <ViewModeTabs
          value={props.metric}
          onChange={props.setMetric}
          options={[
            { value: 'speed', label: t('statistics.speed') },
            { value: 'cost', label: t('statistics.cost') },
          ]}
        />
      </div>
      {models.length > 1 && (
        <div className="rm-model-select">
          <MiniSelect
            value={model?.id ?? ''}
            onChange={props.setModelId}
            options={models.map((m) => ({ id: m.id, label: m.name || t('statistics.archived') }))}
          />
        </div>
      )}
      <div className="rm-quote">
        <div>
          <small>
            {t(props.metric === 'speed' ? 'statistics.outputSpeed' : 'statistics.referenceCost')}
          </small>
          <div>
            <b className="rm-quote-value">{value(current, props.metric)}</b>
            <small>{props.metric === 'speed' ? 'tok/s' : 'USD / 1M Token'}</small>
            <Change current={current} baseline={baseline} metric={props.metric} />
          </div>
        </div>
        <div className="rm-baseline">
          <small>{t('statistics.baseline')}</small>
          <b>{value(baseline, props.metric)}</b>
          <small>{t('statistics.baselineCaption')}</small>
        </div>
      </div>
      <div className="rm-plot" role="img" aria-label={t('statistics.performanceTrend')}>
        <svg viewBox="0 0 800 180" preserveAspectRatio="none" aria-hidden="true">
          {[14, 86, 159].map((at) => (
            <line key={at} x1="0" x2="800" y1={at} y2={at} className="rm-gridline" />
          ))}
          {baseline !== null && (
            <line x1="0" x2="800" y1={y(baseline)} y2={y(baseline)} className="rm-baseline-line" />
          )}
          <path d={path} fill="none" className="rm-chart-line" />
          {valid.map((p) => (
            <circle
              key={p.index}
              cx={x(p.index)}
              cy={y(p.value)}
              r="2.5"
              className="rm-chart-point"
            />
          ))}
        </svg>
        {!valid.length && <span className="rm-chart-empty">{t('statistics.noMeasurements')}</span>}
        {valid.length > 0 && (
          <div className="rm-axis-values">
            <span>{value(high, props.metric)}</span>
            <span>{value(low, props.metric)}</span>
          </div>
        )}
      </div>
      <div className="rm-chart-times">
        <span>{props.days === '1' ? '00:00' : t('statistics.periodStart')}</span>
        <span>{props.days === '1' ? '12:00' : t('statistics.periodMiddle')}</span>
        <span>{props.days === '1' ? '23:00' : t('statistics.today')}</span>
      </div>
      <div className="rm-chart-caption">
        <span>{t('statistics.samples').replace('{count}', number(sampleCount))}</span>
        <span>
          {t('statistics.baselineSamples').replace('{count}', number(baselineSamples))}
          {baseline === null && ` · ${t('statistics.insufficientBaseline')}`}
        </span>
      </div>
      {props.metric === 'cost' && (
        <div className="rm-price-status">
          <span>
            {t('statistics.referenceOnly')} · models.dev ·{' '}
            {props.snapshot?.market?.pricing.fetchedAt
              ? new Date(props.snapshot.market.pricing.fetchedAt).toLocaleDateString()
              : '—'}
          </span>
          <button onClick={props.refreshPrices} disabled={props.priceLoading}>
            <RefreshCw size={12} className={props.priceLoading ? 'animate-spin' : ''} />
            {t('statistics.refreshPrices')}
          </button>
          {props.priceError && <span role="alert">{t('statistics.priceError')}</span>}
        </div>
      )}
    </section>
  );
}
export function RouterMarketLedger(props: Props) {
  const { t, locale, number } = useFormat();
  const model = props.current ? modelFor(props, props.current) : undefined;
  const events = (props.snapshot?.market?.events ?? [])
    .filter((e) => model?.rowIds.has(e.row))
    .slice(0, 10);
  return (
    <section className="rm-ledger">
      <div className="rm-panel-heading">
        <b>{t('statistics.executions')}</b>
        <small>{t('statistics.latestExecutions')}</small>
      </div>
      <div className="rm-event rm-event-head">
        <span>{t('statistics.eventTime')}</span>
        <span>{t('statistics.modelsInUse')}</span>
        <span>{t('statistics.result')}</span>
        <span>{t('statistics.outputSpeed')}</span>
        <span>{t('statistics.referenceOnly')}</span>
      </div>
      {events.map((event, i) => (
        <div className="rm-event" key={`${event.at}/${event.row}/${i}`}>
          <time>{new Date(event.at).toLocaleTimeString(locale || 'en')}</time>
          <span>{model?.name || '—'}</span>
          <span className={event.outcome === 'failed' ? 'rs-error' : ''}>
            {t(
              event.outcome === 'completed'
                ? 'statistics.completed'
                : event.outcome === 'failed'
                  ? 'statistics.errors'
                  : 'statistics.cancelled'
            )}
          </span>
          <span>
            {event.generationMs && event.outputTokens !== null
              ? `${number((event.outputTokens * 1000) / event.generationMs)} tok/s`
              : '—'}
          </span>
          <span>
            {event.costMicros !== null
              ? new Intl.NumberFormat(locale || 'en', {
                  style: 'currency',
                  currency: 'USD',
                  maximumFractionDigits: 6,
                }).format(event.costMicros / 1_000_000)
              : '—'}
          </span>
        </div>
      ))}
      {!events.length && <div className="rm-ledger-empty">{t('statistics.noExecutions')}</div>}
    </section>
  );
}

export function RouterMarketPricing(props: Props) {
  const { t, locale } = useI18n();
  const model = props.current ? modelFor(props, props.current) : undefined;
  const price = props.snapshot?.market?.events.find(
    (e) => model?.rowIds.has(e.row) && e.price
  )?.price;
  const rate = (n: number | null | undefined) =>
    n == null
      ? '—'
      : new Intl.NumberFormat(locale || 'en', {
          style: 'currency',
          currency: 'USD',
          maximumFractionDigits: 6,
        }).format(n);
  return (
    <section className="rm-price-details">
      <h4>{t('statistics.referencePrices')}</h4>
      <div className="rs-meta">
        <span>{t('statistics.completedValue')}</span>
        <b className="rm-up">
          {model && model.metrics.completedPricedReports > 0
            ? `+${rate(model.metrics.completedMicros / 1_000_000)}`
            : '—'}
        </b>
      </div>
      <div className="rs-meta">
        <span>{t('statistics.failedCost')}</span>
        <b className="rm-down">
          {model && model.metrics.failedPricedReports > 0
            ? `−${rate(model.metrics.failedMicros / 1_000_000)}`
            : '—'}
        </b>
      </div>
      <small>
        {t('statistics.priceCoverage')
          .replace('{priced}', String(model?.metrics.pricedReports ?? 0))
          .replace('{total}', String(model?.metrics.attempts ?? 0))}
      </small>
      <div className="rs-meta">
        <span>{t('statistics.firstOutput')}</span>
        <b>
          {model?.metrics.firstOutputSamples
            ? `${Math.round(model.metrics.firstOutputMs / model.metrics.firstOutputSamples)} ms`
            : '—'}
        </b>
      </div>

      <div className="rs-meta">
        <span>{t('statistics.inputPrice')}</span>
        <b>{rate(price?.input)}</b>
      </div>
      <div className="rs-meta">
        <span>{t('statistics.outputPrice')}</span>
        <b>{rate(price?.output)}</b>
      </div>
      <div className="rs-meta">
        <span>{t('statistics.cachePrice')}</span>
        <b>{rate(price?.cacheRead)}</b>
      </div>
      <small>
        {price
          ? `models.dev · ${price.provider} · ${new Date(price.fetchedAt).toLocaleDateString(locale || 'en')}`
          : t('statistics.noRecordedPrice')}
      </small>
      <small>{t('statistics.priceUnit')}</small>
    </section>
  );
}
