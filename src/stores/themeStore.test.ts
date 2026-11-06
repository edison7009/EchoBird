import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import startupHtml from '../../index.html?raw';

const api = vi.hoisted(() => ({ getSettings: vi.fn(), saveSettings: vi.fn() }));
vi.mock('../api/tauri', () => api);
const events = vi.hoisted(() => ({ emit: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => events);

let store: typeof import('./themeStore').useThemeStore;
let storage: Map<string, string>;
let root: {
  dataset: { theme?: string };
  style: { setProperty: ReturnType<typeof vi.fn>; removeProperty: ReturnType<typeof vi.fn> };
};
let media: { matches: boolean; addEventListener: ReturnType<typeof vi.fn> };

beforeEach(async () => {
  vi.resetModules();
  vi.resetAllMocks();
  storage = new Map();
  root = { dataset: { theme: 'dark' }, style: { setProperty: vi.fn(), removeProperty: vi.fn() } };
  media = { matches: false, addEventListener: vi.fn() };
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => storage.get(key) ?? null,
    setItem: (key: string, value: string) => storage.set(key, value),
  });
  vi.stubGlobal('window', { matchMedia: () => media });
  vi.stubGlobal('document', {
    documentElement: root,
    head: { appendChild: vi.fn() },
    body: { offsetHeight: 0 },
    createElement: () => ({ textContent: '', remove: vi.fn() }),
  });
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => callback(0));
  api.getSettings.mockResolvedValue({ themeMode: 'dark' });
  api.saveSettings.mockResolvedValue(undefined);
  events.emit.mockResolvedValue(undefined);
  store = (await import('./themeStore')).useThemeStore;
});

afterEach(() => vi.unstubAllGlobals());

describe('default theme and saved preferences', () => {
  it('starts dark on a light OS before settings load and keeps the first palette', async () => {
    expect(store.getState()).toMatchObject({
      mode: 'dark',
      resolved: 'dark',
      colorTheme: 'oatgray',
    });
    await store.getState().init();
    expect(store.getState()).toMatchObject({
      mode: 'dark',
      resolved: 'dark',
      colorTheme: 'oatgray',
    });
    expect(root.dataset.theme).toBe('dark');
    expect(root.style.setProperty).toHaveBeenCalledWith('--bg-base-rgb', '28 29 28');
    expect(root.style.setProperty).toHaveBeenCalledWith(
      '--reasoning-accent-rgb',
      'var(--accent-rgb)'
    );
    expect(root.style.setProperty).toHaveBeenCalledWith(
      '--reasoning-accent-on-light',
      'rgb(var(--accent-rgb))'
    );
    media.addEventListener.mock.calls[0][1]();
    expect(store.getState().resolved).toBe('dark');
    expect(api.saveSettings).not.toHaveBeenCalled();
  });

  it.each(['light', 'dark'] as const)('restores saved %s mode and palette', async (mode) => {
    api.getSettings.mockResolvedValue({ themeMode: mode, colorTheme: 'aurora' });
    await store.getState().init();
    expect(store.getState()).toMatchObject({ mode, resolved: mode, colorTheme: 'aurora' });
    expect(root.style.setProperty).toHaveBeenCalledWith('--tooltip-bg-rgb', '27 55 59');
    expect(root.style.setProperty).toHaveBeenCalledWith(
      '--reasoning-accent-rgb',
      mode === 'light' ? '90 217 195' : '90 204 217'
    );
    expect(root.style.setProperty).toHaveBeenCalledWith(
      '--view-mode-label-rgb',
      'var(--text-primary-rgb)'
    );
    expect(root.dataset.theme).toBe(mode);
    expect(api.saveSettings).not.toHaveBeenCalled();
  });

  it.each([undefined, null])(
    'preserves legacy system mode represented by %s',
    async (themeMode) => {
      api.getSettings.mockResolvedValue({ themeMode });
      await store.getState().init();
      expect(store.getState()).toMatchObject({ mode: 'system', resolved: 'light' });
      media.matches = true;
      media.addEventListener.mock.calls[0][1]();
      expect(root.dataset.theme).toBe('dark');
      expect(api.saveSettings).not.toHaveBeenCalled();
    }
  );

  it('persists an explicit system choice and restores it after restart', async () => {
    await store.getState().init();
    store.getState().setMode('system');
    expect(events.emit).toHaveBeenCalledWith('echobird-theme-changed', {
      mode: 'system',
      colorTheme: 'oatgray',
    });
    await vi.waitFor(() => expect(api.saveSettings).toHaveBeenCalledOnce());
    const saved = api.saveSettings.mock.calls[0][0];
    expect(saved.themeMode).toBeNull();
    expect(root.dataset.theme).toBe('light');
    api.getSettings.mockResolvedValue(saved);
    vi.resetModules();
    store = (await import('./themeStore')).useThemeStore;
    await store.getState().init();
    expect(store.getState()).toMatchObject({ mode: 'system', resolved: 'light' });
  });

  it('broadcasts an explicit palette change with the current appearance mode', async () => {
    await store.getState().init();
    store.getState().setColorTheme('aurora');
    expect(events.emit).toHaveBeenCalledExactlyOnceWith('echobird-theme-changed', {
      mode: 'dark',
      colorTheme: 'aurora',
    });
    await vi.waitFor(() => expect(api.saveSettings).toHaveBeenCalledOnce());
    expect(api.saveSettings.mock.calls[0][0]).toMatchObject({
      themeMode: 'dark',
      colorTheme: 'aurora',
    });
  });

  it.each(['light', 'dark'] as const)(
    'keeps the neutral palette tab wave on the theme accent in %s mode',
    async (mode) => {
      await store.getState().init();
      store.getState().sync({ mode, colorTheme: 'oatgray' });
      expect(root.style.setProperty).toHaveBeenCalledWith(
        '--view-mode-accent-rgb',
        'var(--accent-rgb)'
      );
      expect(root.style.setProperty).toHaveBeenCalledWith(
        '--view-mode-wave-rgb',
        'var(--view-mode-accent-rgb)'
      );
      expect(root.style.setProperty).toHaveBeenCalledWith('--view-mode-label-rgb', '255 255 255');
      if (mode === 'light')
        expect(root.style.setProperty).toHaveBeenCalledWith('--bg-base-rgb', '255 255 255');
      expect(api.saveSettings).not.toHaveBeenCalled();
    }
  );

  it('syncs another window without rereading, saving or rebroadcasting settings', async () => {
    await store.getState().init();
    api.getSettings.mockClear();
    store.getState().sync({ mode: 'light', colorTheme: 'aurora' });
    expect(store.getState()).toMatchObject({
      mode: 'light',
      resolved: 'light',
      colorTheme: 'aurora',
    });
    expect(root.dataset.theme).toBe('light');
    expect(root.style.setProperty).toHaveBeenCalledWith('--bg-base-rgb', '243 249 247');
    expect(api.getSettings).not.toHaveBeenCalled();
    expect(api.saveSettings).not.toHaveBeenCalled();
    expect(events.emit).not.toHaveBeenCalled();
    store.getState().sync({ mode: 'system', colorTheme: 'forest' });
    media.matches = true;
    media.addEventListener.mock.calls[0][1]();
    expect(store.getState()).toMatchObject({
      mode: 'system',
      resolved: 'dark',
      colorTheme: 'forest',
    });
    expect(api.saveSettings).not.toHaveBeenCalled();
    expect(events.emit).not.toHaveBeenCalled();
  });
});

