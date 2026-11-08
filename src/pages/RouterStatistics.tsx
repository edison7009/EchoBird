import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import { Check, ChevronRight, Eye, EyeOff, GitBranch, RefreshCw, Server } from 'lucide-react';
import { ViewModeTabs } from '../components/ViewModeTabs';
import { MiniSelect } from '../components/MiniSelect';
import { PageActionButton } from '../components/PageActionButton';
import { getModelIcon } from '../components/cards/ModelCard';
import { useI18n } from '../hooks/useI18n';
import { useDocumentVisible } from '../hooks/useDocumentVisible';
import { useNavigationStore } from '../stores/navigationStore';
import * as api from '../api/routerStatistics';
import { statisticsRows } from './routerStatisticsData';
import './RouterStatistics.css';

function useStatisticsState() {
  const generation = useNavigationStore((state) => state.developerGeneration);
  const active = useNavigationStore(
    (state) =>
      state.developerMode &&
      state.activePage === 'freeModels' &&
      state.smartAccessTab === 'statistics'
  );
  const visible = useDocumentVisible();
  const [days, setDays] = useState('1');
  const [group, setGroup] = useState('');
  const [unused, setUnused] = useState(false);
  const [hidden, setHidden] = useState(false);
  const [selected, setSelected] = useState('');
  const [result, setResult] = useState<{
    days: string;
    group: string;
    value: api.RouterStatisticsSnapshot;
  } | null>(null);
  const snapshot = result?.days === days && result.group === group ? result.value : null;
  const [catalog, setCatalog] = useState<api.RouterStatisticsCatalog | null>(null);
  const [loading, setLoading] = useState(false);
  const [dataError, setDataError] = useState(false);
  const [catalogError, setCatalogError] = useState(false);
  const read = useRef<() => void>(() => {});
  const forceRead = useRef<() => void>(() => {});
  const readCatalog = useRef<() => void>(() => {});

  useEffect(() => {
    if (!active || !visible) return;
    let current = true;
    let pending = false;
    const load = async () => {
      if (pending || !current) return;
      pending = true;
      try {
        const value = await api.getSmartRouterStatisticsCatalog();
        if (current) {
          setCatalog(value);
          setCatalogError(false);
          setGroup((group) =>
            group && !value.groups.some((item) => item.id === group) ? '' : group
          );
        }
      } catch {
        if (current) setCatalogError(true);
      } finally {
        pending = false;
      }
    };
    readCatalog.current = () => void load();
    void load();
    return () => {
      current = false;
      readCatalog.current = () => {};
    };
  }, [active, visible, generation]);

  useEffect(() => {
    if (!active || !visible) return;
    let current = true;
    let pending = false;
    let again = false;
    const load = async () => {
      if (pending || !current) return;
      pending = true;
      await Promise.resolve();
      if (!current) return;
      setLoading(true);
      try {
        const value = await api.getSmartRouterStatistics(Number(days), group);
        if (current) {
          setResult({ days, group, value });
          setDataError(false);
        }
      } catch {
        if (current) setDataError(true);
      } finally {
        pending = false;
        if (current) setLoading(false);
        if (current && again) {
          again = false;
          void load();
        }
      }
    };
    read.current = () => void load();
    forceRead.current = () => {
      if (pending) again = true;
      else void load();
    };
    void load();
    return () => {
      current = false;
      read.current = () => {};
      forceRead.current = () => {};
    };
  }, [active, visible, days, group, generation]);

  useEffect(() => {
    if (!active || !visible || !snapshot?.recording) return;
    const timer = window.setInterval(() => read.current(), 10_000);
    return () => window.clearInterval(timer);
  }, [active, visible, snapshot?.recording, generation]);

  useEffect(() => {
    if (!active || !visible) return;
    const changed = () => forceRead.current();
    document.addEventListener(api.ROUTER_STATISTICS_CHANGED, changed);
    return () => document.removeEventListener(api.ROUTER_STATISTICS_CHANGED, changed);
  }, [active, visible]);

  const refreshAll = useCallback(() => {
    forceRead.current();
    readCatalog.current();
  }, []);
  const rows = useMemo(() => statisticsRows(snapshot, catalog, group), [snapshot, catalog, group]);
  const unusedCount = rows.filter((row) => row.enabled && row.counts.attempts === 0).length;
  const shown = unused
    ? rows.filter((row) => row.enabled && row.counts.attempts === 0)
    : rows.filter((row) => row.counts.attempts > 0);
  const current = shown.find((row) => row.id === selected) ?? shown[0];
  return {
    days,
    setDays,
    group,
    setGroup,
    unused,
    setUnused,
    hidden,
    setHidden,
    selected,
    setSelected,
    snapshot,
    catalog,
    loading,
    error: dataError || catalogError,
    refreshAll,
    rows: shown,
    unusedCount,
    current,
  };
}

