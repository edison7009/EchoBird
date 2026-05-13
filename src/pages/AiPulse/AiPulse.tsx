// AI Pulse — Per-item AI news feed.
//
// Two parallel feeds, picked by app locale:
//   • zh users → SuYxh/ai-news-aggregator latest-7d.json (~6000 items, mostly CN)
//   • en users → our own latest-7d-en.json built from HN Algolia + AI lab RSS
//                + GitHub Trending. US/global sources only, no CN dependency.
// Mirror chain: echobird.ai/pulse → CF Worker → GitHub raw / Tencent COS / jsdelivr.
//
// AI 资讯  : items that are NOT projects (news articles, blog posts, HN discussion).
// 明星项目: items where url is on github.com or source mentions Trending/开源.
// The two views are disjoint: every item lands in exactly one tab. Without
// this split the EN feed (built largely from HN AI stories that link to
// github.com + GitHub Trending) makes both views look identical.
//
// Each row is one news item. Click → open the source URL in the system browser.
// No inline reader: the upstream extractor (jina.ai) hits CAPTCHA on many sources
// and EN/ZH coverage is uneven, so an external browser is the cleanest path.

import {
  createContext,
  useContext,
  useState,
  useEffect,
  useCallback,
  useMemo,
  useRef,
} from 'react';
import { open as shellOpen } from '@tauri-apps/plugin-shell';
import { ChevronDown, ChevronRight, ExternalLink, RefreshCw } from 'lucide-react';
import { useI18n } from '../../hooks/useI18n';

// ===== Mirror config =====

// ZH feed: SuYxh's bilingual (mostly CN) aggregator — five mirrors.
const PULSE_MIRRORS_ZH: { name: string; base: string }[] = [
  { name: 'echobird', base: 'https://echobird.ai/pulse' },
  // Tencent COS Hong Kong — refreshed by .github/workflows/refresh-pulse-data.yml
  // every 6h. HK region needs no ICP filing, default domain is bucket-level
  // public-read, and is reliably reachable from mainland China when
  // echobird.ai (Cloudflare) is throttled.
  { name: 'tencent-hk', base: 'https://ainew-1251534910.cos.ap-hongkong.myqcloud.com' },
  // Upstream's own GitHub Pages — official publish point, freshest data
  // (updates within minutes of the upstream Action), CORS enabled,
  // and *.github.io is generally GFW-routable from mainland China.
  { name: 'pages', base: 'https://suyxh.github.io/ai-news-aggregator/data' },
  // jsDelivr CDN — global CDN that proxies the repo. Caches @main for
  // up to 12h so it lags the upstream; useful when github.io is flaky.
  { name: 'jsdelivr', base: 'https://cdn.jsdelivr.net/gh/SuYxh/ai-news-aggregator@main/data' },
  // Last-resort: raw.githubusercontent.com — works globally except in
  // mainland China (GFW-blocked).
  {
    name: 'github-raw',
    base: 'https://raw.githubusercontent.com/SuYxh/ai-news-aggregator/main/data',
  },
];

// EN feed: built by scripts/build_en_pulse.py and committed to docs/pulse/
// + mirrored to Tencent COS. Only available from our infrastructure — none
// of SuYxh's mirrors carry it. Order trades latency vs. CN-reachability the
// same way as the ZH chain.
const PULSE_MIRRORS_EN: { name: string; base: string }[] = [
  { name: 'echobird', base: 'https://echobird.ai/pulse' },
  { name: 'tencent-hk', base: 'https://ainew-1251534910.cos.ap-hongkong.myqcloud.com' },
  // jsDelivr proxy of OUR repo's docs/pulse — useful when echobird.ai is throttled.
  { name: 'jsdelivr', base: 'https://cdn.jsdelivr.net/gh/edison7009/EchoBird@main/docs/pulse' },
  {
    name: 'github-raw',
    base: 'https://raw.githubusercontent.com/edison7009/EchoBird/main/docs/pulse',
  },
];

