import { beforeEach, expect, it, vi } from 'vitest';
import * as api from '../api/plugins';
import type { InstalledPlugin, PluginCandidate, PluginRegistry } from '../api/plugins';
import { pluginTools, usePluginsStore as store } from './pluginsStore';
import { useNavigationStore } from './navigationStore';

vi.mock('../api/plugins', () => ({
  listPlugins: vi.fn(),
  getPluginRegistry: vi.fn(),
  previewPlugin: vi.fn(),
  cancelPlugin: vi.fn(),
  installPlugin: vi.fn(),
  setPluginEnabled: vi.fn(),
  uninstallPlugin: vi.fn(),
}));

const plugin: InstalledPlugin = {
  id: 'fixture.game',
  enabled: true,
  directory: 'packages/fixture.game/version',
  source: '/fixture',
  revision: null,
  icon: null,
  manifest: {
    name: 'game',
    publisher: 'fixture',
    version: '1.0.0',
    displayName: 'Game',
    description: 'Fixture',
    license: 'MIT',
    repository: 'https://example.com/fixture',
    echobird: {
      manifestVersion: 1,
      apiVersion: '0.1-draft',
      platforms: ['windows', 'macos', 'linux'],
      permissions: ['model.request'],
      contributes: { pages: [{ id: 'game', title: 'Game', entry: 'ui/index.html' }] },
    },
  },
};

const updateCandidate: PluginCandidate = {
  token: 'reviewed-update',
  manifest: { ...plugin.manifest, version: '1.1.0' },
  source: plugin.source,
  revision: null,
};

beforeEach(() => {
  useNavigationStore.getState().setDeveloperMode(true);
  store.setState(store.getInitialState(), true);
  vi.clearAllMocks();
  vi.mocked(api.previewPlugin).mockResolvedValue(updateCandidate);
  vi.mocked(api.cancelPlugin).mockResolvedValue(undefined);
});

it('starts no plugin operation while developer preview is paused', async () => {
  useNavigationStore.getState().setDeveloperMode(false);
  await store.getState().load();
  await store.getState().refresh();
  await store.getState().install('stale');
  await store.getState().enable('fixture.game', true);
  await store.getState().uninstall('fixture.game');
  for (const call of Object.values(api)) expect(call).not.toHaveBeenCalled();
});

it.each([
  ['zh-Hans', 'china'],
  ['zh-CN', 'china'],
  ['zh-Hant', 'international'],
  ['zh-TW', 'international'],
  ['en', 'international'],
  ['ja', 'international'],
])('defaults the plugin source from interface locale %s', (locale, region) => {
  store.setState({ regionChosen: false });
  store.getState().syncRegion(locale);
  expect(store.getState().region).toBe(region);
  expect(api.getPluginRegistry).not.toHaveBeenCalled();
});

it('retains a manual source choice across interface language changes', () => {
  const setItem = vi.fn();
  vi.stubGlobal('localStorage', { setItem });
  try {
    store.getState().syncRegion('zh-Hans');
    store.getState().setRegion('international');
    store.getState().syncRegion('zh-Hans');
    expect(store.getState().region).toBe('international');
    expect(setItem).toHaveBeenCalledWith('echobird_plugin_region', 'international');
  } finally {
    vi.unstubAllGlobals();
  }
});

it('does not let old loads clear pending state or restore rows after pause and resume', async () => {
  let finishOld!: (value: InstalledPlugin[]) => void;
  let finishNew!: (value: InstalledPlugin[]) => void;
  vi.mocked(api.listPlugins).mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finishOld = resolve;
      })
  );
  const old = store.getState().load();
  useNavigationStore.getState().setDeveloperMode(false);
  useNavigationStore.getState().setDeveloperMode(true);
  vi.mocked(api.listPlugins).mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finishNew = resolve;
      })
  );
  const current = store.getState().load();
  finishOld([plugin]);
  await old;
  expect(store.getState().installed).toEqual([]);
  expect(store.getState().loading).toBe(true);
  finishNew([plugin]);
  await current;
  expect(store.getState().installed).toEqual([plugin]);
  expect(store.getState().loading).toBe(false);
});

it('does not apply a late mutation from a previous activation', async () => {
  let finish!: (value: InstalledPlugin[]) => void;
  vi.mocked(api.installPlugin).mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      })
  );
  const pending = store.getState().install('candidate');
  useNavigationStore.getState().setDeveloperMode(false);
  useNavigationStore.getState().setDeveloperMode(true);
  finish([plugin]);
  await pending;
  expect(store.getState().installed).toEqual([]);
  expect(store.getState().busy).toBe(false);
});

