// AI Pulse — Per-item AI news feed.
//
// Source: SuYxh/ai-news-aggregator latest-24h.json (auto-refreshed every 2h, ~600 items, native zh).
// Mirror chain: echobird.ai/pulse/latest-24h.json → CF Worker → GitHub raw.
//
// AI 资讯  : all items.
// 明星项目: subset where url is on github.com or source mentions Trending/开源.
//
// Each row is one news item. Click → open the source URL in the system browser.
// No inline reader: the upstream extractor (jina.ai) hits CAPTCHA on many sources
// and EN/ZH coverage is uneven, so an external browser is the cleanest path.

import { createContext, useContext, useState, useEffect, useCallback, useMemo, useRef } from 'react';
import { open as shellOpen } from '@tauri-apps/plugin-shell';
import { ChevronDown, ChevronRight, ExternalLink, RefreshCw } from 'lucide-react';
import { useI18n } from '../../hooks/useI18n';

// ===== Mirror config =====

const PULSE_MIRRORS: { name: string; base: string }[] = [
    { name: 'echobird',   base: 'https://echobird.ai/pulse' },
    // Upstream's own GitHub Pages — official publish point, freshest data
    // (updates within minutes of the upstream Action), CORS enabled,
    // and *.github.io is generally GFW-routable from mainland China.
    { name: 'pages',      base: 'https://suyxh.github.io/ai-news-aggregator/data' },
    // jsDelivr CDN — global CDN that proxies the repo. Caches @main for
    // up to 12h so it lags the upstream; useful when github.io is flaky.
    { name: 'jsdelivr',   base: 'https://cdn.jsdelivr.net/gh/SuYxh/ai-news-aggregator@main/data' },
    // Last-resort: raw.githubusercontent.com — works globally except in
    // mainland China (GFW-blocked).
    { name: 'github-raw', base: 'https://raw.githubusercontent.com/SuYxh/ai-news-aggregator/main/data' },
];

// 7-day window gives much richer EN content (~5000 items vs ~600 for 24h)
// after the strict CJK-title filter and project sub-filter eat into the pool.
// Trade-off: 5.4 MB fetch every 6h, capped to MAX_ITEMS in storage.
const FEED_FILE = 'latest-7d.json';

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

const ITEMS_KEY        = 'pulse:items';
const FEED_META        = 'pulse:meta';
const MAX_ITEMS        = 3000;
const REFRESH_AFTER_MS = 30 * 60 * 1000;

interface FeedMeta { lastFetched: number; }

const loadItems = (): NewsItem[] => {
    try { const raw = localStorage.getItem(ITEMS_KEY); return raw ? JSON.parse(raw) : []; }
    catch { return []; }
};
const saveItems = (items: NewsItem[]) => {
    try { localStorage.setItem(ITEMS_KEY, JSON.stringify(items)); } catch { /* quota */ }
};
const loadMeta = (): FeedMeta | null => {
    try { const raw = localStorage.getItem(FEED_META); return raw ? JSON.parse(raw) : null; }
    catch { return null; }
};
const saveMeta = (m: FeedMeta) => {
    try { localStorage.setItem(FEED_META, JSON.stringify(m)); } catch { /* quota */ }
};

// ===== Network: mirror-aware fetch =====

let preferredMirror = 0;

const looksLikeHtml = (s: string): boolean => {
    const head = s.slice(0, 200).trimStart().toLowerCase();
    return head.startsWith('<!doctype html') || head.startsWith('<html');
};

async function fetchFeed(): Promise<RawFeed> {
    const order = [
        ...PULSE_MIRRORS.slice(preferredMirror),
        ...PULSE_MIRRORS.slice(0, preferredMirror),
    ];
    let lastErr: any = null;
    for (let i = 0; i < order.length; i++) {
        const mirror = order[i];
        try {
            const res = await fetch(`${mirror.base}/${FEED_FILE}`, { cache: 'no-cache' });
            if (!res.ok) { lastErr = new Error(`${mirror.name} ${res.status}`); continue; }
            const text = await res.text();
            if (looksLikeHtml(text)) { lastErr = new Error(`${mirror.name} returned HTML`); continue; }
            try {
                const parsed = JSON.parse(text);
                preferredMirror = (preferredMirror + i) % PULSE_MIRRORS.length;
                return parsed;
            } catch { lastErr = new Error(`${mirror.name} bad JSON`); continue; }
        } catch (e) { lastErr = e; }
    }
    throw lastErr || new Error('all mirrors failed');
}

// ===== Helpers =====

const openExternal = (url: string) => shellOpen(url).catch(() => window.open(url, '_blank'));

const formatRelative = (ts: number, locale: string): string => {
    const sec = Math.max(0, Math.floor((Date.now() - ts) / 1000));
    const isCN = locale.startsWith('zh');
    if (sec < 60)        return isCN ? '刚刚' : 'just now';
    if (sec < 3600)      return isCN ? `${Math.floor(sec/60)}分钟前` : `${Math.floor(sec/60)}m ago`;
    if (sec < 86400)     return isCN ? `${Math.floor(sec/3600)}小时前` : `${Math.floor(sec/3600)}h ago`;
    return isCN ? `${Math.floor(sec/86400)}天前` : `${Math.floor(sec/86400)}d ago`;
};

