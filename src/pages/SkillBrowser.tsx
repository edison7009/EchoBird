/**
 * Skill Browser — claude-skill-registry powered
 * Loads 99K+ agent skills from search-index.json (hardcoded source).
 * Skills are cached locally as ~/.echobird/skills.json.
 * Search filters locally by name + description.
 */
import { useState, useEffect, createContext, useContext, useMemo, useCallback, useRef } from 'react';
import * as api from '../api/tauri';
import type { RegistrySkill, SkillsData, LlmQuickConfig, SkillsI18nMap } from '../api/tauri';
import { useI18n } from '../hooks/useI18n';
import { useConfirm } from '../components/ConfirmDialog';

const SKILLS_INDEX_URL = 'https://echobird.ai/api/skills/index.json';

// Generated once per app launch — makes same-category order vary between sessions
const SESSION_SEED = (Math.random() * 0xffffffff) | 0;

// Category definition: display label + search keywords
interface CategoryDef {
    label: string;
    keywords: string[];
}

const INITIAL_CATEGORIES: CategoryDef[] = [
    { label: 'ALL', keywords: [] },
    { label: 'News', keywords: ['news', 'rss', 'feed', 'journalism', 'newsletter'] },
    { label: 'Search', keywords: ['search', 'find', 'query', 'lookup', 'index'] },
    { label: 'Tools', keywords: ['tool', 'utility', 'helper', 'script', 'automation'] },
    { label: 'Health', keywords: ['health', 'medical', 'fitness', 'wellness', 'healthcare', 'diet', 'nutrition'] },
    { label: 'Finance', keywords: ['finance', 'trading', 'crypto', 'stock', 'payment', 'banking'] },
    { label: 'Coding', keywords: ['code', 'coding', 'programming', 'developer', 'software', 'debug', 'refactor'] },
    { label: 'Marketing', keywords: ['marketing', 'seo', 'ads', 'campaign', 'social media', 'growth'] },
    { label: 'Game', keywords: ['game', 'gaming', 'gamedev', 'unity', 'unreal', 'godot', 'play'] },
    { label: 'Creative', keywords: ['creative', 'design', 'content', 'writing', 'video', 'music', 'art', 'image'] },
    { label: 'Productivity', keywords: ['productivity', 'task', 'calendar', 'assistant', 'email', 'crm', 'workflow'] },
    { label: 'Education', keywords: ['education', 'learn', 'study', 'teach', 'course', 'tutorial', 'homework'] },
    { label: 'Research', keywords: ['research', 'knowledge', 'academic', 'paper', 'science', 'analysis'] },
    { label: 'Language', keywords: ['translate', 'translation', 'language', 'i18n', 'localization', 'nlp', 'grammar', 'dictionary'] },
    { label: 'AI', keywords: ['ai', 'llm', 'gpt', 'claude', 'prompt', 'model', 'embedding', 'rag', 'finetune'] },
    { label: 'Web', keywords: ['web', 'frontend', 'react', 'vue', 'next', 'html', 'css', 'tailwind', 'browser'] },
    { label: 'DevOps', keywords: ['devops', 'deploy', 'docker', 'ci', 'cd', 'infrastructure', 'server', 'kubernetes'] },
    { label: 'Agent', keywords: ['agent', 'autonomous'] },
    { label: 'OpenClaw', keywords: ['openclaw', 'claw'] },
];

// Language display names for translation buttons
const LOCALE_NAMES: Record<string, string> = {
    en: 'English', zh: '中文', 'zh-Hans': '简体中文', 'zh-Hant': '繁體中文',
    ja: '日本語', ko: '한국어',
    de: 'Deutsch', fr: 'Français', es: 'Español', pt: 'Português',
    ru: 'Русский', ar: 'العربية', it: 'Italiano', nl: 'Nederlands',
    pl: 'Polski', tr: 'Türkçe', hi: 'हिन्दी', bn: 'বাংলা',
    th: 'ไทย', vi: 'Tiếng Việt', id: 'Bahasa Indonesia', ms: 'Bahasa Melayu',
    sv: 'Svenska', fi: 'Suomi', cs: 'Čeština', hu: 'Magyar',
    el: 'Ελληνικά', he: 'עברית', fa: 'فارسی',
};

// Category label i18n key map (categories not listed here keep their label as-is)
import type { TKey } from '../i18n/types';
const CAT_I18N: Record<string, TKey> = {
    'ALL': 'skills.catAll',
    'News': 'skills.catNews', 'Search': 'skills.catSearch', 'Tools': 'skills.catTools',
    'Health': 'skills.catHealth', 'Finance': 'skills.catFinance',
    'Coding': 'skills.catCoding', 'Marketing': 'skills.catMarketing', 'Game': 'skills.catGame',
    'Creative': 'skills.catCreative',
    'Productivity': 'skills.catProductivity',
    'Education': 'skills.catEducation', 'Research': 'skills.catResearch',
    'Language': 'skills.catLanguage',
};