it('only enabled, installed pages become desktop tools without overwriting built-in identities', () => {
  expect(pluginTools([{ ...plugin, enabled: false }])).toEqual([]);
  expect(pluginTools([plugin])).toEqual([
    expect.objectContaining({
      id: 'plugin:fixture.game:game',
      plugin: { id: 'fixture.game', pageId: 'game' },
      installed: true,
      noModelConfig: false,
    }),
  ]);
  const noPermission = structuredClone(plugin);
  noPermission.manifest.echobird.permissions = [];
  expect(pluginTools([noPermission])[0].noModelConfig).toBe(true);
});

it('passive loading reads installed metadata without fetching the Hub or changing enablement', async () => {
  // The host parses the bundled index for this system; nothing is downloaded.
  const local: PluginRegistry = { schemaVersion: 1, name: 'EchoBird Plugin Hub', plugins: [] };
  local.plugins.push({ id: 'fixture.local', repository: 'https://example.com', releases: [] });
  store.setState({ region: 'china' });
  vi.mocked(api.listPlugins).mockResolvedValue([plugin]);
  vi.mocked(api.getPluginRegistry).mockResolvedValue(local);
  await store.getState().load();
  expect(store.getState().installed).toEqual([plugin]);
  expect(store.getState().registry).toBe(local);
  expect(api.getPluginRegistry).toHaveBeenCalledExactlyOnceWith('china', false);
  await store.getState().load();
  expect(api.getPluginRegistry).toHaveBeenCalledOnce();
  expect(api.setPluginEnabled).not.toHaveBeenCalled();
  expect(api.installPlugin).not.toHaveBeenCalled();
});

it('a failed bundled index read never hides installed plugins or shows an error', async () => {
  vi.mocked(api.listPlugins).mockResolvedValue([plugin]);
  vi.mocked(api.getPluginRegistry).mockRejectedValue(new Error('unreadable'));
  await store.getState().load();
  expect(store.getState().installed).toEqual([plugin]);
  expect(store.getState().registry.plugins).toEqual([]);
  expect(store.getState().error).toBeNull();
  expect(store.getState().loading).toBe(false);
});

it('failed loading preserves cached entries and explicit failures do not invent success', async () => {
  store.setState({ installed: [plugin] });
  vi.mocked(api.listPlugins).mockRejectedValue(new Error('unavailable'));
  await store.getState().load();
  expect(store.getState().installed).toEqual([plugin]);
  vi.mocked(api.setPluginEnabled).mockRejectedValue(new Error('save failed'));
  await expect(store.getState().enable(plugin.id, false)).rejects.toThrow('save failed');
  expect(store.getState().installed[0].enabled).toBe(true);
});

it('a late passive result cannot restore an uninstalled plugin', async () => {
  let finish!: (plugins: InstalledPlugin[]) => void;
  vi.mocked(api.listPlugins).mockReturnValue(
    new Promise((resolve) => {
      finish = resolve;
    })
  );
  vi.mocked(api.uninstallPlugin).mockResolvedValue([]);
  const loading = store.getState().load();
  await store.getState().uninstall(plugin.id);
  finish([plugin]);
  await loading;
  expect(store.getState().installed).toEqual([]);
});

it('only explicit refresh fetches the selected regional index and keeps the prior index on failure', async () => {
  store.setState({ region: 'china' });
  vi.mocked(api.listPlugins).mockResolvedValue([plugin]);
  vi.mocked(api.getPluginRegistry).mockRejectedValue(new Error('offline'));
  const previous = store.getState().registry;
  await store.getState().refresh();
  expect(api.getPluginRegistry).toHaveBeenCalledExactlyOnceWith('china', true);
  expect(store.getState().registry).toBe(previous);
  expect(store.getState().error).toContain('offline');
});

it('updates a newer local package once without requesting unchanged permissions or changing enablement', async () => {
  const updated = { ...plugin, enabled: false, manifest: updateCandidate.manifest };
  store.setState({ installed: [{ ...plugin, enabled: false }], region: 'international' });
  vi.mocked(api.installPlugin).mockResolvedValue([updated]);
  const approve = vi.fn();
  await expect(store.getState().update(plugin.id, approve)).resolves.toBe('updated');
  expect(api.previewPlugin).toHaveBeenCalledExactlyOnceWith(
    'directory',
    '/fixture',
    'international'
  );
  expect(api.installPlugin).toHaveBeenCalledExactlyOnceWith(updateCandidate.token);
  expect(api.cancelPlugin).toHaveBeenCalledExactlyOnceWith(updateCandidate.token);
  expect(approve).not.toHaveBeenCalled();
  expect(api.setPluginEnabled).not.toHaveBeenCalled();
  expect(store.getState().installed).toEqual([updated]);
  expect(store.getState().busy).toBe(false);
});