const isProjectItem = (item: NewsItem): boolean => {
    if (item.url.includes('github.com/')) return true;
    const s = `${item.source} ${item.site_name || ''}`.toLowerCase();
    return /trending|开源|github|hugging\s*face/i.test(s);
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
    return source
        // Drop parenthetical groups that contain CJK: "(技术社区)", "(创意工作者社区)"
        .replace(/[（(][^)）]*[一-鿿][^)）]*[)）]/g, '')
        // Drop trailing "· ..." segments containing CJK: "Hacker News · 24h最热"
        .replace(/[·∙•|]\s*[^·•|]*[一-鿿][^·•|]*$/g, '')
        .replace(/\s+/g, ' ')
        .trim();
};

// ===== Context =====

interface AiPulseContextValue {
    items: NewsItem[];
    initialLoading: boolean;
    syncing: boolean;
    error: string | null;
    scrollTarget: string | null;
    requestScroll: (date: string) => void;
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
    const [items, setItems] = useState<NewsItem[]>(() => loadItems());
    const [initialLoading, setInitialLoading] = useState(() => loadItems().length === 0);
    const [syncing, setSyncing] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [scrollTarget, setScrollTarget] = useState<string | null>(null);
    const [lastFetched, setLastFetched] = useState<number | null>(() => loadMeta()?.lastFetched || null);
    const seq = useRef(0);
    const itemsRef = useRef(items);
    useEffect(() => { itemsRef.current = items; }, [items]);

    const requestScroll = useCallback((date: string) => {
        setScrollTarget(date);
        setTimeout(() => setScrollTarget(null), 100);
    }, []);