// 7-day window gives much richer EN content (~5000 items vs ~600 for 24h)
// after the strict CJK-title filter and project sub-filter eat into the pool.
// Trade-off: 5.4 MB fetch every 6h, capped to MAX_ITEMS in storage.
const FEED_FILE_ZH = 'latest-7d.json';
const FEED_FILE_EN = 'latest-7d-en.json';

// ===== Types =====

interface RawFeed {
  generated_at: string;
  window_hours: number;
  total_items: number;
  items: NewsItem[];
}

interface NewsItem {
  id: string;
  site_id?: string;
  site_name?: string;
  source: string;
  title: string;
  url: string;
  published_at: string | null;
  first_seen_at?: string;
  last_seen_at?: string;
  title_zh?: string | null;
  title_en?: string | null;
}

type PageVariant = 'news' | 'projects';

// ===== Local cache =====
// Per-locale keys: zh and en use different upstream files, so a single shared
// cache would clobber on every locale switch. Keeping them separate also lets
// each feed track its own refresh cadence.

const ITEMS_KEY = (lang: 'zh' | 'en') => `pulse:items:${lang}`;
const FEED_META = (lang: 'zh' | 'en') => `pulse:meta:${lang}`;
const MAX_ITEMS = 3000;
const REFRESH_AFTER_MS = 30 * 60 * 1000;

interface FeedMeta {
  lastFetched: number;
}

const loadItems = (lang: 'zh' | 'en'): NewsItem[] => {
  try {
    const raw = localStorage.getItem(ITEMS_KEY(lang));
    return raw ? JSON.parse(raw) : [];
  } catch {
    return [];
  }
};
const saveItems = (lang: 'zh' | 'en', items: NewsItem[]) => {
  try {
    localStorage.setItem(ITEMS_KEY(lang), JSON.stringify(items));
  } catch {
    /* quota */
  }
};
const loadMeta = (lang: 'zh' | 'en'): FeedMeta | null => {
  try {
    const raw = localStorage.getItem(FEED_META(lang));
    return raw ? JSON.parse(raw) : null;
  } catch {
    return null;
  }
};
const saveMeta = (lang: 'zh' | 'en', m: FeedMeta) => {
  try {
    localStorage.setItem(FEED_META(lang), JSON.stringify(m));
  } catch {
    /* quota */
  }
};

// ===== Network: mirror-aware fetch =====

// Sticky preferred-mirror index, separate per feed: once a mirror serves a
// good response it stays at the head of the chain for subsequent fetches.
const preferredMirror: Record<'zh' | 'en', number> = { zh: 0, en: 0 };

const looksLikeHtml = (s: string): boolean => {
  const head = s.slice(0, 200).trimStart().toLowerCase();
  return head.startsWith('<!doctype html') || head.startsWith('<html');
};

async function fetchOneFeed(lang: 'zh' | 'en'): Promise<RawFeed> {
  const mirrors = lang === 'en' ? PULSE_MIRRORS_EN : PULSE_MIRRORS_ZH;
  const file = lang === 'en' ? FEED_FILE_EN : FEED_FILE_ZH;
  const start = preferredMirror[lang];
  const order = [...mirrors.slice(start), ...mirrors.slice(0, start)];
  let lastErr: any = null;
  for (let i = 0; i < order.length; i++) {
    const mirror = order[i];
    try {
      const res = await fetch(`${mirror.base}/${file}`, { cache: 'no-cache' });
      if (!res.ok) {
        lastErr = new Error(`${mirror.name} ${res.status}`);
        continue;
      }
      const text = await res.text();
      if (looksLikeHtml(text)) {
        lastErr = new Error(`${mirror.name} returned HTML`);
        continue;
      }
      try {
        const parsed = JSON.parse(text);
        preferredMirror[lang] = (start + i) % mirrors.length;
        return parsed;
      } catch {
        lastErr = new Error(`${mirror.name} bad JSON`);
        continue;
      }
    } catch (e) {
      lastErr = e;
    }
  }
  throw lastErr || new Error('all mirrors failed');
}