describe('startup theme before React mounts', () => {
  it.each([null, 'light', 'dark'])('uses cached %s or defaults to dark on a light OS', (cached) => {
    if (cached) storage.set('theme', cached);
    delete root.dataset.theme;
    const script = startupHtml.match(/<script>([\s\S]*?)<\/script>/)![1];
    new Function('localStorage', 'document', 'window', script)(localStorage, document, window);
    expect(root.dataset.theme).toBe(cached ?? 'dark');
  });
});

describe('plugin palettes', () => {
  const palette = {
    id: 'ocean',
    title: 'Ocean',
    light: { accent: '#112233', background: '#F0F1F2' },
    dark: { accent: '#445566', background: '#010203' },
  };
  it('applies approved tokens and restores the original palette and CSS defaults when removed', async () => {
    await store.getState().init();
    store.getState().setPluginTheme(palette);
    expect(root.style.setProperty).toHaveBeenCalledWith('--accent-rgb', '68 85 102');
    expect(root.style.setProperty).toHaveBeenCalledWith('--bg-base-rgb', '1 2 3');
    expect(root.style.setProperty).toHaveBeenCalledWith('--tooltip-bg-rgb', '1 2 3');
    expect(api.saveSettings).not.toHaveBeenCalled();
    store.getState().setPluginTheme(null);
    expect(root.style.removeProperty).toHaveBeenCalledWith('--accent-rgb');
    expect(root.style.setProperty).toHaveBeenCalledWith('--tooltip-bg-rgb', '37 38 37');
    expect(root.style.setProperty).toHaveBeenLastCalledWith(
      '--view-mode-wave-rgb',
      'var(--view-mode-accent-rgb)'
    );
    expect(store.getState().colorTheme).toBe('oatgray');
  });
  it('follows light/dark without overwriting the saved base color and clears when the user picks a built-in palette', async () => {
    await store.getState().init();
    store.getState().setPluginTheme(palette);
    store.getState().setMode('light');
    expect(root.style.setProperty).toHaveBeenCalledWith('--accent-rgb', '17 34 51');
    expect(root.style.setProperty).toHaveBeenCalledWith('--tooltip-bg-rgb', '1 2 3');
    store.getState().setColorTheme('aurora');
    expect(events.emit).toHaveBeenLastCalledWith('echobird-theme-changed', {
      mode: 'light',
      colorTheme: 'aurora',
      pluginTheme: null,
    });
    await Promise.resolve();
    await Promise.resolve();
    expect(api.saveSettings).toHaveBeenCalledWith(
      expect.objectContaining({ colorTheme: 'aurora' })
    );
  });
  it('synchronizes a plugin palette across windows even if mode and built-in palette stay the same', () => {
    store.getState().sync({ mode: 'dark', colorTheme: 'oatgray', pluginTheme: palette });
    expect(root.style.setProperty).toHaveBeenCalledWith('--accent-rgb', '68 85 102');
    store.getState().sync({ mode: 'dark', colorTheme: 'oatgray', pluginTheme: null });
    expect(root.style.removeProperty).toHaveBeenCalledWith('--accent-rgb');
  });
});
