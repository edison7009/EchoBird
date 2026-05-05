// AI Courses — Curated public AI/ML courses, sourced from a maintained awesome-list.
// Source: dair-ai/ML-YouTube-Courses (17k★, ~80 courses, 10 categories, actively maintained).
// Mirror chain: echobird.ai/courses/README.md → CF Worker → GitHub raw fallback.
//
// Each card click opens the course URL in the system browser.
// Right panel filters by category (parsed from the README TOC).

import { createContext, useContext, useState, useEffect, useCallback, useMemo, useRef } from 'react';
import { open as shellOpen } from '@tauri-apps/plugin-shell';
import { ExternalLink, RefreshCw } from 'lucide-react';
import { useI18n } from '../../hooks/useI18n';

// ===== Mirror config =====

const COURSES_MIRRORS: { name: string; base: string }[] = [
    { name: 'echobird', base: 'https://echobird.ai/courses' },
    { name: 'github',   base: 'https://raw.githubusercontent.com/dair-ai/ML-YouTube-Courses/main' },
];

const COURSES_FILE = 'README.md';

// ===== Types =====

type Lang = 'zh' | 'en';

interface Course {
    id: string;
    name: string;
    url: string;
    description: string;
    category: string;
    lang: Lang;
}

// Persisted shape — only the upstream-fetched EN data. CN supplement is sourced
// directly from CN_COURSES / CN_CATEGORIES at render time so editing the array
// shows up immediately without waiting for cache expiry.
interface CachedEn {
    enCourses: Course[];
    enCategories: string[];
    fetchedAt: number;
}

// Runtime shape exposed via context — EN cache merged with CN supplement.
interface Catalog {
    courses: Course[];
    categoriesByLang: Record<Lang, string[]>;
    fetchedAt: number;
}