// Soft-fallback wrapper: en users hit our self-built file first, but if that
// file isn't yet deployed (fresh PR, GHA hasn't run, etc.) fall through to
// the ZH file — its EN-titled subset (~2000 items) is the same content the
// app shipped before this feature, so the page never looks empty.
async function fetchFeed(lang: 'zh' | 'en'): Promise<RawFeed> {
  if (lang === 'en') {
    try {
      return await fetchOneFeed('en');
    } catch (e) {
      console.warn('[pulse] EN feed unavailable, falling back to bilingual feed:', e);
      return await fetchOneFeed('zh');
    }
  }
  return fetchOneFeed('zh');
}

// ===== Helpers =====

const openExternal = (url: string) => shellOpen(url).catch(() => window.open(url, '_blank'));

const formatRelative = (ts: number, locale: string): string => {
  const sec = Math.max(0, Math.floor((Date.now() - ts) / 1000));
  const isCN = locale.startsWith('zh');
  if (sec < 60) return isCN ? '刚刚' : 'just now';
  if (sec < 3600) return isCN ? `${Math.floor(sec / 60)}分钟前` : `${Math.floor(sec / 60)}m ago`;
  if (sec < 86400)
    return isCN ? `${Math.floor(sec / 3600)}小时前` : `${Math.floor(sec / 3600)}h ago`;
  return isCN ? `${Math.floor(sec / 86400)}天前` : `${Math.floor(sec / 86400)}d ago`;
};

// URL-path-driven classification so that *.blog* hosts and lab-name sources
// stay in News. e.g. github.blog → news (engineering articles), but
// github.com/owner/repo → project; huggingface.co/blog → news, but
// huggingface.co/spaces|models|datasets → project. The source-name regex is
// kept only for upstream ZH aggregators that label items with explicit
// project markers like "GitHub Trending" or "开源周报".
const isProjectItem = (item: NewsItem): boolean => {
  const url = item.url || '';
  if (/^https?:\/\/(www\.)?github\.com\//i.test(url)) return true;
  if (/^https?:\/\/huggingface\.co\/(spaces|models|datasets)\//i.test(url)) return true;
  const s = `${item.source} ${item.site_name || ''}`.toLowerCase();
  return /trending|开源/i.test(s);
};

// Many items have null published_at; first_seen_at is always present.
// Some Chinese aggregators label local CST as "Z" (UTC), putting published_at
// up to 8h in the future — fall back to first_seen_at in that case so the
// relative-time display doesn't pin every fresh item at "刚刚".
const itemTs = (item: NewsItem): string => {
  const pub = item.published_at;
  const seen = item.first_seen_at || item.last_seen_at || '';
  if (!pub) return seen;
  const pubMs = Date.parse(pub);
  if (Number.isFinite(pubMs) && pubMs > Date.now() + 5 * 60 * 1000) return seen || pub;
  return pub;
};

// Local-timezone YYYY-MM-DD. A naive `ts.slice(0, 10)` slices the raw ISO
// string and so groups items by UTC date — for CST users that pushes
// every item from CST 00:00–08:00 back into yesterday's archive bucket.
const itemLocalDate = (item: NewsItem): string => {
  const ts = itemTs(item);
  if (!ts) return '';
  const ms = Date.parse(ts);
  if (!Number.isFinite(ms)) return ts.slice(0, 10);
  const d = new Date(ms);
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, '0');
  const day = String(d.getDate()).padStart(2, '0');
  return `${y}-${m}-${day}`;
};

// Language detection by CJK presence in the title. Bilingual side effect:
// Chinese-app users see WeChat 公众号 too (and have WeChat installed to read them);
// English-app users see only Latin-script items, so the WeChat awkwardness is gone.
const CJK_RE = /[㐀-䶿一-鿿豈-﫿]/;
const itemLang = (item: NewsItem): 'zh' | 'en' => {
  const probe = item.title || item.title_zh || '';
  return CJK_RE.test(probe) ? 'zh' : 'en';
};

// Upstream wraps western sources with Chinese decorations like "Lobsters (技术社区)"
// or "Hacker News · 24h最热". Strip those for EN display so the feed reads as pure global content.
const cleanSourceForEn = (source: string): string => {
  if (!source) return source;
  return (
    source
      // Drop parenthetical groups that contain CJK: "(技术社区)", "(创意工作者社区)"
      .replace(/[（(][^)）]*[一-鿿][^)）]*[)）]/g, '')
      // Drop trailing "· ..." segments containing CJK: "Hacker News · 24h最热"
      .replace(/[·∙•|]\s*[^·•|]*[一-鿿][^·•|]*$/g, '')
      .replace(/\s+/g, ' ')
      .trim()
  );
};

