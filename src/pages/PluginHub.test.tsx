import { type ReactTestInstance, act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import * as api from '../api/plugins';
import type { InstalledPlugin, PluginCandidate, PluginRegistry } from '../api/plugins';
import { usePluginsStore as store } from '../stores/pluginsStore';
import { PluginHubMain, PluginHubPanel } from './PluginHub';
import { pluginCategories } from './PluginHub';
import { ConfirmDialogProvider } from '../components/ConfirmDialog';
import { useNavigationStore } from '../stores/navigationStore';
import bundledIndex from '../data/pluginIndex.json';

const bundledRegistry = bundledIndex as PluginRegistry;

const i18n = vi.hoisted(() => ({ locale: 'en' }));
vi.mock('../hooks/useI18n', () => ({
  useI18n: () => ({ locale: i18n.locale, t: (key: string) => key }),
}));
vi.mock('../api/plugins', () => ({
  listPlugins: vi.fn().mockResolvedValue([]),
  getPluginRegistry: vi.fn(),
  previewPlugin: vi.fn(),
  cancelPlugin: vi.fn().mockResolvedValue(undefined),
  installPlugin: vi.fn(),
  setPluginEnabled: vi.fn(),
  uninstallPlugin: vi.fn(),
  launchPlugin: vi.fn(),
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
const showToast = vi.hoisted(() => vi.fn());
vi.mock('../components/Toast', () => ({ useToast: () => ({ showToast }) }));

const candidate: PluginCandidate = {
  token: 'fixture-preview',
  source: 'https://example.com/plugin',
  revision: null,
  manifest: {
    name: 'game',
    publisher: 'fixture',
    version: '1.0.0',
    displayName: 'Fixture Game',
    description: 'A game fixture',
    repository: 'https://example.com/plugin',
    license: 'MIT',
    echobird: {
      manifestVersion: 1,
      apiVersion: '0.1-draft',
      platforms: ['windows', 'macos', 'linux'],
      permissions: ['model.request'],
      contributes: { pages: [{ id: 'game', title: 'Game', entry: 'ui/index.html' }] },
    },
  },
};
const installed: InstalledPlugin = {
  id: 'fixture.game',
  manifest: candidate.manifest,
  enabled: true,
  directory: 'packages/fixture.game/one',
  icon: null,
  source: candidate.source,
  revision: null,
};
let view: ReactTestRenderer;
const button = (text: string) =>
  view.root.findAllByType('button').find((node) => node.props.children === text)!;

beforeEach(() => {
  i18n.locale = 'en';
  vi.stubGlobal('document', { addEventListener: vi.fn(), removeEventListener: vi.fn() });
  useNavigationStore.getState().setDeveloperMode(true);
  vi.clearAllMocks();
  store.setState(store.getInitialState(), true);
  store.setState({
    region: 'international',
    registry: {
      schemaVersion: 1,
      name: 'EchoBird Plugin Hub',
      plugins: [
        {
          id: 'fixture.game',
          repository: candidate.source,
          releases: [
            {
              manifest: candidate.manifest,
              commit: 'a'.repeat(40),
              sha256: '0'.repeat(64),
              downloads: {},
            },
          ],
        },
      ],
    },
  });
  vi.mocked(api.listPlugins).mockResolvedValue([]);
});
afterEach(() => {
  act(() => view?.unmount());
  vi.unstubAllGlobals();
});

const mount = async () =>
  act(async () => {
    view = create(<PluginHubMain active />, {
      createNodeMock: (element) => (element.type === 'dialog' ? { showModal: vi.fn() } : null),
    });
  });

it.each([false, true])(
  'uses source descriptions in cards, search and details independently of interface language (installed: %s)',
  async (isInstalled) => {
    vi.mocked(api.listPlugins).mockResolvedValue(isInstalled ? [installed] : []);
    const registry = structuredClone(store.getState().registry);
    registry.plugins[0].descriptions = {
      international: 'English source description',
      china: '中文来源说明',
    };
    store.setState({
      registry,
      installed: isInstalled ? [installed] : [],
      selected: installed.id,
      region: 'international',
      regionChosen: true,
    });
    const render = () => (
      <ConfirmDialogProvider>
        <PluginHubMain active />
        <PluginHubPanel />
      </ConfirmDialogProvider>
    );
    await act(async () => {
      view = create(render());
    });
    const descriptions = () => view.root.findAllByType('p').map((node) => node.props.children);
    for (const locale of ['en', 'zh-Hans']) {
      i18n.locale = locale;
      act(() => view.update(render()));
      expect(descriptions().filter((text) => text === 'English source description')).toHaveLength(
        2
      );
      expect(descriptions()).not.toContain('中文来源说明');
    }
    act(() => store.setState({ region: 'china' }));
    for (const locale of ['en', 'zh-Hans']) {
      i18n.locale = locale;
      act(() => view.update(render()));
      expect(descriptions().filter((text) => text === '中文来源说明')).toHaveLength(2);
      expect(descriptions()).not.toContain('English source description');
    }
    act(() =>
      view.root
        .findByProps({ 'aria-label': 'plugins.search' })
        .props.onChange({ target: { value: '中文来源' } })
    );
    expect(view.root.findAllByType('article')).toHaveLength(1);
    act(() => store.setState({ region: 'cnb' }));
    expect(view.root.findAllByType('article')).toHaveLength(1);
    expect(descriptions()).toContain('中文来源说明');
    act(() => store.setState({ region: 'international' }));
    expect(view.root.findAllByType('article')).toHaveLength(0);
    act(() =>
      view.root
        .findByProps({ 'aria-label': 'plugins.search' })
        .props.onChange({ target: { value: candidate.manifest.description } })
    );
    expect(view.root.findAllByType('article')).toHaveLength(1);
    act(() => {
      const fallback = structuredClone(registry);
      delete fallback.plugins[0].descriptions;
      store.setState({ registry: fallback });
      view.root
        .findByProps({ 'aria-label': 'plugins.search' })
        .props.onChange({ target: { value: '' } });
    });
    expect(descriptions().filter((text) => text === candidate.manifest.description)).toHaveLength(
      2
    );
    expect(store.getState().registry.plugins[0].releases[0].manifest).toEqual(candidate.manifest);
    expect(api.previewPlugin).not.toHaveBeenCalled();
    expect(api.installPlugin).not.toHaveBeenCalled();
    expect(api.launchPlugin).not.toHaveBeenCalled();
    expect(api.getPluginRegistry).not.toHaveBeenCalled();
  }
);

it('keeps the notice across categories and remembers dismissal when the page reopens', async () => {
  const saved = new Map<string, string>();
  const setItem = vi.fn((key: string, value: string) => saved.set(key, value));
  vi.stubGlobal('localStorage', { getItem: (key: string) => saved.get(key) ?? null, setItem });
  await mount();
  expect(view.root.findAllByProps({ className: 'ph-account-notice' })).toHaveLength(1);
  act(() => button('plugins.category.account').props.onClick());
  expect(view.root.findAllByProps({ className: 'ph-account-notice' })).toHaveLength(1);
  expect(setItem).not.toHaveBeenCalled();
  act(() => button('plugins.accountNoticeDismiss').props.onClick());
  expect(view.root.findAllByProps({ className: 'ph-account-notice' })).toHaveLength(0);
  expect(setItem).toHaveBeenCalledWith('echobird-plugin-account-notice-dismissed', 'true');
  act(() => view.unmount());
  await mount();
  expect(view.root.findAllByProps({ className: 'ph-account-notice' })).toHaveLength(0);
  expect(api.previewPlugin).not.toHaveBeenCalled();
  expect(api.installPlugin).not.toHaveBeenCalled();
  expect(api.launchPlugin).not.toHaveBeenCalled();
});

it('allows dismissing the notice when local preferences are unavailable', async () => {
  vi.stubGlobal('localStorage', {
    getItem: () => {
      throw new Error('storage unavailable');
    },
    setItem: () => {
      throw new Error('storage unavailable');
    },
  });
  await mount();
  expect(view.root.findAllByProps({ className: 'ph-account-notice' })).toHaveLength(1);
  act(() => button('plugins.accountNoticeDismiss').props.onClick());
  expect(view.root.findAllByProps({ className: 'ph-account-notice' })).toHaveLength(0);
});

it('browsing, searching and changing the list selection never install or execute plugins', async () => {
  await mount();
  act(() =>
    view.root.findByProps({ 'aria-pressed': false, className: 'ph-card-select' }).props.onClick()
  );
  act(() =>
    view.root
      .findByProps({ 'aria-label': 'plugins.search' })
      .props.onChange({ target: { value: 'missing' } })
  );
  expect(JSON.stringify(view.toJSON())).toContain('plugins.empty');
  expect(api.previewPlugin).not.toHaveBeenCalled();
  expect(api.installPlugin).not.toHaveBeenCalled();
  expect(api.launchPlugin).not.toHaveBeenCalled();
  expect(api.getPluginRegistry).not.toHaveBeenCalled();
});

it('places page plugins in Apps and the native modules in Account login without running them', async () => {
  store.setState({ registry: bundledRegistry });
  await mount();
  const categories = view.root.findByProps({
    className: 'ph-categories relative mb-5 flex shrink-0 flex-wrap gap-1',
  });
  expect(categories.findAllByType('button').map((node) => node.props.children)).toEqual([
    'toolCat.all',
    'plugins.category.application',
    'plugins.category.account',
    'plugins.category.appearance',
    'plugins.category.feature',
    'plugins.category.other',
  ]);
  act(() => button('plugins.category.application').props.onClick());
  expect(
    view.root.findAllByType('article').map((node) => node.findByType('strong').props.children)
  ).toEqual(
    bundledRegistry.plugins
      .filter(
        (plugin) =>
          plugin.releases[0].manifest.echobird.contributes.pages.length ||
          plugin.releases[0].manifest.category === 'application'
      )
      .map((plugin) => plugin.releases[0].manifest.displayName)
  );
  expect(view.root.findAllByType('article')).toHaveLength(3);
  act(() => button('plugins.category.account').props.onClick());
  expect(view.root.findAllByType('article')).toHaveLength(9);
  expect(
    view.root.findAllByType('article').map((node) => node.findByType('strong').props.children)
  ).toEqual(
    bundledRegistry.plugins
      .filter((plugin) =>
        (plugin.releases[0].manifest.categories ?? [plugin.releases[0].manifest.category]).includes(
          'account'
        )
      )
      .map((plugin) => plugin.releases[0].manifest.displayName)
  );
  act(() => button('plugins.category.appearance').props.onClick());
  expect(
    view.root.findAllByType('article').map((node) => node.findByType('strong').props.children)
  ).toEqual(['Ocean Theme']);
  act(() => button('plugins.category.feature').props.onClick());
  expect(
    view.root.findAllByType('article').map((node) => node.findByType('strong').props.children)
  ).toEqual(['Text Tools']);
  act(() => button('toolCat.all').props.onClick());
  expect(view.root.findAllByType('article')).toHaveLength(bundledRegistry.plugins.length);
  expect(api.getPluginRegistry).not.toHaveBeenCalled();
  expect(api.previewPlugin).not.toHaveBeenCalled();
  expect(api.installPlugin).not.toHaveBeenCalled();
  expect(api.launchPlugin).not.toHaveBeenCalled();
});

it('combines categories with search on discover and installed while preserving selection', async () => {
  const registry = structuredClone(store.getState().registry);
  const provider = structuredClone(registry.plugins[0]);
  provider.id = 'fixture.provider';
  provider.releases[0].manifest = {
    ...provider.releases[0].manifest,
    name: 'provider',
    displayName: 'Provider',
    category: 'account',
  };
  registry.plugins.push(provider);
  const installedProvider = {
    ...installed,
    id: provider.id,
    manifest: provider.releases[0].manifest,
  };
  vi.mocked(api.listPlugins).mockResolvedValue([installed, installedProvider]);
  store.setState({
    registry,
    selected: 'fixture.game',
    installed: [installed, installedProvider],
  });
  await mount();
  act(() => button('plugins.category.account').props.onClick());
  expect(view.root.findAllByType('article')).toHaveLength(1);
  expect(view.root.findByType('strong').props.children).toBe('Provider');
  act(() => button('plugins.installed').props.onClick());
  expect(view.root.findAllByType('article')).toHaveLength(1);
  act(() =>
    view.root
      .findByProps({ 'aria-label': 'plugins.search' })
      .props.onChange({ target: { value: 'missing' } })
  );
  expect(view.root.findAllByType('article')).toHaveLength(0);
  act(() => view.root.findByProps({ 'aria-label': 'plugins.clear' }).props.onClick());
  act(() => button('plugins.category.application').props.onClick());
  expect(view.root.findByType('strong').props.children).toBe(candidate.manifest.displayName);
  expect(store.getState().selected).toBe('fixture.game');
  expect(api.getPluginRegistry).not.toHaveBeenCalled();
  expect(api.setPluginEnabled).not.toHaveBeenCalled();
  expect(api.uninstallPlugin).not.toHaveBeenCalled();
  expect(api.previewPlugin).not.toHaveBeenCalled();
});

it('opens all plugins with an empty search and the standard toast without installing or logging in', async () => {
  const registry = structuredClone(store.getState().registry);
  const provider = structuredClone(registry.plugins[0]);
  provider.id = 'fixture.account-provider';
  provider.releases[0].manifest = {
    ...provider.releases[0].manifest,
    name: 'account-provider',
    displayName: 'Community provider',
    category: 'account',
    echobird: {
      ...provider.releases[0].manifest.echobird,
      contributes: {
        ...provider.releases[0].manifest.echobird.contributes,
        providers: [
          {
            id: 'claude-code',
            title: 'Claude Code accounts',
            applications: ['claudecode'],
            pageId: 'game',
            searchTerms: ['Claude Code'],
          },
        ],
      },
    },
  };
  registry.plugins.push(provider);
  store.setState({ registry });
  await mount();
  act(() => button('plugins.category.account').props.onClick());
  act(() =>
    view.root
      .findByProps({ 'aria-label': 'plugins.search' })
      .props.onChange({ target: { value: 'missing' } })
  );
  act(() => button('plugins.installed').props.onClick());
  act(() => useNavigationStore.getState().openAccountPlugins());
  expect(view.root.findByProps({ 'aria-label': 'plugins.search' }).props.value).toBe('');
  expect(view.root.findAllByType('article')).toHaveLength(2);
  expect(showToast).toHaveBeenCalledExactlyOnceWith('info', 'plugins.loginRequiresPlugin');
  expect(useNavigationStore.getState().accountPluginsRequest).toBeNull();
  expect(
    view.root
      .findAllByProps({ 'aria-pressed': true })
      .some((node) => node.props.children === 'toolCat.all')
  ).toBe(true);
  expect(store.getState().region).toBe('international');
  expect(api.getPluginRegistry).not.toHaveBeenCalled();
  expect(api.previewPlugin).not.toHaveBeenCalled();
  expect(api.installPlugin).not.toHaveBeenCalled();
  expect(api.launchPlugin).not.toHaveBeenCalled();
  expect(api.setPluginEnabled).not.toHaveBeenCalled();
  expect(api.uninstallPlugin).not.toHaveBeenCalled();
});

it('keeps search with toolbar actions and shows the source selector only on discover', async () => {
  await mount();
  const toolbar = view.root.findByProps({ className: 'ph-toolbar' });
  expect(toolbar.findByProps({ 'aria-label': 'plugins.search' })).toBeTruthy();
  expect(toolbar.findByProps({ 'aria-label': 'plugins.refresh' })).toBeTruthy();
  expect(toolbar.findByProps({ 'aria-label': 'plugins.sources' }).props['aria-expanded']).toBe(
    false
  );
  act(() => toolbar.findByProps({ 'aria-label': 'plugins.sources' }).props.onClick());
  expect(view.root.findAllByProps({ role: 'menuitemradio' })[0].props['aria-checked']).toBe(true);
  act(() => view.root.findAllByProps({ role: 'menuitemradio' })[1].props.onClick());
  expect(store.getState().region).toBe('china');
  expect(view.root.findAllByProps({ role: 'menu' })).toHaveLength(0);
  act(() => toolbar.findByProps({ 'aria-label': 'plugins.sources' }).props.onClick());
  const sources = view.root.findAllByProps({ role: 'menuitemradio' });
  expect(sources).toHaveLength(3);
  act(() => sources[2].props.onClick());
  expect(store.getState().region).toBe('cnb');
  act(() => button('plugins.installed').props.onClick());
  expect(view.root.findAllByProps({ 'aria-label': 'plugins.sources' })).toHaveLength(0);
  expect(view.root.findAllByType('select')).toHaveLength(0);
  expect(api.getPluginRegistry).not.toHaveBeenCalled();
  expect(api.previewPlugin).not.toHaveBeenCalled();
});

it('closes the source menu on Escape, outside input and leaving discovery', async () => {
  await mount();
  const sourceButton = () => view.root.findByProps({ 'aria-label': 'plugins.sources' });
  act(() => sourceButton().props.onClick());
  const keydown = vi
    .mocked(document.addEventListener)
    .mock.calls.find(([name]) => name === 'keydown')![1] as (event: KeyboardEvent) => void;
  act(() => keydown({ key: 'Escape' } as KeyboardEvent));
  expect(sourceButton().props['aria-expanded']).toBe(false);
  act(() => sourceButton().props.onClick());
  const outside = vi
    .mocked(document.addEventListener)
    .mock.calls.find(([name]) => name === 'pointerdown')![1] as (event: PointerEvent) => void;
  act(() => outside({ target: {} } as PointerEvent));
  expect(sourceButton().props['aria-expanded']).toBe(false);
  act(() => sourceButton().props.onClick());
  act(() => button('plugins.installed').props.onClick());
  act(() => button('plugins.discover').props.onClick());
  expect(sourceButton().props['aria-expanded']).toBe(false);
  expect(api.previewPlugin).not.toHaveBeenCalled();
  expect(api.getPluginRegistry).not.toHaveBeenCalled();
});

it('uses regional discovery icons before installation and the installed local icon afterwards', async () => {
  const registry = structuredClone(store.getState().registry);
  registry.plugins[0].icons = {
    international: 'https://example.com/original.svg',
    china: 'https://example.cn/original.svg',
  };
  store.setState({ registry });
  await mount();
  expect(view.root.findByType('img').props.src).toBe('https://example.com/original.svg');
  act(() => store.setState({ region: 'china' }));
  expect(view.root.findByType('img').props.src).toBe('https://example.cn/original.svg');
  act(() =>
    store.setState({ installed: [{ ...installed, icon: 'data:image/svg+xml;base64,fixture' }] })
  );
  expect(view.root.findByType('img').props.src).toBe('data:image/svg+xml;base64,fixture');
});

it('prefers an opted-in client icon in cards and details and only requests a fallback after failure', async () => {
  const registry = structuredClone(store.getState().registry);
  registry.plugins[0].clientIcon = 'workbuddy.png';
  registry.plugins[0].icons = { international: 'https://example.com/fallback-client.png' };
  store.setState({ registry, selected: 'fixture.game' });
  await act(async () => {
    view = create(
      <ConfirmDialogProvider>
        <PluginHubMain active />
        <PluginHubPanel />
      </ConfirmDialogProvider>
    );
  });
  expect(view.root.findAllByType('img').map((node) => node.props.src)).toEqual([
    '/icons/tools/workbuddy.png',
    '/icons/tools/workbuddy.png',
  ]);
  act(() => view.root.findAllByType('img')[0].props.onError());
  expect(view.root.findAllByType('img')[0].props.src).toBe(
    'https://example.com/fallback-client.png'
  );
  await act(async () =>
    view.root.findAllByType('img')[1].props.onLoad({
      currentTarget: { decode: () => Promise.reject(new Error('unavailable client image')) },
    })
  );
  expect(view.root.findAllByType('img').map((node) => node.props.src)).toEqual([
    'https://example.com/fallback-client.png',
    'https://example.com/fallback-client.png',
  ]);
  expect(api.getPluginRegistry).not.toHaveBeenCalled();
  expect(api.installPlugin).not.toHaveBeenCalled();
});

it('waits for image decoding, ignores the previous source and reuses decoded icons after remount', async () => {
  const registry = structuredClone(store.getState().registry);
  registry.plugins[0].icons = {
    international: 'https://example.com/decode-one.png',
    china: 'https://example.cn/decode-two.png',
  };
  store.setState({ registry });
  await mount();
  const icon = () => view.root.findByProps({ className: 'ph-plugin-icon' });
  expect(icon().props['aria-busy']).toBe(true);
  let first!: () => void;
  const firstDecode = new Promise<void>((resolve) => {
    first = resolve;
  });
  act(() =>
    view.root.findByType('img').props.onLoad({ currentTarget: { decode: () => firstDecode } })
  );
  act(() => store.setState({ region: 'china' }));
  expect(view.root.findByType('img').props.src).toBe(registry.plugins[0].icons.china);
  await act(async () => first());
  expect(icon().props['data-ready']).toBe(false);
  let second!: () => void;
  const secondDecode = new Promise<void>((resolve) => {
    second = resolve;
  });
  act(() =>
    view.root.findByType('img').props.onLoad({ currentTarget: { decode: () => secondDecode } })
  );
  expect(icon().props['data-ready']).toBe(false);
  await act(async () => second());
  expect(icon().props['data-ready']).toBe(true);
  expect(view.root.findAllByProps({ className: 'ph-icon-skeleton' })).toHaveLength(0);
  act(() => view.unmount());
  await mount();
  expect(icon().props['data-ready']).toBe(true);
  expect(icon().props['aria-busy']).toBeUndefined();
});

it('falls back after decode failure and retries the same icon on a later visit', async () => {
  const registry = structuredClone(store.getState().registry);
  registry.plugins[0].icons = { international: 'https://example.com/decode-error.png' };
  store.setState({ registry });
  await mount();
  await act(async () =>
    view.root
      .findByType('img')
      .props.onLoad({ currentTarget: { decode: () => Promise.reject(new Error('decode failed')) } })
  );
  expect(view.root.findAllByType('img')).toHaveLength(0);
  expect(view.root.findAllByProps({ className: 'ph-icon-skeleton' })).toHaveLength(0);
  act(() => view.unmount());
  await mount();
  expect(view.root.findByProps({ className: 'ph-plugin-icon' }).props['aria-busy']).toBe(true);
  expect(view.root.findAllByType('img')).toHaveLength(1);
});

it('shows cold-load skeletons instead of an empty installed list and preserves cards during refresh', async () => {
  store.setState({ loading: true });
  await mount();
  expect(view.root.findAllByType('article')).toHaveLength(1);
  expect(view.root.findAllByProps({ className: 'ph-card-skeleton' })).toHaveLength(0);
  act(() => button('plugins.installed').props.onClick());
  expect(view.root.findAllByProps({ className: 'ph-card-skeleton' })).toHaveLength(3);
  expect(JSON.stringify(view.toJSON())).not.toContain('plugins.empty');
  act(() => store.setState({ loading: false, loaded: true }));
  expect(view.root.findAllByProps({ className: 'ph-card-skeleton' })).toHaveLength(0);
  expect(JSON.stringify(view.toJSON())).toContain('plugins.empty');
  expect(api.previewPlugin).not.toHaveBeenCalled();
  expect(api.getPluginRegistry).not.toHaveBeenCalled();
});

it('shows plugin status on the title row and the update action beside the toggle', async () => {
  const older = { ...installed, manifest: { ...installed.manifest, version: '0.0.1' } };
  vi.mocked(api.listPlugins).mockResolvedValue([older]);
  await mount();
  const card = view.root.findAllByType('article')[0];
  const status = card.findByProps({ className: 'ph-card-status enabled' });
  expect(status.parent?.props.className).toBe('ph-card-title');
  const [update, toggle] = card.findByProps({ className: 'ph-card-installed' }).children;
  expect((update as ReactTestInstance).props.className).toBe('ph-install');
  expect((toggle as ReactTestInstance).props.hideLabel).toBe(true);
  act(() => store.setState({ installed: [{ ...older, enabled: false }] }));
  expect(card.findByProps({ className: 'ph-card-status ' })).toBeTruthy();
});

it('keeps a single selected card highlighted when switching plugins without running them', async () => {
  const registry = structuredClone(store.getState().registry);
  registry.plugins.push({
    ...registry.plugins[0],
    id: 'fixture.other',
    releases: [
      {
        ...registry.plugins[0].releases[0],
        manifest: { ...candidate.manifest, name: 'other', displayName: 'Other plugin' },
      },
    ],
  });
  store.setState({ registry });
  await mount();
  const cards = () => view.root.findAllByType('article');
  act(() => cards()[0].findByProps({ className: 'ph-card-select' }).props.onClick());
  expect(cards()[0].props.className).toContain('selected');
  expect(cards()[1].props.className).not.toContain('selected');
  act(() => cards()[1].findByProps({ className: 'ph-card-select' }).props.onClick());
  expect(cards()[0].props.className).not.toContain('selected');
  expect(cards()[1].props.className).toContain('selected');
  expect(api.previewPlugin).not.toHaveBeenCalled();
  expect(api.launchPlugin).not.toHaveBeenCalled();
});

it.each([false, true])(
  'plugin details omit capability and selected-model instructions (installed: %s)',
  async (isInstalled) => {
    store.setState({ selected: installed.id, installed: isInstalled ? [installed] : [] });
    await act(async () => {
      view = create(
        <ConfirmDialogProvider>
          <PluginHubPanel />
        </ConfirmDialogProvider>
      );
    });
    const details = JSON.stringify(view.toJSON());
    expect(details).toContain(candidate.manifest.displayName);
    expect(details).toContain('plugins.version');
    expect(details).not.toContain('plugins.permissions');
    expect(details).not.toContain('plugins.modelPermission');
  }
);

it('reviews both model and clipboard capabilities when installing a translator', async () => {
  vi.mocked(api.previewPlugin).mockResolvedValue({
    ...candidate,
    manifest: {
      ...candidate.manifest,
      echobird: {
        ...candidate.manifest.echobird,
        permissions: ['model.request', 'clipboard.write'],
      },
    },
  });
  await mount();
  await act(async () => button('plugins.install').props.onClick());
  expect(JSON.stringify(view.toJSON())).toContain('plugins.modelPermission');
  expect(JSON.stringify(view.toJSON())).toContain('plugins.clipboardPermission');
  expect(api.installPlugin).not.toHaveBeenCalled();
});

it('Hub installation reviews the downloaded manifest, source and permission before the explicit commit', async () => {
  vi.mocked(api.previewPlugin).mockResolvedValue(candidate);
  vi.mocked(api.installPlugin).mockResolvedValue([installed]);
  await mount();
  await act(async () => button('plugins.install').props.onClick());
  expect(api.previewPlugin).toHaveBeenCalledExactlyOnceWith(
    'catalog',
    'fixture.game',
    'international'
  );
  expect(api.installPlugin).not.toHaveBeenCalled();
  expect(JSON.stringify(view.toJSON())).toContain('plugins.modelPermission');
  expect(JSON.stringify(view.toJSON())).toContain(candidate.source);
  await act(async () =>
    view.root
      .findAllByType('button')
      .find((node) => node.props.className === 'ph-primary')!
      .props.onClick()
  );
  expect(api.installPlugin).toHaveBeenCalledExactlyOnceWith(candidate.token);
  expect(store.getState().installed).toEqual([installed]);
  expect(api.launchPlugin).not.toHaveBeenCalled();
});

it.each(['cancel', 'leave'] as const)(
  '%s discards a preview that arrives late without installing it',
  async (action) => {
    let finish!: (candidate: PluginCandidate) => void;
    vi.mocked(api.previewPlugin).mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      })
    );
    await mount();
    await act(async () => button('plugins.install').props.onClick());
    if (action === 'cancel')
      act(() => view.root.findByProps({ 'aria-label': 'plugins.close' }).props.onClick());
    else act(() => view.update(<PluginHubMain active={false} />));
    await act(async () => finish(candidate));
    expect(api.cancelPlugin).toHaveBeenCalledWith(candidate.token);
    expect(api.installPlugin).not.toHaveBeenCalled();
    expect(store.getState().installed).toEqual([]);
  }
);

it('a failed install preserves the existing plugin and displays the explicit error', async () => {
  store.setState({ installed: [installed] });
  vi.mocked(api.previewPlugin).mockResolvedValue(candidate);
  vi.mocked(api.installPlugin).mockRejectedValue(new Error('disk unavailable'));
  await mount();
  act(() => store.setState({ installed: [] }));
  await act(async () => button('plugins.install').props.onClick());
  act(() => store.setState({ installed: [installed] }));
  await act(async () =>
    view.root
      .findAllByType('button')
      .find((node) => node.props.className === 'ph-primary')!
      .props.onClick()
  );
  expect(store.getState().installed).toEqual([installed]);
  expect(JSON.stringify(view.toJSON())).toContain('disk unavailable');
});

it('lists one plugin under every category it declares', () => {
  const manifest = {
    category: 'application',
    categories: ['application', 'account'],
    echobird: { contributes: { pages: [] } },
  } as unknown as Parameters<typeof pluginCategories>[0];
  expect(pluginCategories(manifest)).toEqual(['application', 'account']);
  expect(pluginCategories({ ...manifest, categories: undefined })).toEqual(['application']);
});