    const sync = useCallback(async (force = false) => {
        const meta = loadMeta();
        if (!force && meta && Date.now() - meta.lastFetched < REFRESH_AFTER_MS && itemsRef.current.length > 0) {
            setInitialLoading(false);
            return;
        }
        const my = ++seq.current;
        setSyncing(true);
        setError(null);
        try {
            const feed = await fetchFeed();
            if (my !== seq.current) return;

            const existing = loadItems();
            // Dedupe by url — upstream often ingests the same article through
            // multiple aggregators (e.g. raw HN + 24h-hot HN + Lobsters = 3 ids, 1 url).
            // First occurrence wins so the oldest entry's metadata stays stable.
            const byUrl = new Map<string, NewsItem>();
            for (const it of [...existing, ...feed.items]) {
                if (!byUrl.has(it.url)) byUrl.set(it.url, it);
            }
            const merged = Array.from(byUrl.values())
                .sort((a, b) => itemTs(b).localeCompare(itemTs(a)))
                .slice(0, MAX_ITEMS);

            saveItems(merged);
            const now = Date.now();
            saveMeta({ lastFetched: now });
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

    const retry = useCallback(() => { sync(true); }, [sync]);

    useEffect(() => { sync(); }, [sync]);

    const value = useMemo<AiPulseContextValue>(() => ({
        items, initialLoading, syncing, error, scrollTarget, requestScroll, lastFetched, retry,
    }), [items, initialLoading, syncing, error, scrollTarget, requestScroll, lastFetched, retry]);

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
                className={`text-sm px-3 py-1.5 border rounded-md transition-colors flex items-center gap-2 ${!syncing
                    ? 'border-cyber-border/50 text-cyber-text hover:bg-cyber-text/10'
                    : 'border-cyber-border text-cyber-text-muted cursor-not-allowed'}`}
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
    const title = lang === 'en' && item.title_en ? item.title_en : (item.title_zh || item.title);
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
            <ExternalLink size={14} className="flex-shrink-0 mt-1 text-cyber-text-muted/50 group-hover:text-cyber-text transition-colors" />
        </button>
    );
}

// ===== Feed =====

function ItemFeed({ variant }: { variant: PageVariant }) {
    const { t, locale } = useI18n();
    const { items, initialLoading, syncing, error, scrollTarget, retry } = useAiPulse();
    const containerRef = useRef<HTMLDivElement>(null);
    const lang: 'zh' | 'en' = locale.startsWith('zh') ? 'zh' : 'en';

    const visible = useMemo(() => {
        // Show only items written in the user's app language.
        // zh users → Chinese-titled items (incl. WeChat 公众号 they can read).
        // en users → English-titled items (Hacker News, Bloomberg, TechCrunch …).
        const langMatched = items.filter(it => itemLang(it) === lang);
        return variant === 'projects' ? langMatched.filter(isProjectItem) : langMatched;
    }, [items, variant, lang]);

    useEffect(() => {
        if (!scrollTarget) return;
        const anchor = containerRef.current?.querySelector(`[data-pulse-date="${scrollTarget}"]`);
        if (anchor) (anchor as HTMLElement).scrollIntoView({ behavior: 'smooth', block: 'start' });
    }, [scrollTarget]);

    if ((initialLoading || syncing) && visible.length === 0) {
        return (
            <div ref={containerRef} className="space-y-2">
                {Array.from({ length: 8 }).map((_, i) => (
                    <div key={i} className="p-3 bg-cyber-surface rounded-card animate-pulse">
                        <div className="h-3 w-full bg-cyber-border/50 rounded mb-2" />
                        <div className="h-3 w-2/3 bg-cyber-border/30 rounded" />
                    </div>
                ))}
            </div>
        );
    }

    if (error && visible.length === 0) {
        return (
            <div className="p-8 text-center text-sm font-mono">
                <div className="text-cyber-warning mb-2">{t('pulse.fetchFailed')}</div>
                <div className="text-xs text-cyber-text-muted/60 mb-4 break-all max-w-md mx-auto">{error}</div>
                <button
                    onClick={retry}
                    className="text-xs px-4 py-2 border border-cyber-border/50 rounded text-cyber-text hover:bg-cyber-text/10 transition-colors"
                >
                    {t('btn.refresh')}
                </button>
            </div>
        );
    }

    if (visible.length === 0) {
        return (
            <div className="p-8 text-center text-sm text-cyber-text-secondary font-mono">
                {t('pulse.empty')}
            </div>
        );
    }

    let lastDate = '';
    return (
        <div ref={containerRef} className="space-y-2">
            {/* Reserved 2px slot — opacity toggle prevents layout shift on sync start/stop */}
            <div className="sticky top-0 z-20 h-0.5 overflow-hidden pointer-events-none">
                <div
                    className={`h-full w-1/3 bg-cyber-accent/70 transition-opacity duration-150 ${
                        syncing ? 'opacity-100 animate-[loading_1.2s_ease-in-out_infinite]' : 'opacity-0'
                    }`}
                />
            </div>
            {visible.map(item => {
                const date = itemLocalDate(item);
                const isFirstOfDate = !!date && date !== lastDate;
                if (date) lastDate = date;
                return (
                    <div key={item.id} data-pulse-date={isFirstOfDate ? date : undefined}>
                        <ItemRow item={item} />
                    </div>
                );
            })}
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

export function AiPulsePanel() {
    const { t, locale } = useI18n();
    const { items, requestScroll } = useAiPulse();
    const lang: 'zh' | 'en' = locale.startsWith('zh') ? 'zh' : 'en';

    const cachedDates = useMemo(() => {
        const set = new Set<string>();
        for (const it of items) {
            if (itemLang(it) !== lang) continue;
            const d = itemLocalDate(it);
            if (d) set.add(d);
        }
        return Array.from(set).sort((a, b) => b.localeCompare(a));
    }, [items, lang]);

    const grouped = useMemo(() => groupByMonth(cachedDates), [cachedDates]);
    const months = useMemo(() => Array.from(grouped.keys()), [grouped]);

    const [expanded, setExpanded] = useState<Set<string>>(new Set());
    useEffect(() => {
        setExpanded(prev => prev.size === 0 && months.length > 0 ? new Set(months.slice(0, 1)) : prev);
    }, [months]);

    const toggle = (ym: string) => setExpanded(prev => {
        const next = new Set(prev);
        next.has(ym) ? next.delete(ym) : next.add(ym);
        return next;
    });

    return (
        <>
            <div className="px-3 py-2 mb-1 flex items-center justify-between bg-transparent">
                <div className="text-[15px] font-semibold text-cyber-text">{t('pulse.archive')}</div>
                <span className="text-[13px] font-mono text-cyber-text-muted">{cachedDates.length} {t('pulse.days')}</span>
            </div>
            <div className="flex-1 px-2 overflow-y-auto pb-4">
                {cachedDates.length === 0 ? (
                    <div className="px-3 py-8 text-center text-[14px] text-cyber-text-muted leading-relaxed">
                        {t('pulse.loadingFirst')}
                    </div>
                ) : (
                    <div className="space-y-1">
                        {months.map(ym => {
                            const isOpen = expanded.has(ym);
                            const days = grouped.get(ym) || [];
                            return (
                                <div key={ym}>
                                    <button
                                        onClick={() => toggle(ym)}
                                        className="w-full flex items-center gap-1.5 px-2 py-2 text-[15px] font-mono text-cyber-text-secondary hover:text-cyber-text rounded transition-colors"
                                    >
                                        {isOpen ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
                                        <span>{ym}</span>
                                        <span className="ml-auto text-[13px] text-cyber-text-muted">{days.length}</span>
                                    </button>
                                    {isOpen && (
                                        <div className="ml-3 border-l border-cyber-border/20 pl-2 space-y-0.5">
                                            {days.map(d => (
                                                <button
                                                    key={d}
                                                    onClick={() => requestScroll(d)}
                                                    className="w-full text-left px-2 py-1.5 rounded text-[14px] font-mono text-cyber-text-secondary hover:bg-cyber-elevated hover:text-cyber-text transition-colors"
                                                >
                                                    {d.slice(8)}
                                                </button>
                                            ))}
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