// ===== Context =====

interface AiPulseContextValue {
  items: NewsItem[];
  initialLoading: boolean;
  syncing: boolean;
  error: string | null;
  // Currently-viewed date (YYYY-MM-DD). null until set — AiPulsePanel
  // auto-fills it with the latest cached date on first mount, after
  // which date-button clicks just replace the value.
  selectedDate: string | null;
  selectDate: (date: string) => void;
  lastFetched: number | null;
  retry: () => void;
}

const AiPulseContext = createContext<AiPulseContextValue | null>(null);

function useAiPulse() {
  const ctx = useContext(AiPulseContext);
  if (!ctx) throw new Error('AiPulse context missing');
  return ctx;
}

// ===== Provider =====

export function AiPulseProvider({ children }: { children: React.ReactNode }) {
  const { locale } = useI18n();
  const lang: 'zh' | 'en' = locale.startsWith('zh') ? 'zh' : 'en';

  const [items, setItems] = useState<NewsItem[]>(() => loadItems(lang));
  const [initialLoading, setInitialLoading] = useState(() => loadItems(lang).length === 0);
  const [syncing, setSyncing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selectedDate, setSelectedDate] = useState<string | null>(null);
  const [lastFetched, setLastFetched] = useState<number | null>(
    () => loadMeta(lang)?.lastFetched || null
  );
  const seq = useRef(0);
  const itemsRef = useRef(items);
  useEffect(() => {
    itemsRef.current = items;
  }, [items]);

  const selectDate = useCallback((date: string) => setSelectedDate(date), []);

  const sync = useCallback(async (targetLang: 'zh' | 'en', force = false) => {
    const meta = loadMeta(targetLang);
    const cached = loadItems(targetLang);
    if (!force && meta && Date.now() - meta.lastFetched < REFRESH_AFTER_MS && cached.length > 0) {
      setInitialLoading(false);
      return;
    }
    const my = ++seq.current;
    setSyncing(true);
    setError(null);
    try {
      const feed = await fetchFeed(targetLang);
      if (my !== seq.current) return;

      // Dedupe by url against THIS lang's existing cache only — never
      // cross-pollinate zh ↔ en caches.
      const existing = loadItems(targetLang);
      const byUrl = new Map<string, NewsItem>();
      for (const it of [...existing, ...feed.items]) {
        if (!byUrl.has(it.url)) byUrl.set(it.url, it);
      }
      const merged = Array.from(byUrl.values())
        .sort((a, b) => itemTs(b).localeCompare(itemTs(a)))
        .slice(0, MAX_ITEMS);

      saveItems(targetLang, merged);
      const now = Date.now();
      saveMeta(targetLang, { lastFetched: now });
      setItems(merged);
      setLastFetched(now);
    } catch (e: any) {
      if (my !== seq.current) return;
      setError(e?.message || 'Network error');
    } finally {
      if (my === seq.current) {
        setInitialLoading(false);
        setSyncing(false);
      }
    }
  }, []);

  const retry = useCallback(() => {
    sync(lang, true);
  }, [sync, lang]);

  // Refetch + swap cache whenever the locale flips. selectedDate gets reset
  // because the date sets between zh and en feeds usually don't overlap.
  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setItems(loadItems(lang));
    setLastFetched(loadMeta(lang)?.lastFetched || null);
    setSelectedDate(null);
    setInitialLoading(loadItems(lang).length === 0);
    sync(lang);
  }, [lang, sync]);

  const value = useMemo<AiPulseContextValue>(
    () => ({
      items,
      initialLoading,
      syncing,
      error,
      selectedDate,
      selectDate,
      lastFetched,
      retry,
    }),
    [items, initialLoading, syncing, error, selectedDate, selectDate, lastFetched, retry]
  );

  return <AiPulseContext.Provider value={value}>{children}</AiPulseContext.Provider>;
}