const StatisticsContext = createContext<ReturnType<typeof useStatisticsState> | null>(null);
function useStatistics() {
  const value = useContext(StatisticsContext);
  if (!value) throw new Error('Router statistics provider is missing');
  return value;
}
export function RouterStatisticsProvider({ children }: { children: ReactNode }) {
  const value = useStatisticsState();
  return <StatisticsContext.Provider value={value}>{children}</StatisticsContext.Provider>;
}

const kinds = [
  'statistics.key',
  'statistics.token',
  'statistics.password',
  'statistics.privateKey',
  'statistics.otherCredential',
] as const;
const failures = [
  'statistics.rateLimit',
  'statistics.authentication',
  'statistics.quota',
  'statistics.modelUnavailable',
  'statistics.connection',
  'statistics.otherError',
] as const;

function SourceIcon({ provider, model }: { provider: string; model: string }) {
  const icon = getModelIcon(provider, model);
  return icon ? <img src={icon} alt="" /> : <Server size={23} />;
}

function useStatisticsFormat() {
  const { t, locale } = useI18n();
  const language = locale || 'en';
  const number = (value: number) => value.toLocaleString(language);
  const last = (value: number) => {
    if (!value) return '—';
    const seconds = Math.max(0, (Date.now() - value) / 1000);
    if (seconds < 60) return t('statistics.now');
    const formatter = new Intl.RelativeTimeFormat(language, { numeric: 'auto' });
    if (seconds < 3600) return formatter.format(-Math.floor(seconds / 60), 'minute');
    if (seconds < 86400) return formatter.format(-Math.floor(seconds / 3600), 'hour');
    if (seconds < 31 * 86400) return formatter.format(-Math.floor(seconds / 86400), 'day');
    return new Date(value).toLocaleDateString(language);
  };
  const date = (day: number) =>
    new Intl.DateTimeFormat(language, { month: '2-digit', day: '2-digit', timeZone: 'UTC' }).format(
      new Date((day - 719163) * 86400000)
    );
  return { t, number, last, date };
}

