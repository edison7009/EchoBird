import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { QuickAddAccounts, CustomSwitchEntry } from './QuickAddAccounts';
import { useNavigationStore } from '../../stores/navigationStore';
import { useAccountPrivacyStore } from '../../stores/accountPrivacyStore';
import * as accounts from '../../api/accountProxy';
import * as models from '../../api/models';
import { createCustomRouterGroup, ROUTER_GROUP_LIMIT } from '../../api/customRouterGroup';
import type { SmartRouterConfig } from '../../api/smartRouter';
const { context, mount } = vi.hoisted(() => ({
  mount: vi.fn(),
  context: {
    activeRouterId: 'smart-router',
    groupPending: false,
    activeDragId: null,
    customModels: [] as unknown[],
    selectedIds: new Set<string>(),
    routers: [] as SmartRouterConfig[],
    selectRouter: vi.fn(),
  },
}));
vi.mock('react-dom', () => ({ createPortal: (node: React.ReactNode) => node }));
vi.mock('./provider', () => ({ useFreeModels: () => context }));
vi.mock('../../api/customRouterGroup', () => ({
  createCustomRouterGroup: vi.fn(),
  MAX_ROUTER_GROUPS: 7,
  ROUTER_GROUP_LIMIT: 'SMART_ROUTER_GROUP_LIMIT',
}));
vi.mock('../../components/useTabCanvas', () => ({ mountTabCanvas: mount }));
vi.mock('../../hooks/useI18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }));
vi.mock('../../hooks/useAccountCapability', () => ({
  accountProxySources: () => [
    { id: 'codex', name: 'Codex', icon: 'codex.svg' },
    { id: 'claudeCode', name: 'Claude Code', icon: 'claude.svg' },
  ],
  useAccountProviderFilter: () => available,
}));
vi.mock('../../api/accountProxy', async (original) => ({
  ...(await original<typeof import('../../api/accountProxy')>()),
  listProxyAccounts: vi.fn(),
  getAccountProxyModels: vi.fn(),
  getCachedAccountProxyModels: vi.fn(),
  cacheAccountProxyModels: vi.fn(),
}));
vi.mock('../../api/models', () => ({
  getModels: vi.fn(),
  getCachedUpstreamModelCatalog: vi.fn(),
  fetchUpstreamModelCatalog: vi.fn(),
}));
const available = () => true;
const newGroup = { id: 'smart-router-new' } as SmartRouterConfig;
const savedApi = {
  internalId: 'api-saved',
  name: 'Fixture API',
  baseUrl: 'https://api.example.invalid/v1',
  anthropicUrl: '',
  apiKey: 'fixture-private-key',
  modelId: 'api-alpha',
  scope: 'modelCenter' as const,
};
let tree: ReactTestRenderer;
const buttons = () => tree.root.findAllByType('button');
const button = (label: string) =>
  buttons().find((node) => node.props['aria-label'] === label || node.children.includes(label))!;
async function render(node = <QuickAddAccounts />) {
  await act(async () => {
    tree = create(node);
  });
}
async function click(label: string) {
  await act(async () => {
    await button(label).props.onClick();
  });
}
async function open() {
  await render();
  await click('freeModels.quickAdd');
}
async function clearSection(index: number) {
  await act(async () => {
    const section = tree.root.findAllByType('section')[index];
    for (const toggle of section
      .findAllByType('button')
      .filter((node) => node.props['aria-pressed'] === true))
      toggle.props.onClick();
  });
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
beforeEach(() => {
  vi.clearAllMocks();
  context.activeRouterId = 'smart-router';
  context.customModels = [];
  context.selectedIds = new Set();
  context.routers = [{ id: 'smart-router' } as SmartRouterConfig];
  useNavigationStore.setState({
    activePage: 'freeModels',
    smartAccessTab: 'configuration',
    appsTab: 'desktop',
  });
  useAccountPrivacyStore.setState({ hidden: false });
  vi.stubGlobal('document', { activeElement: null });
  vi.stubGlobal('HTMLElement', class {});
  vi.mocked(models.getModels).mockResolvedValue([]);
  vi.mocked(models.getCachedUpstreamModelCatalog).mockResolvedValue(null);
  vi.mocked(models.fetchUpstreamModelCatalog).mockResolvedValue({
    models: [{ id: 'api-alpha' }, { id: 'api-beta' }, { id: 'api-gamma' }],
    requiresApiKey: true,
  });
  vi.mocked(accounts.listProxyAccounts).mockImplementation(async (provider) => [
    { source: { provider, accountId: `${provider}-account` }, label: `${provider}@example.test` },
  ]);
  vi.mocked(accounts.getAccountProxyModels).mockResolvedValue(['alpha', 'beta', 'gamma']);
  vi.mocked(accounts.getCachedAccountProxyModels).mockReturnValue(undefined);
  vi.mocked(createCustomRouterGroup).mockReset().mockResolvedValue(newGroup);
  context.selectRouter.mockResolvedValue(undefined);
});
afterEach(() => {
  act(() => tree?.unmount());
  vi.unstubAllGlobals();
});
it('loads sources only after explicit opening and creates one separate group on confirmation', async () => {
  await render();
  expect(accounts.listProxyAccounts).not.toHaveBeenCalled();
  expect(models.getModels).not.toHaveBeenCalled();
  expect(createCustomRouterGroup).not.toHaveBeenCalled();
  await click('freeModels.quickAdd');
  expect(tree.root.findAllByType('section')).toHaveLength(2);
  await clearSection(0);
  await clearSection(1);
  await click('alpha');
  expect(createCustomRouterGroup).not.toHaveBeenCalled();
  await click('common.confirm');
  expect(createCustomRouterGroup).toHaveBeenCalledWith(
    [
      {
        existing: undefined,
        input: expect.objectContaining({
          modelId: 'alpha',
          accountSource: { provider: 'codex', accountId: 'codex-account' },
          scope: 'smartRouter',
        }),
      },
    ],
    'freeModels.router.title',
    expect.any(Function),
    undefined
  );
  expect(context.selectRouter).toHaveBeenCalledWith(newGroup.id);
  expect(tree.root.findAllByProps({ role: 'dialog' })).toHaveLength(0);
});
it('includes current router models in the new group and defaults to the first two per account', async () => {
  context.customModels = [
    {
      id: 'old',
      internalId: 'old',
      modelId: 'alpha',
      accountSource: { provider: 'codex', accountId: 'codex-account' },
      scope: 'smartRouter',
    },
  ];
  context.selectedIds.add('old');
  await open();
  expect(button('alpha').props['aria-pressed']).toBe(true);
  expect(button('beta').props['aria-pressed']).toBe(true);
  expect(button('gamma').props['aria-pressed']).toBe(false);
  await click('common.confirm');
  const choices = vi.mocked(createCustomRouterGroup).mock.calls[0][0];
  expect(choices).toHaveLength(4);
  expect(choices[0].existing?.internalId).toBe('old');
});
it('shows a specific capacity error if groups fill while the dialog is open', async () => {
  await open();
  vi.mocked(createCustomRouterGroup).mockRejectedValueOnce(new Error(ROUTER_GROUP_LIMIT));
  await click('common.confirm');
  expect(JSON.stringify(tree.toJSON())).toContain('freeModels.router.limit');
  expect(context.selectRouter).not.toHaveBeenCalled();
});
it('keeps a failed new-group draft for retry rather than partially removing selections', async () => {
  await open();
  vi.mocked(createCustomRouterGroup).mockRejectedValueOnce(new Error('fixture rollback completed'));
  await click('common.confirm');
  expect(tree.root.findAllByProps({ role: 'alert' })).toHaveLength(1);
  expect(button('alpha').props['aria-pressed']).toBe(true);
  await click('common.confirm');
  expect(createCustomRouterGroup).toHaveBeenCalledTimes(2);
});
it('hides both entry points at seven groups, with a positive control below the cap', async () => {
  await render();
  expect(button('freeModels.quickAdd')).toBeTruthy();
  context.routers = Array.from({ length: 7 }, () => newGroup);
  await act(async () => tree.update(<QuickAddAccounts />));
  expect(buttons()).toHaveLength(0);
  await act(async () =>
    tree.update(<CustomSwitchEntry toolId="claudedesktop" toolName="Claude" />)
  );
  expect(buttons()).toHaveLength(0);
  expect(accounts.listProxyAccounts).not.toHaveBeenCalled();
});
it.each(['claudedesktop', 'copilotdesktop'])(
  'offers the shared desktop entry for %s without selecting or applying on entry',
  async (toolId) => {
    useNavigationStore.setState({ activePage: 'apps' });
    await render(<CustomSwitchEntry toolId={toolId} toolName="Claude Desktop" />);
    expect(button('agent.addCustomSwitch')).toBeTruthy();
    expect(accounts.listProxyAccounts).not.toHaveBeenCalled();
    await click('agent.addCustomSwitch');
    await click('common.confirm');
    expect(createCustomRouterGroup).toHaveBeenCalledWith(
      expect.any(Array),
      'freeModels.router.title',
      expect.any(Function),
      'Claude Desktop · agent.customSwitch'
    );
    await act(async () =>
      tree.update(<CustomSwitchEntry toolId="unsupported" toolName="Unsupported" />)
    );
    expect(buttons()).toHaveLength(0);
  }
);
it('cancels a desktop draft on tool/page changes and rejects late group completion', async () => {
  useNavigationStore.setState({ activePage: 'apps' });
  await render(<CustomSwitchEntry toolId="claudedesktop" toolName="Claude" />);
  await click('agent.addCustomSwitch');
  const pending = deferred<SmartRouterConfig>();
  vi.mocked(createCustomRouterGroup).mockReturnValueOnce(pending.promise);
  act(() => {
    button('common.confirm').props.onClick();
  });
  const current = vi.mocked(createCustomRouterGroup).mock.calls[0][2];
  expect(current()).toBe(true);
  await act(async () => {
    useNavigationStore.setState({ activePage: 'models' });
  });
  expect(current()).toBe(false);
  await act(async () => pending.resolve(newGroup));
  expect(context.selectRouter).not.toHaveBeenCalled();
});
it('cancels initial source loading and ignores late catalogs without group creation', async () => {
  const pending = deferred<Awaited<ReturnType<typeof models.getModels>>>();
  vi.mocked(models.getModels).mockReturnValue(pending.promise);
  await open();
  expect(tree.root.findAllByProps({ 'aria-label': 'accountCenter.loading' })).toHaveLength(1);
  await click('btn.cancel');
  await act(async () => pending.resolve([]));
  expect(createCustomRouterGroup).not.toHaveBeenCalled();
});
it('keeps account names private and silently hides failed sources', async () => {
  useAccountPrivacyStore.setState({ hidden: true });
  vi.mocked(accounts.listProxyAccounts).mockImplementation(async (provider) => [
    {
      source: { provider, accountId: `${provider}@example.test` },
      label: `${provider}@example.test`,
    },
  ]);
  vi.mocked(accounts.getAccountProxyModels).mockImplementation(async (source) => {
    if (source.provider === 'codex') throw new Error('fixture unavailable');
    return ['single'];
  });
  await open();
  expect(JSON.stringify(tree.toJSON())).not.toContain('@example.test');
  expect(JSON.stringify(tree.toJSON())).not.toContain(
    encodeURIComponent('claudeCode@example.test')
  );
  expect(tree.root.findAllByType('section')).toHaveLength(1);
  expect(tree.root.findAllByProps({ role: 'alert' })).toHaveLength(0);
  expect(button('single').props['aria-pressed']).toBe(true);
});
it('groups API credentials, refreshes the full upstream list, and reuses saved API configurations', async () => {
  vi.mocked(accounts.listProxyAccounts).mockResolvedValue([]);
  vi.mocked(models.getModels).mockResolvedValue([
    savedApi,
    { ...savedApi, internalId: 'api-second', modelId: 'api-beta', baseUrl: savedApi.baseUrl + '/' },
    { ...savedApi, internalId: 'smart-router' },
    { ...savedApi, internalId: 'local-server' },
  ]);
  vi.mocked(models.getCachedUpstreamModelCatalog).mockResolvedValue({
    models: [{ id: 'stale' }],
    requiresApiKey: true,
  });
  await open();
  expect(tree.root.findAllByType('section')).toHaveLength(1);
  expect(models.fetchUpstreamModelCatalog).toHaveBeenCalledTimes(1);
  expect(models.getCachedUpstreamModelCatalog).not.toHaveBeenCalled();
  expect(button('api-gamma')).toBeTruthy();
  expect(JSON.stringify(tree.toJSON())).not.toContain(savedApi.apiKey);
  await click('common.confirm');
  expect(
    vi.mocked(createCustomRouterGroup).mock.calls[0][0].map((choice) => choice.existing?.internalId)
  ).toEqual(['api-saved', 'api-second']);
});
it('uses saved API models when upstream discovery fails and keeps them in a new group', async () => {
  vi.mocked(accounts.listProxyAccounts).mockResolvedValue([]);
  vi.mocked(models.getModels).mockResolvedValue([savedApi]);
  vi.mocked(models.fetchUpstreamModelCatalog).mockRejectedValue(new Error('fixture offline'));
  await open();
  expect(button('api-alpha').props['aria-pressed']).toBe(true);
  await click('common.confirm');
  expect(vi.mocked(createCustomRouterGroup).mock.calls[0][0][0].existing).toEqual(savedApi);
});
it('keeps ready sources in their columns while skeletons resolve and does not submit early', async () => {
  const pending = deferred<string[]>();
  vi.mocked(accounts.getAccountProxyModels).mockImplementation(async (source) =>
    source.provider === 'codex' ? pending.promise : ['ready']
  );
  await open();
  const ready = tree.root
    .findAllByType('section')
    .find((section) => section.findAllByProps({ 'aria-label': 'ready' }).length)!;
  const column = ready.parent!.props['data-quick-add-column'];
  expect(tree.root.findAllByProps({ 'aria-label': 'voice.loading' })).toHaveLength(1);
  expect(button('common.confirm').props.disabled).toBe(true);
  await act(async () => pending.resolve(Array.from({ length: 30 }, (_, i) => `loaded-${i}`)));
  expect(ready.parent!.props['data-quick-add-column']).toBe(column);
  expect(button('common.confirm').props.disabled).toBe(false);
});
it('uses official provider logos and the default box for an unknown API channel', async () => {
  vi.mocked(accounts.listProxyAccounts).mockResolvedValue([]);
  vi.mocked(models.getModels).mockResolvedValue([
    { ...savedApi, internalId: 'official', name: 'DeepSeek', baseUrl: 'https://api.deepseek.com' },
    {
      ...savedApi,
      internalId: 'relay',
      name: 'Fixture Relay',
      baseUrl: 'https://relay.example.invalid/v1',
    },
  ]);
  await open();
  const sections = tree.root.findAllByType('section');
  expect(sections[0].findByType('img').props.src).toBe('./icons/models/deepseek.svg');
  expect(sections[1].findAllByType('img')).toHaveLength(0);
  expect(
    sections[1]
      .findAllByType('svg')
      .some((node) => String(node.props.className).includes('lucide-box'))
  ).toBe(true);
});

it('collapses each account independently without refetching or changing its selected models', async () => {
  await open();
  const sections = tree.root.findAllByType('section');
  const header = (index: number) =>
    sections[index]
      .findAllByType('button')
      .find((node) => node.props['aria-expanded'] !== undefined)!;
  expect(header(0).props['aria-expanded']).toBe(true);
  expect(header(1).props['aria-expanded']).toBe(true);
  await click('alpha');
  const reads = [
    vi.mocked(accounts.listProxyAccounts).mock.calls.length,
    vi.mocked(accounts.getAccountProxyModels).mock.calls.length,
    vi.mocked(models.getModels).mock.calls.length,
  ];
  await act(async () => header(0).props.onClick());
  expect(header(0).props['aria-expanded']).toBe(false);
  expect(sections[0].findByProps({ id: header(0).props['aria-controls'] }).props.hidden).toBe(true);
  expect(sections[0].findAllByProps({ 'aria-label': 'alpha' })).toHaveLength(0);
  expect(sections[1].findAllByProps({ 'aria-label': 'alpha' })).toHaveLength(1);
  await act(async () => header(0).props.onClick());
  expect(button('alpha').props['aria-pressed']).toBe(false);
  expect(button('beta').props['aria-pressed']).toBe(true);
  expect([
    vi.mocked(accounts.listProxyAccounts).mock.calls.length,
    vi.mocked(accounts.getAccountProxyModels).mock.calls.length,
    vi.mocked(models.getModels).mock.calls.length,
  ]).toEqual(reads);
  await act(async () => header(0).props.onClick());
  expect(createCustomRouterGroup).not.toHaveBeenCalled();
  await click('common.confirm');
  const choices = vi.mocked(createCustomRouterGroup).mock.calls[0][0];
  expect(
    choices
      .filter((choice) => choice.input.accountSource?.provider === 'codex')
      .map((choice) => choice.input.modelId)
  ).toEqual(['beta']);
  expect(choices).toHaveLength(3);
});

it('retains API model choices when collapsed and resets expansion after reopening', async () => {
  vi.mocked(accounts.listProxyAccounts).mockResolvedValue([]);
  vi.mocked(models.getModels).mockResolvedValue([savedApi]);
  await open();
  await click('api-gamma');
  const header = () =>
    tree.root
      .findAllByType('section')[0]
      .findAllByType('button')
      .find((node) => node.props['aria-expanded'] !== undefined)!;
  await act(async () => header().props.onClick());
  expect(header().props['aria-expanded']).toBe(false);
  expect(models.fetchUpstreamModelCatalog).toHaveBeenCalledTimes(1);
  await click('common.confirm');
  expect(
    vi.mocked(createCustomRouterGroup).mock.calls[0][0].map((choice) => choice.input.modelId)
  ).toEqual(['api-alpha', 'api-beta', 'api-gamma']);
  await click('freeModels.quickAdd');
  expect(header().props['aria-expanded']).toBe(true);
});

it('keeps a loading account collapsed when its directory finishes without reloading it', async () => {
  const pending = deferred<string[]>();
  vi.mocked(accounts.getAccountProxyModels).mockImplementation(async (source) =>
    source.provider === 'codex' ? pending.promise : ['ready']
  );
  await open();
  const section = tree.root.findAllByType('section')[0];
  const header = () =>
    section.findAllByType('button').find((node) => node.props['aria-expanded'] !== undefined)!;
  await act(async () => header().props.onClick());
  expect(button('common.confirm').props.disabled).toBe(true);
  await act(async () => pending.resolve(['loaded-a', 'loaded-b', 'loaded-c']));
  expect(header().props['aria-expanded']).toBe(false);
  expect(section.findAllByProps({ 'aria-label': 'loaded-a' })).toHaveLength(0);
  expect(button('common.confirm').props.disabled).toBe(false);
  await act(async () => header().props.onClick());
  expect(button('loaded-a').props['aria-pressed']).toBe(true);
  expect(button('loaded-b').props['aria-pressed']).toBe(true);
  expect(button('loaded-c').props['aria-pressed']).toBe(false);
  expect(
    vi
      .mocked(accounts.getAccountProxyModels)
      .mock.calls.filter(([source]) => source.provider === 'codex')
  ).toHaveLength(1);
});
