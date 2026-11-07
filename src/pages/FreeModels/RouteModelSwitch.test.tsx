import { useLayoutEffect, type ComponentProps, type ReactNode } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { DndContext, PointerSensor, useSensor, useSensors } from '@dnd-kit/core';
import * as api from '../../api/tauri';
import * as accountApi from '../../api/accountProxy';
import * as modelApi from '../../api/models';
import type { ModelConfig } from '../../api/types';
import type { FreeModelDirectory, FreeModelEntry } from '../../api/freeModels';
import { ModelIdCombobox } from '../../components/ModelIdCombobox';
import { useNavigationStore } from '../../stores/navigationStore';
import { useAccountPrivacyStore } from '../../stores/accountPrivacyStore';
import {
  FreeModelsMain,
  FreeModelsProvider,
  FreeModelsTitleActions,
  useFreeModels,
} from './FreeModels';

const { confirm, showToast, nexus, startDrag, keyboardDrag, dragState, measure, getModelIcon } =
  vi.hoisted(() => ({
    confirm: vi.fn(),
    showToast: vi.fn(),
    startDrag: vi.fn(),
    keyboardDrag: vi.fn(),
    dragState: { id: null as string | null },
    measure: vi.fn(),
    getModelIcon: vi.fn(),
    nexus: {
      updateUserModel: vi.fn(),
      setNewModelForm: vi.fn(),
      setEditingModelId: vi.fn(),
      setModelModalDestination: vi.fn(),
      setShowAddModelModal: vi.fn(),
      setKeyDestroyed: vi.fn(),
      setShowApiKey: vi.fn(),
    },
  }));