export function RouterStatisticsMain() {
  const state = useStatistics();
  const { t, number, last, date } = useStatisticsFormat();
  const snapshot = state.snapshot;
  const totals = snapshot?.totals;
  const history = Boolean(snapshot && totals && totals.calls > 0);
  const name = (row: (typeof state.rows)[number]) =>
    state.hidden ? '******' : row.name || t('statistics.archived');
  const completed = totals?.completed ?? 0;
  const settled = completed + (totals?.failed ?? 0);
  const count = state.days === '1' ? 24 : Number(state.days);
  const trend = Array.from({ length: count }, (_, index) => {
    const records =
      snapshot?.trend.filter((point) =>
        state.days === '1'
          ? point.day === snapshot.today && point.hour === index
          : point.day === snapshot.today - count + 1 + index
      ) ?? [];
    return {
      calls: records.reduce((sum, item) => sum + item.totals.calls, 0),
      completed: records.reduce((sum, item) => sum + item.totals.completed, 0),
    };
  });
  const maximum = Math.max(1, ...trend.map((point) => point.calls));
  return (
    <div className="router-statistics">
      <div className="rs-scroll">
        <div className="rs-toolbar">
          <ViewModeTabs
            value={state.days}
            onChange={state.setDays}
            options={[
              { value: '1', label: t('statistics.today') },
              { value: '7', label: t('statistics.week') },
              { value: '14', label: t('statistics.fortnight') },
            ]}
          />
          <MiniSelect
            value={state.group}
            onChange={(value) => {
              state.setGroup(value);
              state.setUnused(false);
            }}
            options={[
              { id: '', label: t('statistics.allGroups') },
              ...(state.catalog?.groups ?? []).map((group) => ({
                id: group.id,
                label: group.name,
              })),
            ]}
            className="rs-group"
          />
        </div>
        {state.error && (
          <p role="alert" className="rs-status">
            {t('statistics.loadError')}
          </p>
        )}
        <section className="rs-summary">
          <div>
            <span>{t('statistics.calls')}</span>
            <strong>{history ? number(totals?.calls ?? 0) : '—'}</strong>
            <small>
              {history && (
                <>
                  {number(completed)} {t('statistics.completed')} · {number(totals?.failed ?? 0)}{' '}
                  {t('statistics.failed')}
                </>
              )}
            </small>
          </div>
          <div>
            <span>{t('statistics.completionRate')}</span>
            <strong>
              {settled ? ((completed / settled) * 100).toFixed(1) : '—'}
              {settled > 0 && <em>%</em>}
            </strong>
            <small>
              <Check size={12} />
              {t('statistics.settled')}
            </small>
          </div>
          <div>
            <span>{t('statistics.rescued')}</span>
            <strong>
              {history ? number(totals?.rescued ?? 0) : '—'}
              {history && <em>{t('statistics.times')}</em>}
            </strong>
            <small>
              <GitBranch size={12} />
              {t('statistics.rescuedCaption')}
            </small>
          </div>
          <div>
            <span>{t('statistics.averageTime')}</span>
            <strong>
              {completed ? ((totals?.durationMs ?? 0) / completed / 1000).toFixed(1) : '—'}
              {completed > 0 && <em>{t('statistics.seconds')}</em>}
            </strong>
            <small>{t('statistics.durationCaption')}</small>
          </div>
        </section>
        <section className="rs-trend-section">
          <div className="rs-heading">
            <h3>{t('statistics.trend')}</h3>
            <span className="rs-legend">
              <i />
              {t('statistics.completed')}
              <i />
              {t('statistics.failed')}
            </span>
          </div>
          <div className="rs-trend" style={{ gridTemplateColumns: `repeat(${count},1fr)` }}>
            {trend.map((point, index) => (
              <div
                className="rs-trend-column"
                key={index}
                aria-label={point.calls > 0 ? `${index}: ${number(point.calls)}` : undefined}
              >
                {point.calls > 0 && (
                  <div style={{ height: `${(point.calls / maximum) * 100}%` }}>
                    <i
                      style={{
                        height: `${point.calls ? (point.completed / point.calls) * 100 : 0}%`,
                      }}
                    />
                  </div>
                )}
              </div>
            ))}
          </div>
          <div className="rs-trend-labels">
            {(state.days === '1'
              ? ['00:00', '06:00', '12:00', '18:00', '23:00']
              : [
                  date((snapshot?.today ?? 0) - count + 1),
                  date((snapshot?.today ?? 0) - Math.floor(count / 2)),
                  date(snapshot?.today ?? 0),
                ]
            ).map((label) => (
              <span key={label}>{label}</span>
            ))}
          </div>
        </section>
        <section className="rs-usage">
          <div className="rs-heading">
            <h3>{t('statistics.distribution')}</h3>
            <div className="rs-actions">
              {state.unusedCount > 0 && (
                <button aria-pressed={state.unused} onClick={() => state.setUnused(!state.unused)}>
                  {t('statistics.unused')} {number(state.unusedCount)}
                </button>
              )}
              <PageActionButton
                icon={state.hidden ? EyeOff : Eye}
                aria-label={t(state.hidden ? 'statistics.showAccounts' : 'statistics.hideAccounts')}
                onClick={() => state.setHidden(!state.hidden)}
              />
              <PageActionButton
                icon={RefreshCw}
                spinning={state.loading}
                aria-label={t('btn.refresh')}
                onClick={state.refreshAll}
                disabled={state.loading}
              />
            </div>
          </div>
          <div className="rs-table">
            <div className="rs-table-head">
              <span>{t('statistics.distribution')}</span>
              <span>{t('statistics.attempts')}</span>
              <span>{t('statistics.completed')}</span>
              <span>{t('statistics.errors')}</span>
              <span>{t('statistics.recent')}</span>
            </div>
            {state.rows.length === 0 &&
              Array.from({ length: 3 }, (_, index) => (
                <div key={index} className="rs-row rs-placeholder" aria-hidden="true">
                  <span className="rs-identity">
                    <b>—</b>
                    <small>—</small>
                  </span>
                  <span>—</span>
                  <span>—</span>
                  <span>—</span>
                  <span>—</span>
                </div>
              ))}
            {state.rows.map((row) => (
              <button
                key={row.id}
                aria-pressed={row.id === state.current?.id}
                className={`rs-row ${row.id === state.current?.id ? 'rs-selected' : ''}`}
                onClick={() => state.setSelected(row.id)}
              >
                <span className="rs-source">
                  <SourceIcon provider={row.provider} model={row.model} />
                  <span className="rs-identity">
                    <b>{name(row)}</b>
                    <small>
                      {row.provider || '—'}
                      {!row.enabled && !row.archived && <em>{t('statistics.paused')}</em>}
                    </small>
                  </span>
                  <span className="rs-tags">
                    {row.counts.protections.map(
                      (value, index) =>
                        value > 0 && (
                          <span key={kinds[index]} className={`rs-tag rs-kind-${index}`}>
                            <i />
                            {t(kinds[index])} ×{number(value)}
                          </span>
                        )
                    )}
                  </span>
                </span>
                <span className="rs-load">
                  <b>{number(row.counts.attempts)}</b>
                  <i>
                    <i
                      style={{
                        width: `${(row.counts.attempts / Math.max(1, ...state.rows.map((row) => row.counts.attempts))) * 100}%`,
                      }}
                    />
                  </i>
                </span>
                <span>{number(row.counts.completed)}</span>
                <span className={row.counts.failed ? 'rs-error' : ''}>
                  {number(row.counts.failed)}
                </span>
                <span className="rs-last">
                  {last(row.lastUsedAt)}
                  <ChevronRight size={12} />
                </span>
              </button>
            ))}
          </div>
        </section>
        <footer className="rs-footer">{t('statistics.privacy')}</footer>
        {(totals?.cancelled ?? 0) > 0 && (
          <p className="rs-status">
            {t('statistics.cancelled')} {number(totals?.cancelled ?? 0)}
          </p>
        )}
        {snapshot?.storageError && (
          <p role="alert" className="rs-status">
            {t('statistics.storageError')}
          </p>
        )}
        {(snapshot?.capped || state.catalog?.capped) && (
          <p className="rs-status">{t('statistics.capped')}</p>
        )}
      </div>
    </div>
  );
}

