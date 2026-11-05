import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { AiPulseProvider, useAiPulse } from './provider';
import { fetchFeed, filterNewsItems, type RawFeed, type NewsItem } from './feed';

vi.mock('../../hooks/useI18n', () => ({
  useI18n: () => ({ locale: 'en', t: (key: string) => key }),
}));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('./feed', async (original) => ({
  ...(await original<typeof import('./feed')>()),
  fetchFeed: vi.fn(),
}));
let view: ReactTestRenderer;
const article: NewsItem = {
  id: 'fixture',
  title: 'AI model update',
  source: 'OpenAI',
  url: 'https://example.com/news',
  published_at: new Date().toISOString(),
};
function Actions() {
  const { retry, items } = useAiPulse();
  return (
    <>
      <button onClick={retry}>Retry</button>
      <span>{items.length}</span>
    </>
  );
}
const render = (enabled: boolean, active = true) => (
  <AiPulseProvider enabled={enabled} active={active}>
    <Actions />
  </AiPulseProvider>
);
beforeEach(() => {
  vi.clearAllMocks();
  vi.stubGlobal('localStorage', {
    getItem: vi.fn(() => null),
    setItem: vi.fn(),
    removeItem: vi.fn(),
  });
  vi.mocked(invoke).mockResolvedValue([]);
});
afterEach(() => {
  act(() => view?.unmount());
  vi.unstubAllGlobals();
});
it('does not hydrate, migrate, fetch or retry while disabled, then works on an enabled visit', async () => {
  vi.mocked(fetchFeed).mockResolvedValue({ items: [article] } as unknown as RawFeed);
  await act(async () => {
    view = create(render(false));
  });
  await act(async () => view.root.findByType('button').props.onClick());
  expect(invoke).not.toHaveBeenCalled();
  expect(fetchFeed).not.toHaveBeenCalled();
  expect(localStorage.setItem).not.toHaveBeenCalled();
  await act(async () => view.update(render(true, false)));
  expect(invoke).not.toHaveBeenCalled();
  await act(async () => view.update(render(true)));
  expect(invoke).toHaveBeenCalledWith('pulse_load_all', { lang: 'en' });
  expect(fetchFeed).toHaveBeenCalledOnce();
  expect(invoke).toHaveBeenCalledWith('pulse_save', { lang: 'en', items: [article] });
});
it('aborts pending news fetches and cannot archive a late response after disabling', async () => {
  let finish!: (feed: RawFeed) => void;
  vi.mocked(fetchFeed).mockReturnValueOnce(
    new Promise((resolve) => {
      finish = resolve;
    })
  );
  await act(async () => {
    view = create(render(true));
  });
  expect(fetchFeed).toHaveBeenCalledOnce();
  const signal = vi.mocked(fetchFeed).mock.calls[0][1]!;
  expect(signal.aborted).toBe(false);
  await act(async () => view.update(render(false)));
  expect(signal.aborted).toBe(true);
  expect(filterNewsItems([article])).toHaveLength(1);
  await act(async () => finish({ items: [article] } as unknown as RawFeed));
  expect(vi.mocked(invoke).mock.calls.some(([command]) => command === 'pulse_save')).toBe(false);
  await act(async () => view.root.findByType('button').props.onClick());
  expect(fetchFeed).toHaveBeenCalledOnce();
});

it('releases the in-memory feed on disable without deleting disk archives', async () => {
  vi.mocked(invoke).mockResolvedValue([article]);
  vi.mocked(fetchFeed).mockResolvedValue({ items: [] } as unknown as RawFeed);
  await act(async () => {
    view = create(render(true));
  });
  expect(view.root.findByType('span').children).toEqual(['1']);
  vi.mocked(invoke).mockClear();
  await act(async () => view.update(render(false)));
  expect(view.root.findByType('span').children).toEqual(['0']);
  expect(invoke).not.toHaveBeenCalled();
});
