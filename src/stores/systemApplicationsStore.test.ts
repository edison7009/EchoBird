import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import {
  invalidateSystemApplicationIcons,
  preloadSystemApplicationIcons,
} from '../utils/systemApplicationIcons';
import * as api from '../api/tauri';
import { useSystemApplicationsStore as store } from './systemApplicationsStore';

vi.mock('../api/tauri', () => ({ listSystemApplications: vi.fn() }));
vi.mock('../utils/systemApplicationIcons', () => ({
  invalidateSystemApplicationIcons: vi.fn(),
  preloadSystemApplicationIcons: vi.fn(),
}));

beforeEach(() => {
  vi.clearAllMocks();
  store.setState(store.getInitialState(), true);
});

it('loads ordinary applications and reuses the inventory on passive re-entry', async () => {
  vi.mocked(api.listSystemApplications).mockResolvedValue([
    { id: 'system:one', name: 'Local app', path: '/custom/App', createdAt: 1234 },
  ]);
  await store.getState().load();
  await store.getState().load();
  expect(api.listSystemApplications).toHaveBeenCalledExactlyOnceWith(false);
  expect(store.getState().applications[0]).toMatchObject({
    id: 'system:one',
    systemAppId: 'system:one',
    installed: true,
    noModelConfig: true,
    createdAt: 1234,
  });
});

it('preserves rows while refreshing and after a failure, then allows a retry', async () => {
  const app = { id: 'system:one', name: 'Local app', path: '/custom/App' };
  vi.mocked(api.listSystemApplications).mockResolvedValue([app]);
  await store.getState().load();
  let reject!: (error: Error) => void;
  vi.mocked(api.listSystemApplications).mockImplementation(
    () =>
      new Promise((_, fail) => {
        reject = fail;
      })
  );
  const refresh = store.getState().load(true);
  await store.getState().load(true);
  expect(store.getState().applications).toHaveLength(1);
  expect(api.listSystemApplications).toHaveBeenCalledTimes(2);
  reject(new Error('Unavailable'));
  await refresh;
  expect(store.getState()).toMatchObject({ loading: false, error: 'Error: Unavailable' });
  expect(store.getState().applications).toHaveLength(1);
  vi.mocked(api.listSystemApplications).mockResolvedValue([]);
  await store.getState().load(true);
  expect(store.getState()).toMatchObject({ applications: [], loaded: true, error: null });
});

it('reuses a successful empty inventory and retries an initial failure', async () => {
  vi.mocked(api.listSystemApplications)
    .mockRejectedValueOnce(new Error('Unavailable'))
    .mockResolvedValue([]);
  await store.getState().load();
  expect(store.getState().loaded).toBe(false);
  await store.getState().load();
  await store.getState().load();
  expect(api.listSystemApplications).toHaveBeenCalledTimes(2);
});

it('leaves icon work to the active page and invalidates icons on explicit refresh', async () => {
  vi.useFakeTimers();
  try {
    vi.mocked(api.listSystemApplications).mockResolvedValue([
      { id: 'one', name: 'One', path: '/one' },
      { id: 'two', name: 'Two', path: '/two' },
    ]);
    await store.getState().load();
    expect(store.getState().applications).toHaveLength(2);
    expect(preloadSystemApplicationIcons).not.toHaveBeenCalled();
    await vi.runAllTimersAsync();
    expect(preloadSystemApplicationIcons).not.toHaveBeenCalled();
    await store.getState().load();
    expect(preloadSystemApplicationIcons).not.toHaveBeenCalled();
    await store.getState().load(true);
    expect(invalidateSystemApplicationIcons).toHaveBeenCalledOnce();
    await vi.runAllTimersAsync();
    expect(preloadSystemApplicationIcons).not.toHaveBeenCalled();
  } finally {
    vi.useRealTimers();
  }
});

afterEach(() => vi.unstubAllGlobals());

it('persists only IDs, deduplicates repeated adds and keeps inventory when hiding', () => {
  const values = new Map<string, string>();
  const setItem = vi.fn((key: string, value: string) => values.set(key, value));
  vi.stubGlobal('localStorage', { getItem: (key: string) => values.get(key) ?? null, setItem });
  store.setState({
    applications: [{ id: 'system:one', name: 'One', installed: true, category: 'Desktop' }],
  });
  store.getState().setInMyApplications('system:one', true);
  store.getState().setInMyApplications('system:one', true);
  expect(store.getState().myApplicationIds).toEqual(['system:one']);
  expect(setItem).toHaveBeenCalledExactlyOnceWith(
    'echobird_my_system_applications',
    '["system:one"]'
  );
  store.getState().setInMyApplications('system:one', false);
  expect(store.getState().myApplicationIds).toEqual([]);
  expect(store.getState().applications).toHaveLength(1);
  expect(values.get('echobird_my_system_applications')).toBe('[]');
  expect(values.get('echobird_hidden_system_applications')).toBe('["system:one"]');
  expect(api.listSystemApplications).not.toHaveBeenCalled();
});

it.each([
  ['["system:one", "system:one", null, 7, ""]', ['system:one']],
  ['{}', []],
  ['broken', []],
])('hydrates valid display IDs on a new session (%s)', async (raw, expected) => {
  vi.stubGlobal('localStorage', { getItem: () => raw });
  vi.resetModules();
  const { useSystemApplicationsStore: nextSession } = await import('./systemApplicationsStore');
  expect(nextSession.getState().myApplicationIds).toEqual(expected);
  expect(nextSession.getState().applications).toEqual([]);
  expect(api.listSystemApplications).not.toHaveBeenCalled();
});

it('keeps session choices usable if preference storage is unavailable', () => {
  vi.stubGlobal('localStorage', {
    setItem: () => {
      throw new Error('Storage denied');
    },
  });
  expect(() => store.getState().setInMyApplications('system:one', true)).not.toThrow();
  expect(store.getState().myApplicationIds).toEqual(['system:one']);
});

it('remembers hiding a default entry across sessions and clears that override when restored', async () => {
  const values = new Map<string, string>();
  const setItem = vi.fn((key: string, value: string) => values.set(key, value));
  vi.stubGlobal('localStorage', { getItem: (key: string) => values.get(key) ?? null, setItem });
  store.getState().setInMyApplications('system:default', false);
  store.getState().setInMyApplications('system:default', false);
  expect(setItem).toHaveBeenCalledTimes(2);
  vi.resetModules();
  const { useSystemApplicationsStore: nextSession } = await import('./systemApplicationsStore');
  expect(nextSession.getState().myApplicationIds).toEqual([]);
  expect(nextSession.getState().hiddenApplicationIds).toEqual(['system:default']);
  nextSession.getState().setInMyApplications('system:default', true);
  expect(nextSession.getState().hiddenApplicationIds).toEqual([]);
  expect(values.get('echobird_hidden_system_applications')).toBe('[]');
});