// ===== Curated CN supplement =====
//
// dair-ai/ML-YouTube-Courses is 100% YouTube — blocked in mainland China. We bundle
// a small list of well-known CN-accessible courses (Bilibili / 中国大学MOOC / 学堂在线)
// as a permanent supplement. These are picks that don't churn (canonical re-uploads
// of evergreen courses by famous teachers).
const CN_COURSES: Course[] = [
    {
        id: 'cn-lihongyi-ml',
        name: '李宏毅 · 机器学习',
        url: 'https://www.bilibili.com/video/BV1Wv411h7kN',
        description: '台大李宏毅教授,中文圈最受欢迎的机器学习课程,涵盖 ML / DL / 强化学习 / Transformer / Diffusion',
        category: '机器学习',
        lang: 'zh',
    },
    {
        id: 'cn-andrew-ng-ml',
        name: '吴恩达 · 机器学习(中文字幕)',
        url: 'https://www.bilibili.com/video/BV1Bq421A74G',
        description: '斯坦福 CS229 经典机器学习课程,B站官方中文字幕版',
        category: '机器学习',
        lang: 'zh',
    },
    {
        id: 'cn-andrew-ng-dl',
        name: '吴恩达 · 深度学习专项课(中文)',
        url: 'https://www.bilibili.com/video/BV1FT4y1E74V',
        description: '深度学习五部曲:神经网络与深度学习 / 改善深层 NN / 结构化机器学习项目 / CNN / 序列模型',
        category: '深度学习',
        lang: 'zh',
    },
    {
        id: 'cn-limu-d2l',
        name: '李沐 · 动手学深度学习',
        url: 'https://courses.d2l.ai/zh-v2/',
        description: 'Amazon 资深首席科学家李沐主讲,中文 PyTorch 实现,教材免费在线阅读',
        category: '深度学习',
        lang: 'zh',
    },
    {
        id: 'cn-limu-bilibili',
        name: '跟李沐学 AI · B 站频道',
        url: 'https://space.bilibili.com/1567748478',
        description: '李沐持续更新的论文精读、深度学习、面试经验等系列视频',
        category: '深度学习',
        lang: 'zh',
    },
    {
        id: 'cn-tsinghua-nlp',
        name: '清华 NLP · 刘知远',
        url: 'https://www.bilibili.com/video/BV1Pe4y1B72v',
        description: '清华大学刘知远团队主讲的自然语言处理课程,从词向量到大模型',
        category: '自然语言处理',
        lang: 'zh',
    },
    {
        id: 'cn-zju-ml-huhaoji',
        name: '浙大 · 胡浩基机器学习',
        url: 'https://www.bilibili.com/video/BV1qf4y1x7kB',
        description: '浙江大学胡浩基副教授主讲,理论推导清晰,适合数学基础扎实的学习者',
        category: '机器学习',
        lang: 'zh',
    },
    {
        id: 'cn-cs231n-zh',
        name: '斯坦福 CS231n · 中文翻译',
        url: 'https://www.bilibili.com/video/BV1nJ411z7fe',
        description: '李飞飞团队的计算机视觉名课,B 站中文翻译版',
        category: '计算机视觉',
        lang: 'zh',
    },
    {
        id: 'cn-llm-tutorial',
        name: 'LLM 大模型从入门到实战',
        url: 'https://www.bilibili.com/video/BV1iz421h7Kv',
        description: '社区维护的大模型实战课,涵盖 RAG / 微调 / Agent / Prompt Engineering',
        category: '大模型',
        lang: 'zh',
    },
    // ── 学习平台 (gateway entries — let the platform itself handle search & nav) ──
    {
        id: 'cn-platform-atomgit',
        name: 'AtomGit AI Hub · 学习中心',
        url: 'https://ai.atomgit.com/learn',
        description: 'GitCode 旗下 AI 学习平台,集中各类学习路径与实战教程',
        category: '学习平台',
        lang: 'zh',
    },
    {
        id: 'cn-platform-aistudio',
        name: '飞桨 AI Studio',
        url: 'https://aistudio.baidu.com/learn',
        description: '百度飞桨学习平台,大量免费公开课与可在线运行的实战项目,自带算力',
        category: '学习平台',
        lang: 'zh',
    },
    {
        id: 'cn-platform-modelscope',
        name: 'ModelScope · 魔搭学习',
        url: 'https://www.modelscope.cn/learn',
        description: '阿里达摩院出品,模型 / 课程 / 数据集一体化的中文 AI 社区',
        category: '学习平台',
        lang: 'zh',
    },
    {
        id: 'cn-platform-datawhale',
        name: 'Datawhale 开源学习社区',
        url: 'https://datawhale.cn/',
        description: '国内最活跃的 AI 开源学习社区,主打开源教程 / 组队学习 / 入门到进阶',
        category: '学习平台',
        lang: 'zh',
    },
    {
        id: 'cn-platform-openbayes',
        name: 'OpenBayes 公开教程',
        url: 'https://openbayes.com/console/public/tutorials',
        description: '算力平台开放的公开 Notebook 教程,可一键运行',
        category: '学习平台',
        lang: 'zh',
    },
    {
        id: 'cn-platform-geektime',
        name: '极客时间 · AI 专栏',
        url: 'https://time.geekbang.org/category/intel-100',
        description: 'AI / ML / 大模型方向收费专栏,内容深度与体系化程度高',
        category: '学习平台',
        lang: 'zh',
    },
    {
        id: 'cn-platform-coursera-cn',
        name: 'Coursera 中文站',
        url: 'https://www.coursera.org/zh-CN',
        description: '国际公开课中文界面入口,大量斯坦福 / DeepLearning.AI 课程有中文字幕',
        category: '学习平台',
        lang: 'zh',
    },
    {
        id: 'cn-platform-icourse163',
        name: '中国大学 MOOC',
        url: 'https://www.icourse163.org/search.htm?search=人工智能',
        description: '清华 / 北大 / 浙大 / 中科大 / 复旦等顶尖高校 AI 公开课聚合入口',
        category: '学习平台',
        lang: 'zh',
    },
    {
        id: 'cn-platform-xuetangx',
        name: '学堂在线',
        url: 'https://www.xuetangx.com/search?query=人工智能',
        description: '清华出品 MOOC 平台,聚合国内顶尖高校 AI 课程',
        category: '学习平台',
        lang: 'zh',
    },
];
const CN_CATEGORIES = ['学习平台', '机器学习', '深度学习', '自然语言处理', '计算机视觉', '大模型'];

