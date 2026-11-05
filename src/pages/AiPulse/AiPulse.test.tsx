import React from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-shell';
import { AiNewsMain, AiPulsePanel, AiPulseProvider, AiPulseTitleActions } from './AiPulse';
import { fetchFeed, type NewsItem, type RawFeed } from './feed';

let locale = 'zh-Hans';
vi.mock('../../hooks/useI18n', () => ({
  useI18n: () => ({ locale, t: (key: string) => key }),
}));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/plugin-shell', () => ({ open: vi.fn().mockResolvedValue(undefined) }));
vi.mock('./feed', async (importOriginal) => ({
  ...(await importOriginal<typeof import('./feed')>()),
  fetchFeed: vi.fn(),
}));

const news = (id: string, title: string, day = '14'): NewsItem => ({
  id,
  title,
  source: 'AI Lab',
  url: `https://example.com/${id}`,
  published_at: new Date(2026, 9, Number(day), 12).toISOString(),
});
const chinese = news('zh', '中文 AI 资讯');
const english = news('en', 'Global AI news');
const older = news('older', '昨日 AI 资讯', '13');
const project = { ...news('project', 'AI 开源项目'), url: 'https://github.com/example/ai' };
const feed = (items: NewsItem[]): RawFeed => ({
  generated_at: '2026-10-14T03:00:00Z',
  window_hours: 168,
  total_items: items.length,
  items,
});
let renderer: ReactTestRenderer;
let archive: Record<string, NewsItem[]>;
let storage: Map<string, string>;

function Harness({ active = true }) {
  return (
    <AiPulseProvider active={active}>
      <AiPulseTitleActions />
      <AiNewsMain />
      <AiPulsePanel />
    </AiPulseProvider>
  );
}

function text(node: ReactTestInstance): string {
  return node.children.map((child) => (typeof child === 'string' ? child : text(child))).join('');
}
function button(label: string) {
  const result = renderer.root.findAllByType('button').find((node) => text(node) === label);
  if (!result) throw new Error(`Missing button: ${label}`);
  return result;
}
const markup = () => JSON.stringify(renderer.toJSON());
async function mount(active = true) {
  await act(async () => {
    renderer = create(<Harness active={active} />);
  });
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => (resolve = done));
  return { promise, resolve };
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.useFakeTimers();
  vi.setSystemTime(new Date(2026, 9, 14, 15));
  locale = 'zh-Hans';
  archive = { zh: [chinese, older, project], en: [english] };
  storage = new Map(
    ['zh', 'en'].flatMap((lang) => [
      [`pulse:migrated-to-disk:${lang}`, '1'],
      [`pulse:meta:${lang}`, JSON.stringify({ lastFetched: Date.now() })],
    ])
  );
  vi.stubGlobal('localStorage', {
    getItem: vi.fn((key: string) => storage.get(key) ?? null),
    setItem: vi.fn((key: string, value: string) => storage.set(key, value)),
    removeItem: vi.fn((key: string) => storage.delete(key)),
  });
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    const { lang, items } = args as { lang: string; items: NewsItem[] };
    if (command === 'pulse_load_all') return archive[lang];
    if (command === 'pulse_save') archive[lang] = items;
  });
  vi.mocked(fetchFeed).mockResolvedValue(feed([chinese]));
});