// ===== Context =====
interface SkillBrowserCtx {
    filteredSkills: RegistrySkill[];
    totalCount: number;
    selectedSkill: RegistrySkill | null;
    setSelectedSkill: (v: RegistrySkill | null) => void;
    isLoading: boolean;
    searchQuery: string;
    setSearchQuery: (v: string) => void;
    lastUpdated?: string;
    locale: string;
    // Categories
    categories: CategoryDef[];
    activeCategory: string;
    setActiveCategory: (v: string) => void;
    addCategory: (name: string) => void;
    removeCategory: (label: string) => void;
    resetCategories: () => void;
    setCategories: (cats: CategoryDef[]) => void;
    persistCategories: (cats: CategoryDef[]) => void;
    // Favorites
    favorites: Set<string>;
    toggleFavorite: (skillId: string) => void;
    isFavorite: (skillId: string) => boolean;
    // AI i18n overlay
    i18nMap: SkillsI18nMap;
    saveI18n: (updates: SkillsI18nMap) => void;
}

const SkillBrowserContext = createContext<SkillBrowserCtx | null>(null);
const useSkillBrowser = () => {
    const ctx = useContext(SkillBrowserContext);
    if (!ctx) throw new Error('useSkillBrowser must be used within SkillBrowserProvider');
    return ctx;
};

// ===== Provider =====
interface SkillBrowserProviderProps {
    preloadedSkills?: import('../api/tauri').SkillsData | null;
    children: React.ReactNode;
}