export function RouterStatisticsPanel() {
  const state = useStatistics();
  const { t, number, last } = useStatisticsFormat();
  const row = state.current;
  if (!row) {
    return (
      <div className="rs-detail">
        <div className="flex h-full items-center justify-center text-sm text-cyber-text-secondary">
          {t('statistics.selectSource')}
        </div>
      </div>
    );
  }
  const settled = row.counts.completed + row.counts.failed;
  const name = state.hidden ? '******' : row.name || t('statistics.archived');
  const pending = Math.max(0, row.counts.attempts - settled - row.counts.cancelled);
  const group =
    row.groups
      .map((id) => state.catalog?.groups.find((group) => group.id === id)?.name)
      .filter(Boolean)
      .join(' / ') || t('statistics.archived');
  return (
    <div className="rs-detail">
      <div className="rs-detail-identity">
        <SourceIcon provider={row.provider} model={row.model} />
        <div>
          <strong>{name}</strong>
          <span>{row.provider}</span>
        </div>
      </div>
      <div className="rs-detail-rate">
        <strong>
          {settled ? ((row.counts.completed / settled) * 100).toFixed(1) : '—'}
          {settled > 0 && <small>%</small>}
        </strong>
        <span>{t('statistics.attemptRate')}</span>
      </div>
      <div className="rs-detail-counts">
        <div>
          <b>{number(row.counts.attempts)}</b>
          <span>{t('statistics.attempts')}</span>
        </div>
        <div>
          <b className={row.counts.failed ? 'rs-error' : ''}>{number(row.counts.failed)}</b>
          <span>{t('statistics.errors')}</span>
        </div>
      </div>
      <div className="rs-meta">
        <span>{t('statistics.lastUsed')}</span>
        <b>{last(row.lastUsedAt)}</b>
      </div>
      <div className="rs-meta">
        <span>{t('statistics.group')}</span>
        <b>{group}</b>
      </div>
      {row.counts.cancelled > 0 && (
        <div className="rs-meta">
          <span>{t('statistics.cancelled')}</span>
          <b>{number(row.counts.cancelled)}</b>
        </div>
      )}
      {pending > 0 && (
        <div className="rs-meta">
          <span>{t('statistics.unsettled')}</span>
          <b>{number(pending)}</b>
        </div>
      )}
      <section className="rs-detail-models">
        <h4>{t('statistics.modelsInUse')}</h4>
        {row.models.map((model) => (
          <div key={model.name}>
            <div>
              <span>{model.name || '—'}</span>
              <b>{number(model.attempts)}</b>
            </div>
            <i>
              <i
                style={{
                  width: `${row.counts.attempts ? (model.attempts / row.counts.attempts) * 100 : 0}%`,
                }}
              />
            </i>
          </div>
        ))}
      </section>
      <section className="rs-detail-errors">
        <h4>{t('statistics.errorReasons')}</h4>
        {row.counts.errors.map(
          (value, index) =>
            value > 0 && (
              <div key={failures[index]}>
                <span>{t(failures[index])}</span>
                <b>{number(value)}</b>
              </div>
            )
        )}
      </section>
    </div>
  );
}