// ===== Title actions =====

export function AiPulseTitleActions() {
  const { t } = useI18n();
  const { syncing, retry } = useAiPulse();
  return (
    <div className="ml-auto flex-shrink-0 flex items-center gap-2">
      <button
        onClick={retry}
        disabled={syncing}
        title={t('btn.refresh')}
        className={`text-sm px-3 py-1.5 border rounded-md transition-colors flex items-center gap-2 ${
          !syncing
            ? 'border-cyber-border/50 text-cyber-text hover:bg-cyber-text/10'
            : 'border-cyber-border text-cyber-text-muted cursor-not-allowed'
        }`}
      >
        <RefreshCw size={13} className={syncing ? 'animate-spin' : ''} />
        {t('btn.refresh')}
      </button>
    </div>
  );
}

// ===== Item row =====

function ItemRow({ item }: { item: NewsItem }) {
  const { locale } = useI18n();
  const lang = locale.startsWith('zh') ? 'zh' : 'en';
  const tsRaw = itemTs(item);
  const ts = tsRaw ? Date.parse(tsRaw) : 0;
  const title = lang === 'en' && item.title_en ? item.title_en : item.title_zh || item.title;
  const sourceLabel = lang === 'en' ? cleanSourceForEn(item.source) : item.source;

  return (
    <button
      onClick={() => openExternal(item.url)}
      className="group w-full text-left rounded-card hover:bg-cyber-text/5 transition-colors px-5 py-4 flex gap-3 items-start"
    >
      <div className="flex-1 min-w-0">
        <div className="text-[15px] font-medium text-cyber-text leading-relaxed group-hover:text-cyber-accent transition-colors">
          {title}
        </div>
        <div className="mt-2 text-[13px] text-cyber-text-secondary flex items-center gap-2">
          <span className="truncate max-w-[240px]">{sourceLabel}</span>
          {ts > 0 && (
            <>
              <span className="opacity-50">·</span>
              <span>{formatRelative(ts, locale)}</span>
            </>
          )}
        </div>
      </div>
      <ExternalLink
        size={14}
        className="flex-shrink-0 mt-1 text-cyber-text-muted/50 group-hover:text-cyber-text transition-colors"
      />
    </button>
  );
}

// ===== Feed =====