export function SkillBrowserProvider({ preloadedSkills, children }: SkillBrowserProviderProps) {
    const { locale } = useI18n();
    const [allSkills, setAllSkills] = useState<RegistrySkill[]>(
        preloadedSkills?.skills ?? []
    );
    const [isLoading, setIsLoading] = useState(!preloadedSkills?.skills?.length);
    const [selectedSkill, setSelectedSkill] = useState<RegistrySkill | null>(null);
    const [searchQuery, setSearchQuery] = useState('');
    const [lastUpdated, setLastUpdated] = useState<string | undefined>(
        preloadedSkills?.lastUpdated
    );
    const [categories, setCategories] = useState<CategoryDef[]>(() => {
        if (preloadedSkills?.userCategories?.length) {
            const userCats: CategoryDef[] = preloadedSkills.userCategories.map(c =>
                typeof c === 'string' ? { label: c, keywords: [c.toLowerCase()] } : c as unknown as CategoryDef
            );
            return [...INITIAL_CATEGORIES, ...userCats];
        }
        return INITIAL_CATEGORIES;
    });
    const [activeCategory, setActiveCategory] = useState('ALL');
    const [favorites, setFavorites] = useState<Set<string>>(new Set());
    const [i18nMap, setI18nMap] = useState<SkillsI18nMap>({});

    // Load on mount: if preloaded, only load favorites + background refresh.
    // If NOT preloaded (cache miss), do full loadLocal.
    useEffect(() => {
        // Load i18n overlay
        api.loadSkillsI18n().then(setI18nMap).catch(() => { });

        if (preloadedSkills?.skills?.length) {
            // Already have skills — just load favorites + background refresh
            api.loadSkillsFavorites()
                .then(favData => {
                    if (favData.favorites?.length > 0) setFavorites(new Set(favData.favorites));
                })
                .catch(() => { });
            fetchAllSources();
        } else {
            loadLocal();
        }
    }, []);

    const loadLocal = async () => {
        setIsLoading(true);
        let gotLocalData = false;
        try {
            const data = await api.loadSkillsData();
            if (data.skills && data.skills.length > 0) {
                setAllSkills(data.skills);
                setLastUpdated(data.lastUpdated);
                gotLocalData = true;
                setIsLoading(false); // Unblock UI early — local cache ready
            }
            if (data.userCategories && data.userCategories.length > 0) {
                // Merge user categories: convert legacy string[] to CategoryDef[]
                const userCats: CategoryDef[] = data.userCategories.map(c =>
                    typeof c === 'string' ? { label: c, keywords: [c.toLowerCase()] } : c as unknown as CategoryDef
                );
                setCategories([...INITIAL_CATEGORIES, ...userCats]);
            }
            // Load favorites from separate file
            const favData = await api.loadSkillsFavorites();
            if (favData.favorites && favData.favorites.length > 0) {
                setFavorites(new Set(favData.favorites));
            }

        } catch (err) {
            console.error('[SkillBrowser] Failed to load local cache:', err);
        } finally {
            if (gotLocalData) {
                // Already unblocked above
            } else {
                // No local cache — keep isLoading=true while remote fetch runs
            }
        }
        // Sync with remote in background; setIsLoading(false) when done if not already done
        fetchAllSources().finally(() => { if (!gotLocalData) setIsLoading(false); });
    };

    const fetchAllSources = async () => {
        try {
            // 1. Read skill source index
            const indexRaw = await api.fetchSkillSource(SKILLS_INDEX_URL);
            const index = JSON.parse(indexRaw.replace(/^\uFEFF/, '')) as { sources: { id: string; name: string; url: string }[] };
            const urls = index.sources.map(s => s.url);

            // 2. Fetch all sources in parallel
            const results = await Promise.allSettled(
                urls.map(url => api.fetchSkillSource(url).then(raw => {
                    const parsed = JSON.parse(raw);
                    return (parsed.s || parsed.skills || []) as RegistrySkill[];
                }))
            );
            const merged: RegistrySkill[] = [];
            for (const r of results) {
                if (r.status === 'fulfilled') merged.push(...r.value);
            }
            if (merged.length > 0) {
                const now = new Date().toISOString();
                setAllSkills(merged);
                setLastUpdated(now);
                const cacheData: SkillsData = {
                    skills: merged,
                    userCategories: getUserCategories(categories),
                    lastUpdated: now,
                };
                await api.saveSkillsData(cacheData);
            }
        } catch (err) {
            console.error('[SkillBrowser] Failed to fetch skills:', err);
        }
    };

    // Save categories helper — only persist user-added categories
    const getUserCategories = (cats: CategoryDef[]) =>
        cats.filter(c => !INITIAL_CATEGORIES.some(ic => ic.label === c.label)).map(c => c.label);

    const persistCategories = async (updated: CategoryDef[]) => {
        try {
            const data = await api.loadSkillsData();
            data.userCategories = getUserCategories(updated);
            await api.saveSkillsData(data);
        } catch (err) {
            console.error('[SkillBrowser] Failed to save categories:', err);
        }
    };

    const addCategory = useCallback(async (name: string) => {
        const trimmed = name.trim();
        if (!trimmed) return;
        setCategories(prev => {
            if (prev.some(c => c.label === trimmed)) return prev;
            const newCat: CategoryDef = { label: trimmed, keywords: [trimmed.toLowerCase()] };
            const updated = [...prev, newCat];
            persistCategories(updated);
            return updated;
        });
    }, []);

    const removeCategory = useCallback(async (label: string) => {
        setCategories(prev => {
            const updated = prev.filter(c => c.label !== label);
            persistCategories(updated);
            return updated;
        });
        setActiveCategory(prev => prev === label ? 'ALL' : prev);
    }, []);

    const resetCategories = useCallback(async () => {
        setCategories([...INITIAL_CATEGORIES]);
        setActiveCategory('ALL');
        persistCategories([...INITIAL_CATEGORIES]);
    }, []);

    // Favorites management
    const toggleFavorite = useCallback((skillId: string) => {
        setFavorites(prev => {
            const next = new Set(prev);
            if (next.has(skillId)) next.delete(skillId);
            else next.add(skillId);
            // Persist outside updater (scheduled async, runs after state commit)
            queueMicrotask(() => {
                api.saveSkillsFavorites({ favorites: Array.from(next) }).catch(() => { });
            });
            return next;
        });
    }, []);

    const isFavorite = useCallback((skillId: string) => favorites.has(skillId), [favorites]);

    // Only trigger re-filter when favorites change in FAVORITES mode
    const favoritesKey = activeCategory === 'FAVORITES' ? favorites : null;

    // Apply i18n overlay: replace n/d if translation available for current locale
    const overlayedSkills = useMemo(() => {
        if (!Object.keys(i18nMap).length) return allSkills;
        return allSkills.map(s => {
            const tr = i18nMap[s.i];
            if (!tr || tr.locale !== locale) return s;
            return { ...s, n: tr.n || s.n, d: tr.d || s.d };
        });
    }, [allSkills, i18nMap, locale]);

    // Filter + sort
    const filteredSkills = useMemo(() => {
        let result = overlayedSkills;
        if (activeCategory === 'FAVORITES') {
            result = result.filter(s => favorites.has(s.i));
        } else if (activeCategory !== 'ALL') {
            const cat = categories.find(c => c.label === activeCategory);
            if (cat) {
                const kws = cat.keywords.map(k => k.toLowerCase());
                result = result.filter(s => {
                    const name = s.n.toLowerCase();
                    const desc = s.d.toLowerCase();
                    return kws.some(kw => name.includes(kw) || desc.includes(kw));
                });
            }
        }

        if (searchQuery.trim()) {
            const q = searchQuery.toLowerCase().trim();
            result = result.filter(s =>
                s.n.toLowerCase().includes(q) ||
                s.d.toLowerCase().includes(q)
            );
        }

        // Stable shuffle: use a seed derived from filter params so we only
        // re-shuffle when the filters actually change, not on every render.
        // SESSION_SEED is generated once per app launch so the same category
        // shows a different order on every session.
        const sorted = [...result];
        let seed = SESSION_SEED;
        const seedStr = `${activeCategory}-${searchQuery}`;
        for (let i = 0; i < seedStr.length; i++) seed = ((seed << 5) - seed + seedStr.charCodeAt(i)) | 0;
        const seededRandom = (s: number) => {
            s = Math.imul(s ^ (s >>> 16), 0x45d9f3b);
            s = Math.imul(s ^ (s >>> 13), 0x45d9f3b);
            return ((s ^ (s >>> 16)) >>> 0) / 0xffffffff;
        };
        for (let i = sorted.length - 1; i > 0; i--) {
            seed = (seed + i) | 0;
            const j = Math.floor(seededRandom(seed) * (i + 1));
            [sorted[i], sorted[j]] = [sorted[j], sorted[i]];
        }

        return sorted;
    }, [overlayedSkills, searchQuery, activeCategory, favoritesKey]);

    // Save i18n overlay: merge updates into existing map and persist
    const saveI18n = useCallback((updates: SkillsI18nMap) => {
        setI18nMap(prev => {
            const merged = { ...prev, ...updates };
            queueMicrotask(() => {
                api.saveSkillsI18n(merged).catch(e => console.error('[SkillBrowser] Failed to save i18n:', e));
            });
            return merged;
        });
    }, []);

    return (
        <SkillBrowserContext.Provider value={{
            filteredSkills,
            totalCount: allSkills.length,
            selectedSkill, setSelectedSkill,
            isLoading,
            searchQuery, setSearchQuery,
            lastUpdated,
            locale,
            categories, activeCategory, setActiveCategory,
            addCategory, removeCategory, resetCategories,
            setCategories, persistCategories,
            favorites, toggleFavorite, isFavorite,
            i18nMap, saveI18n,
        }}>
            {children}
        </SkillBrowserContext.Provider>
    );
}

