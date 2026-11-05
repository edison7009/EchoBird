// AI news: original list, source toggle and date archive.
import {
  createContext,
  useContext,
  useState,
  useEffect,
  useCallback,
  useMemo,
  useRef,
} from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useI18n } from '../../hooks/useI18n';
import { fetchFeed, filterNewsItems, type NewsItem } from './feed';

// ===== Archive (disk-backed) =====
// History lives in ~/.echobird/pulse/YYYY/MM/DD_{lang}.json, fanned out
// by the Rust pulse_archive service. Previous versions cached items in
// localStorage which was capped at 3000 entries AND wiped on every Tauri
// WebView origin reset — that's why users kept seeing "only the last
// few days" after each app upgrade. The disk archive removes both
// problems: there is no cap, and the data is independent of the WebView.
//
// The 30-min cadence and the legacy `lastFetched` timestamp still live
// in localStorage because they're (a) tiny and (b) safe to lose — if a
// WebView reset wipes them we simply refetch once.

const FEED_META = (lang: 'zh' | 'en') => `pulse:meta:${lang}`;

const REFRESH_AFTER_MS = 30 * 60 * 1000;

// Legacy cache key. Items kept under this key before the disk archive
// landed are migrated to disk on first run and then the key is removed.
const LEGACY_ITEMS_KEY = (lang: 'zh' | 'en') => `pulse:items:${lang}`;

const MIGRATION_DONE_KEY = (lang: 'zh' | 'en') => `pulse:migrated-to-disk:${lang}`;

interface FeedMeta {
  lastFetched: number;
}

const loadItems = (lang: 'zh' | 'en'): Promise<NewsItem[]> =>
  invoke<NewsItem[]>('pulse_load_all', { lang }).then(filterNewsItems);