afterEach(() => {
  act(() => renderer?.unmount());
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('restored AI news', () => {
  it.each(['zh', 'en'])(
    'filters old %s archives without refetching or rewriting them',
    async (lang) => {
      locale = lang === 'zh' ? 'zh-Hans' : 'en';
      const good = lang === 'zh' ? chinese : english;
      const discussion = news(
        'discussion',
        lang === 'zh' ? '广告业的 AI 转型' : 'AI in advertising'
      );
      const promotion = news(
        'promotion',
        lang === 'zh'
          ? '[推广] 一个下午验证 Claude 中转，送 10 美元试用金'
          : '[Sponsored] AI trial credits'
      );
      const xPost = {
        ...news('x-post', lang === 'zh' ? 'AI 模型讨论' : 'AI model discussion'),
        url: 'https://x.com/user/status/123',
      };
      const v2ex = {
        ...news('v2ex', lang === 'zh' ? 'AI 产品体验' : 'AI product review'),
        url: 'https://www.v2ex.com/t/123',
      };
      archive[lang] = [good, discussion, promotion, xPost, v2ex];
      await mount();
      expect(markup()).toContain(good.title);
      expect(markup()).toContain(discussion.title);
      for (const blocked of [promotion, xPost, v2ex]) expect(markup()).not.toContain(blocked.title);
      expect(button('142')).toBeDefined();
      expect(fetchFeed).not.toHaveBeenCalled();
      expect(invoke).toHaveBeenCalledExactlyOnceWith('pulse_load_all', { lang });
      expect(localStorage.setItem).not.toHaveBeenCalled();
    }
  );

  it('does not archive blocked content returned by a remote source', async () => {
    archive.zh = [];
    const promotion = news('promotion', '[推广] AI 中转试用金');
    const twitter = {
      ...news('twitter', 'AI 动态'),
      url: 'https://mobile.twitter.com/user/status/123',
    };
    vi.mocked(fetchFeed).mockResolvedValue(feed([chinese, promotion, twitter]));
    await mount();
    expect(invoke).toHaveBeenCalledWith('pulse_save', { lang: 'zh', items: [chinese] });
    expect(markup()).toContain(chinese.title);
    expect(markup()).not.toContain(promotion.title);
    expect(markup()).not.toContain(twitter.title);
    expect(button('141')).toBeDefined();
  });

  it('filters the old localStorage cache before migrating it to the archive', async () => {
    storage.delete('pulse:migrated-to-disk:zh');
    const promotion = news('promotion', '[推广] AI 中转试用金');
    storage.set('pulse:items:zh', JSON.stringify([chinese, promotion]));
    await mount();
    expect(invoke).toHaveBeenCalledWith('pulse_save', { lang: 'zh', items: [chinese] });
    expect(storage.has('pulse:items:zh')).toBe(false);
    expect(markup()).not.toContain(promotion.title);
  });

  it('does no news I/O while inactive, then loads a fresh archive without network or writes', async () => {
    await mount(false);
    expect(invoke).not.toHaveBeenCalled();
    expect(fetchFeed).not.toHaveBeenCalled();
    expect(localStorage.setItem).not.toHaveBeenCalled();
    await act(async () => renderer.update(<Harness />));
    expect(invoke).toHaveBeenCalledExactlyOnceWith('pulse_load_all', { lang: 'zh' });
    expect(fetchFeed).not.toHaveBeenCalled();
    expect(markup()).toContain(chinese.title);
    expect(markup()).not.toContain(older.title);
    expect(markup()).not.toContain(project.title);
    expect(open).not.toHaveBeenCalled();
  });

  it('keeps date counts aligned with the news list and opens original articles only on click', async () => {
    await mount();
    expect(button('141')).toBeDefined();
    expect(button('131')).toBeDefined();
    act(() => button('131').props.onClick());
    expect(markup()).toContain(older.title);
    expect(markup()).not.toContain(chinese.title);
    const article = renderer.root
      .findAllByType('button')
      .find((node) => text(node).includes(older.title))!;
    await act(async () => {
      article.props.onClick();
    });
    expect(open).toHaveBeenCalledExactlyOnceWith(older.url);
    await act(async () => renderer.update(<Harness active={false} />));
    await act(async () => renderer.update(<Harness />));
    expect(markup()).toContain(older.title);
    expect(fetchFeed).not.toHaveBeenCalled();
  });

  it('refreshes an empty install, saves the window and loads it back', async () => {
    archive.zh = [];
    await mount();
    expect(fetchFeed).toHaveBeenCalledExactlyOnceWith('zh', expect.any(AbortSignal));
    expect(invoke).toHaveBeenCalledWith('pulse_save', { lang: 'zh', items: [chinese] });
    expect(markup()).toContain(chinese.title);
    expect(storage.has('pulse:meta:zh')).toBe(true);
  });

  it('preserves cached rows on refresh failure and prevents duplicate refreshes', async () => {
    await mount();
    const request = deferred<RawFeed>();
    vi.mocked(fetchFeed).mockReturnValue(request.promise);
    act(() => {
      button('btn.refresh').props.onClick();
      button('btn.refresh').props.onClick();
    });
    await act(async () => {});
    expect(fetchFeed).toHaveBeenCalledTimes(1);
    expect(button('btn.refresh').props.disabled).toBe(true);
    vi.mocked(fetchFeed).mockRejectedValue(new Error('offline'));
    await act(async () => request.resolve(feed([chinese])));
    await act(async () => button('btn.refresh').props.onClick());
    expect(markup()).toContain(chinese.title);
    expect(markup()).toContain('pulse.fetchFailed');
    expect(button('btn.refresh').props.disabled).toBe(false);
  });

  it('cancels on exit and ignores a late response without archive or metadata writes', async () => {
    archive.zh = [];
    const request = deferred<RawFeed>();
    vi.mocked(fetchFeed).mockReturnValue(request.promise);
    await mount();
    const signal = vi.mocked(fetchFeed).mock.calls[0][1];
    await act(async () => renderer.update(<Harness active={false} />));
    expect(signal.aborted).toBe(true);
    await act(async () => request.resolve(feed([chinese])));
    expect(invoke).not.toHaveBeenCalledWith('pulse_save', expect.anything());
    expect(localStorage.setItem).not.toHaveBeenCalled();
    expect(markup()).not.toContain(chinese.title);
  });

  it('preserves visible history when the native archive cannot be read on return', async () => {
    await mount();
    await act(async () => renderer.update(<Harness active={false} />));
    vi.mocked(invoke).mockRejectedValueOnce('archive parse error');
    await act(async () => renderer.update(<Harness />));
    expect(markup()).toContain(chinese.title);
    expect(markup()).toContain('pulse.fetchFailed');
    expect(fetchFeed).not.toHaveBeenCalled();
    expect(invoke).not.toHaveBeenCalledWith('pulse_save', expect.anything());
    expect(localStorage.setItem).not.toHaveBeenCalled();
  });

  it('switches sources and locale without letting a late Chinese response replace global news', async () => {
    archive.zh = [];
    const request = deferred<RawFeed>();
    vi.mocked(fetchFeed).mockReturnValue(request.promise);
    await mount();
    const signal = vi.mocked(fetchFeed).mock.calls[0][1];
    await act(async () => button('全球').props.onClick());
    expect(signal.aborted).toBe(true);
    expect(markup()).toContain(english.title);
    await act(async () => request.resolve(feed([chinese])));
    expect(markup()).not.toContain(chinese.title);
    expect(invoke).not.toHaveBeenCalledWith('pulse_save', expect.anything());
    await act(async () => button('国内').props.onClick());
    locale = 'en';
    await act(async () => renderer.update(<Harness />));
    expect(markup()).toContain(english.title);
    expect(markup()).not.toContain('国内');
  });

  it('migrates the old localStorage archive once before displaying saved history', async () => {
    storage.delete('pulse:migrated-to-disk:zh');
    storage.set('pulse:items:zh', JSON.stringify([chinese]));
    await mount();
    expect(invoke).toHaveBeenCalledWith('pulse_save', { lang: 'zh', items: [chinese] });
    expect(storage.has('pulse:items:zh')).toBe(false);
    expect(storage.get('pulse:migrated-to-disk:zh')).toBe('1');
    expect(markup()).toContain(chinese.title);
  });
});