it('does not install an unchanged version and cleans up its reviewed candidate', async () => {
  store.setState({ installed: [plugin] });
  vi.mocked(api.previewPlugin).mockResolvedValue({ ...updateCandidate, manifest: plugin.manifest });
  const approve = vi.fn();
  await expect(store.getState().update(plugin.id, approve)).resolves.toBe('current');
  expect(api.installPlugin).not.toHaveBeenCalled();
  expect(approve).not.toHaveBeenCalled();
  expect(api.cancelPlugin).toHaveBeenCalledExactlyOnceWith(updateCandidate.token);
});

it('leaves the installed version intact when newly requested permissions are declined', async () => {
  store.setState({ installed: [plugin] });
  const candidate = {
    ...updateCandidate,
    manifest: {
      ...updateCandidate.manifest,
      echobird: {
        ...updateCandidate.manifest.echobird,
        permissions: ['model.request', 'clipboard.write'],
      },
    },
  };
  vi.mocked(api.previewPlugin).mockResolvedValue(candidate);
  const approve = vi.fn(async () => false);
  await expect(store.getState().update(plugin.id, approve)).resolves.toBe('cancelled');
  expect(approve).toHaveBeenCalledExactlyOnceWith(candidate);
  expect(api.installPlugin).not.toHaveBeenCalled();
  expect(api.cancelPlugin).toHaveBeenCalledExactlyOnceWith(candidate.token);
  expect(store.getState().installed).toEqual([plugin]);
});

it('rejects a mismatched update identity before installation or permission approval', async () => {
  store.setState({ installed: [plugin] });
  vi.mocked(api.previewPlugin).mockResolvedValue({
    ...updateCandidate,
    manifest: { ...updateCandidate.manifest, publisher: 'other' },
  });
  const approve = vi.fn();
  await expect(store.getState().update(plugin.id, approve)).rejects.toThrow('identity');
  expect(api.installPlugin).not.toHaveBeenCalled();
  expect(approve).not.toHaveBeenCalled();
  expect(api.cancelPlugin).toHaveBeenCalledExactlyOnceWith(updateCandidate.token);
  expect(store.getState().installed).toEqual([plugin]);
});

it('cancels a late update preview after pausing and resuming without installing it', async () => {
  store.setState({ installed: [plugin] });
  let resolve!: (value: PluginCandidate) => void;
  vi.mocked(api.previewPlugin).mockReturnValueOnce(new Promise((done) => (resolve = done)));
  const pending = store.getState().update(plugin.id, vi.fn());
  useNavigationStore.getState().setDeveloperMode(false);
  useNavigationStore.getState().setDeveloperMode(true);
  resolve(updateCandidate);
  await expect(pending).resolves.toBe('cancelled');
  expect(api.installPlugin).not.toHaveBeenCalled();
  expect(api.cancelPlugin).toHaveBeenCalledExactlyOnceWith(updateCandidate.token);
  expect(store.getState().installed).toEqual([plugin]);
  expect(store.getState().busy).toBe(false);
});

it('checks the current regional catalog before previewing an update', async () => {
  const registry: PluginRegistry = {
    schemaVersion: 1,
    name: 'Fixture',
    plugins: [
      {
        id: plugin.id,
        repository: plugin.manifest.repository,
        releases: [
          {
            manifest: plugin.manifest,
            commit: 'a'.repeat(40),
            sha256: 'b'.repeat(64),
            downloads: {},
          },
        ],
      },
    ],
  };
  store.setState({ installed: [plugin], registry, region: 'china' });
  vi.mocked(api.getPluginRegistry).mockResolvedValue(registry);
  await expect(store.getState().update(plugin.id, vi.fn())).resolves.toBe('current');
  expect(api.getPluginRegistry).toHaveBeenCalledExactlyOnceWith('china', true);
  expect(api.previewPlugin).not.toHaveBeenCalled();
  expect(api.installPlugin).not.toHaveBeenCalled();
});