const saveItems = async (lang: 'zh' | 'en', items: NewsItem[]): Promise<void> => {
  const allowed = filterNewsItems(items);
  if (allowed.length > 0) await invoke('pulse_save', { lang, items: allowed });
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

// One-shot move from the old localStorage cache into the disk archive.
// Runs at most once per (lang, install). Failure leaves the legacy key
// intact and the migration flag unset, so we'll retry on next launch.
const migrateLegacyToDisk = async (lang: 'zh' | 'en'): Promise<void> => {
  if (localStorage.getItem(MIGRATION_DONE_KEY(lang)) === '1') return;
  try {
    const raw = localStorage.getItem(LEGACY_ITEMS_KEY(lang));
    if (raw) {
      const items: NewsItem[] = JSON.parse(raw);
      await saveItems(lang, items);
    }
    localStorage.removeItem(LEGACY_ITEMS_KEY(lang));
    localStorage.setItem(MIGRATION_DONE_KEY(lang), '1');
  } catch (e) {
    console.warn(`[pulse] migrate ${lang} failed, will retry next launch`, e);
  }
};

// ===== Context =====

interface AiPulseContextValue {
  items: NewsItem[];
  initialLoading: boolean;
  syncing: boolean;
  error: string | null;
  // Currently viewed date (YYYY-MM-DD). null until set — AiPulsePanel
  // auto-fills it with the latest cached date on first mount, after
  // which date-button clicks just replace the value.
  selectedDate: string | null;
  selectDate: (date: string) => void;
  retry: () => void;
  // Source of items to display. For zh locale, the user toggles
  // between 'zh' (国内 — SuYxh aggregator) and 'en' (全球 — our
  // HN/RSS-built feed). For en locale, always 'en' — see comment in
  // the Provider for the rationale (no need to expose Chinese-titled
  // items to global users).
  feedSource: 'zh' | 'en';
  setFeedSource: (s: 'zh' | 'en') => void;
}

const AiPulseContext = createContext<AiPulseContextValue | null>(null);

export function useAiPulse() {
  const ctx = useContext(AiPulseContext);
  if (!ctx) throw new Error('AiPulse context missing');
  return ctx;
}

// ===== Provider =====

// localStorage key for the zh-user feed-source toggle (国内 / 全球). en
// users don't get the toggle so this key is only ever written by zh
// sessions.
const FEED_SOURCE_KEY = 'pulse:feed-source';

export function AiPulseProvider({
  children,
  active,
  enabled = true,
}: {
  children: React.ReactNode;
  active: boolean;
  enabled?: boolean;
}) {
  const { locale } = useI18n();
  // Content routing intentionally narrower than UI routing: only zh-Hans
  // (mainland Simplified) gets the CN-aggregated feed + 国内/全球 toggle.
  // zh-Hant (TW/HK/MO) and ja users see the EN feed — TW/HK devs follow
  // the international AI stack (NVIDIA/CUDA + arXiv + HN), and the SuYxh
  // feed is dominated by CN-domestic platforms (Bilibili, 知乎, 飞桨).
  const isZhLocale = locale === 'zh-Hans';

  // zh users get a 国内/全球 toggle; their selection is persisted in
  // localStorage and lives on `zhFeedPreference`. en users always see
  // 'en' regardless of what zh-side preference was stashed earlier —
  // they don't get the toggle and shouldn't be exposed to Chinese-
  // titled content.
  //
  // feedSource is DERIVED from (isZhLocale, zhFeedPreference) rather
  // than a separate stateful field — so when the user flips the app
  // locale in Settings, feedSource recomputes immediately and the
  // News page swaps to the appropriate feed (previous bug:
  // feedSource was useState'd from localStorage once at mount and
  // never re-synced with locale, leaving zh content visible after
  // switching to en).
  const [zhFeedPreference, setZhFeedPreferenceState] = useState<'zh' | 'en'>(() => {
    const stored = localStorage.getItem(FEED_SOURCE_KEY);
    return stored === 'en' ? 'en' : 'zh';
  });
  const feedSource: 'zh' | 'en' = isZhLocale ? zhFeedPreference : 'en';
  const setFeedSource = useCallback(
    (s: 'zh' | 'en') => {
      if (!isZhLocale) return; // en users have no toggle, no-op
      localStorage.setItem(FEED_SOURCE_KEY, s);
      setZhFeedPreferenceState(s);
    },
    [isZhLocale]
  );

  // Active language for fetching/caching. Derived from feedSource so
  // that flipping the toggle OR the locale re-runs the cache+fetch
  // effect below.
  const lang: 'zh' | 'en' = feedSource;

  const [items, setItems] = useState<NewsItem[]>([]);
  const [initialLoading, setInitialLoading] = useState(true);
  const [syncing, setSyncing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selectedDate, setSelectedDate] = useState<string | null>(null);
  const seq = useRef(0);
  const requestRef = useRef<AbortController | null>(null);
  const loadedLang = useRef(lang);

  const selectDate = useCallback((date: string) => setSelectedDate(date), []);

  const sync = useCallback(async (targetLang: 'zh' | 'en', force = false) => {
    if (requestRef.current) return;
    const controller = new AbortController();
    requestRef.current = controller;
    const my = ++seq.current;
    const current = () => my === seq.current && !controller.signal.aborted;
    setSyncing(true);
    setError(null);
    try {
      await migrateLegacyToDisk(targetLang);
      if (!current()) return;
      const cached = await loadItems(targetLang);
      if (!current()) return;
      setItems(cached);
      if (cached.length > 0) setInitialLoading(false);

      const meta = loadMeta(targetLang);
      if (!force && meta && Date.now() - meta.lastFetched < REFRESH_AFTER_MS && cached.length > 0) {
        return;
      }
      const feed = await fetchFeed(targetLang, controller.signal);
      if (!current()) return;
      await saveItems(targetLang, feed.items);
      if (!current()) return;
      const merged = await loadItems(targetLang);
      if (!current()) return;
      const now = Date.now();
      saveMeta(targetLang, { lastFetched: now });
      setItems(merged);
    } catch (e) {
      if (current()) setError(e instanceof Error ? e.message : String(e));
    } finally {
      if (current()) {
        requestRef.current = null;
        setInitialLoading(false);
        setSyncing(false);
      }
    }
  }, []);

  const retry = useCallback(() => {
    if (enabled && active) void sync(lang, true);
  }, [enabled, active, sync, lang]);

  useEffect(() => {
    if (!enabled) {
      // Release the archived feed in memory when the whole feature is disabled.
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setItems([]);
      setInitialLoading(true);
      return;
    }
    if (!active) return;
    if (loadedLang.current !== lang) {
      loadedLang.current = lang;
      setItems([]);
      setSelectedDate(null);
      setInitialLoading(true);
    }
    // Navigation starts archive hydration and its pending indicator together.
    void sync(lang);
    return () => {
      seq.current += 1;
      requestRef.current?.abort();
      requestRef.current = null;
    };
  }, [enabled, active, lang, sync]);

  const value = useMemo<AiPulseContextValue>(
    () => ({
      items,
      initialLoading,
      syncing,
      error,
      selectedDate,
      selectDate,
      retry,
      feedSource,
      setFeedSource,
    }),
    [
      items,
      initialLoading,
      syncing,
      error,
      selectedDate,
      selectDate,
      retry,
      feedSource,
      setFeedSource,
    ]
  );

  return <AiPulseContext.Provider value={value}>{children}</AiPulseContext.Provider>;
}