// ===== Title Bar Actions =====
export function SkillBrowserSearch() {
    const { searchQuery, setSearchQuery } = useSkillBrowser();
    const { t } = useI18n();
    const inputRef = useRef<HTMLInputElement>(null);

    return (
        <div className="ml-auto flex-shrink-0 flex items-center gap-2">
            <div className="relative flex items-center">
                <input
                    ref={inputRef}
                    type="text"
                    value={searchQuery}
                    onChange={e => setSearchQuery(e.target.value)}
                    placeholder={t('search.skills')}
                    className="h-7 w-44 bg-black/60 border border-cyber-border px-3 pr-7 text-[11px] text-cyber-text placeholder-cyber-text-muted focus:border-cyber-warning focus:outline-none py-0"
                />
                {searchQuery && (
                    <button
                        onClick={() => { setSearchQuery(''); inputRef.current?.focus(); }}
                        className="absolute right-1.5 top-1/2 -translate-y-1/2 text-cyber-text-secondary hover:text-cyber-text text-xs"
                    >
                        ×
                    </button>
                )}
            </div>
        </div>
    );
}

// ===== Helper: get LLM config from first available model =====
// Triple-fallback: Anthropic first (explicit or derived /v1→/anthropic), then OpenAI
async function getLlmConfig(): Promise<LlmQuickConfig | null> {
    try {
        const models = await api.getModels();
        if (!models.length) return null;
        const m = models[0];

        // Derive Anthropic URL from OpenAI URL when possible (/v1 → /anthropic)
        const deriveAnthropicUrl = (base: string): string | null => {
            if (!base) return null;
            const stripped = base.trim().replace(/\/v1\/?$/, '');
            if (stripped !== base.trim()) return `${stripped}/anthropic`;
            return null;
        };

        const anthropicUrl = m.anthropicUrl || deriveAnthropicUrl(m.baseUrl || '') || null;
        return {
            provider: anthropicUrl ? 'anthropic' : 'openai',
            // When Anthropic is chosen, pass as primary base_url; OpenAI kept for fallback
            base_url: anthropicUrl ? anthropicUrl : (m.baseUrl || ''),
            api_key: m.apiKey,
            model: m.modelId || m.name,
            proxy_url: m.proxyUrl,
            // Pass OpenAI URL as fallback for backend to downgrade if Anthropic returns 400
            openai_fallback_url: anthropicUrl ? (m.baseUrl || '') : undefined,
        };
    } catch { return null; }
}

// ===== Title Bar: Translate List =====
export function SkillBrowserTranslateList() {
    const { t } = useI18n();
    const confirm = useConfirm();
    const { filteredSkills, saveI18n, locale, i18nMap } = useSkillBrowser();
    const [isTranslating, setIsTranslating] = useState(false);
    const langName = LOCALE_NAMES[locale] || locale;

    const handleTranslateList = async () => {
        if (isTranslating) return;
        const config = await getLlmConfig();
        if (!config) {
            await confirm({ title: t('skills.noModelTitle'), message: t('skills.noModelMsg'), type: 'warning', confirmText: t('common.confirm'), cancelText: '' });
            return;
        }

        setIsTranslating(true);
        try {
            // Skip already-translated skills, take first 50 untranslated
            const untranslated = filteredSkills.filter(s => {
                const tr = i18nMap[s.i];
                return !tr || tr.locale !== locale;
            });
            if (!untranslated.length) { setIsTranslating(false); return; }
            const batch = untranslated.slice(0, 50);
            const items = batch.map((s, i) => `${i}|${s.n}|${s.d.slice(0, 80)}`);
            const prompt = `Translate the following skill names and descriptions to ${langName}.\nInput format: index|name|description (one per line)\nOutput format: JSON array of objects [{"i":index,"n":"translated name","d":"translated description"}]\nOnly output the JSON array, nothing else.\n\n${items.join('\n')}`;

            const result = await api.llmQuickChat(config, prompt);
            // Parse JSON from response (might be wrapped in markdown code block)
            const jsonStr = result.replace(/```json?\n?/g, '').replace(/```/g, '').trim();
            const translations: Array<{ i: number; n: string; d: string }> = JSON.parse(jsonStr);

            // Save to i18n overlay
            const i18nUpdates: SkillsI18nMap = {};
            for (const tr of translations) {
                const skill = batch[tr.i];
                if (skill && (tr.n || tr.d)) {
                    i18nUpdates[skill.i] = { n: tr.n, d: tr.d, locale };
                }
            }
            // Preserve scroll position across i18n state update
            const scrollEl = document.querySelector('[data-skills-scroll]') as HTMLElement | null;
            const scrollTop = scrollEl?.scrollTop ?? 0;
            saveI18n(i18nUpdates);
            requestAnimationFrame(() => { if (scrollEl) scrollEl.scrollTop = scrollTop; });
        } catch (e) {
            console.error('[SkillBrowser] Translation failed:', e);
        } finally {
            setIsTranslating(false);
        }
    };

    return (
        <button
            onClick={handleTranslateList}
            disabled={isTranslating}
            className={`text-[11px] ml-2 ${isTranslating ? 'text-cyber-warning animate-pulse' : 'text-cyber-text-muted hover:text-cyber-warning'} disabled:opacity-40`}
        >
            {isTranslating ? `[${t('skills.translating')}]` : `[${t('skills.translateTo')} ${langName}]`}
        </button>
    );
}