function ItemFeed({ variant }: { variant: PageVariant }) {
  const { t, locale } = useI18n();
  const { items, initialLoading, syncing, error, selectedDate, retry } = useAiPulse();
  const lang: 'zh' | 'en' = locale.startsWith('zh') ? 'zh' : 'en';

  // Two-stage filter: language → variant. Done as a single memo because
  // both upstream inputs change rarely and we'll re-derive several
  // date-keyed views off the result. News and Projects are disjoint
  // partitions — see isProjectItem comment above.
  const variantFiltered = useMemo(() => {
    const langMatched = items.filter((it) => itemLang(it) === lang);
    return variant === 'projects'
      ? langMatched.filter(isProjectItem)
      : langMatched.filter((it) => !isProjectItem(it));
  }, [items, variant, lang]);

  // Latest cached date for this variant+lang. Used as the fallback when
  // selectedDate is null (panel hasn't initialised it yet, or items just
  // arrived). Computing here — not in the provider — keeps the default
  // language-aware: zh users won't accidentally land on an en-only date.
  const latestDate = useMemo(() => {
    let latest = '';
    for (const it of variantFiltered) {
      const d = itemLocalDate(it);
      if (d && d > latest) latest = d;
    }
    return latest;
  }, [variantFiltered]);

  const effectiveDate = selectedDate || latestDate;

  // Per-day batching: feed shows a single day's items. Switching dates in
  // the right panel replaces the batch wholesale instead of scrolling
  // through every older day, which was the original UX bug.
  const visible = useMemo(() => {
    if (!effectiveDate) return [];
    return variantFiltered.filter((it) => itemLocalDate(it) === effectiveDate);
  }, [variantFiltered, effectiveDate]);

  if ((initialLoading || syncing) && visible.length === 0 && variantFiltered.length === 0) {
    return (
      <div className="space-y-2">
        <div className="sticky top-0 z-20 h-0.5 overflow-hidden pointer-events-none">
          <div className="h-full w-1/3 bg-cyber-accent/70 animate-[loading_1.2s_ease-in-out_infinite]" />
        </div>
        {Array.from({ length: 8 }).map((_, i) => (
          <div key={i} className="p-3 bg-cyber-surface rounded-card animate-pulse">
            <div className="h-3 w-full bg-cyber-border/50 rounded mb-2" />
            <div className="h-3 w-2/3 bg-cyber-border/30 rounded" />
          </div>
        ))}
      </div>
    );
  }

  if (error && variantFiltered.length === 0) {
    return (
      <div className="p-8 text-center text-sm font-mono">
        <div className="text-cyber-warning mb-2">{t('pulse.fetchFailed')}</div>
        <div className="text-xs text-cyber-text-muted/60 mb-4 break-all max-w-md mx-auto">
          {error}
        </div>
        <button
          onClick={retry}
          className="text-xs px-4 py-2 border border-cyber-border/50 rounded text-cyber-text hover:bg-cyber-text/10 transition-colors"
        >
          {t('btn.refresh')}
        </button>
      </div>
    );
  }

  return (
    <div className="space-y-2">
      <div className="sticky top-0 z-20 h-0.5 overflow-hidden pointer-events-none">
        <div
          className={`h-full w-1/3 bg-cyber-accent/70 transition-opacity duration-150 ${
            syncing ? 'opacity-100 animate-[loading_1.2s_ease-in-out_infinite]' : 'opacity-0'
          }`}
        />
      </div>
      {visible.length === 0 ? (
        <div className="p-8 text-center text-sm text-cyber-text-secondary font-mono">
          {t('pulse.empty')}
        </div>
      ) : (
        visible.map((item) => <ItemRow key={item.id} item={item} />)
      )}
    </div>
  );
}

export function AiNewsMain() {
  return (
    <div className="flex-1 overflow-y-auto pb-4">
      <ItemFeed variant="news" />
    </div>
  );
}

export function AiProjectsMain() {
  return (
    <div className="flex-1 overflow-y-auto pb-4">
      <ItemFeed variant="projects" />
    </div>
  );
}

// ===== Right panel: date tree =====

function groupByMonth(dates: string[]): Map<string, string[]> {
  const map = new Map<string, string[]>();
  for (const d of dates) {
    const ym = d.slice(0, 7);
    if (!map.has(ym)) map.set(ym, []);
    map.get(ym)!.push(d);
  }
  return map;
}

