import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AiCareerFamily, SavedSession } from '../../api/aiCareer';
import * as api from '../../api/aiCareer';
import { copyText } from '../../utils/copyText';
import { useAiCareerStore } from '../../stores/aiCareerStore';
import { AiCareerPanel } from './AiCareerPanel';

const { i18n, showToast } = vi.hoisted(() => ({
  i18n: { t: (key: string) => key, locale: 'en' },
  showToast: vi.fn(),
}));
vi.mock('../../hooks/useI18n', () => ({ useI18n: () => i18n }));
vi.mock('../../components/Toast', () => ({ useToast: () => ({ showToast }) }));
vi.mock('../../utils/copyText', () => ({ copyText: vi.fn() }));
vi.mock('../../api/aiCareer', () => ({
  aiCareerFamilyHistory: vi.fn(),
  revealAiCareerSessionFile: vi.fn(),
}));
vi.mock('react-dom', () => ({ createPortal: (children: React.ReactNode) => children }));

function session(index: number, overrides: Partial<SavedSession> = {}): SavedSession {
  return {
    id: `codex_native_${index}`,
    name: `Session ${index}`,
    tool: 'codex',
    cwd: 'C:\\Projects\\Example',
    session_token: `native-${index}`,
    saved_at: '2026-10-10T12:00:00Z',
    file_path: `C:\\sessions\\${index}.jsonl`,
    turn_count: 5,
    input_tokens: 100,
    output_tokens: 20,
    ...overrides,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe('career family sessions', () => {
  let renderer: ReactTestRenderer;
  let intersection: IntersectionObserverCallback;
  const disconnect = vi.fn();
  const observe = vi.fn();
  const flush = async () => {
    await act(async () => {
      await Promise.resolve();
    });
  };
  const open = async (family: AiCareerFamily = 'codex') => {
    await act(async () => {
      renderer = create(<AiCareerPanel family={family} />, {
        createNodeMock: () => ({ offsetWidth: 150 }),
      });
    });
  };
  const text = () => JSON.stringify(renderer.toJSON());
  const action = (label: string) =>
    renderer.root.findAllByType('button').find((button) => button.props['aria-label'] === label)!;
  const scroll = () => {
    intersection(
      [{ isIntersecting: true }] as IntersectionObserverEntry[],
      {} as IntersectionObserver
    );
  };

  beforeEach(() => {
    i18n.t = (key: string) => key;
    i18n.locale = 'en';
    vi.stubGlobal('window', {
      setTimeout,
      clearTimeout,
      innerWidth: 1000,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    });
    vi.stubGlobal('document', { body: {} });
    vi.stubGlobal(
      'IntersectionObserver',
      class {
        constructor(callback: IntersectionObserverCallback) {
          intersection = callback;
        }
        observe = observe;
        disconnect = disconnect;
      }
    );
    useAiCareerStore.setState({
      selectedFamily: 'codex',
      refreshKey: 0,
      refreshing: false,
      cachedScopes: {},
    });
    vi.mocked(api.aiCareerFamilyHistory).mockResolvedValue([]);
    vi.mocked(api.revealAiCareerSessionFile).mockResolvedValue();
    vi.mocked(copyText).mockResolvedValue(true);
  });

  afterEach(() => {
    act(() => renderer?.unmount());
    vi.resetAllMocks();
    vi.unstubAllGlobals();
  });

  it('requests twenty at a time and prevents concurrent requests for the same page', async () => {
    const next = deferred<SavedSession[]>();
    vi.mocked(api.aiCareerFamilyHistory)
      .mockResolvedValueOnce(Array.from({ length: 20 }, (_, i) => session(i)))
      .mockReturnValueOnce(next.promise);
    await open();
    expect(api.aiCareerFamilyHistory).toHaveBeenNthCalledWith(1, 'codex', 0, 20);
    act(() => {
      scroll();
      scroll();
    });
    expect(api.aiCareerFamilyHistory).toHaveBeenCalledTimes(2);
    expect(api.aiCareerFamilyHistory).toHaveBeenNthCalledWith(2, 'codex', 20, 20);
    await act(async () => next.resolve([session(20)]));
    expect(text()).toContain('Session 20');
    expect(observe).toHaveBeenCalled();
    expect(disconnect).toHaveBeenCalled();
  });

  it('shows the cached first page while revalidating, replaces it, then paginates from twenty', async () => {
    const cached = Array.from({ length: 20 }, (_, index) =>
      session(index, { name: `Cached ${index}` })
    );
    const fresh = Array.from({ length: 20 }, (_, index) =>
      session(index + 100, { name: `Fresh ${index}` })
    );
    const firstPage = deferred<SavedSession[]>();
    const nextPage = deferred<SavedSession[]>();
    useAiCareerStore.setState({ cachedScopes: { codex: { sessions: cached } } });
    vi.mocked(api.aiCareerFamilyHistory)
      .mockReturnValueOnce(firstPage.promise)
      .mockReturnValueOnce(nextPage.promise);
    await open();
    expect(text()).toContain('Cached 0');
    expect(text()).not.toContain('animate-pulse');
    expect(api.aiCareerFamilyHistory).toHaveBeenCalledExactlyOnceWith('codex', 0, 20);
    expect(observe).not.toHaveBeenCalled();
    expect(useAiCareerStore.getState().cachedScopes.codex?.sessions).toEqual(cached);

    await act(async () => firstPage.resolve(fresh));
    expect(text()).toContain('Fresh 0');
    expect(text()).not.toContain('Cached 0');
    expect(observe).toHaveBeenCalled();
    act(() => {
      scroll();
      scroll();
    });
    expect(api.aiCareerFamilyHistory).toHaveBeenCalledTimes(2);
    expect(api.aiCareerFamilyHistory).toHaveBeenLastCalledWith('codex', 20, 20);
    await act(async () => nextPage.resolve([session(200, { name: 'Next page' })]));
    expect(text()).toContain('Next page');
    expect(text()).toContain('Fresh 0');
    expect(useAiCareerStore.getState().cachedScopes.codex?.sessions).toEqual(fresh);
    expect(useAiCareerStore.getState().cachedScopes.codex?.sessions).toHaveLength(20);
  });

  it('treats a cached empty first page as ready while its real request is pending', async () => {
    const firstPage = deferred<SavedSession[]>();
    useAiCareerStore.setState({ cachedScopes: { codex: { sessions: [] } } });
    vi.mocked(api.aiCareerFamilyHistory).mockReturnValueOnce(firstPage.promise);
    await open();
    expect(text()).toContain('aiCareer.noSessions');
    expect(text()).not.toContain('animate-pulse');
    expect(api.aiCareerFamilyHistory).toHaveBeenCalledExactlyOnceWith('codex', 0, 20);
    expect(observe).not.toHaveBeenCalled();
    await act(async () => firstPage.resolve([session(3)]));
    expect(text()).toContain('Session 3');
    expect(text()).not.toContain('aiCareer.noSessions');
    expect(useAiCareerStore.getState().cachedScopes.codex?.sessions).toEqual([session(3)]);
  });

  it.each(['codex', 'antigravity', 'workbuddy'] as const)(
    'settles an uncached empty history for %s in the shared empty state',
    async (family) => {
      const firstPage = deferred<SavedSession[]>();
      vi.mocked(api.aiCareerFamilyHistory).mockReturnValueOnce(firstPage.promise);
      await open(family);
      expect(text()).toContain('animate-pulse');
      expect(text()).not.toContain('aiCareer.noSessions');
      await act(async () => firstPage.resolve([]));
      expect(text()).not.toContain('animate-pulse');
      expect(text()).toContain('aiCareer.noSessions');
      expect(text()).not.toContain('aiCareer.retry');
      expect(useAiCareerStore.getState().cachedScopes[family]?.sessions).toEqual([]);
    }
  );

  it('retains cached rows without a new error when background revalidation fails', async () => {
    const cached = [session(4)];
    useAiCareerStore.setState({ cachedScopes: { codex: { sessions: cached } } });
    vi.mocked(api.aiCareerFamilyHistory).mockRejectedValueOnce(new Error('background failure'));
    await open();
    expect(text()).toContain('Session 4');
    expect(text()).not.toContain('animate-pulse');
    expect(text()).not.toContain('aiCareer.loadMoreFailed');
    expect(text()).not.toContain('aiCareer.sessionLoadFailed');
    expect(text()).not.toContain('aiCareer.retry');
    expect(useAiCareerStore.getState().cachedScopes.codex?.sessions).toEqual(cached);
    expect(showToast).not.toHaveBeenCalled();
  });

  it('keeps explicit refresh and retry failures visible over the cache until fresh data succeeds', async () => {
    const firstPage = deferred<SavedSession[]>();
    useAiCareerStore.setState({
      refreshing: true,
      cachedScopes: { codex: { sessions: [session(4)] } },
    });
    vi.mocked(api.aiCareerFamilyHistory)
      .mockReturnValueOnce(firstPage.promise)
      .mockRejectedValueOnce(new Error('retry failure'))
      .mockResolvedValueOnce([session(5)]);
    await open();
    expect(text()).toContain('Session 4');
    expect(text()).not.toContain('animate-pulse');
    await act(async () => {
      useAiCareerStore.setState({ refreshing: false });
      firstPage.reject(new Error('refresh failure'));
    });
    expect(text()).toContain('aiCareer.loadMoreFailed');
    const retry = () =>
      renderer.root
        .findAllByType('button')
        .find((button) => button.children.includes('aiCareer.retry'))!;
    await act(async () => retry().props.onClick());
    expect(api.aiCareerFamilyHistory).toHaveBeenLastCalledWith('codex', 0, 20);
    expect(text()).toContain('Session 4');
    expect(text()).toContain('aiCareer.loadMoreFailed');
    await act(async () => retry().props.onClick());
    expect(text()).toContain('Session 5');
    expect(text()).not.toContain('Session 4');
    expect(text()).not.toContain('aiCareer.loadMoreFailed');
    expect(useAiCareerStore.getState().cachedScopes.codex?.sessions).toEqual([session(5)]);
    expect(showToast).not.toHaveBeenCalled();
  });

  it.each(['family', 'refresh', 'leave'] as const)(
    'does not persist a late first page after %s',
    async (kind) => {
      const oldPage = deferred<SavedSession[]>();
      vi.mocked(api.aiCareerFamilyHistory)
        .mockReturnValueOnce(oldPage.promise)
        .mockResolvedValueOnce([session(100, { name: 'Current session' })]);
      await open();
      await act(async () => {
        if (kind === 'family') renderer.update(<AiCareerPanel family="claude" />);
        else if (kind === 'refresh') useAiCareerStore.setState({ refreshKey: 1 });
        else renderer.unmount();
      });
      const currentCache = useAiCareerStore.getState().cachedScopes;
      if (kind !== 'leave') {
        expect(text()).toContain('Current session');
        expect(currentCache[kind === 'family' ? 'claude' : 'codex']?.sessions).toEqual([
          session(100, { name: 'Current session' }),
        ]);
      }
      await act(async () => oldPage.resolve([session(999, { name: 'Stale session' })]));
      expect(useAiCareerStore.getState().cachedScopes).toBe(currentCache);
      expect(JSON.stringify(currentCache)).not.toContain('Stale session');
      if (kind !== 'leave') expect(text()).not.toContain('Stale session');
    }
  );

  it('updates translated relative times without fetching when a language pack becomes ready', async () => {
    const savedAt = new Date().toISOString();
    vi.mocked(api.aiCareerFamilyHistory).mockResolvedValueOnce(
      Array.from({ length: 20 }, (_, index) => session(index, { saved_at: savedAt }))
    );
    await open();
    expect(text()).toContain('aiCareer.timeJustNow');
    i18n.t = (key: string) => `translated:${key}`;
    await act(async () => renderer.update(<AiCareerPanel family="codex" />));
    expect(api.aiCareerFamilyHistory).toHaveBeenCalledTimes(1);
    expect(text()).toContain('translated:aiCareer.timeJustNow');
    expect(text()).toContain('translated:aiCareer.copyId');
  });

  it('shows the native model and localized message count after the project and date', async () => {
    i18n.t = (key: string) =>
      ({ 'aiCareer.timeJustNow': 'Just now', 'aiCareer.messages': '{count} messages' })[key] ?? key;
    vi.mocked(api.aiCareerFamilyHistory).mockResolvedValueOnce([
      session(1, {
        saved_at: new Date().toISOString(),
        model_id: '  provider/model-v1  ',
        message_count: 1234,
      }),
    ]);
    await open();
    expect(text()).toContain('Example · Just now · provider/model-v1 · 1,234 messages');
    expect(text()).toContain('aiCareer.input');
    expect(text()).toContain('120');
    expect(api.aiCareerFamilyHistory).toHaveBeenCalledExactlyOnceWith('codex', 0, 20);
    expect(copyText).not.toHaveBeenCalled();
    expect(api.revealAiCareerSessionFile).not.toHaveBeenCalled();

    i18n.locale = 'zh-CN';
    i18n.t = (key: string) =>
      ({ 'aiCareer.timeJustNow': '刚刚', 'aiCareer.messages': '{count} 条消息' })[key] ?? key;
    await act(async () => renderer.update(<AiCareerPanel family="codex" />));
    expect(text()).toContain('Example · 刚刚 · provider/model-v1 · 1,234 条消息');
    expect(api.aiCareerFamilyHistory).toHaveBeenCalledTimes(1);
  });

  it.each([
    [{}, 'Example · Just now'],
    [{ model_id: null, message_count: null }, 'Example · Just now'],
    [{ model_id: '  ', message_count: null }, 'Example · Just now'],
    [{ model_id: 'gpt-native' }, 'Example · Just now · gpt-native'],
    [{ model_id: null, message_count: 0 }, 'Example · Just now · 0 messages'],
  ] satisfies [Partial<SavedSession>, string][])(
    'omits unknown session metadata while preserving a measured zero: %o',
    async (metadata, expected) => {
      i18n.t = (key: string) =>
        ({ 'aiCareer.timeJustNow': 'Just now', 'aiCareer.messages': '{count} messages' })[key] ??
        key;
      vi.mocked(api.aiCareerFamilyHistory).mockResolvedValueOnce([
        session(1, { saved_at: new Date().toISOString(), ...metadata }),
      ]);
      await open();
      const summary = renderer.root.findByProps({
        className: 'flex-1 min-w-0 text-[11px] text-cyber-text-secondary truncate',
      });
      expect(summary.children).toEqual([expected]);
      expect(text()).not.toContain('5 messages');
      expect(text()).not.toContain('undefined');
    }
  );

  it('keeps a pending page isolated from language readiness and appends it once', async () => {
    const page = deferred<SavedSession[]>();
    const savedAt = new Date().toISOString();
    vi.mocked(api.aiCareerFamilyHistory)
      .mockResolvedValueOnce(Array.from({ length: 20 }, (_, index) => session(index)))
      .mockReturnValueOnce(page.promise)
      .mockResolvedValue([]);
    await open();
    act(scroll);
    i18n.t = (key: string) => `translated:${key}`;
    await act(async () => renderer.update(<AiCareerPanel family="codex" />));
    expect(api.aiCareerFamilyHistory).toHaveBeenCalledTimes(2);
    await act(async () => page.resolve([session(20, { saved_at: savedAt })]));
    expect(api.aiCareerFamilyHistory).toHaveBeenCalledTimes(2);
    expect(text()).toContain('translated:aiCareer.timeJustNow');
    expect(text().match(/Session 20/g)).toHaveLength(1);
    expect(text()).toContain('Session 0');
  });

  it.each(['family', 'refresh'] as const)(
    'ignores a late page from an earlier %s generation',
    async (kind) => {
      const oldPage = deferred<SavedSession[]>();
      vi.mocked(api.aiCareerFamilyHistory)
        .mockResolvedValueOnce(Array.from({ length: 20 }, (_, i) => session(i)))
        .mockReturnValueOnce(oldPage.promise)
        .mockResolvedValueOnce([session(100, { name: 'Current session' })]);
      await open();
      act(scroll);
      await act(async () => {
        if (kind === 'family') renderer.update(<AiCareerPanel family="claude" />);
        else useAiCareerStore.setState({ refreshKey: 1 });
      });
      await act(async () => oldPage.resolve([session(999, { name: 'Stale session' })]));
      expect(text()).toContain('Current session');
      expect(text()).not.toContain('Stale session');
      expect(api.aiCareerFamilyHistory).toHaveBeenLastCalledWith(
        kind === 'family' ? 'claude' : 'codex',
        0,
        20
      );
    }
  );

  it('keeps loaded rows after pagination fails and retries the same offset', async () => {
    vi.mocked(api.aiCareerFamilyHistory)
      .mockResolvedValueOnce(Array.from({ length: 20 }, (_, i) => session(i)))
      .mockRejectedValueOnce(new Error('disk failure'))
      .mockResolvedValueOnce([session(20)]);
    await open();
    act(scroll);
    await flush();
    expect(text()).toContain('Session 0');
    expect(text()).toContain('aiCareer.loadMoreFailed');
    const retry = renderer.root
      .findAllByType('button')
      .find((button) => button.children.includes('aiCareer.retry'))!;
    await act(async () => retry.props.onClick());
    expect(api.aiCareerFamilyHistory).toHaveBeenLastCalledWith('codex', 20, 20);
    expect(text()).toContain('Session 20');
    expect(showToast).not.toHaveBeenCalled();
  });

  it('distinguishes initial failure from an empty history and retries without a dialog', async () => {
    vi.mocked(api.aiCareerFamilyHistory).mockRejectedValueOnce(new Error('read failure'));
    await open();
    expect(text()).toContain('aiCareer.sessionLoadFailed');
    expect(text()).not.toContain('aiCareer.noSessions');
    const retry = renderer.root
      .findAllByType('button')
      .find((button) => button.children.includes('aiCareer.retry'))!;
    await act(async () => retry.props.onClick());
    expect(text()).toContain('aiCareer.noSessions');
    expect(showToast).not.toHaveBeenCalled();
  });

  it.each([
    ['codex', 'codex resume'],
    ['mimo', 'mimo --session'],
  ])('copies native actions from the real %s history payload', async (tool, command) => {
    vi.mocked(api.aiCareerFamilyHistory).mockResolvedValue([
      session(7, { tool, id: `${tool}_native_7` }),
    ]);
    await open(tool as AiCareerFamily);
    expect(api.aiCareerFamilyHistory).toHaveBeenCalledWith(tool, 0, 20);
    expect(action('aiCareer.copyResume').props.disabled).toBe(false);
    expect(api.revealAiCareerSessionFile).not.toHaveBeenCalled();
    expect(copyText).not.toHaveBeenCalled();
    await act(async () => action('aiCareer.copyId').props.onClick());
    await act(async () => action('aiCareer.copyResume').props.onClick());
    await act(async () => action('aiCareer.copyPath').props.onClick());
    await act(async () => action('aiCareer.showInFolder').props.onClick());
    expect(copyText).toHaveBeenNthCalledWith(1, 'native-7');
    expect(copyText).toHaveBeenNthCalledWith(2, `${command} native-7`);
    expect(copyText).toHaveBeenNthCalledWith(3, 'C:\\sessions\\7.jsonl');
    expect(api.revealAiCareerSessionFile).toHaveBeenCalledWith('C:\\sessions\\7.jsonl');
    expect(text()).toContain('Example');
    expect(text()).toContain('120');
    expect(text()).not.toContain('cache');
  });

  it('disables missing actions and keeps unreported tokens unknown while preserving measured zero', async () => {
    vi.mocked(api.aiCareerFamilyHistory).mockResolvedValue([
      session(1, { session_token: null, file_path: null, input_tokens: null, output_tokens: 0 }),
    ]);
    await open();
    for (const button of renderer.root.findAllByType('button'))
      expect(button.props.disabled).toBe(true);
    expect(text()).toContain('—');
    expect(text()).toContain('aiCareer.output');
    expect(text()).toContain('"0"');
    expect(text()).not.toContain('NaN');
  });

  it('reports explicit clipboard and reveal failures without changing the history', async () => {
    vi.mocked(api.aiCareerFamilyHistory).mockResolvedValue([session(2)]);
    vi.mocked(copyText).mockResolvedValue(false);
    vi.mocked(api.revealAiCareerSessionFile).mockRejectedValue(new Error('gone'));
    await open();
    await act(async () => action('aiCareer.copyId').props.onClick());
    await act(async () => action('aiCareer.showInFolder').props.onClick());
    expect(showToast).toHaveBeenCalledTimes(2);
    expect(showToast).toHaveBeenCalledWith('error', 'aiCareer.actionFailed');
    expect(text()).toContain('Session 2');
  });

  it('ignores pending requests and explicit-action failures after leaving the page', async () => {
    const pendingPage = deferred<SavedSession[]>();
    vi.mocked(api.aiCareerFamilyHistory).mockReturnValue(pendingPage.promise);
    await open();
    act(() => renderer.unmount());
    await act(async () => pendingPage.reject(new Error('late failure')));
    expect(showToast).not.toHaveBeenCalled();

    const pendingReveal = deferred<void>();
    vi.mocked(api.aiCareerFamilyHistory).mockResolvedValue([session(1)]);
    vi.mocked(api.revealAiCareerSessionFile).mockReturnValue(pendingReveal.promise);
    await open();
    act(() => action('aiCareer.showInFolder').props.onClick());
    act(() => renderer.unmount());
    await act(async () => pendingReveal.reject(new Error('late reveal failure')));
    expect(showToast).not.toHaveBeenCalled();
  });

  it('does not fetch another page from an observer callback queued before leaving', async () => {
    vi.mocked(api.aiCareerFamilyHistory).mockResolvedValue(
      Array.from({ length: 20 }, (_, i) => session(i))
    );
    await open();
    const previousCallback = intersection;
    act(() => renderer.unmount());
    previousCallback(
      [{ isIntersecting: true }] as IntersectionObserverEntry[],
      {} as IntersectionObserver
    );
    expect(api.aiCareerFamilyHistory).toHaveBeenCalledTimes(1);
  });

  it('shows the shared tooltip on keyboard focus and removes it on blur', async () => {
    vi.mocked(api.aiCareerFamilyHistory).mockResolvedValue([session(1)]);
    await open();
    act(() =>
      action('aiCareer.copyId').props.onFocus({
        currentTarget: { getBoundingClientRect: () => ({ left: 400, top: 100, width: 20 }) },
      })
    );
    expect(renderer.root.findByProps({ role: 'tooltip' }).children).toEqual(['aiCareer.copyId']);
    act(() => action('aiCareer.copyId').props.onBlur());
    expect(renderer.root.findAllByProps({ role: 'tooltip' })).toHaveLength(0);
  });
});