// ===== Local cache =====

// New cache key (`:en`) so any old persisted blob from prior schema versions
// is naturally ignored — no migration code needed.
const CACHE_KEY = 'courses:cache:en';
const REFRESH_AFTER_MS = 6 * 3600 * 1000;

const loadCachedEn = (): CachedEn | null => {
    try {
        const raw = localStorage.getItem(CACHE_KEY);
        if (!raw) return null;
        const parsed = JSON.parse(raw);
        if (!Array.isArray(parsed?.enCourses) || !Array.isArray(parsed?.enCategories)) return null;
        return parsed as CachedEn;
    } catch { return null; }
};
const saveCachedEn = (c: CachedEn) => {
    try { localStorage.setItem(CACHE_KEY, JSON.stringify(c)); } catch { /* quota */ }
};

// Build the runtime catalog by merging the fetched EN cache with the always-fresh CN supplement.
const buildCatalog = (en: CachedEn | null): Catalog => ({
    courses:          [...(en?.enCourses    || []), ...CN_COURSES],
    categoriesByLang: { en: en?.enCategories || [], zh: CN_CATEGORIES },
    fetchedAt:        en?.fetchedAt || Date.now(),
});

// ===== Network: mirror-aware fetch =====

let preferredMirror = 0;

const looksLikeHtml = (s: string): boolean => {
    const head = s.slice(0, 200).trimStart().toLowerCase();
    return head.startsWith('<!doctype html') || head.startsWith('<html');
};

async function fetchReadme(): Promise<string> {
    const order = [
        ...COURSES_MIRRORS.slice(preferredMirror),
        ...COURSES_MIRRORS.slice(0, preferredMirror),
    ];
    let lastErr: any = null;
    for (let i = 0; i < order.length; i++) {
        const mirror = order[i];
        try {
            const res = await fetch(`${mirror.base}/${COURSES_FILE}`, { cache: 'no-cache' });
            if (!res.ok) { lastErr = new Error(`${mirror.name} ${res.status}`); continue; }
            const text = await res.text();
            if (looksLikeHtml(text)) { lastErr = new Error(`${mirror.name} returned HTML`); continue; }
            preferredMirror = (preferredMirror + i) % COURSES_MIRRORS.length;
            return text;
        } catch (e) { lastErr = e; }
    }
    throw lastErr || new Error('all mirrors failed');
}

// ===== Markdown parser =====
//
// dair-ai/ML-YouTube-Courses layout:
//
//   # 📺 ML YouTube Courses
//   <intro paragraph>
//
//   **Machine Learning**            ← category header (TOC)
//   - [Stanford CS229](#anchor)     ← TOC entry
//   - ...
//
//   **Deep Learning**
//   - ...
//
//   ## Stanford CS229: Machine Learning   ← course body
//   <description bullets>
//   🔗 [Link to Course](https://...)
//
// We parse the TOC to map course-name → category, then walk the course bodies.

const slugify = (s: string) =>
    s.toLowerCase().replace(/[^\w一-鿿]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 80);

