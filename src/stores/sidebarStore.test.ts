import { afterEach, beforeEach, expect, it, vi } from 'vitest';

const key = 'echobird-sidebar-collapsed';
let saved: Map<string, string>;
let storage: { getItem: ReturnType<typeof vi.fn>; setItem: ReturnType<typeof vi.fn> };

beforeEach(() => {
  vi.resetModules();
  saved = new Map();
  storage = {
    getItem: vi.fn((key: string) => saved.get(key) ?? null),
    setItem: vi.fn((key: string, value: string) => saved.set(key, value)),
  };
  vi.stubGlobal('localStorage', storage);
});
afterEach(() => vi.unstubAllGlobals());

it.each([null, 'false', 'true'])(
  'loads the %s sidebar preference without writing',
  async (value) => {
    if (value !== null) saved.set(key, value);
    const { useSidebarStore } = await import('./sidebarStore');
    expect(useSidebarStore.getState().collapsed).toBe(value !== 'false');
    expect(storage.setItem).not.toHaveBeenCalled();
  }
);

it('shares the layout immediately and remembers it after restart', async () => {
  const { useSidebarStore } = await import('./sidebarStore');
  const title = vi.fn();
  const sidebar = vi.fn();
  const stopTitle = useSidebarStore.subscribe(title);
  const stopSidebar = useSidebarStore.subscribe(sidebar);
  useSidebarStore.getState().setCollapsed(true);
  expect(title).toHaveBeenCalledOnce();
  expect(sidebar).toHaveBeenCalledOnce();
  expect(storage.setItem).toHaveBeenCalledExactlyOnceWith(key, 'true');
  stopTitle();
  stopSidebar();
  vi.resetModules();
  const restarted = (await import('./sidebarStore')).useSidebarStore;
  expect(restarted.getState().collapsed).toBe(true);
  restarted.getState().setCollapsed(false);
  expect(storage.setItem).toHaveBeenLastCalledWith(key, 'false');
});

it('keeps layout controls usable when local preferences are unavailable', async () => {
  storage.getItem.mockImplementation(() => {
    throw new Error('unavailable');
  });
  storage.setItem.mockImplementation(() => {
    throw new Error('unavailable');
  });
  const { useSidebarStore } = await import('./sidebarStore');
  expect(useSidebarStore.getState().collapsed).toBe(true);
  expect(() => useSidebarStore.getState().setCollapsed(true)).not.toThrow();
  expect(useSidebarStore.getState().collapsed).toBe(true);
  useSidebarStore.getState().setCollapsed(false);
  expect(useSidebarStore.getState().collapsed).toBe(false);
});