// ===== Main Content (center area) =====
export function SkillBrowserMain() {
    const {
        filteredSkills, selectedSkill, setSelectedSkill,
        isLoading, searchQuery,
        categories, activeCategory, setActiveCategory,
        addCategory, removeCategory, resetCategories, isFavorite,
        setCategories, persistCategories,
    } = useSkillBrowser();
    const { t } = useI18n();
    const [showAddInput, setShowAddInput] = useState(false);
    const [newCatName, setNewCatName] = useState('');
    const addInputRef = useRef<HTMLInputElement>(null);
    const snapshotRef = useRef<CategoryDef[]>([]);
    const scrollContainerRef = useRef<HTMLDivElement>(null);
    const sentinelRef = useRef<HTMLDivElement>(null);

    // Infinite scroll — load 60 at a time
    const PAGE_SIZE = 60;
    const [displayCount, setDisplayCount] = useState(PAGE_SIZE);
    const visibleSkills = filteredSkills.slice(0, displayCount);
    const hasMore = displayCount < filteredSkills.length;

    // Reset display count AND scroll position when filter changes
    useEffect(() => {
        setDisplayCount(PAGE_SIZE);
        scrollContainerRef.current?.scrollTo(0, 0);
    }, [activeCategory, searchQuery]);

    // IntersectionObserver to load more — NO displayCount in deps to avoid
    // destroy-recreate loop that would cascade-load all 99K items.
    useEffect(() => {
        const sentinel = sentinelRef.current;
        const container = scrollContainerRef.current;
        if (!sentinel || !container) return;

        const observer = new IntersectionObserver(
            (entries) => {
                if (entries[0].isIntersecting) {
                    setDisplayCount(prev => Math.min(prev + PAGE_SIZE, filteredSkills.length));
                }
            },
            { root: container, rootMargin: '200px' }
        );
        observer.observe(sentinel);
        return () => observer.disconnect();
    }, [filteredSkills.length]);

    // Focus input when shown
    useEffect(() => {
        if (showAddInput) addInputRef.current?.focus();
    }, [showAddInput]);

    // Enter edit mode: save snapshot
    const enterEditMode = () => {
        snapshotRef.current = [...categories];
        setShowAddInput(true);
    };

    // Draft add (no persist)
    const draftAddCategory = () => {
        const trimmed = newCatName.trim();
        if (!trimmed || categories.some(c => c.label === trimmed)) return;
        const newCat: CategoryDef = { label: trimmed, keywords: [trimmed.toLowerCase()] };
        setCategories([...categories, newCat]);
        setNewCatName('');
    };

    // Draft remove (no persist)
    const draftRemoveCategory = (label: string) => {
        setCategories(categories.filter(c => c.label !== label));
        if (activeCategory === label) setActiveCategory('ALL');
    };

    // ✓ Commit all changes
    const commitChanges = () => {
        let final = categories;
        const trimmed = newCatName.trim();
        if (trimmed && !categories.some(c => c.label === trimmed)) {
            const newCat: CategoryDef = { label: trimmed, keywords: [trimmed.toLowerCase()] };
            final = [...categories, newCat];
            setCategories(final);
        }
        persistCategories(final);
        setShowAddInput(false);
        setNewCatName('');
    };

    // × Cancel / revert
    const cancelChanges = () => {
        setCategories(snapshotRef.current);
        setShowAddInput(false);
        setNewCatName('');
    };

    // ↺ Reset to defaults (stay in edit mode as draft)
    const draftReset = () => {
        setCategories([...INITIAL_CATEGORIES]);
        setActiveCategory('ALL');
    };

    return (
        <div className="flex flex-col h-full">
            {/* Category filter bar */}
            <div className="flex items-center gap-1.5 pt-3 pb-3 mb-3 flex-shrink-0 flex-wrap">
                {/* Category tabs */}
                {categories.map(cat => (
                    <button
                        key={cat.label}
                        onClick={() => setActiveCategory(cat.label)}
                        className={`relative h-7 px-3 text-xs flex items-center border ${activeCategory === cat.label
                            ? 'bg-cyber-warning text-black border-cyber-warning'
                            : 'border-cyber-border text-cyber-text-secondary hover:border-cyber-warning hover:text-cyber-warning'
                            }`}
                    >
                        {CAT_I18N[cat.label] ? t(CAT_I18N[cat.label]) : cat.label}
                        {showAddInput && cat.label !== 'ALL' && (
                            <span
                                onClick={(e) => { e.stopPropagation(); draftRemoveCategory(cat.label); }}
                                className={`ml-1.5 text-base leading-none ${activeCategory === cat.label ? 'text-black/60 hover:text-black' : 'text-cyber-text-muted hover:text-red-400'}`}
                            >
                                ×
                            </span>
                        )}
                    </button>
                ))}

                {/* Edit mode: add input + confirm + cancel + reset */}
                {showAddInput ? (
                    <div className="flex items-center gap-1">
                        <input
                            ref={addInputRef}
                            type="text"
                            value={newCatName}
                            onChange={e => setNewCatName(e.target.value)}
                            onKeyDown={e => {
                                if (e.key === 'Enter') draftAddCategory();
                                if (e.key === 'Escape') cancelChanges();
                            }}
                            placeholder={t('skills.keyword')}
                            className="h-7 w-36 bg-black/60 border border-cyber-warning px-2 text-xs text-cyber-text placeholder-cyber-text-muted focus:outline-none focus:border-cyber-warning"
                        />
                        <button
                            onClick={commitChanges}
                            className="h-7 w-7 flex items-center justify-center text-base border border-cyber-accent text-cyber-accent hover:bg-cyber-accent/10"
                        >
                            ✓
                        </button>
                        <button
                            onClick={cancelChanges}
                            className="h-7 w-7 flex items-center justify-center text-base border border-cyber-border text-cyber-text-muted hover:border-red-400 hover:text-red-400"
                        >
                            ×
                        </button>
                        <button
                            onClick={draftReset}
                            className="h-7 w-7 flex items-center justify-center text-base border border-cyber-border text-cyber-text-muted hover:border-cyber-warning hover:text-cyber-warning"

                        >
                            ↺
                        </button>
                    </div>
                ) : (
                    <button
                        onClick={enterEditMode}
                        className="h-7 w-7 flex items-center justify-center text-base border border-cyber-border text-cyber-text-muted hover:border-cyber-warning hover:text-cyber-warning"
                    >
                        ✎
                    </button>
                )}

                {/* Favorites tab */}
                <button
                    onClick={() => setActiveCategory(activeCategory === 'FAVORITES' ? 'ALL' : 'FAVORITES')}
                    className={`h-7 px-3 text-xs border flex items-center ${activeCategory === 'FAVORITES'
                        ? 'bg-cyber-warning text-black border-cyber-warning'
                        : 'border-cyber-border text-cyber-text-secondary hover:border-cyber-warning hover:text-cyber-warning'
                        }`}
                >
                    {t('skills.favorites')}
                </button>
            </div>

            {/* Skills list */}
            <div className="flex-1 overflow-y-auto" ref={scrollContainerRef} data-skills-scroll>
                <div className="grid grid-cols-1 md:grid-cols-2 gap-5">
                    {isLoading && filteredSkills.length === 0 ? (
                        /* Skeleton */
                        [...Array(8)].map((_, i) => (
                            <div
                                key={i}
                                className="p-3 border border-cyber-warning/20 bg-black/80 animate-pulse"
                            >
                                <div className="h-4 bg-cyber-warning/20 w-3/4 mb-2"></div>
                                <div className="h-3 bg-cyber-warning/10 w-1/2"></div>
                            </div>
                        ))
                    ) : filteredSkills.length === 0 ? (
                        /* Empty state */
                        <div className="col-span-2 text-center py-10 text-cyber-text-secondary">
                            {activeCategory === 'FAVORITES' ? t('mother.noFavorites') : t('skills.noMatch')}
                        </div>
                    ) : (
                        /* Skills grid */
                        visibleSkills.map((skill, idx) => (
                            <div
                                key={`${skill.i}-${idx}`}
                                className={`relative p-4 cyber-yellow-box ${selectedSkill === skill ? 'selected' : ''} cursor-pointer flex items-start justify-between group`}
                                onClick={() => setSelectedSkill(skill)}
                            >
                                {isFavorite(skill.i) && (
                                    <span className="absolute top-2 right-2 text-cyber-warning text-xl drop-shadow-[0_0_4px_rgba(250,204,21,0.6)]">★</span>
                                )}
                                <div className="min-w-0 flex-1">
                                    <div className={`text-lg font-bold truncate ${selectedSkill === skill ? 'text-cyber-warning' : 'text-cyber-text-secondary'}`}>
                                        {skill.n}
                                    </div>
                                    <div className="text-sm text-cyber-text-secondary mt-1 opacity-70 line-clamp-2">
                                        {skill.d}
                                    </div>
                                </div>
                            </div>
                        ))
                    )}
                    {/* Load more sentinel + skeleton */}
                    {hasMore && (
                        <>
                            <div ref={sentinelRef} className="col-span-2 h-1" />
                            {[...Array(4)].map((_, i) => (
                                <div
                                    key={`skeleton-${i}`}
                                    className="p-3 border border-cyber-warning/20 bg-black/80 animate-pulse"
                                >
                                    <div className="h-4 bg-cyber-warning/20 w-3/4 mb-2"></div>
                                    <div className="h-3 bg-cyber-warning/10 w-1/2"></div>
                                </div>
                            ))}
                        </>
                    )}
                </div>
            </div>
        </div>
    );
}