function parseDairAi(md: string): { courses: Course[]; categories: string[] } {
    const lines = md.split('\n');
    const categoryByCourse = new Map<string, string>();
    const seenCategories: string[] = [];

    // Phase 1: TOC scan — runs until the first `## ` body section.
    let inToc = true;
    let curCat = '';
    for (const line of lines) {
        if (line.startsWith('## ')) { inToc = false; break; }
        if (!inToc) continue;
        const catMatch = line.match(/^\s*\*\*([^*]+)\*\*\s*$/);
        if (catMatch) {
            curCat = catMatch[1].trim();
            if (!seenCategories.includes(curCat)) seenCategories.push(curCat);
            continue;
        }
        const linkMatch = line.match(/^\s*-\s*\[([^\]]+)\]\(#([^)]+)\)/);
        if (linkMatch && curCat) {
            categoryByCourse.set(linkMatch[1].trim(), curCat);
        }
    }

    // Phase 2: course bodies
    const courses: Course[] = [];
    let curName = '';
    let curBullets: string[] = [];
    let curUrl = '';

    const flush = () => {
        if (curName && curUrl) {
            // Cap description: first ~3 short bullets joined
            const desc = curBullets
                .slice(0, 3)
                .map(b => b.replace(/^\s*[-•*]\s*/, '').trim())
                .filter(Boolean)
                .join(' · ');
            courses.push({
                id: slugify(curName),
                name: curName,
                url: curUrl,
                description: desc,
                category: categoryByCourse.get(curName) || 'Others',
                lang: 'en',
            });
        }
        curName = '';
        curBullets = [];
        curUrl = '';
    };

    for (const line of lines) {
        if (line.startsWith('## ')) {
            flush();
            curName = line.slice(3).trim();
            continue;
        }
        if (!curName) continue;
        const linkMatch = line.match(/🔗\s*\[Link to Course\]\(([^)]+)\)/);
        if (linkMatch) { curUrl = linkMatch[1].trim(); continue; }
        const bulletMatch = line.match(/^\s*[-*]\s+(.+)/);
        if (bulletMatch) {
            const text = bulletMatch[1].trim();
            // Skip TOC-style anchor links that may appear inside a section
            if (!/^\[.+\]\(#/.test(text)) curBullets.push(text);
        }
    }
    flush();

    return { courses, categories: seenCategories };
}

// ===== Helpers =====

const openExternal = (url: string) => shellOpen(url).catch(() => window.open(url, '_blank'));
const hostnameOf = (url: string): string => {
    try { return new URL(url).hostname.replace(/^www\./, ''); }
    catch { return url; }
};

// ===== Context =====

interface AiCoursesContextValue {
    catalog: Catalog;                    // always populated (CN supplement is the floor)
    initialLoading: boolean;
    syncing: boolean;
    error: string | null;
    selectedCategory: string;
    setSelectedCategory: (c: string) => void;
    retry: () => void;
}

const AiCoursesContext = createContext<AiCoursesContextValue | null>(null);

function useAiCourses() {
    const ctx = useContext(AiCoursesContext);
    if (!ctx) throw new Error('AiCourses context missing');
    return ctx;
}

// ===== Provider =====

export function AiCoursesProvider({ children }: { children: React.ReactNode }) {
    // Hydrate the EN cache once on mount so re-renders don't re-read localStorage.
    const initialCachedEn = useMemo(() => loadCachedEn(), []);
    const [cachedEn, setCachedEn] = useState<CachedEn | null>(initialCachedEn);
    const [initialLoading, setInitialLoading] = useState(initialCachedEn === null);
    const [syncing, setSyncing] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [selectedCategory, setSelectedCategory] = useState<string>('all');
    const seq = useRef(0);
    const cacheRef = useRef(cachedEn);
    useEffect(() => { cacheRef.current = cachedEn; }, [cachedEn]);

    // Catalog is computed: EN cache + CN supplement. Recomputes when cache changes,
    // so editing CN_COURSES in source ALWAYS reflects on next mount.
    const catalog = useMemo(() => buildCatalog(cachedEn), [cachedEn]);

    const sync = useCallback(async (force = false) => {
        const cur = cacheRef.current;
        if (!force && cur && Date.now() - cur.fetchedAt < REFRESH_AFTER_MS) {
            setInitialLoading(false);
            return;
        }
        const my = ++seq.current;
        setSyncing(true);
        setError(null);
        try {
            const md = await fetchReadme();
            if (my !== seq.current) return;
            const parsed = parseDairAi(md);
            const fresh: CachedEn = {
                enCourses:    parsed.courses,
                enCategories: parsed.categories,
                fetchedAt:    Date.now(),
            };
            saveCachedEn(fresh);
            setCachedEn(fresh);
        } catch (e: any) {
            if (my !== seq.current) return;
            // Network down — buildCatalog still surfaces the CN supplement, so users see something.
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

    const value = useMemo<AiCoursesContextValue>(() => ({
        catalog, initialLoading, syncing, error, selectedCategory, setSelectedCategory, retry,
    }), [catalog, initialLoading, syncing, error, selectedCategory, retry]);

    return <AiCoursesContext.Provider value={value}>{children}</AiCoursesContext.Provider>;
}

// ===== Title actions =====

export function AiCoursesTitleActions() {
    const { t } = useI18n();
    const { syncing, retry } = useAiCourses();
    return (
        <div className="ml-auto flex-shrink-0 flex items-center gap-2">
            <button
                onClick={retry}
                disabled={syncing}
                title={t('btn.refresh')}
                className={`text-xs font-mono px-2 py-1 border rounded transition-colors flex items-center gap-1.5 ${!syncing
                    ? 'border-cyber-border/50 text-cyber-text hover:bg-cyber-text/10'
                    : 'border-cyber-border text-cyber-text-muted cursor-not-allowed'}`}
            >
                <RefreshCw size={12} className={syncing ? 'animate-spin' : ''} />
                {t('btn.refresh')}
            </button>
        </div>
    );
}

// ===== Card =====

function CourseCard({ course }: { course: Course }) {
    return (
        <button
            onClick={() => openExternal(course.url)}
            className="group w-full text-left bg-cyber-surface rounded-card border border-cyber-border/15 hover:border-cyber-border/40 hover:bg-cyber-elevated transition-colors p-4 flex flex-col h-full"
        >
            <div className="text-[10px] font-mono text-cyber-text-secondary uppercase tracking-wider mb-1.5 truncate">
                {course.category}
            </div>
            <div className="text-base font-bold text-cyber-text leading-snug mb-3 group-hover:text-cyber-accent transition-colors line-clamp-2">
                {course.name}
            </div>

            {course.description && (
                <div className="text-xs text-cyber-text-secondary leading-relaxed flex-1 line-clamp-3">
                    {course.description}
                </div>
            )}

            <div className="mt-3 pt-2 border-t border-cyber-border/10 flex items-center gap-2">
                <span className="text-[10px] font-mono text-cyber-text-muted/60 truncate flex-1">
                    {hostnameOf(course.url)}
                </span>
                <ExternalLink size={11} className="text-cyber-text-muted/40 group-hover:text-cyber-text transition-colors" />
            </div>
        </button>
    );
}

// ===== Main =====

export function AiCoursesMain() {
    const { t, locale } = useI18n();
    const { catalog, initialLoading, syncing, error, selectedCategory, retry } = useAiCourses();
    const lang: Lang = locale.startsWith('zh') ? 'zh' : 'en';

    const visible = useMemo(() => {
        const langMatched = catalog.courses.filter(c => c.lang === lang);
        if (selectedCategory === 'all') return langMatched;
        return langMatched.filter(c => c.category === selectedCategory);
    }, [catalog, selectedCategory, lang]);

    if ((initialLoading || syncing) && visible.length === 0) {
        return (
            <div className="flex-1 overflow-y-auto pb-4">
                <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-3">
                    {Array.from({ length: 9 }).map((_, i) => (
                        <div key={i} className="p-4 bg-cyber-surface rounded-card animate-pulse h-32">
                            <div className="h-3 w-20 bg-cyber-border/40 rounded mb-2" />
                            <div className="h-4 w-3/4 bg-cyber-border/50 rounded mb-3" />
                            <div className="h-3 w-full bg-cyber-border/30 rounded mb-2" />
                            <div className="h-3 w-2/3 bg-cyber-border/30 rounded" />
                        </div>
                    ))}
                </div>
            </div>
        );
    }

    if (error && visible.length === 0) {
        return (
            <div className="flex-1 overflow-y-auto pb-4">
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
            </div>
        );
    }

    return (
        <div className="flex-1 overflow-y-auto pb-4">
            {/* Reserved 2px slot — opacity toggle prevents layout shift on sync start/stop */}
            <div className="sticky top-0 z-20 h-0.5 overflow-hidden pointer-events-none mb-2">
                <div
                    className={`h-full w-1/3 bg-cyber-accent/70 transition-opacity duration-150 ${
                        syncing ? 'opacity-100 animate-[loading_1.2s_ease-in-out_infinite]' : 'opacity-0'
                    }`}
                />
            </div>
            <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-3">
                {visible.map(c => <CourseCard key={c.id} course={c} />)}
            </div>
        </div>
    );
}

// ===== Right panel: category filter =====

export function AiCoursesPanel() {
    const { t, locale } = useI18n();
    const { catalog, selectedCategory, setSelectedCategory } = useAiCourses();
    const lang: Lang = locale.startsWith('zh') ? 'zh' : 'en';

    // Reset filter when categories of the current lang don't include the selection
    useEffect(() => {
        if (selectedCategory === 'all') return;
        const cats = catalog.categoriesByLang[lang] || [];
        if (!cats.includes(selectedCategory)) setSelectedCategory('all');
    }, [lang, catalog, selectedCategory, setSelectedCategory]);

    const categories = catalog.categoriesByLang[lang] || [];
    const langCourses = useMemo(
        () => catalog.courses.filter(c => c.lang === lang),
        [catalog, lang]
    );
    const total = langCourses.length;
    const countByCat = useMemo(() => {
        const m = new Map<string, number>();
        for (const c of langCourses) m.set(c.category, (m.get(c.category) || 0) + 1);
        return m;
    }, [langCourses]);

    return (
        <>
            <div className="p-2 flex items-center justify-between bg-transparent">
                <div className="text-xs font-bold text-cyber-text-secondary px-2">{t('courses.filter')}</div>
                {total > 0 && (
                    <span className="text-[10px] font-mono text-cyber-text-muted/60">{total}</span>
                )}
            </div>
            <div className="flex-1 px-2 overflow-y-auto pb-4 space-y-1">
                <button
                    onClick={() => setSelectedCategory('all')}
                    className={`w-full text-left px-3 py-2 rounded text-xs font-mono transition-colors flex items-center justify-between ${
                        selectedCategory === 'all'
                            ? 'bg-cyber-elevated text-cyber-text'
                            : 'text-cyber-text-secondary hover:bg-cyber-surface hover:text-cyber-text'
                    }`}
                >
                    <span>{t('courses.cat.all')}</span>
                    <span className="text-[10px] text-cyber-text-muted/60">{total}</span>
                </button>
                {categories.map(cat => (
                    <button
                        key={cat}
                        onClick={() => setSelectedCategory(cat)}
                        className={`w-full text-left px-3 py-2 rounded text-xs font-mono transition-colors flex items-center justify-between ${
                            selectedCategory === cat
                                ? 'bg-cyber-elevated text-cyber-text'
                                : 'text-cyber-text-secondary hover:bg-cyber-surface hover:text-cyber-text'
                        }`}
                    >
                        <span className="truncate">{cat}</span>
                        <span className="text-[10px] text-cyber-text-muted/60 ml-2">{countByCat.get(cat) || 0}</span>
                    </button>
                ))}
            </div>
        </>
    );
}