export function AiPulsePanel({ variant = 'news' }: { variant?: PageVariant }) {
  const { t, locale } = useI18n();
  const { items, selectedDate, selectDate } = useAiPulse();
  const lang: 'zh' | 'en' = locale.startsWith('zh') ? 'zh' : 'en';

  // Per-day item count, filtered by lang AND variant so the number on
  // each date button reflects what the user will actually see in the
  // feed when they click. cachedDates derives from the same map's keys
  // — a date with zero matching items wouldn't appear at all.
  // Must mirror ItemFeed's variantFiltered: news = !isProjectItem,
  // projects = isProjectItem (the two are disjoint).
  const dateCount = useMemo(() => {
    const map = new Map<string, number>();
    for (const it of items) {
      if (itemLang(it) !== lang) continue;
      const proj = isProjectItem(it);
      if (variant === 'projects' ? !proj : proj) continue;
      const d = itemLocalDate(it);
      if (!d) continue;
      map.set(d, (map.get(d) || 0) + 1);
    }
    return map;
  }, [items, lang, variant]);

  const cachedDates = useMemo(
    () => Array.from(dateCount.keys()).sort((a, b) => b.localeCompare(a)),
    [dateCount]
  );

  const grouped = useMemo(() => groupByMonth(cachedDates), [cachedDates]);
  const months = useMemo(() => Array.from(grouped.keys()), [grouped]);

  // Auto-pick the latest date once items arrive so the feed has something
  // to show without forcing the user to click first. Re-runs only when
  // selectedDate is still null — once the user picks anything, we leave
  // their choice alone even if a refresh adds a newer day.
  useEffect(() => {
    if (selectedDate) return;
    if (cachedDates.length === 0) return;
    selectDate(cachedDates[0]);
  }, [cachedDates, selectedDate, selectDate]);

  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setExpanded((prev) =>
      prev.size === 0 && months.length > 0 ? new Set(months.slice(0, 1)) : prev
    );
  }, [months]);

  const toggle = (ym: string) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(ym)) {
        next.delete(ym);
      } else {
        next.add(ym);
      }
      return next;
    });

  return (
    <>
      <div className="px-3 py-2 mb-1 bg-transparent">
        <div className="text-[15px] font-semibold text-cyber-text">{t('pulse.archive')}</div>
      </div>
      <div className="flex-1 px-2 overflow-y-auto pb-4">
        {cachedDates.length === 0 ? (
          <div className="px-3 py-8 text-center text-[14px] text-cyber-text-muted leading-relaxed">
            {t('pulse.loadingFirst')}
          </div>
        ) : (
          <div className="space-y-1">
            {months.map((ym) => {
              const isOpen = expanded.has(ym);
              const days = grouped.get(ym) || [];
              // Sum items across the month so the badge mirrors what
              // the user gets when they expand the group, not a
              // redundant day count (the day chevrons already
              // implicitly convey day count by their list length).
              const monthCount = days.reduce((s, d) => s + (dateCount.get(d) || 0), 0);
              return (
                <div key={ym}>
                  <button
                    onClick={() => toggle(ym)}
                    className="w-full flex items-center gap-1.5 px-2 py-2 text-[15px] font-mono text-cyber-text-secondary hover:text-cyber-text rounded transition-colors"
                  >
                    {isOpen ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
                    <span>{ym}</span>
                    <span className="ml-auto text-[13px] text-cyber-text-muted">{monthCount}</span>
                  </button>
                  {isOpen && (
                    <div className="ml-3 border-l border-cyber-border/20 pl-2 space-y-0.5">
                      {days.map((d) => {
                        const isActive = d === selectedDate;
                        const count = dateCount.get(d) || 0;
                        return (
                          <button
                            key={d}
                            onClick={() => selectDate(d)}
                            className={`w-full flex items-center px-2 py-1.5 rounded text-[14px] font-mono transition-colors ${
                              isActive
                                ? 'bg-cyber-accent/15 text-cyber-text border border-cyber-accent/40'
                                : 'text-cyber-text-secondary hover:bg-cyber-elevated hover:text-cyber-text border border-transparent'
                            }`}
                          >
                            <span>{d.slice(8)}</span>
                            <span
                              className={`ml-auto text-[12px] ${isActive ? 'text-cyber-text-secondary' : 'text-cyber-text-muted'}`}
                            >
                              {count}
                            </span>
                          </button>
                        );
                      })}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </div>
    </>
  );
}