// ===== Right Panel =====
export function SkillBrowserPanel() {
    const { selectedSkill, setSelectedSkill, toggleFavorite, isFavorite, saveI18n, i18nMap, locale: ctxLocale } = useSkillBrowser();
    const { t, locale } = useI18n();
    const confirm = useConfirm();
    const [skillContent, setSkillContent] = useState<string>('');
    const [isLoadingContent, setIsLoadingContent] = useState(false);
    const [isTranslating, setIsTranslating] = useState(false);
    const [isFixing, setIsFixing] = useState(false);

    // Extract author name from GitHub path
    const authorName = selectedSkill ? selectedSkill.i.split('/')[0] : '';

    // Fetch SKILL.md content when selection changes
    useEffect(() => {
        if (!selectedSkill) {
            setSkillContent('');
            return;
        }
        let cancelled = false;
        setIsLoadingContent(true);
        setSkillContent('');

        const rawUrl = `https://raw.githubusercontent.com/${selectedSkill.i.split('/').slice(0, 2).join('/')}/${selectedSkill.b}/${selectedSkill.i.split('/').slice(2).join('/')}`;

        api.fetchSkillSource(rawUrl)
            .then(content => {
                if (!cancelled) setSkillContent(content);
            })
            .catch(() => {
                if (!cancelled) {
                    // Fallback: use saved expanded_d if GitHub content unavailable
                    const saved = i18nMap[selectedSkill.i]?.expanded_d;
                    setSkillContent(saved || '');
                }
            })
            .finally(() => {
                if (!cancelled) setIsLoadingContent(false);
            });

        return () => { cancelled = true; };
    }, [selectedSkill, i18nMap]);

    const handleTranslateDetail = async () => {
        if (!selectedSkill || isTranslating) return;
        const config = await getLlmConfig();
        if (!config) {
            await confirm({ title: t('skills.noModelTitle'), message: t('skills.noModelMsg'), type: 'warning', confirmText: t('common.confirm'), cancelText: '' });
            return;
        }

        setIsTranslating(true);
        try {
            const content = skillContent || selectedSkill.d;
            const prompt = `Translate the following skill information to ${LOCALE_NAMES[locale] || locale}.\n\nName: ${selectedSkill.n}\nDescription: ${selectedSkill.d}\n${skillContent ? `\nFull content:\n${content.slice(0, 2000)}` : ''}\n\nOutput JSON: {"n":"translated name","d":"translated description"${skillContent ? ',"content":"translated content summary (max 500 chars)"' : ''}}\nOnly output JSON, nothing else.`;

            const result = await api.llmQuickChat(config, prompt);
            const jsonStr = result.replace(/```json?\n?/g, '').replace(/```/g, '').trim();
            const tr = JSON.parse(jsonStr);

            // Detail translation: only store translated content in local state
            // Do NOT write n/d to i18nMap — that would interfere with list translations
            if (tr.content) setSkillContent(tr.content);
            else if (tr.d) setSkillContent(tr.d); // fallback if no full content returned
        } catch (e) {
            console.error('[SkillBrowser] Detail translation failed:', e);
        } finally {
            setIsTranslating(false);
        }
    };

    const handleFixContent = async () => {
        if (!selectedSkill || isFixing) return;
        const config = await getLlmConfig();
        if (!config) {
            await confirm({ title: t('skills.noModelTitle'), message: t('skills.noModelMsg'), type: 'warning', confirmText: t('common.confirm'), cancelText: '' });
            return;
        }

        setIsFixing(true);
        try {
            // Try to fetch real content for AI to analyze
            const rawUrl = `https://raw.githubusercontent.com/${selectedSkill.i.split('/').slice(0, 2).join('/')}/${selectedSkill.b}/${selectedSkill.i.split('/').slice(2).join('/')}`;
            let fetchedContent = '';
            let urlWorks = false;
            try {
                fetchedContent = await api.fetchSkillSource(rawUrl);
                urlWorks = true;
            } catch { /* URL broken or inaccessible */ }

            const isShortDesc = selectedSkill.d.length < 120;
            const prompt = `You are a skill metadata auditor. Analyze this skill entry and produce improvements.\n\nSkill data:\n- GitHub path (i): "${selectedSkill.i}"\n- Branch (b): "${selectedSkill.b || 'main'}"\n- Name (n): "${selectedSkill.n}"\n- Description (d): "${selectedSkill.d}"\n- Author: "${(selectedSkill as any).a || 'unknown'}"\n- URL accessible: ${urlWorks}\n${fetchedContent ? `\nFetched README/content (first 1500 chars):\n${fetchedContent.slice(0, 1500)}` : '\nContent could not be fetched (URL broken or private).'}\n\nTasks:\n1. Fix broken or incorrect GitHub path / branch (set i/b fields if needed)\n2. Fix missing or wrong author (set a field if needed)\n3. Fix name if inaccurate (set n field if needed)\n4. ALWAYS write an "expanded_d": a rich, detailed description in 3-5 sentences covering: what this skill does, key use cases, and how to use it. Use the README content if available, otherwise infer from the name and current description. Minimum 150 characters.\n5. Also set "d" only if the current one is wrong/truncated (< 80 chars or clearly cut off), otherwise leave "d" empty.\n\nOutput ONLY JSON (no markdown, no explanation):\n{"i":"fixed path or empty","b":"fixed branch or empty","n":"fixed name or empty","d":"short one-line summary if needs fixing, else empty","expanded_d":"always a detailed 3-5 sentence description","a":"fixed author or empty","ok":true/false}\n\nSet "ok":true only if path/author/name are all already correct (expanded_d is always required).`;

            const result = await api.llmQuickChat(config, prompt);
            const jsonStr = result.replace(/```json?\n?/g, '').replace(/```/g, '').trim();
            const fix = JSON.parse(jsonStr);

            if (fix.ok && !fix.i && !fix.n && !fix.d && !fix.a && !fix.expanded_d) {
                await confirm({ title: t('skills.fixOk'), type: 'normal', confirmText: t('common.confirm'), cancelText: '' });
                setIsFixing(false);
                return;
            }

            // Apply structural fixes to skill
            const updatedSkill = {
                ...selectedSkill,
                ...(fix.i ? { i: fix.i } : {}),
                ...(fix.b ? { b: fix.b } : {}),
                ...(fix.n ? { n: fix.n } : {}),
                ...(fix.d ? { d: fix.d } : {}),
            };
            setSelectedSkill(updatedSkill);

            // Always display expanded description in detail panel
            if (fix.expanded_d) setSkillContent(fix.expanded_d);

            // Save to i18n overlay: short d (if fixed) + expanded detail
            saveI18n({
                [selectedSkill.i]: {
                    ...i18nMap[selectedSkill.i],
                    locale,
                    ...(fix.n ? { n: fix.n } : {}),
                    ...(fix.d ? { d: fix.d } : {}),
                    ...(fix.expanded_d ? { expanded_d: fix.expanded_d } : {}),
                }
            });

            // Re-fetch content if path changed
            if (fix.i) {
                setIsLoadingContent(true);
                const newRawUrl = `https://raw.githubusercontent.com/${fix.i.split('/').slice(0, 2).join('/')}/${fix.b || selectedSkill.b}/${fix.i.split('/').slice(2).join('/')}`;
                try {
                    const content = await api.fetchSkillSource(newRawUrl);
                    setSkillContent(content);
                } catch { setSkillContent(''); }
                setIsLoadingContent(false);
            } else if (fetchedContent) {
                setSkillContent(fetchedContent);
            }

        } catch (e) {
            console.error('[SkillBrowser] Fix failed:', e);
            await confirm({ title: t('skills.fixFailed'), type: 'warning', confirmText: t('common.confirm'), cancelText: '' });
        } finally {
            setIsFixing(false);
        }
    };

    return (
        <>
            <div className="px-4 pt-0.5 pb-3 text-sm flex items-center justify-between bg-transparent">
                <span className="text-cyber-warning">{t('skills.details')}</span>
                <button
                    onClick={handleTranslateDetail}
                    disabled={isTranslating || !selectedSkill}
                    className={`text-[11px] ${isTranslating ? 'text-cyber-warning animate-pulse' : 'text-cyber-text-muted hover:text-cyber-warning'} disabled:opacity-40`}
                >
                    {isTranslating ? `[${t('skills.translating')}]` : `[${t('skills.translateTo')} ${LOCALE_NAMES[locale] || locale}]`}
                </button>
            </div>
            <div className="flex-1 p-4 overflow-y-auto">
                {selectedSkill ? (
                    <div className="space-y-4">
                        <div>
                            <h3 className="text-lg font-bold text-cyber-warning mb-2">{selectedSkill.n}</h3>
                            <div className="text-xs space-y-1 text-cyber-text-secondary">
                                <div>{t('skills.author')}: <span className="text-cyber-text">{authorName}</span></div>
                            </div>
                        </div>
                        <div className="border-t border-cyber-border pt-4">
                            <div className="text-xs text-cyber-text-secondary mb-2 flex items-center justify-between">
                                {t('skills.description')}
                                <button
                                    onClick={handleFixContent}
                                    disabled={isFixing || !selectedSkill}
                                    className={`text-[11px] ${isFixing ? 'text-cyber-warning animate-pulse' : 'text-cyber-text-muted hover:text-cyber-warning'} disabled:opacity-40`}
                                >
                                    {isFixing ? `[${t('skills.fixing')}]` : `[${t('skills.fixContent')}]`}
                                </button>
                            </div>
                            {isLoadingContent ? (
                                <div className="space-y-2 animate-pulse">
                                    <div className="h-3 bg-cyber-warning/15 w-full"></div>
                                    <div className="h-3 bg-cyber-warning/10 w-4/5"></div>
                                    <div className="h-3 bg-cyber-warning/10 w-3/5"></div>
                                </div>
                            ) : skillContent ? (
                                <pre className="text-xs text-cyber-text leading-relaxed whitespace-pre-wrap font-mono break-words">
                                    {skillContent}
                                </pre>
                            ) : (
                                <p className="text-sm text-cyber-text leading-relaxed whitespace-pre-wrap">
                                    {selectedSkill.d || t('skills.noDescription')}
                                </p>
                            )}
                        </div>
                    </div>
                ) : (
                    <p className="text-cyber-text-secondary text-center py-10">
                        {t('skills.selectToView')}
                    </p>
                )}
            </div>
            {selectedSkill && (
                <div className="p-4 pt-0">
                    <button
                        onClick={() => toggleFavorite(selectedSkill.i)}
                        className={`w-full py-2 text-sm font-bold flex items-center justify-center gap-2 rounded-button ${isFavorite(selectedSkill.i)
                            ? 'border border-cyber-warning/50 text-cyber-warning hover:bg-cyber-warning/10'
                            : 'bg-cyber-warning text-black hover:bg-cyber-warning/80 shadow-[0_0_15px_rgba(250,204,21,0.2)]'
                            }`}
                    >
                        {isFavorite(selectedSkill.i) ? `✕ ${t('skills.removeFavorite')}` : `☆ ${t('skills.addFavorite')}`}
                    </button>
                </div>
            )}
        </>
    );
}