vi.mock('../../hooks/useI18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }));
vi.mock('../../components', () => ({ getModelIcon }));
vi.mock('../../components/ConfirmDialog', () => ({ useConfirm: () => confirm }));
vi.mock('../../components/Toast', () => ({ useToast: () => ({ showToast }) }));
vi.mock('../ModelNexus/context', () => ({ useModelNexus: () => nexus }));
vi.mock('react-dom', () => ({ createPortal: (children: ReactNode) => children }));
vi.mock('../../components/ModelIdCombobox', async (original) => {
  const actual = await original<typeof import('../../components/ModelIdCombobox')>();
  return {
    ModelIdCombobox: (props: ComponentProps<typeof actual.ModelIdCombobox>) =>
      props.optionLabels || props.freeModelIds?.length ? (
        <actual.ModelIdCombobox {...props} />
      ) : (
        <input value={props.value} disabled={props.disabled} readOnly />
      ),
  };
});
vi.mock('@dnd-kit/core', () => ({
  DndContext: ({ children }: { children: ReactNode }) => children,
  DragOverlay: ({ children }: { children: ReactNode }) => children,
  PointerSensor: vi.fn(),
  KeyboardSensor: vi.fn(),
  closestCenter: vi.fn(),
  pointerWithin: vi.fn(),
  rectIntersection: vi.fn(),
  useDroppable: () => ({ setNodeRef: vi.fn(), isOver: false }),
  useDndContext: () => ({ measureDroppableContainers: measure }),
  useSensor: vi.fn(),
  useSensors: vi.fn(),
}));
vi.mock('@dnd-kit/sortable', async (original) => ({
  ...(await original<typeof import('@dnd-kit/sortable')>()),
  SortableContext: ({ children }: { children: ReactNode }) => children,
  useSortable: ({ id }: { id: string }) => ({
    attributes: { tabIndex: 0 },
    listeners: { onPointerDown: startDrag, onKeyDown: keyboardDrag },
    setNodeRef: vi.fn(),
    setActivatorNodeRef: vi.fn(),
    transform: null,
    transition: undefined,
    isDragging: dragState.id === id,
  }),
}));
vi.mock('../../api/accountProxy', async (original) => ({
  ...(await original<typeof import('../../api/accountProxy')>()),
  getCachedAccountProxyModels: vi.fn(),
  cacheAccountProxyModels: vi.fn(),
  getAccountProxyModels: vi.fn(),
  listProxyAccounts: vi.fn(),
}));
vi.mock('../../api/models', () => ({
  getCachedUpstreamModelCatalog: vi.fn(),
  fetchUpstreamModelCatalog: vi.fn(),
}));
vi.mock('../../api/tauri', () => ({
  getSmartRouterConfig: vi.fn(),
  listSmartRouters: async () => [await api.getSmartRouterConfig()],
  getSmartRouterCandidates: vi.fn(),
  getSmartRouterActivity: vi.fn(),
  setSmartRouterEnabled: vi.fn(),
  setSmartRouterCandidates: vi.fn(),
  removeSmartRouterCandidate: vi.fn(),
  getFreeModelDirectory: vi.fn(),
  getCachedFreeModelDirectory: vi.fn(),
  updateModel: vi.fn(),
  deleteModel: vi.fn(),
  queryModelUsage: vi.fn(),
  isKeyDestroyed: vi.fn(),
}));

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

function freeEntry(modelId: string, baseUrl = 'https://free.example/v1'): FreeModelEntry {
  return {
    id: modelId,
    modelId,
    baseUrl,
    providerId: 'free-provider',
    provider: 'Free provider',
    freeType: 'perpetual',
    freeTier: 'Free',
    rateLimits: '',
    notes: '',
    docsUrl: '',
    cardRequired: null,
    phoneRequired: null,
    commercialOk: null,
    verifiedAt: '2026-10-12',
  };
}

describe('Direct route card model selection', () => {
  let renderer: ReactTestRenderer;
  let context: ReturnType<typeof useFreeModels>;
  let savedModels: ModelConfig[];
  let savedOrder: string[];
  const account: ModelConfig = {
    internalId: 'account-model',
    name: 'Account provider',
    modelId: 'account-old',
    baseUrl: 'https://account.example/v1',
    apiKey: '',
    accountSource: { provider: 'codex', accountId: 'account-one' },
    scope: 'smartRouter',
  };
  const free: ModelConfig = {
    internalId: 'free-model',
    name: 'Free provider',
    modelId: 'free-old',
    baseUrl: 'https://free.example/v1',
    anthropicUrl: 'https://free.example/anthropic',
    apiKey: 'saved-secret',
    scope: 'smartRouter',
  };
  const cached: FreeModelDirectory = {
    version: 1,
    updatedAt: '2026-10-12',
    models: [
      freeEntry('free-old'),
      freeEntry('free-new'),
      freeEntry('another-endpoint', 'https://free.example/v1/other'),
    ],
  };

  function config() {
    return {
      id: 'smart-router',
      name: 'Auto Router',
      masterEnabled: false,
      groupEnabled: true,
      enabled: false,
      candidateIds: [...savedOrder],
      usableCandidateCount: savedOrder.length,
      baseUrl: 'http://127.0.0.1:62185/v1',
      modelId: 'smart-router',
      port: 62185,
      running: false,
    };
  }
  function Harness() {
    const value = useFreeModels();
    const activePage = useNavigationStore((state) => state.activePage);
    useLayoutEffect(() => {
      context = value;
    });
    return activePage === 'freeModels' ? <FreeModelsMain /> : null;
  }
  async function open(showTitleActions = false) {
    await act(async () => {
      renderer = create(
        <FreeModelsProvider>
          {showTitleActions && <FreeModelsTitleActions />}
          <Harness />
        </FreeModelsProvider>,
        {
          createNodeMock: (element) =>
            String(element.props['aria-label']).startsWith('model.modelId')
              ? {
                  focus: vi.fn(),
                  blur: vi.fn(),
                  contains: () => false,
                  getBoundingClientRect: () => ({ left: 12, width: 160, top: 100, bottom: 130 }),
                }
              : null,
        }
      );
    });
  }
  function picker(modelId: string) {
    return renderer.root
      .findAllByType(ModelIdCombobox)
      .find((node) => node.props.value === modelId)!;
  }
  function card(modelId: string): ReactTestInstance {
    return renderer.root
      .findAll(
        (node) =>
          node.type === 'div' && String(node.props.className).includes('free-model-route-node')
      )
      .find((node) =>
        node.findAllByType(ModelIdCombobox).some((combo) => combo.props.value === modelId)
      )!;
  }
  function textOf(node: ReactTestInstance): string {
    return node.children
      .map((child) => (typeof child === 'string' ? child : textOf(child)))
      .join(' ');
  }
  function visibilityButton() {
    return renderer.root
      .findAllByType('button')
      .find((node) =>
        ['freeModels.hideAccountNames', 'freeModels.showAccountNames'].includes(
          node.props['aria-label']
        )
      )!;
  }
  function dragHandle(route: ReactTestInstance) {
    return route.findAllByType('button').find((node) => node.props.onPointerDown === startDrag)!;
  }
  function clickCard(modelId: string) {
    act(() => dragHandle(card(modelId)).props.onClick({ detail: 1 }));
  }
  async function openMenu(modelId: string) {
    if (picker(modelId).props.open)
      await act(async () => picker(modelId).props.onOpenChange(false));
    await act(async () => picker(modelId).props.onOpenChange(true));
  }
  function cardFor(modelId: string): ReactTestInstance {
    return renderer.root
      .findAll(
        (node) =>
          node.type === 'div' && String(node.props.className).includes('free-model-route-node')
      )
      .find((node) =>
        node
          .findAllByType('button')
          .some((button) => String(button.props['aria-label']).endsWith(`: ${modelId}`))
      )!;
  }
  async function select(oldId: string, newId: string) {
    await act(async () => {
      picker(oldId).props.onChange(newId);
    });
  }
  function expectNoConfigurationChanges() {
    expect(api.getFreeModelDirectory).not.toHaveBeenCalled();
    expect(api.queryModelUsage).not.toHaveBeenCalled();
    expect(api.updateModel).not.toHaveBeenCalled();
    expect(api.setSmartRouterCandidates).not.toHaveBeenCalled();
    expect(api.removeSmartRouterCandidate).not.toHaveBeenCalled();
    expect(api.setSmartRouterEnabled).not.toHaveBeenCalled();
    expect(nexus.setShowAddModelModal).not.toHaveBeenCalled();
    expect(confirm).not.toHaveBeenCalled();
    expect(showToast).not.toHaveBeenCalled();
  }
  function expectNoPassiveChanges() {
    expect(api.getFreeModelDirectory).not.toHaveBeenCalled();
    expect(accountApi.getAccountProxyModels).not.toHaveBeenCalled();
    expect(modelApi.fetchUpstreamModelCatalog).not.toHaveBeenCalled();
    expect(accountApi.cacheAccountProxyModels).not.toHaveBeenCalled();
    expectNoConfigurationChanges();
  }

  beforeEach(() => {
    dragState.id = null;
    useAccountPrivacyStore.setState({ hidden: false });
    vi.stubGlobal('localStorage', { getItem: vi.fn().mockReturnValue(null), setItem: vi.fn() });
    vi.mocked(useSensor).mockImplementation((sensor, options) => ({
      sensor,
      options: options ?? {},
    }));
    vi.mocked(useSensors).mockImplementation((...sensors) =>
      sensors.filter((sensor) => sensor !== undefined && sensor !== null)
    );
    savedModels = [{ ...account }, { ...free }];
    savedOrder = savedModels.map((model) => model.internalId);
    getModelIcon.mockImplementation((name: string, modelId: string) =>
      modelId ? `/models/${modelId}.svg` : name === 'Free provider' ? '/providers/free.svg' : null
    );
    vi.stubGlobal('window', {
      setInterval,
      clearInterval,
      setTimeout,
      innerHeight: 800,
      innerWidth: 1000,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    });
    vi.stubGlobal('document', {
      body: {},
      visibilityState: 'visible',
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    });
    vi.stubGlobal('requestAnimationFrame', () => 1);
    vi.stubGlobal('cancelAnimationFrame', vi.fn());
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    useNavigationStore.setState({ activePage: 'freeModels' });
    vi.mocked(api.getSmartRouterConfig).mockImplementation(async () => config());
    vi.mocked(api.getSmartRouterCandidates).mockImplementation(async () =>
      savedModels.filter((model) => savedOrder.includes(model.internalId))
    );
    vi.mocked(api.getSmartRouterActivity).mockResolvedValue({
      candidateId: null,
      active: false,
      sequence: 0,
      updatedAtMs: 0,
    });
    vi.mocked(api.getCachedFreeModelDirectory).mockResolvedValue(cached);
    vi.mocked(modelApi.getCachedUpstreamModelCatalog).mockResolvedValue(null);
    vi.mocked(modelApi.fetchUpstreamModelCatalog).mockResolvedValue({
      models: [{ id: 'free-old' }, { id: 'free-new' }],
      requiresApiKey: false,
    });
    vi.mocked(accountApi.getCachedAccountProxyModels).mockImplementation((source) =>
      source.accountId === 'account-one' ? ['account-old', 'account-new'] : undefined
    );
    vi.mocked(accountApi.getAccountProxyModels).mockImplementation(
      async (source) => accountApi.getCachedAccountProxyModels(source) ?? []
    );
    vi.mocked(accountApi.listProxyAccounts).mockResolvedValue([
      {
        source: { provider: 'codex', accountId: 'account-one' },
        label: 'codex@example.com',
        plan: 'prolite',
      },
    ]);
    vi.mocked(api.updateModel).mockImplementation(async (id, updates) => {
      const existing = savedModels.find((model) => model.internalId === id);
      if (!existing) return null;
      const updated = { ...existing, ...updates };
      savedModels = savedModels.map((model) => (model.internalId === id ? updated : model));
      return updated;
    });
    vi.mocked(api.setSmartRouterCandidates).mockImplementation(async (ids) => {
      savedOrder = [...ids];
      return config();
    });
    vi.mocked(api.removeSmartRouterCandidate).mockImplementation(async (id) => {
      savedOrder = savedOrder.filter((candidate) => candidate !== id);
      return config();
    });
  });

  afterEach(() => {
    act(() => renderer?.unmount());
    vi.resetAllMocks();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it.each(['cards', 'list'] as const)(
    'keeps the keyless access label through new model choices and remounting in %s',
    async (layout) => {
      savedModels = savedModels.map((model) =>
        model.internalId === free.internalId ? { ...model, apiKey: '' } : model
      );
      vi.mocked(api.getCachedFreeModelDirectory).mockResolvedValue({
        ...cached,
        models: [{ ...freeEntry('free-old'), notes: 'Leave the API key empty. Public access.' }],
      });
      vi.mocked(modelApi.getCachedUpstreamModelCatalog).mockResolvedValue({
        models: [{ id: 'free-old' }, { id: 'new-release' }, { id: 'future/another' }],
        requiresApiKey: false,
      });
      const route = (id: string) =>
        renderer.root
          .findAll(
            (node) =>
              node.type === 'div' &&
              String(node.props.className).startsWith(
                context.layout === 'cards' ? 'free-model-route-node ' : 'free-model-route-row '
              )
          )
          .find((node) =>
            node.findAllByType(ModelIdCombobox).some((combo) => combo.props.value === id)
          )!;
      await open();
      act(() => context.setLayout(layout));
      expect(textOf(route('free-old'))).toContain('freeModels.noApiKey');
      expect(textOf(route('account-old'))).not.toContain('freeModels.noApiKey');
      expectNoPassiveChanges();
      await select('free-old', 'new-release');
      expect(textOf(route('new-release'))).toContain('freeModels.noApiKey');
      act(() => context.setLayout(layout === 'cards' ? 'list' : 'cards'));
      expect(textOf(route('new-release'))).toContain('freeModels.noApiKey');
      await select('new-release', 'future/another');
      expect(textOf(route('future/another'))).toContain('freeModels.noApiKey');
      act(() => renderer.unmount());
      await open();
      act(() => context.setLayout(layout));
      expect(textOf(route('future/another'))).toContain('freeModels.noApiKey');
      expect(api.updateModel).toHaveBeenCalledTimes(2);
      expect(modelApi.fetchUpstreamModelCatalog).not.toHaveBeenCalled();
      expect(api.setSmartRouterCandidates).not.toHaveBeenCalled();
      expect(api.removeSmartRouterCandidate).not.toHaveBeenCalled();
      expect(showToast).not.toHaveBeenCalled();
    }
  );

  it('does not infer the keyless access label from free pricing or another endpoint', async () => {
    vi.mocked(api.getCachedFreeModelDirectory).mockResolvedValue({
      ...cached,
      models: [
        freeEntry('free-old'),
        {
          ...freeEntry('free-old', `${free.baseUrl}/other`),
          notes: 'Leave the API key empty. Public access.',
        },
      ],
    });
    vi.mocked(modelApi.getCachedUpstreamModelCatalog).mockResolvedValue({
      models: [{ id: 'free-old', free: true, multiplier: 0 }],
      requiresApiKey: false,
    });
    await open();
    expect(textOf(card('free-old'))).not.toContain('freeModels.noApiKey');
    expectNoPassiveChanges();
  });

  it('API catalog retains every cached upstream ID and the selected ID without passive remote requests', async () => {
    vi.mocked(modelApi.getCachedUpstreamModelCatalog).mockResolvedValue({
      models: [
        { id: 'remote-free', free: true },
        { id: 'remote-paid', multiplier: 0.8 },
      ],
      requiresApiKey: false,
    });
    await open();
    expect(modelApi.getCachedUpstreamModelCatalog).toHaveBeenCalledExactlyOnceWith({
      baseUrl: free.baseUrl,
      anthropicUrl: free.anthropicUrl,
      apiKey: free.apiKey,
    });
    expect(picker('free-old').props.options).toEqual(['free-old', 'remote-free', 'remote-paid']);
    expectNoPassiveChanges();
    await select('free-old', 'remote-paid');
    expect(api.updateModel).toHaveBeenCalledExactlyOnceWith(free.internalId, {
      modelId: 'remote-paid',
    });
    expect(savedModels.find((model) => model.internalId === free.internalId)?.apiKey).toBe(
      free.apiKey
    );
  });

  it('opens saved account and API model menus without refresh icons and loads their directories on demand', async () => {
    vi.mocked(accountApi.getAccountProxyModels).mockResolvedValue(['account-old', 'account-new']);
    await open();
    expect(picker('account-old').props.onRefresh).toBeUndefined();
    expect(picker('free-old').props.onRefresh).toBeUndefined();
    expectNoPassiveChanges();
    await act(async () => clickCard('account-old'));
    expect(accountApi.getAccountProxyModels).toHaveBeenCalledExactlyOnceWith(account.accountSource);
    expect(modelApi.fetchUpstreamModelCatalog).not.toHaveBeenCalled();
    await act(async () => clickCard('free-old'));
    expect(modelApi.fetchUpstreamModelCatalog).toHaveBeenCalledExactlyOnceWith({
      baseUrl: free.baseUrl,
      anthropicUrl: free.anthropicUrl,
      apiKey: free.apiKey,
    });
    expect(api.updateModel).not.toHaveBeenCalled();
    expect(showToast).not.toHaveBeenCalled();
  });

  it.each(['account', 'API'] as const)(
    'loads a successful %s directory only once across closing and reopening its menu',
    async (kind) => {
      const id = kind === 'account' ? 'account-old' : 'free-old';
      const request =
        kind === 'account' ? accountApi.getAccountProxyModels : modelApi.fetchUpstreamModelCatalog;
      await open();
      expectNoPassiveChanges();
      await openMenu(id);
      await act(async () => picker(id).props.onOpenChange(false));
      expect(request).toHaveBeenCalledOnce();
      await openMenu(id);
      expect(picker(id).props.open).toBe(true);
      expect(request).toHaveBeenCalledOnce();
      expectNoConfigurationChanges();
    }
  );

  it.each(['account', 'API'] as const)(
    'silently retains a failed %s directory and retries on its next menu opening',
    async (kind) => {
      const id = kind === 'account' ? 'account-old' : 'free-old';
      const original =
        kind === 'account' ? ['account-old', 'account-new'] : ['free-old', 'free-new'];
      const request =
        kind === 'account' ? accountApi.getAccountProxyModels : modelApi.fetchUpstreamModelCatalog;
      if (kind === 'account') {
        vi.mocked(accountApi.getAccountProxyModels)
          .mockRejectedValueOnce('accountError.network')
          .mockResolvedValueOnce(['recovered-choice']);
      } else {
        vi.mocked(modelApi.fetchUpstreamModelCatalog)
          .mockRejectedValueOnce('UPSTREAM_MODELS_HTTP_503')
          .mockResolvedValueOnce({
            models: [{ id: 'recovered-choice' }],
            requiresApiKey: false,
          });
      }
      await open();
      await openMenu(id);
      expect(picker(id).props.options).toEqual(original);
      expect(picker(id).props.refreshing).toBe(false);
      expect(request).toHaveBeenCalledOnce();
      expectNoConfigurationChanges();
      await openMenu(id);
      expect(picker(id).props.options).toEqual([id, 'recovered-choice']);
      expect(request).toHaveBeenCalledTimes(2);
      expectNoConfigurationChanges();
    }
  );

  it.each([
    { state: 'missing', models: undefined },
    { state: 'empty', models: [] },
  ])(
    'keeps the saved account model and accessible picker when its directory cache is $state',
    async ({ models }) => {
      vi.mocked(accountApi.getCachedAccountProxyModels).mockReturnValue(models);
      await open();
      expect(picker('account-old').props.options).toEqual(['account-old']);
      expect(picker('account-old').props.onRefresh).toBeUndefined();
      expect(accountApi.cacheAccountProxyModels).not.toHaveBeenCalled();
      expectNoPassiveChanges();
      await openMenu('account-old');
      expect(picker('account-old').props.open).toBe(true);
      expect(accountApi.getAccountProxyModels).toHaveBeenCalledExactlyOnceWith(
        account.accountSource
      );
      expectNoConfigurationChanges();
    }
  );

  it('refreshes an existing account directory without re-adding or replacing its selected model', async () => {
    const source = account.accountSource!;
    vi.mocked(accountApi.getCachedAccountProxyModels).mockReturnValue(undefined);
    vi.mocked(accountApi.getAccountProxyModels).mockResolvedValue(['remote-other', 'remote-free']);
    await open();
    vi.mocked(accountApi.listProxyAccounts).mockResolvedValue([
      { source, label: 'Refreshed identity', freeModelIds: ['remote-free'] },
    ]);
    await openMenu('account-old');
    expect(accountApi.getAccountProxyModels).toHaveBeenCalledExactlyOnceWith(source);
    expect(accountApi.cacheAccountProxyModels).toHaveBeenCalledExactlyOnceWith(source, [
      'remote-other',
      'remote-free',
    ]);
    expect(picker('account-old').props.options).toEqual([
      'account-old',
      'remote-other',
      'remote-free',
    ]);
    expect(picker('account-old').props.freeModelIds).toEqual(['remote-free']);
    expect(picker('account-old').props.refreshing).toBe(false);
    expect(api.updateModel).not.toHaveBeenCalled();
    expect(api.setSmartRouterCandidates).not.toHaveBeenCalled();
    expect(api.queryModelUsage).not.toHaveBeenCalled();
    expect(showToast).not.toHaveBeenCalled();
  });

  it('keeps refreshed account choices after remounting without another remote request', async () => {
    let models: string[] | undefined;
    vi.mocked(accountApi.getCachedAccountProxyModels).mockImplementation((source) =>
      source.accountId === 'account-one' ? models : undefined
    );
    vi.mocked(accountApi.cacheAccountProxyModels).mockImplementation((source, next) => {
      if (source.provider === 'codex' && source.accountId === 'account-one') models = [...next];
    });
    vi.mocked(accountApi.getAccountProxyModels).mockResolvedValue(['remote-one', 'remote-two']);
    await open();
    await openMenu('account-old');
    act(() => renderer.unmount());
    await open();
    expect(picker('account-old').props.options).toEqual([
      'account-old',
      'remote-one',
      'remote-two',
    ]);
    expect(accountApi.getAccountProxyModels).toHaveBeenCalledOnce();
    expect(accountApi.cacheAccountProxyModels).toHaveBeenCalledOnce();
    expect(api.updateModel).not.toHaveBeenCalled();
    expect(showToast).not.toHaveBeenCalled();
  });

  it('uses a newer account directory from another picker after refreshing the card', async () => {
    vi.mocked(accountApi.getAccountProxyModels).mockResolvedValue(['refreshed-choice']);
    await open();
    await openMenu('account-old');
    expect(picker('account-old').props.options).toEqual(['account-old', 'refreshed-choice']);
    vi.mocked(accountApi.getCachedAccountProxyModels).mockReturnValue([
      'account-old',
      'external-choice',
    ]);
    act(() =>
      renderer.update(
        <FreeModelsProvider>
          <Harness />
        </FreeModelsProvider>
      )
    );
    expect(picker('account-old').props.options).toEqual(['account-old', 'external-choice']);
    expect(accountApi.getAccountProxyModels).toHaveBeenCalledOnce();
    expect(accountApi.cacheAccountProxyModels).toHaveBeenCalledOnce();
    expect(api.updateModel).not.toHaveBeenCalled();
    expect(showToast).not.toHaveBeenCalled();
  });

  it('retains refreshed account choices and metadata when another card is added and cache writes are unavailable', async () => {
    const source = account.accountSource!;
    const labels = { 'fresh-choice': 'Fresh model label' };
    vi.mocked(accountApi.getAccountProxyModels).mockResolvedValue(['fresh-choice', 'fresh-other']);
    await open();
    vi.mocked(accountApi.listProxyAccounts).mockResolvedValue([
      {
        source,
        label: 'Fresh account identity',
        modelLabels: labels,
        freeModelIds: ['fresh-choice'],
      },
    ]);
    await openMenu('account-old');
    const added: ModelConfig = {
      ...free,
      internalId: 'added-model',
      name: 'Another saved model',
      modelId: 'another-current',
      baseUrl: 'https://another.example/v1',
    };
    savedModels.push(added);
    await act(async () => context.addSelectedModel({ ...added, modelId: 'another-current' }));
    expect(picker('account-old').props.options).toEqual([
      'account-old',
      'fresh-choice',
      'fresh-other',
    ]);
    expect(picker('account-old').props.optionLabels).toEqual(labels);
    expect(picker('account-old').props.freeModelIds).toEqual(['fresh-choice']);
    expect(picker('account-old').props.ariaLabel).toContain('Fresh account identity');
    expect(accountApi.getAccountProxyModels).toHaveBeenCalledOnce();
    expect(accountApi.cacheAccountProxyModels).toHaveBeenCalledOnce();
    expect(api.updateModel).not.toHaveBeenCalled();
    expect(showToast).not.toHaveBeenCalled();
  });

  it('refreshes only the requested regional account even when another region shares its ID', async () => {
    const source = { provider: 'qoderCn' as const, accountId: 'shared' };
    savedModels = [
      { ...account, accountSource: { provider: 'qoder', accountId: 'shared' } },
      {
        ...account,
        internalId: 'cn-model',
        modelId: 'cn-old',
        accountSource: source,
      },
    ];
    savedOrder = savedModels.map((model) => model.internalId);
    vi.mocked(accountApi.getCachedAccountProxyModels).mockImplementation((account) =>
      account.provider === 'qoder' ? ['account-old', 'global-choice'] : ['cn-old', 'cn-choice']
    );
    vi.mocked(accountApi.getAccountProxyModels).mockResolvedValue(['cn-new']);
    await open();
    await openMenu('cn-old');
    expect(accountApi.getAccountProxyModels).toHaveBeenCalledExactlyOnceWith(source);
    expect(accountApi.cacheAccountProxyModels).toHaveBeenCalledExactlyOnceWith(source, ['cn-new']);
    expect(picker('cn-old').props.options).toEqual(['cn-old', 'cn-new']);
    expect(picker('account-old').props.options).toEqual(['account-old', 'global-choice']);
    expect(api.updateModel).not.toHaveBeenCalled();
    expect(showToast).not.toHaveBeenCalled();
  });

  it('preserves cached account choices and selection when loading its opened menu fails', async () => {
    vi.mocked(accountApi.getAccountProxyModels).mockRejectedValue('accountError.network');
    await open();
    await openMenu('account-old');
    expect(picker('account-old').props.options).toEqual(['account-old', 'account-new']);
    expect(picker('account-old').props.refreshing).toBe(false);
    expect(accountApi.cacheAccountProxyModels).not.toHaveBeenCalled();
    expect(api.updateModel).not.toHaveBeenCalled();
    expect(showToast).not.toHaveBeenCalled();
  });

  it('retains a successful account model refresh if optional account metadata cannot be loaded', async () => {
    vi.mocked(accountApi.getAccountProxyModels).mockResolvedValue(['remote-choice']);
    await open();
    vi.mocked(accountApi.listProxyAccounts).mockRejectedValue('saved metadata unavailable');
    await openMenu('account-old');
    expect(picker('account-old').props.options).toEqual(['account-old', 'remote-choice']);
    expect(picker('account-old').props.refreshing).toBe(false);
    expect(accountApi.cacheAccountProxyModels).toHaveBeenCalledExactlyOnceWith(
      account.accountSource,
      ['remote-choice']
    );
    expect(api.updateModel).not.toHaveBeenCalled();
    expect(showToast).not.toHaveBeenCalled();
  });

  it('deduplicates repeated account menu openings while its directory request is pending', async () => {
    const request = deferred<string[]>();
    vi.mocked(accountApi.getAccountProxyModels).mockReturnValue(request.promise);
    await open();
    act(() => {
      picker('account-old').props.onOpenChange(true);
      picker('account-old').props.onOpenChange(true);
    });
    expect(picker('account-old').props.refreshing).toBe(true);
    expect(picker('account-old').props.options).toEqual(['account-old', 'account-new']);
    expect(accountApi.getAccountProxyModels).toHaveBeenCalledExactlyOnceWith(account.accountSource);
    await act(async () => request.resolve(['fresh-choice']));
    expect(picker('account-old').props.options).toEqual(['account-old', 'fresh-choice']);
    expect(picker('account-old').props.refreshing).toBe(false);
    expect(accountApi.cacheAccountProxyModels).toHaveBeenCalledOnce();
    expect(api.updateModel).not.toHaveBeenCalled();
  });

  it.each(['leaving', 'changing-source', 'removing'] as const)(
    'ignores an account directory response after %s without stale cache writes',
    async (change) => {
      const request = deferred<string[]>();
      vi.mocked(accountApi.getAccountProxyModels).mockReturnValue(request.promise);
      await open();
      clickCard('account-old');
      if (change === 'leaving') {
        await act(async () => useNavigationStore.getState().setActivePage('models'));
      } else if (change === 'changing-source') {
        const replacement = {
          ...account,
          modelId: 'replacement-old',
          accountSource: { provider: 'codex' as const, accountId: 'replacement' },
        };
        savedModels = savedModels.map((model) =>
          model.internalId === account.internalId ? replacement : model
        );
        await act(async () => context.updateSelectedModel(replacement));
      } else {
        await act(async () => context.removeSelectedModel(account.internalId));
      }
      await act(async () => request.resolve(['late-choice']));
      if (change === 'leaving') {
        await act(async () => useNavigationStore.getState().setActivePage('freeModels'));
        expect(picker('account-old').props.options).toEqual(['account-old', 'account-new']);
      } else if (change === 'changing-source') {
        expect(picker('replacement-old').props.options).toEqual(['replacement-old']);
      } else {
        expect(picker('account-old')).toBeUndefined();
      }
      expect(accountApi.cacheAccountProxyModels).not.toHaveBeenCalled();
      expect(api.updateModel).not.toHaveBeenCalled();
      expect(showToast).not.toHaveBeenCalled();
    }
  );

  it('ignores refreshed account metadata that finishes after leaving the page', async () => {
    const metadata = deferred<accountApi.ProxyAccount[]>();
    vi.mocked(accountApi.getAccountProxyModels).mockResolvedValue(['fresh-choice']);
    await open();
    vi.mocked(accountApi.listProxyAccounts).mockReturnValue(metadata.promise);
    await openMenu('account-old');
    expect(picker('account-old').props.refreshing).toBe(true);
    await act(async () => useNavigationStore.getState().setActivePage('models'));
    await act(async () =>
      metadata.resolve([
        { source: account.accountSource!, label: 'Late identity', freeModelIds: ['fresh-choice'] },
      ])
    );
    expect(accountApi.cacheAccountProxyModels).not.toHaveBeenCalled();
    expect(api.updateModel).not.toHaveBeenCalled();
    expect(showToast).not.toHaveBeenCalled();
  });

  it('API catalog menu opening loads metadata and preserves a current ID omitted upstream', async () => {
    vi.mocked(api.getCachedFreeModelDirectory).mockResolvedValue({
      ...cached,
      models: [
        ...cached.models,
        {
          ...freeEntry('big-pickle'),
          freeType: 'renewing-quota',
          notes: 'Leave the API key empty. Public free model.',
        },
        freeEntry('priced-free'),
        {
          ...freeEntry('quota-only'),
          freeType: 'renewing-quota',
        },
      ],
    });
    vi.mocked(modelApi.fetchUpstreamModelCatalog).mockResolvedValue({
      models: [
        { id: 'big-pickle' },
        { id: 'priced-free', multiplier: 0.5 },
        { id: 'vendor/charged:free', free: false },
        { id: 'remote-free', free: true, multiplier: 0.5 },
        { id: 'quota-only' },
      ],
      requiresApiKey: false,
    });
    await open();
    expect(picker('free-old').props.onRefresh).toBeUndefined();
    await openMenu('free-old');
    expect(modelApi.fetchUpstreamModelCatalog).toHaveBeenCalledExactlyOnceWith({
      baseUrl: free.baseUrl,
      anthropicUrl: free.anthropicUrl,
      apiKey: free.apiKey,
    });
    expect(picker('free-old').props.options).toEqual([
      'free-old',
      'big-pickle',
      'priced-free',
      'vendor/charged:free',
      'remote-free',
      'quota-only',
    ]);
    expect(picker('free-old').props.freeModelIds).toEqual(
      expect.arrayContaining(['big-pickle', 'remote-free'])
    );
    expect(picker('free-old').props.freeModelIds).not.toContain('priced-free');
    expect(picker('free-old').props.freeModelIds).not.toContain('quota-only');
    expect(picker('free-old').props.paidModelIds).toEqual(['priced-free', 'vendor/charged:free']);
    expect(picker('free-old').props.optionMultipliers).toEqual({
      'priced-free': 0.5,
      'remote-free': 0.5,
    });
    expect(api.updateModel).not.toHaveBeenCalled();
    expect(accountApi.getAccountProxyModels).not.toHaveBeenCalled();
  });

  it('API catalog failed menu loading silently retains cached choices', async () => {
    vi.mocked(modelApi.getCachedUpstreamModelCatalog).mockResolvedValue({
      models: [{ id: 'cached-choice' }],
      requiresApiKey: false,
    });
    vi.mocked(modelApi.fetchUpstreamModelCatalog).mockRejectedValue('UPSTREAM_MODELS_HTTP_503');
    await open();
    await openMenu('free-old');
    expect(picker('free-old').props.options).toEqual(['free-old', 'cached-choice']);
    expect(showToast).not.toHaveBeenCalled();
    expect(api.updateModel).not.toHaveBeenCalled();
  });

  it('API catalog accepts a new candidate with its actual saved credentials without fetching remotely', async () => {
    savedOrder = [account.internalId];
    savedModels = savedModels.map((model) =>
      model.internalId === free.internalId
        ? { ...model, apiKey: 'encrypted:opaque-saved-key' }
        : model
    );
    vi.mocked(modelApi.getCachedUpstreamModelCatalog).mockResolvedValue({
      models: [{ id: 'saved-catalog-choice' }],
      requiresApiKey: false,
    });
    await open();
    expect(modelApi.getCachedUpstreamModelCatalog).not.toHaveBeenCalled();
    await act(async () =>
      context.addSelectedModel({
        internalId: free.internalId,
        name: free.name,
        baseUrl: free.baseUrl,
        anthropicUrl: free.anthropicUrl,
        apiKey: 'encrypted:opaque-saved-key',
        modelId: free.modelId ?? '',
        scope: free.scope,
      })
    );
    expect(modelApi.getCachedUpstreamModelCatalog).toHaveBeenCalledExactlyOnceWith({
      baseUrl: free.baseUrl,
      anthropicUrl: free.anthropicUrl,
      apiKey: 'encrypted:opaque-saved-key',
    });
    expect(picker('free-old').props.options).toEqual(['free-old', 'saved-catalog-choice']);
    expect(api.setSmartRouterCandidates).toHaveBeenCalledExactlyOnceWith([
      account.internalId,
      free.internalId,
    ]);
    expect(modelApi.fetchUpstreamModelCatalog).not.toHaveBeenCalled();
    expect(api.updateModel).not.toHaveBeenCalled();
    expect(api.getSmartRouterCandidates).toHaveBeenCalledOnce();
  });

  it('API catalog permits legacy candidate input without credentials and keeps its binding unknown', async () => {
    savedOrder = [account.internalId];
    await open();
    await act(async () =>
      context.addSelectedModel({
        internalId: free.internalId,
        name: free.name,
        baseUrl: free.baseUrl,
        modelId: 'free-old',
        scope: free.scope,
      })
    );
    expect(api.setSmartRouterCandidates).toHaveBeenCalledExactlyOnceWith([
      account.internalId,
      free.internalId,
    ]);
    expect(api.getSmartRouterCandidates).toHaveBeenCalledOnce();
    expect(
      context.customModels.find((model) => model.id === free.internalId)?.apiKey
    ).toBeUndefined();
    expect(picker('free-old').props.onRefresh).toBeUndefined();
    expect(modelApi.getCachedUpstreamModelCatalog).not.toHaveBeenCalled();
    expect(modelApi.fetchUpstreamModelCatalog).not.toHaveBeenCalled();
    expect(api.updateModel).not.toHaveBeenCalled();
  });

  it('API catalog passive cache failures keep fallback choices without dialogs or remote reads', async () => {
    vi.mocked(modelApi.getCachedUpstreamModelCatalog).mockRejectedValue('unreadable cache');
    await open();
    expect(picker('free-old').props.options).toEqual(['free-old', 'free-new']);
    expectNoPassiveChanges();
  });

  it('API catalog opening supersedes a late cache read and deduplicates repeated opening requests', async () => {
    const cache = deferred<modelApi.UpstreamModelCatalog>();
    const request = deferred<modelApi.UpstreamModelCatalog>();
    vi.mocked(modelApi.getCachedUpstreamModelCatalog).mockReturnValueOnce(cache.promise);
    vi.mocked(modelApi.fetchUpstreamModelCatalog).mockReturnValueOnce(request.promise);
    await open();
    act(() => {
      picker('free-old').props.onOpenChange(true);
      picker('free-old').props.onOpenChange(true);
    });
    expect(picker('free-old').props.refreshing).toBe(true);
    expect(modelApi.fetchUpstreamModelCatalog).toHaveBeenCalledOnce();
    await act(async () =>
      request.resolve({ models: [{ id: 'fresh-choice' }], requiresApiKey: false })
    );
    expect(picker('free-old').props.refreshing).toBe(false);
    await act(async () =>
      cache.resolve({ models: [{ id: 'old-cached-choice' }], requiresApiKey: false })
    );
    expect(picker('free-old').props.options).toEqual(['free-old', 'fresh-choice']);
    expect(api.updateModel).not.toHaveBeenCalled();
    expect(showToast).not.toHaveBeenCalled();
  });

  it('API catalog results are bound to exact endpoint credentials and ignore late replies after editing or leaving', async () => {
    const request = deferred<modelApi.UpstreamModelCatalog>();
    vi.mocked(modelApi.getCachedUpstreamModelCatalog).mockImplementation(async (input) => ({
      models: [{ id: input.apiKey === 'replacement-secret' ? 'new-key-choice' : 'old-key-choice' }],
      requiresApiKey: false,
    }));
    vi.mocked(modelApi.fetchUpstreamModelCatalog).mockReturnValueOnce(request.promise);
    await open();
    clickCard('free-old');
    savedModels = savedModels.map((model) =>
      model.internalId === free.internalId ? { ...model, apiKey: 'replacement-secret' } : model
    );
    await act(async () =>
      context.updateSelectedModel({ ...free, modelId: 'free-old', apiKey: 'replacement-secret' })
    );
    expect(picker('free-old').props.options).toEqual(['free-old', 'new-key-choice']);
    await act(async () =>
      request.resolve({ models: [{ id: 'late-old-key' }], requiresApiKey: false })
    );
    expect(picker('free-old').props.options).toEqual(['free-old', 'new-key-choice']);
    expect(showToast).not.toHaveBeenCalled();
    const leaving = deferred<modelApi.UpstreamModelCatalog>();
    vi.mocked(modelApi.fetchUpstreamModelCatalog).mockReturnValueOnce(leaving.promise);
    clickCard('free-old');
    await act(async () => useNavigationStore.getState().setActivePage('models'));
    await act(async () =>
      leaving.resolve({ models: [{ id: 'late-left-page' }], requiresApiKey: false })
    );
    await act(async () => useNavigationStore.getState().setActivePage('freeModels'));
    expect(picker('free-old').props.options).toEqual(['free-old', 'new-key-choice']);
    expect(api.updateModel).not.toHaveBeenCalled();
    expect(showToast).not.toHaveBeenCalled();
  });

  it('keeps arrow and hub identity across groups while discarding late catalog results', async () => {
    const routers = [config(), { ...config(), id: 'second', name: 'Second', port: 62186 }];
    vi.spyOn(api, 'listSmartRouters').mockResolvedValue(routers);
    const request = deferred<modelApi.UpstreamModelCatalog>();
    vi.mocked(modelApi.fetchUpstreamModelCatalog).mockReturnValueOnce(request.promise);
    await open();
    await act(async () => context.setLayout('cards'));
    const next = renderer.root.findByProps({ 'aria-label': 'freeModels.router.next' });
    const hub = renderer.root.findAll(
      (node) =>
        node.type === 'div' && String(node.props.className).includes('free-model-router-hub ')
    )[0];
    clickCard('free-old');
    expect(modelApi.fetchUpstreamModelCatalog).toHaveBeenCalledOnce();
    await act(async () => next.props.onClick());
    expect(context.activeRouterId).toBe('second');
    expect(renderer.root.findByProps({ 'aria-label': 'freeModels.router.next' })).toBe(next);
    expect(
      renderer.root.findAll(
        (node) =>
          node.type === 'div' && String(node.props.className).includes('free-model-router-hub ')
      )[0]
    ).toBe(hub);
    await act(async () =>
      request.resolve({ models: [{ id: 'late-first-group' }], requiresApiKey: false })
    );
    expect(picker('free-old').props.options).not.toContain('late-first-group');
    expectNoConfigurationChanges();
  });

  it('ignores old removal confirmation after a group round trip but permits a new removal', async () => {
    vi.spyOn(api, 'listSmartRouters').mockResolvedValue([
      config(),
      { ...config(), id: 'second', name: 'Second', port: 62186 },
    ]);
    const confirmation = deferred<boolean>();
    confirm.mockReturnValueOnce(confirmation.promise);
    await open();
    await act(async () => context.setLayout('list'));
    const removeFirst = () =>
      renderer.root
        .findAllByType('button')
        .find((node) => String(node.props['aria-label']).startsWith('btn.remove:'))!
        .props.onClick();
    await act(async () => {
      removeFirst();
    });
    expect(confirm).toHaveBeenCalledOnce();
    await act(async () => context.selectRouter('second'));
    await act(async () => context.selectRouter('smart-router'));
    await act(async () => confirmation.resolve(true));
    expect(api.removeSmartRouterCandidate).not.toHaveBeenCalled();
    confirm.mockResolvedValueOnce(true);
    await act(async () => {
      removeFirst();
    });
    expect(api.removeSmartRouterCandidate).toHaveBeenCalledExactlyOnceWith('account-model');
  });

  it.each([undefined, '', '   ', 'Pro'])('shows only an actual Cline plan: %s', async (plan) => {
    savedModels = [{ ...account, accountSource: { provider: 'cline', accountId: 'account-one' } }];
    savedOrder = [account.internalId];
    vi.mocked(accountApi.listProxyAccounts).mockResolvedValue([
      {
        source: { provider: 'cline', accountId: 'account-one' },
        label: 'cline@example.com',
        plan,
      },
    ]);
    await open();
    const content = textOf(card('account-old'));
    expect(content).toContain('cline@example.com');
    expect(content).not.toContain('—');
    expect(content.includes('Pro')).toBe(Boolean(plan?.trim()));
    expectNoPassiveChanges();
  });

  it.each([undefined, 'Pro+'])(
    'keeps the shared Kiro source, provider icon and optional plan when changing models (%s)',
    async (plan) => {
      const source = { provider: 'kiro' as const, accountId: 'shared-kiro' };
      savedModels = [{ ...account, modelId: 'claude-sonnet-fixture', accountSource: source }];
      savedOrder = [account.internalId];
      vi.mocked(accountApi.listProxyAccounts).mockResolvedValue([
        { source, label: 'kiro@example.test', plan },
      ]);
      vi.mocked(accountApi.getCachedAccountProxyModels).mockReturnValue([
        'claude-sonnet-fixture',
        'claude-opus-fixture',
      ]);
      await open();
      const content = textOf(card('claude-sonnet-fixture'));
      expect(content).toContain('kiro@example.test');
      expect(content).not.toContain('—');
      expect(content.includes('Pro+')).toBe(Boolean(plan));
      expect(card('claude-sonnet-fixture').findByType('img').props.src).toBe(
        '/icons/tools/kirocrew.svg'
      );
      expect(accountApi.listProxyAccounts).toHaveBeenCalledExactlyOnceWith('kiro');
      expectNoPassiveChanges();
      await select('claude-sonnet-fixture', 'claude-opus-fixture');
      expect(api.updateModel).toHaveBeenCalledExactlyOnceWith(account.internalId, {
        modelId: 'claude-opus-fixture',
      });
      expect(savedModels[0].accountSource).toEqual(source);
      expect(savedModels[0].apiKey).toBe('');
      expect(card('claude-opus-fixture').findByType('img').props.src).toBe(
        '/icons/tools/kirocrew.svg'
      );
      expect(accountApi.getAccountProxyModels).not.toHaveBeenCalled();
      expect(api.setSmartRouterCandidates).not.toHaveBeenCalled();
      expect(nexus.setShowAddModelModal).not.toHaveBeenCalled();
    }
  );

  it('renders readable Qoder labels while keeping native selections and region caches unchanged', async () => {
    savedModels = [
      { ...account, modelId: 'cmodel', accountSource: { provider: 'qoder', accountId: 'shared' } },
      {
        ...account,
        internalId: 'cn-model',
        modelId: 'qmodel_38max',
        accountSource: { provider: 'qoderCn', accountId: 'shared' },
      },
    ];
    savedOrder = savedModels.map((model) => model.internalId);
    const original = structuredClone(savedModels);
    const writeCache = vi.spyOn(accountApi, 'cacheAccountProxyModels');
    vi.mocked(accountApi.getCachedAccountProxyModels).mockImplementation((source) =>
      source.provider === 'qoder' ? ['Cantus', 'Qwen3.8-Max'] : ['qmodel_38max', 'cn-other']
    );
    vi.mocked(accountApi.listProxyAccounts).mockImplementation(async (provider) => [
      {
        source: { provider: provider === 'qoder' ? 'qoderCn' : 'qoder', accountId: 'shared' },
        label: 'Other region',
        modelLabels: { cmodel: 'Wrong Region', qmodel_38max: 'Wrong Region' },
      },
      {
        source: { provider, accountId: 'shared' },
        label: `${provider} identity`,
        plan: 'free',
        modelLabels: {
          [provider === 'qoder' ? 'cmodel' : 'qmodel_38max']:
            provider === 'qoder' ? 'Cantus' : 'Qwen3.8-Max',
        },
      },
    ]);
    await open();
    expect(picker('cmodel').props.options).toEqual(['cmodel', 'Qwen3.8-Max']);
    expect(picker('qmodel_38max').props.options).toEqual(['qmodel_38max', 'cn-other']);
    expect(textOf(card('cmodel'))).toContain('Cantus');
    expect(textOf(card('cmodel'))).toContain('Free');
    expect(textOf(card('qmodel_38max'))).toContain('Qwen3.8-Max');
    expect(textOf(card('qmodel_38max'))).toContain('agent.qoderTrial');
    expect(textOf(card('cmodel'))).not.toContain('Wrong Region');
    expect(writeCache).not.toHaveBeenCalled();
    expectNoPassiveChanges();
    await openMenu('cmodel');
    const selected = renderer.root.find(
      (node) => node.type === 'div' && node.children[0] === 'Cantus' && node.props.onMouseDown
    );
    expect(selected.props.className).toContain('bg-cyber-text/15');
    await act(async () => selected.props.onMouseDown({ button: 0, preventDefault: vi.fn() }));
    expect(savedModels).toEqual(original);
    expect(writeCache).toHaveBeenCalledExactlyOnceWith({ provider: 'qoder', accountId: 'shared' }, [
      'Cantus',
      'Qwen3.8-Max',
    ]);
    expect(accountApi.getAccountProxyModels).toHaveBeenCalledExactlyOnceWith({
      provider: 'qoder',
      accountId: 'shared',
    });
    expectNoConfigurationChanges();

    const stopPropagation = vi.fn();
    act(() =>
      card('cmodel')
        .find((node) => String(node.props.className).includes('free-model-route-picker'))
        .props.onPointerDown({
          stopPropagation,
          target: {},
          currentTarget: { contains: () => true },
        })
    );
    expect(stopPropagation).toHaveBeenCalledOnce();
    expect(startDrag).toHaveBeenCalledOnce();
    await act(async () =>
      renderer.root.findByType(DndContext).props.onDragStart({ active: { id: account.internalId } })
    );
    const overlay = renderer.root.find(
      (node) =>
        node.type === 'div' && String(node.props.className).includes('is-dragging shadow-2xl')
    );
    expect(textOf(overlay)).toContain('Cantus');
    await act(async () => renderer.root.findByType(DndContext).props.onDragCancel());
    expectNoConfigurationChanges();
    await select('cmodel', 'Qwen3.8-Max');
    expect(api.updateModel).toHaveBeenCalledExactlyOnceWith(account.internalId, {
      modelId: 'Qwen3.8-Max',
    });
  });

  it.each(['workBuddy', 'workBuddyAi', 'qoder', 'qoderCn', 'kilo', 'cline'] as const)(
    'shows %s cached free metadata while loading only that account when its menu opens',
    async (provider) => {
      const source = { provider, accountId: 'saved' };
      savedModels = [{ ...account, modelId: 'auto', accountSource: source }];
      savedOrder = [account.internalId];
      vi.mocked(accountApi.getCachedAccountProxyModels).mockReturnValue(['auto', 'hy3', 'charged']);
      vi.mocked(accountApi.listProxyAccounts).mockResolvedValue([
        {
          source: {
            provider: provider === 'workBuddy' ? 'workBuddyAi' : 'workBuddy',
            accountId: 'saved',
          },
          label: 'Other region',
          freeModelIds: ['charged'],
        },
        {
          source: { provider, accountId: 'other' },
          label: 'Other account',
          freeModelIds: ['auto'],
        },
        { source, label: 'Saved identity', freeModelIds: ['hy3'] },
      ]);
      await open();
      expect(picker('auto').props.freeModelIds).toEqual(['hy3']);
      expectNoPassiveChanges();
      await openMenu('auto');
      const options = renderer.root.findAll(
        (node) => node.type === 'div' && node.props.onMouseDown
      );
      expect(options.map(textOf)).toEqual(['hy3 model.free', 'auto', 'charged']);
      expect(accountApi.getAccountProxyModels).toHaveBeenCalledExactlyOnceWith(source);
      expectNoConfigurationChanges();
      await act(async () => options[0].props.onMouseDown({ button: 0, preventDefault: vi.fn() }));
      expect(api.updateModel).toHaveBeenCalledExactlyOnceWith(account.internalId, {
        modelId: 'hy3',
      });
      expect(savedModels[0].accountSource).toEqual(source);
      expect(textOf(picker('hy3'))).toContain('model.free');
      expect(accountApi.getAccountProxyModels).toHaveBeenCalledExactlyOnceWith(source);
      expect(api.queryModelUsage).not.toHaveBeenCalled();
      expect(api.setSmartRouterCandidates).not.toHaveBeenCalled();
    }
  );

  it('keeps unknown native IDs readable as IDs without borrowing another account label', async () => {
    savedModels = [
      { ...account, modelId: 'cmodel', accountSource: { provider: 'qoder', accountId: 'missing' } },
    ];
    savedOrder = [account.internalId];
    vi.mocked(accountApi.getCachedAccountProxyModels).mockReturnValue(['cmodel', 'unknown-model']);
    vi.mocked(accountApi.listProxyAccounts).mockResolvedValue([
      {
        source: { provider: 'qoder', accountId: 'other' },
        label: 'Other account',
        modelLabels: { cmodel: 'Other Account Model' },
      },
    ]);
    await open();
    expect(picker('cmodel').props.options).toEqual(['cmodel', 'unknown-model']);
    expect(picker('cmodel').props.optionLabels).toBeUndefined();
    expect(textOf(card('cmodel'))).not.toContain('Other Account Model');
    expectNoPassiveChanges();
  });

  it('uses each account cache and the exact free endpoint without fetching, applying or opening an editor', async () => {
    savedModels.push(
      {
        ...account,
        internalId: 'other-account',
        modelId: 'other-old',
        accountSource: { provider: 'codex', accountId: 'account-two' },
      },
      {
        ...account,
        internalId: 'uncached-account',
        modelId: 'uncached',
        baseUrl: free.baseUrl,
        accountSource: { provider: 'codex', accountId: 'missing' },
      },
      { ...free, internalId: 'manual', modelId: 'manual-id', baseUrl: 'https://manual.example/v1' },
      { ...free, internalId: 'local-server', modelId: 'local-id' }
    );
    savedOrder = savedModels.map((model) => model.internalId);
    vi.mocked(accountApi.getCachedAccountProxyModels).mockImplementation((source) =>
      source.accountId === 'account-one'
        ? ['account-old', 'account-new']
        : source.accountId === 'account-two'
          ? ['other-old', 'other-new']
          : undefined
    );
    await open();
    expect(picker('account-old').props.options).toEqual(['account-old', 'account-new']);
    expect(picker('other-old').props.options).toEqual(['other-old', 'other-new']);
    expect(picker('free-old').props.options).toEqual(['free-old', 'free-new']);
    expect(picker('manual-id').props.options).toEqual(['manual-id']);
    expect(picker('uncached').props.options).toEqual(['uncached']);
    expect(renderer.root.findAllByType(ModelIdCombobox)).toHaveLength(5);
    for (const combo of renderer.root.findAllByType(ModelIdCombobox)) {
      expect(combo.props.selectOnly).toBe(true);
      expect(combo.props.compact).toBe(true);
      expect(combo.props.onRefresh).toBeUndefined();
    }
    expect(JSON.stringify(renderer.toJSON())).toContain('uncached');
    expect(JSON.stringify(renderer.toJSON())).toContain('manual-id');
    expect(JSON.stringify(renderer.toJSON())).toContain('local-id');
    expect(accountApi.listProxyAccounts).toHaveBeenCalledExactlyOnceWith('codex');
    await act(async () => useNavigationStore.getState().setActivePage('models'));
    await act(async () => useNavigationStore.getState().setActivePage('freeModels'));
    expect(vi.mocked(accountApi.listProxyAccounts).mock.calls).toEqual([['codex'], ['codex']]);
    expectNoPassiveChanges();
  });

  it('keeps the cached endpoint provider icon across model-family changes, drag previews and reloads', async () => {
    savedModels = [
      { ...free, name: 'Saved nickname', modelId: 'claude-old', scope: 'smartRouter' },
    ];
    savedOrder = [free.internalId];
    vi.mocked(api.getCachedFreeModelDirectory).mockResolvedValue({
      ...cached,
      models: [freeEntry('claude-old'), freeEntry('deepseek-new')],
    });
    await open();
    expect(card('claude-old').findByType('img').props.src).toBe('/providers/free.svg');
    expect(textOf(card('claude-old'))).toContain('Saved nickname');
    expectNoPassiveChanges();
    await select('claude-old', 'deepseek-new');
    expect(card('deepseek-new').findByType('img').props.src).toBe('/providers/free.svg');
    expect(api.updateModel).toHaveBeenCalledExactlyOnceWith(free.internalId, {
      modelId: 'deepseek-new',
    });
    await act(async () => {
      renderer.root.findByType(DndContext).props.onDragStart({ active: { id: free.internalId } });
    });
    const overlay = renderer.root.find(
      (node) =>
        node.type === 'div' &&
        node.props['aria-hidden'] === true &&
        String(node.props.className).includes('free-model-route-node')
    );
    expect(overlay.findByType('img').props.src).toBe('/providers/free.svg');
    await act(async () => renderer.root.findByType(DndContext).props.onDragCancel());
    act(() => renderer.unmount());
    await open();
    expect(card('deepseek-new').findByType('img').props.src).toBe('/providers/free.svg');
    expect(api.updateModel).toHaveBeenCalledOnce();
    expect(api.getFreeModelDirectory).not.toHaveBeenCalled();
    expect(api.setSmartRouterCandidates).not.toHaveBeenCalled();
    expect(api.removeSmartRouterCandidate).not.toHaveBeenCalled();
  });

  it('preserves model-family icons for saved models on a free endpoint and tool icons for accounts', async () => {
    savedModels = [
      { ...free, modelId: 'claude-saved', scope: 'modelCenter' },
      { ...free, internalId: 'legacy-model', modelId: 'deepseek-legacy', scope: undefined },
      { ...account, modelId: 'account-old', baseUrl: free.baseUrl },
    ];
    savedOrder = savedModels.map((model) => model.internalId);
    await open();
    expect(card('claude-saved').findByType('img').props.src).toBe('/models/claude-saved.svg');
    expect(card('deepseek-legacy').findByType('img').props.src).toBe('/models/deepseek-legacy.svg');
    const codex = accountApi.accountProxyProviders.find((provider) => provider.id === 'codex')!;
    expect(card('account-old').findByType('img').props.src).toBe(`/icons/tools/${codex.icon}`);
    expectNoPassiveChanges();
  });

  it('retains the model-icon fallback for a missing free endpoint and the local server', async () => {
    savedModels = [
      {
        ...free,
        internalId: 'missing-provider',
        baseUrl: `${free.baseUrl}/other-missing`,
        modelId: 'claude-missing',
      },
      { ...free, internalId: 'local-server', modelId: 'deepseek-local' },
    ];
    savedOrder = savedModels.map((model) => model.internalId);
    await open();
    expect(cardFor('claude-missing').findByType('img').props.src).toBe(
      '/models/claude-missing.svg'
    );
    expect(cardFor('deepseek-local').findByType('img').props.src).toBe(
      '/models/deepseek-local.svg'
    );
    expectNoPassiveChanges();
  });

  it('toggles account identities in cards, accessible labels and drag previews while preserving plans and saved models', async () => {
    const buddy: ModelConfig = {
      ...account,
      internalId: 'buddy-model',
      name: 'Old account nickname',
      modelId: 'buddy-auto',
      accountSource: { provider: 'workBuddy', accountId: 'account-one' },
    };
    savedModels.push(buddy, {
      ...account,
      internalId: 'second-codex',
      modelId: 'second-old',
      accountSource: { provider: 'codex', accountId: 'account-two' },
    });
    savedOrder = savedModels.map((model) => model.internalId);
    vi.mocked(accountApi.getCachedAccountProxyModels).mockImplementation((source) =>
      source.provider === 'workBuddy'
        ? ['buddy-auto', 'buddy-new']
        : source.accountId === 'account-two'
          ? ['second-old', 'second-new']
          : ['account-old', 'account-new']
    );
    vi.mocked(accountApi.listProxyAccounts).mockImplementation(async (provider) =>
      provider === 'workBuddy'
        ? [
            {
              source: { provider: 'workBuddy', accountId: 'account-one' },
              label: 'Ben.',
              plan: '体验版',
            },
          ]
        : [
            {
              source: { provider: 'codex', accountId: 'account-one' },
              label: 'codex@example.com',
              plan: 'prolite',
            },
            {
              source: { provider: 'codex', accountId: 'account-two' },
              label: 'second@example.com',
              plan: 'team',
            },
          ]
    );
    const original = structuredClone(savedModels);
    await open(true);
    const titles = renderer.root
      .findAllByType('span')
      .filter((node) => String(node.props.className).includes('font-semibold'))
      .map((node) => node.children.join(''));
    expect(titles).toContain('codex@example.com');
    expect(titles).toContain('second@example.com');
    expect(titles).toContain('Ben.');
    expect(titles).toContain('Free provider');
    expect(titles).not.toContain('Account provider');
    expect(titles).not.toContain('Old account nickname');
    expect(accountApi.listProxyAccounts).toHaveBeenCalledTimes(2);
    expect(accountApi.listProxyAccounts).toHaveBeenCalledWith('codex');
    expect(accountApi.listProxyAccounts).toHaveBeenCalledWith('workBuddy');
    expect(textOf(card('account-old'))).toContain('Pro 100');
    expect(textOf(card('second-old'))).toContain('Business');
    expect(textOf(card('buddy-auto'))).toContain('体验版');
    for (const [modelId, plan] of [
      ['account-old', 'Pro 100'],
      ['second-old', 'Business'],
      ['buddy-auto', '体验版'],
    ]) {
      const tier = card(modelId)
        .findAllByType('span')
        .find((node) => node.children.join('').includes(plan))!;
      expect(tier.children.join('')).toBe(plan);
    }
    expect(visibilityButton().props['aria-pressed']).toBe(false);
    await act(async () => visibilityButton().props.onClick());
    expect(visibilityButton().props['aria-pressed']).toBe(true);
    const hidden = JSON.stringify(renderer.toJSON());
    for (const identity of [
      'codex@example.com',
      'second@example.com',
      'Ben.',
      'Old account nickname',
    ]) {
      expect(hidden).not.toContain(identity);
    }
    expect(textOf(card('account-old'))).toContain('Pro 100');
    expect(textOf(card('second-old'))).toContain('Business');
    expect(textOf(card('buddy-auto'))).toContain('体验版');
    expect(textOf(card('free-old'))).toContain('Free provider');
    expect(picker('account-old').props.ariaLabel).not.toContain('codex@example.com');
    await act(async () =>
      renderer.root.findByType(DndContext).props.onDragStart({ active: { id: 'account-model' } })
    );
    const overlay = renderer.root.find(
      (node) =>
        node.type === 'div' &&
        node.props['aria-hidden'] === true &&
        String(node.props.className).includes('free-model-route-node')
    );
    expect(textOf(overlay)).not.toContain('codex@example.com');
    expect(textOf(overlay)).toContain('Pro 100');
    await act(async () => renderer.root.findByType(DndContext).props.onDragCancel());
    await act(async () => visibilityButton().props.onClick());
    expect(textOf(card('account-old'))).toContain('codex@example.com');
    expect(textOf(card('second-old'))).toContain('second@example.com');
    expect(textOf(card('buddy-auto'))).toContain('Ben.');
    expect(picker('account-old').props.ariaLabel).toContain('codex@example.com');
    expect(localStorage.setItem).toHaveBeenNthCalledWith(
      1,
      'echobird-router-account-names-hidden',
      'true'
    );
    expect(localStorage.setItem).toHaveBeenLastCalledWith(
      'echobird-router-account-names-hidden',
      'false'
    );
    expect(accountApi.listProxyAccounts).toHaveBeenCalledTimes(2);
    expect(savedModels).toEqual(original);
    expectNoPassiveChanges();
  });

  it('retains hidden names across navigation and remounts, including late account metadata', async () => {
    const storage = new Map<string, string>();
    vi.mocked(localStorage.getItem).mockImplementation((key) => storage.get(key) ?? null);
    vi.mocked(localStorage.setItem).mockImplementation((key, value) => {
      storage.set(key, value);
    });
    const pending = deferred<accountApi.ProxyAccount[]>();
    vi.mocked(accountApi.listProxyAccounts).mockReturnValue(pending.promise);
    await open(true);
    await act(async () => visibilityButton().props.onClick());
    expect(textOf(card('account-old'))).not.toContain('ChatGPT / Codex');
    await act(async () =>
      pending.resolve([
        { source: account.accountSource!, label: 'late@example.com', plan: 'prolite' },
      ])
    );
    expect(textOf(card('account-old'))).not.toContain('late@example.com');
    expect(textOf(card('account-old'))).toContain('Pro 100');
    await act(async () => useNavigationStore.setState({ activePage: 'apps' }));
    await act(async () => useNavigationStore.setState({ activePage: 'freeModels' }));
    expect(textOf(card('account-old'))).not.toContain('late@example.com');
    act(() => renderer.unmount());
    await open(true);
    expect(visibilityButton().props['aria-pressed']).toBe(true);
    expect(textOf(card('account-old'))).not.toContain('late@example.com');
    expect(textOf(card('free-old'))).toContain('Free provider');
    expect(localStorage.setItem).toHaveBeenCalledTimes(1);
    expectNoPassiveChanges();
  });

  it('keeps the visibility toggle usable when preference storage is unavailable', async () => {
    vi.mocked(localStorage.getItem).mockImplementation(() => {
      throw new Error('Storage unavailable');
    });
    vi.mocked(localStorage.setItem).mockImplementation(() => {
      throw new Error('Storage unavailable');
    });
    await open(true);
    expect(textOf(card('account-old'))).toContain('codex@example.com');
    await act(async () => visibilityButton().props.onClick());
    expect(textOf(card('account-old'))).not.toContain('codex@example.com');
    expect(textOf(card('account-old'))).toContain('Pro 100');
    await act(async () => visibilityButton().props.onClick());
    expect(textOf(card('account-old'))).toContain('codex@example.com');
    expectNoPassiveChanges();
  });

  it('uses the provider name while account metadata is pending or missing without exposing internal IDs', async () => {
    const pending = deferred<accountApi.ProxyAccount[]>();
    vi.mocked(accountApi.listProxyAccounts).mockReturnValueOnce(pending.promise);
    await open();
    const titles = () =>
      renderer.root
        .findAllByType('span')
        .filter((node) => String(node.props.className).includes('font-semibold'))
        .map((node) => node.children.join(''));
    expect(titles()).toContain('ChatGPT / Codex');
    expect(titles()).not.toContain('Account provider');
    expect(titles().join(' ')).not.toContain('account-one');
    expect(textOf(card('account-old'))).not.toContain('—');
    await act(async () => pending.resolve([]));
    expect(titles()).toContain('ChatGPT / Codex');
    expect(textOf(card('account-old'))).not.toContain('—');
    expect(titles().join(' ')).not.toContain('account-one');
    expectNoPassiveChanges();
  });

  it('keeps the loaded identity after a failed passive reread and refreshes it only on router entry', async () => {
    await open();
    expect(textOf(card('account-old'))).toContain('codex@example.com');
    expect(textOf(card('account-old'))).toContain('Pro 100');
    expect(accountApi.listProxyAccounts).toHaveBeenCalledExactlyOnceWith('codex');
    await select('account-old', 'account-new');
    expect(accountApi.listProxyAccounts).toHaveBeenCalledOnce();
    vi.mocked(api.updateModel).mockClear();
    vi.mocked(accountApi.listProxyAccounts).mockRejectedValueOnce(
      new Error('Local metadata unavailable')
    );
    await act(async () => useNavigationStore.getState().setActivePage('models'));
    expect(accountApi.listProxyAccounts).toHaveBeenCalledOnce();
    await act(async () => useNavigationStore.getState().setActivePage('freeModels'));
    expect(vi.mocked(accountApi.listProxyAccounts).mock.calls).toEqual([['codex'], ['codex']]);
    expect(textOf(card('account-new'))).toContain('codex@example.com');
    expect(textOf(card('account-new'))).toContain('Pro 100');
    expect(showToast).not.toHaveBeenCalled();
    expectNoPassiveChanges();
  });

  it('loads no account metadata outside the active page and ignores a late earlier entry', async () => {
    useNavigationStore.setState({ activePage: 'models' });
    await open();
    expect(accountApi.listProxyAccounts).not.toHaveBeenCalled();
    const earlier = deferred<accountApi.ProxyAccount[]>();
    vi.mocked(accountApi.listProxyAccounts).mockReturnValueOnce(earlier.promise);
    await act(async () => useNavigationStore.getState().setActivePage('freeModels'));
    expect(accountApi.listProxyAccounts).toHaveBeenCalledExactlyOnceWith('codex');
    await act(async () => useNavigationStore.getState().setActivePage('models'));
    expect(accountApi.listProxyAccounts).toHaveBeenCalledOnce();
    await act(async () => useNavigationStore.getState().setActivePage('freeModels'));
    expect(accountApi.listProxyAccounts).toHaveBeenCalledTimes(2);
    expect(textOf(card('account-old'))).toContain('codex@example.com');
    await act(async () =>
      earlier.resolve([
        {
          source: { provider: 'codex', accountId: 'account-one' },
          label: 'stale@example.com',
          plan: 'free',
        },
      ])
    );
    expect(textOf(card('account-old'))).toContain('codex@example.com');
    expect(textOf(card('account-old'))).not.toContain('stale@example.com');
    expectNoPassiveChanges();
  });

  it('persists only the model ID and synchronizes the route and saved-model view without changing priority or credentials', async () => {
    await open();
    await select('free-old', 'free-new');
    const updated = { ...free, modelId: 'free-new' };
    expect(api.updateModel).toHaveBeenCalledExactlyOnceWith(free.internalId, {
      modelId: 'free-new',
    });
    expect(context.customModels.find((model) => model.id === free.internalId)?.modelId).toBe(
      'free-new'
    );
    expect([...context.selectedIds]).toEqual(['account-model', 'free-model']);
    expect(nexus.updateUserModel).toHaveBeenCalledExactlyOnceWith(updated, {
      name: updated.name,
      baseUrl: updated.baseUrl,
      anthropicUrl: updated.anthropicUrl,
      apiKey: updated.apiKey,
      modelId: updated.modelId,
      accountSource: updated.accountSource,
    });
    expect(picker('free-new').props.disabled).toBe(false);
    expect(api.setSmartRouterCandidates).not.toHaveBeenCalled();
    expect(nexus.setShowAddModelModal).not.toHaveBeenCalled();
    act(() => renderer.unmount());
    await open();
    expect(picker('free-new')).toBeDefined();
    expect(api.updateModel).toHaveBeenCalledOnce();
    expect(showToast).not.toHaveBeenCalled();
  });

  it('toggles API and account model choices on repeated card clicks while drag previews have no menu action', async () => {
    savedModels.push({
      ...free,
      internalId: 'manual',
      modelId: 'manual-id',
      baseUrl: 'https://manual.example/v1',
    });
    savedOrder = savedModels.map((model) => model.internalId);
    await open();
    expect(picker('account-old').props.open).toBe(false);
    clickCard('account-old');
    expect(picker('account-old').props.open).toBe(true);
    expect(picker('free-old').props.open).toBe(false);
    clickCard('account-old');
    expect(picker('account-old').props.open).toBe(false);
    clickCard('account-old');
    expect(picker('account-old').props.open).toBe(true);
    act(() => dragHandle(cardFor('manual-id')).props.onClick?.({ detail: 1 }));
    expect(picker('manual-id').props.open).toBe(true);
    expect(picker('manual-id').props.options).toEqual(['manual-id']);
    expect(renderer.root.findAllByType(ModelIdCombobox)).toHaveLength(3);
    await act(async () =>
      renderer.root.findByType(DndContext).props.onDragStart({ active: { id: 'account-model' } })
    );
    const overlay = renderer.root.find(
      (node) =>
        node.type === 'div' &&
        node.props['aria-hidden'] === true &&
        String(node.props.className).includes('free-model-route-node')
    );
    expect(overlay.findAllByType(ModelIdCombobox)).toHaveLength(0);
    act(() => overlay.findByType('button').props.onClick?.({ detail: 1 }));
    expect(accountApi.getAccountProxyModels).toHaveBeenCalledExactlyOnceWith(account.accountSource);
    expect(modelApi.fetchUpstreamModelCatalog).toHaveBeenCalledExactlyOnceWith({
      baseUrl: 'https://manual.example/v1',
      anthropicUrl: free.anthropicUrl,
      apiKey: free.apiKey,
    });
    expectNoConfigurationChanges();
  });

  it('uses Enter to open the card menu while preserving Space for keyboard dragging', async () => {
    await open();
    const preventDefault = vi.fn();
    act(() =>
      dragHandle(card('account-old')).props.onKeyDown({
        key: 'Enter',
        code: 'Enter',
        preventDefault,
      })
    );
    expect(preventDefault).toHaveBeenCalledOnce();
    expect(picker('account-old').props.open).toBe(true);
    expect(keyboardDrag).not.toHaveBeenCalled();
    const space = { key: ' ', code: 'Space', preventDefault: vi.fn() };
    act(() => dragHandle(card('account-old')).props.onKeyDown(space));
    expect(keyboardDrag).toHaveBeenCalledExactlyOnceWith(space);
    expect(accountApi.getAccountProxyModels).toHaveBeenCalledExactlyOnceWith(account.accountSource);
    expect(modelApi.fetchUpstreamModelCatalog).not.toHaveBeenCalled();
    expectNoConfigurationChanges();
  });

  it('keeps the saved account picker usable when its cached directory becomes unavailable', async () => {
    await open();
    clickCard('account-old');
    expect(picker('account-old').props.open).toBe(true);
    vi.mocked(accountApi.getCachedAccountProxyModels).mockReturnValue(undefined);
    act(() =>
      renderer.update(
        <FreeModelsProvider>
          <Harness />
        </FreeModelsProvider>
      )
    );
    expect(picker('account-old').props.options).toEqual(['account-old', 'account-new']);
    expect(picker('account-old').props.open).toBe(true);
    vi.mocked(accountApi.getCachedAccountProxyModels).mockReturnValue([
      'account-old',
      'account-new',
    ]);
    act(() =>
      renderer.update(
        <FreeModelsProvider>
          <Harness />
        </FreeModelsProvider>
      )
    );
    expect(picker('account-old').props.open).toBe(true);
    expect(accountApi.getAccountProxyModels).toHaveBeenCalledExactlyOnceWith(account.accountSource);
    expect(modelApi.fetchUpstreamModelCatalog).not.toHaveBeenCalled();
    expectNoConfigurationChanges();
  });

  it('closes the card menu for a model save and prevents clicks from reopening it while pending', async () => {
    const pending = deferred<ModelConfig | null>();
    vi.mocked(api.updateModel).mockReturnValue(pending.promise);
    await open();
    clickCard('account-old');
    expect(picker('account-old').props.open).toBe(true);
    await select('account-old', 'account-new');
    expect(picker('account-old').props.disabled).toBe(true);
    expect(picker('account-old').props.open).toBe(false);
    clickCard('account-old');
    expect(picker('account-old').props.open).toBe(false);
    await act(async () => pending.resolve({ ...account, modelId: 'account-new' }));
    expect(picker('account-new').props.open).toBe(false);
    expect(picker('account-new').props.disabled).toBe(false);
    expect(api.updateModel).toHaveBeenCalledExactlyOnceWith('account-model', {
      modelId: 'account-new',
    });
    expect(api.setSmartRouterCandidates).not.toHaveBeenCalled();
    expect(api.removeSmartRouterCandidate).not.toHaveBeenCalled();
    expect(startDrag).not.toHaveBeenCalled();
  });

  it('closes and locks an open menu while the same card is dragged and keeps it closed after release', async () => {
    await open();
    clickCard('account-old');
    expect(picker('account-old').props.open).toBe(true);
    dragState.id = 'account-model';
    act(() =>
      renderer.update(
        <FreeModelsProvider>
          <Harness />
        </FreeModelsProvider>
      )
    );
    expect(picker('account-old').props.disabled).toBe(true);
    expect(picker('account-old').props.open).toBe(false);
    clickCard('account-old');
    expect(picker('account-old').props.open).toBe(false);
    dragState.id = null;
    act(() =>
      renderer.update(
        <FreeModelsProvider>
          <Harness />
        </FreeModelsProvider>
      )
    );
    expect(picker('account-old').props.disabled).toBe(false);
    expect(picker('account-old').props.open).toBe(false);
    clickCard('account-old');
    expect(picker('account-old').props.open).toBe(true);
    expect(accountApi.getAccountProxyModels).toHaveBeenCalledExactlyOnceWith(account.accountSource);
    expect(modelApi.fetchUpstreamModelCatalog).not.toHaveBeenCalled();
    expectNoConfigurationChanges();
  });

  it('ignores the current model, permits dragging from the model row, and excludes portalled options and keyboard selection', async () => {
    await open();
    const route = card('account-old');
    const handle = dragHandle(route);
    expect(handle).toBeDefined();
    handle.props.onPointerDown();
    expect(startDrag).toHaveBeenCalledOnce();
    startDrag.mockClear();
    const pickerWrapper = picker('account-old').parent!;
    const stopPointer = vi.fn();
    const stopKeyboard = vi.fn();
    const modelRowTarget = {};
    const rowPress = {
      target: modelRowTarget,
      currentTarget: { contains: (target: unknown) => target === modelRowTarget },
      stopPropagation: stopPointer,
    };
    pickerWrapper.props.onPointerDown(rowPress);
    expect(startDrag).toHaveBeenCalledExactlyOnceWith(rowPress);
    startDrag.mockClear();
    const menuPress = {
      target: {},
      currentTarget: rowPress.currentTarget,
      stopPropagation: vi.fn(),
    };
    pickerWrapper.props.onPointerDown(menuPress);
    expect(menuPress.stopPropagation).toHaveBeenCalledOnce();
    expect(startDrag).not.toHaveBeenCalled();
    pickerWrapper.props.onKeyDown({ key: ' ', stopPropagation: stopKeyboard });
    expect(stopPointer).toHaveBeenCalledOnce();
    expect(stopKeyboard).toHaveBeenCalledOnce();
    expect(keyboardDrag).not.toHaveBeenCalled();
    await select('account-old', 'account-old');
    await select('account-old', 'unlisted-model');
    expect(api.updateModel).not.toHaveBeenCalled();
    expect(startDrag).not.toHaveBeenCalled();
    expect(confirm).not.toHaveBeenCalled();
    expect(nexus.setShowAddModelModal).not.toHaveBeenCalled();
  });

  it('preserves quick clicks, held clicks, immediate dragging and hold-then-drag while suppressing drag-release clicks with the installed sensor', async () => {
    await open();
    const descriptor = renderer.root
      .findByType(DndContext)
      .props.sensors.find((sensor: { sensor: unknown }) => sensor.sensor === PointerSensor);
    const constraint = descriptor.options.activationConstraint;
    expect(constraint.distance).toBeGreaterThan(0);
    expect(constraint.delay).toBeUndefined();
    expect(constraint.tolerance).toBeUndefined();
    const actual = await vi.importActual<typeof import('@dnd-kit/core')>('@dnd-kit/core');
    vi.useFakeTimers();
    try {
      const getSelection = vi.fn(() => null);
      const sensorDocument = Object.assign(new EventTarget(), { getSelection });
      const sensorWindow = new EventTarget();
      vi.stubGlobal('document', sensorDocument);
      vi.stubGlobal('window', sensorWindow);
      expect(document).toBe(sensorDocument);
      const addListener = vi.spyOn(sensorDocument, 'addEventListener');
      const removeListener = vi.spyOn(sensorDocument, 'removeEventListener');
      const onStart = vi.fn();
      const onAbort = vi.fn();
      const onMove = vi.fn();
      const onEnd = vi.fn();
      const press = () => {
        const event = Object.assign(new Event('pointerdown'), { clientX: 10, clientY: 10 });
        Object.defineProperty(event, 'target', { value: new EventTarget() });
        type Props = ConstructorParameters<typeof actual.PointerSensor>[0];
        return new actual.PointerSensor({
          active: 'account-model',
          activeNode: {} as Props['activeNode'],
          context: { current: {} as Props['context']['current'] },
          event,
          options: descriptor.options,
          onStart,
          onAbort,
          onPending: vi.fn(),
          onCancel: vi.fn(),
          onMove,
          onEnd,
        });
      };
      for (const heldMs of [0, 1000]) {
        press();
        vi.advanceTimersByTime(heldMs);
        expect(onStart).not.toHaveBeenCalled();
        sensorDocument.dispatchEvent(new Event('pointerup'));
        vi.runOnlyPendingTimers();
      }
      expect(onStart).not.toHaveBeenCalled();
      expect(onAbort.mock.calls).toEqual([['account-model'], ['account-model']]);
      expect(addListener.mock.calls.some(([name]) => name === 'click')).toBe(false);

      onAbort.mockClear();
      onEnd.mockClear();
      press();
      vi.advanceTimersByTime(1000);
      expect(onStart).not.toHaveBeenCalled();
      sensorDocument.dispatchEvent(
        Object.assign(new Event('pointermove'), { clientX: 11, clientY: 10 })
      );
      expect(onStart).not.toHaveBeenCalled();
      sensorDocument.dispatchEvent(
        Object.assign(new Event('pointermove'), { clientX: 22, clientY: 10 })
      );
      expect(onStart).toHaveBeenCalledExactlyOnceWith({ x: 10, y: 10 });
      expect(getSelection).toHaveBeenCalledOnce();
      vi.advanceTimersByTime(1000);
      expect(onStart).toHaveBeenCalledOnce();
      sensorDocument.dispatchEvent(
        Object.assign(new Event('pointermove', { cancelable: true }), {
          clientX: 30,
          clientY: 10,
        })
      );
      expect(onMove).toHaveBeenCalledExactlyOnceWith({ x: 30, y: 10 });
      const capture = addListener.mock.calls.find(([name]) => name === 'click')!;
      expect(capture[2]).toEqual({ capture: true });
      const stopPropagation = vi.fn();
      (capture[1] as (event: { stopPropagation: () => void }) => void)({ stopPropagation });
      expect(stopPropagation).toHaveBeenCalledOnce();
      sensorDocument.dispatchEvent(new Event('pointerup'));
      expect(onAbort).not.toHaveBeenCalled();
      expect(onEnd).toHaveBeenCalledOnce();
      expect(removeListener.mock.calls.some(([name]) => name === 'click')).toBe(false);
      vi.advanceTimersByTime(50);
      expect(removeListener).toHaveBeenCalledWith('click', capture[1], capture[2]);

      onStart.mockClear();
      press();
      sensorDocument.dispatchEvent(
        Object.assign(new Event('pointermove'), {
          clientX: 22,
          clientY: 10,
        })
      );
      expect(onStart).toHaveBeenCalledExactlyOnceWith({ x: 10, y: 10 });
      expect(onAbort).not.toHaveBeenCalled();
      vi.advanceTimersByTime(1000);
      expect(onStart).toHaveBeenCalledOnce();
      sensorDocument.dispatchEvent(new Event('pointerup'));
      expect(onEnd).toHaveBeenCalledTimes(2);
      vi.runOnlyPendingTimers();
    } finally {
      vi.useRealTimers();
    }
  });

  it('blocks repeated selections and omits card actions while preserving the old model until persistence succeeds', async () => {
    const pending = deferred<ModelConfig | null>();
    vi.mocked(api.updateModel).mockReturnValue(pending.promise);
    await open();
    const selectModel = picker('account-old').props.onChange;
    await act(async () => {
      selectModel('account-new');
      selectModel('account-new');
    });
    expect(api.updateModel).toHaveBeenCalledExactlyOnceWith('account-model', {
      modelId: 'account-new',
    });
    expect(picker('account-old').props.disabled).toBe(true);
    expect(context.customModels.find((model) => model.id === 'account-model')?.modelId).toBe(
      'account-old'
    );
    const actions = card('account-old')
      .findAllByType('button')
      .filter((node) => String(node.props['aria-label']).startsWith('btn.'));
    expect(actions).toHaveLength(0);
    expect(confirm).not.toHaveBeenCalled();
    expect(nexus.setShowAddModelModal).not.toHaveBeenCalled();
    expect(picker('free-old').props.disabled).toBe(false);
    expect(nexus.updateUserModel).not.toHaveBeenCalled();
    await act(async () => pending.resolve({ ...account, modelId: 'account-new' }));
    expect(picker('account-new').props.disabled).toBe(false);
    expect(nexus.updateUserModel).toHaveBeenCalledOnce();
    expect([...context.selectedIds]).toEqual(['account-model', 'free-model']);
  });

  it.each(['presentation', 'page navigation'] as const)(
    'keeps a pending card locked across %s until persistence completes',
    async (exit) => {
      const pending = deferred<ModelConfig | null>();
      vi.mocked(api.updateModel).mockImplementationOnce(async () => {
        const updated = await pending.promise;
        savedModels = savedModels.map((model) =>
          model.internalId === 'account-model' && updated ? updated : model
        );
        return updated;
      });
      await open();
      await select('account-old', 'account-new');
      if (exit === 'presentation') {
        await act(async () => context.setLayout('list'));
        expect(picker('account-old').props.disabled).toBe(true);
        await act(async () => context.setLayout('cards'));
      } else {
        await act(async () => useNavigationStore.getState().setActivePage('models'));
        expect(renderer.root.findAllByType(ModelIdCombobox)).toHaveLength(0);
        await act(async () => useNavigationStore.getState().setActivePage('freeModels'));
      }
      expect(context.savingModelIds.has('account-model')).toBe(true);
      expect(picker('account-old').props.disabled).toBe(true);
      const actions = card('account-old')
        .findAllByType('button')
        .filter((node) => String(node.props['aria-label']).startsWith('btn.'));
      expect(actions).toHaveLength(0);
      await act(async () => {
        picker('account-old').props.onChange('account-new');
        expect(await context.changeSelectedModel('account-model', 'account-new')).toBeUndefined();
      });
      expect(api.updateModel).toHaveBeenCalledExactlyOnceWith('account-model', {
        modelId: 'account-new',
      });
      expect(api.removeSmartRouterCandidate).not.toHaveBeenCalled();
      expect(api.setSmartRouterCandidates).not.toHaveBeenCalled();
      expect(api.getSmartRouterCandidates).toHaveBeenCalledOnce();
      expect(confirm).not.toHaveBeenCalled();
      expect(nexus.setShowAddModelModal).not.toHaveBeenCalled();
      expect(nexus.updateUserModel).not.toHaveBeenCalled();
      const updated = { ...account, modelId: 'account-new' };
      await act(async () => pending.resolve(updated));
      expect(context.savingModelIds.has('account-model')).toBe(false);
      expect(picker('account-new').props.disabled).toBe(false);
      expect([...context.selectedIds]).toEqual(['account-model', 'free-model']);
      expect(nexus.updateUserModel).toHaveBeenCalledExactlyOnceWith(updated, {
        name: updated.name,
        baseUrl: updated.baseUrl,
        anthropicUrl: '',
        apiKey: updated.apiKey,
        modelId: updated.modelId,
        accountSource: updated.accountSource,
      });
      expect(api.updateModel).toHaveBeenCalledOnce();
      expect(showToast).not.toHaveBeenCalled();
    }
  );

  it.each(['rejection', 'missing'] as const)(
    'retains the old model and restores controls after a %s result',
    async (result) => {
      await open();
      if (result === 'rejection')
        vi.mocked(api.updateModel).mockRejectedValueOnce(new Error('Disk write failed'));
      else vi.mocked(api.updateModel).mockResolvedValueOnce(null);
      await select('account-old', 'account-new');
      expect(picker('account-old').props.disabled).toBe(false);
      expect(context.customModels.find((model) => model.id === 'account-model')?.modelId).toBe(
        'account-old'
      );
      expect(nexus.updateUserModel).not.toHaveBeenCalled();
      expect(showToast).toHaveBeenCalledExactlyOnceWith('error', 'error.requestFailed');
      expect([...context.selectedIds]).toEqual(['account-model', 'free-model']);
      expect(api.setSmartRouterCandidates).not.toHaveBeenCalled();
      expect(confirm).not.toHaveBeenCalled();
      await select('account-old', 'account-new');
      expect(picker('account-new')).toBeDefined();
      expect(nexus.updateUserModel).toHaveBeenCalledOnce();
    }
  );

  it('serializes a pending model change with priority writes and page reloads', async () => {
    const pending = deferred<ModelConfig | null>();
    vi.mocked(api.updateModel).mockImplementationOnce(async () => {
      const updated = await pending.promise;
      savedModels = savedModels.map((model) =>
        model.internalId === 'account-model' && updated ? updated : model
      );
      return updated;
    });
    await open();
    await select('account-old', 'account-new');
    await act(async () => {
      renderer.root
        .findByType(DndContext)
        .props.onDragEnd({ active: { id: 'free-model' }, over: { id: 'account-model' } });
      useNavigationStore.getState().setActivePage('models');
    });
    await act(async () => useNavigationStore.getState().setActivePage('freeModels'));
    expect(api.setSmartRouterCandidates).not.toHaveBeenCalled();
    expect(api.getSmartRouterCandidates).toHaveBeenCalledOnce();
    await act(async () => pending.resolve({ ...account, modelId: 'account-new' }));
    expect(api.setSmartRouterCandidates).toHaveBeenCalledExactlyOnceWith([
      'free-model',
      'account-model',
    ]);
    expect([...context.selectedIds]).toEqual(['free-model', 'account-model']);
    expect(picker('account-new')).toBeDefined();
    expect(api.getSmartRouterCandidates).toHaveBeenCalledTimes(2);
    expect(showToast).not.toHaveBeenCalled();
  });

  it('does not resurrect a candidate removed while its model change was queued', async () => {
    const removal = deferred<Awaited<ReturnType<typeof api.removeSmartRouterCandidate>>>();
    vi.mocked(api.removeSmartRouterCandidate).mockReturnValueOnce(removal.promise);
    await open();
    let removing!: Promise<void>;
    await act(async () => {
      removing = context.removeSelectedModel('account-model');
      picker('account-old').props.onChange('account-new');
    });
    expect(api.updateModel).not.toHaveBeenCalled();
    savedOrder = ['free-model'];
    await act(async () => {
      removal.resolve(config());
      await removing;
    });
    expect([...context.selectedIds]).toEqual(['free-model']);
    expect(context.customModels.some((model) => model.id === 'account-model')).toBe(false);
    expect(api.updateModel).not.toHaveBeenCalled();
    expect(nexus.updateUserModel).not.toHaveBeenCalled();
    expect(showToast).toHaveBeenCalledExactlyOnceWith('error', 'error.requestFailed');
  });

  it('queues a drag removal behind a pending model selection and retains the saved model', async () => {
    const pending = deferred<ModelConfig | null>();
    vi.mocked(api.updateModel).mockImplementationOnce(async () => {
      const updated = await pending.promise;
      savedModels = savedModels.map((model) =>
        model.internalId === 'account-model' && updated ? updated : model
      );
      return updated;
    });
    await open();
    await select('account-old', 'account-new');
    expect(picker('account-old').props.disabled).toBe(true);
    await act(async () => {
      renderer.root.findByType(DndContext).props.onDragStart({ active: { id: 'account-model' } });
    });
    await act(async () => {
      renderer.root.findByType(DndContext).props.onDragEnd({
        active: { id: 'account-model' },
        over: { id: 'router-remove-drop-zone' },
      });
    });
    expect(api.removeSmartRouterCandidate).not.toHaveBeenCalled();
    expect([...context.selectedIds]).toEqual(['account-model', 'free-model']);
    await act(async () => pending.resolve({ ...account, modelId: 'account-new' }));
    expect(api.removeSmartRouterCandidate).toHaveBeenCalledExactlyOnceWith('account-model');
    expect([...context.selectedIds]).toEqual(['free-model']);
    expect(context.customModels.some((model) => model.id === 'account-model')).toBe(false);
    expect(savedModels.find((model) => model.internalId === 'account-model')?.modelId).toBe(
      'account-new'
    );
    expect(api.deleteModel).not.toHaveBeenCalled();
    expect(api.setSmartRouterCandidates).not.toHaveBeenCalled();
    expect(confirm).not.toHaveBeenCalled();
    expect(showToast).not.toHaveBeenCalled();
    expect(nexus.setShowAddModelModal).not.toHaveBeenCalled();
  });
});
