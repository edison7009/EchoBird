import { useLayoutEffect, useRef, type ReactNode } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { readText as readClipboardText } from '@tauri-apps/plugin-clipboard-manager';
import * as api from '../../api/tauri';
import { copyText } from '../../utils/copyText';
import { useNavigationStore } from '../../stores/navigationStore';
import { FreeModelsProvider, useFreeModels } from './provider';
import { RouterGroups } from './RouterGroups';
import type { ViewModeTabPosition } from '../../components/ViewModeTabGroup';

const { confirm, showToast } = vi.hoisted(() => ({ confirm: vi.fn(), showToast: vi.fn() }));
vi.mock('../../hooks/useI18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }));
vi.mock('../../components/ConfirmDialog', () => ({ useConfirm: () => confirm }));
vi.mock('../../components/Toast', () => ({ useToast: () => ({ showToast }) }));
vi.mock('../../components/BirdEasterEgg', () => ({ BirdEasterEgg: () => null }));
vi.mock('react-dom', () => ({ createPortal: (children: ReactNode) => children }));
vi.mock('../../utils/copyText', () => ({ copyText: vi.fn() }));
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ readText: vi.fn() }));
vi.mock('../../api/tauri', () => ({
  listSmartRouters: vi.fn(),
  getSmartRouterCandidates: vi.fn(),
  getCachedFreeModelDirectory: vi.fn(),
  getSmartRouterSettings: vi.fn(),
  getSmartRouterAvailablePort: vi.fn(),
  encryptSecret: vi.fn(),
  decryptSecret: vi.fn(),
  saveSmartRouter: vi.fn(),
  deleteSmartRouter: vi.fn(),
  setSmartRouterGroupEnabled: vi.fn(),
  setSmartRouterEnabled: vi.fn(),
  setSmartRouterCandidates: vi.fn(),
  setSmartRouterCandidateEnabled: vi.fn(),
  updateModel: vi.fn(),
}));

describe('Independent smart router groups', () => {
  let renderer: ReactTestRenderer;
  let context: ReturnType<typeof useFreeModels>;
  let groups: api.SmartRouterConfig[];
  let tabNodes: Map<
    string,
    {
      offsetParent: object;
      focus: ReturnType<typeof vi.fn>;
    }
  >;
  const restoreFocusRef = { current: false };
  const secondId = 'smart-router-734b8fb0-218f-4eb7-ac33-57901df8f983';
  function Harness() {
    const value = useFreeModels();
    const hubRef = useRef<HTMLDivElement>(null);
    const tabPositionRef = useRef<ViewModeTabPosition | null>(null);
    useLayoutEffect(() => {
      context = value;
    });
    return (
      <RouterGroups
        key={value.activeRouterId}
        hubRef={hubRef}
        restoreFocusRef={restoreFocusRef}
        tabPositionRef={tabPositionRef}
        refreshAction={<button>Refresh</button>}
      />
    );
  }
  const button = (label: string) =>
    renderer.root.find(
      (node) =>
        node.type === 'button' && node.props['aria-label'] === label && node.props.role !== 'switch'
    );
  const input = (id: string) => renderer.root.findByProps({ id });
  const address = (protocol = 'OpenAI') => input(`router-${protocol}`);
  const dice = (protocol = 'OpenAI') => button(`freeModels.router.randomPort ${protocol}`);
  const addButtons = () =>
    renderer.root.findAll(
      (node) => node.type === 'button' && node.props['aria-label'] === 'freeModels.router.add'
    );
  function populateGroups(count: number) {
    const first = groups[0];
    groups = Array.from({ length: count }, (_, index) => ({
      ...first,
      id: index === 0 ? first.id : `smart-router-test-${index + 1}`,
      name: index === 0 ? first.name : `Group ${index + 1}`,
      port: first.port + index,
      baseUrl: `http://127.0.0.1:${first.port + index}/v1`,
    }));
  }
  async function open() {
    await act(async () => {
      renderer = create(
        <FreeModelsProvider>
          <Harness />
        </FreeModelsProvider>,
        {
          createNodeMock: (element) => {
            return tabNodes.get(element.props.id) ?? null;
          },
        }
      );
    });
  }
  beforeEach(() => {
    groups = [
      {
        id: 'smart-router',
        name: 'First',
        masterEnabled: true,
        groupEnabled: true,
        enabled: true,
        candidateIds: ['one'],
        usableCandidateCount: 1,
        baseUrl: 'http://127.0.0.1:53683/v1',
        port: 53683,
        modelId: 'auto',
        running: true,
      },
      {
        id: secondId,
        name: 'Second',
        masterEnabled: true,
        groupEnabled: false,
        enabled: false,
        candidateIds: ['two'],
        disabledCandidateIds: ['two'],
        usableCandidateCount: 0,
        baseUrl: 'http://127.0.0.1:53684/v1',
        port: 53684,
        modelId: 'second',
        running: false,
      },
    ];
    tabNodes = new Map(
      groups.map((group) => [`router-group-tab-${group.id}`, { offsetParent: {}, focus: vi.fn() }])
    );
    restoreFocusRef.current = false;
    useNavigationStore.setState({ activePage: 'freeModels' });
    vi.stubGlobal('window', { setTimeout });
    vi.stubGlobal('document', {
      body: {},
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    });
    vi.stubGlobal('localStorage', { getItem: () => null, setItem: vi.fn() });
    vi.mocked(api.listSmartRouters).mockImplementation(async () =>
      groups.map((group) => ({ ...group }))
    );
    vi.mocked(api.getSmartRouterCandidates).mockImplementation(async (id) => [
      {
        internalId: id ? 'two' : 'one',
        name: id ? 'Model Two' : 'Model One',
        modelId: 'upstream',
        baseUrl: 'https://fixture.test/v1',
        apiKey: '',
      },
    ]);
    vi.mocked(api.getSmartRouterSettings).mockResolvedValue({
      id: 'smart-router',
      name: 'First',
      modelId: 'auto',
      port: 53683,
      apiKey: 'private-key',
    });
    vi.mocked(api.getSmartRouterAvailablePort).mockResolvedValue(60001);
    vi.mocked(api.encryptSecret).mockResolvedValue('enc:v1:sealed-fixture');
    vi.mocked(api.decryptSecret).mockResolvedValue('private-key');
    vi.mocked(readClipboardText).mockResolvedValue('');
    vi.mocked(api.getCachedFreeModelDirectory).mockResolvedValue(null);
    vi.mocked(api.setSmartRouterEnabled).mockImplementation(async (enabled) => {
      groups = groups.map((group) => ({
        ...group,
        masterEnabled: enabled,
        enabled: enabled && group.groupEnabled,
        running: enabled && group.groupEnabled,
      }));
      return groups[0];
    });
    vi.mocked(api.setSmartRouterGroupEnabled).mockImplementation(async (id, enabled) => {
      groups = groups.map((group) =>
        group.id === id
          ? { ...group, groupEnabled: enabled, enabled: enabled && group.masterEnabled }
          : group
      );
      return groups.find((group) => group.id === id)!;
    });
    vi.mocked(api.deleteSmartRouter).mockImplementation(async (id) => {
      groups = groups.filter((group) => group.id !== id);
    });
    confirm.mockResolvedValue(true);
    vi.mocked(copyText).mockResolvedValue(true);
  });
  afterEach(() => {
    act(() => renderer?.unmount());
    vi.useRealTimers();
    vi.resetAllMocks();
    vi.unstubAllGlobals();
  });

  it('switches only the edited group, preserves paused candidates, and never applies or starts anything', async () => {
    await open();
    await act(async () => context.setLayout('list'));
    await act(async () => button('Second').props.onClick());
    expect(context.activeRouterId).toBe(secondId);
    expect(context.customModels.map((model) => model.id)).toEqual(['two']);
    expect([...context.pausedIds]).toEqual(['two']);
    expect(context.routerEnabled).toBe(true);
    expect(context.activeRouterEnabled).toBe(false);
    expect(api.getSmartRouterCandidates).toHaveBeenLastCalledWith(secondId);
    await act(async () => context.setLayout('list'));
    expect(button('Second').props['aria-pressed']).toBe(true);
    await act(async () => button('First').props.onClick());
    expect(context.activeRouterId).toBe('smart-router');
    expect(context.customModels.map((model) => model.id)).toEqual(['one']);
    for (const fn of [
      api.saveSmartRouter,
      api.getSmartRouterSettings,
      api.getSmartRouterAvailablePort,
      api.encryptSecret,
      api.decryptSecret,
      api.setSmartRouterEnabled,
      api.setSmartRouterGroupEnabled,
      api.updateModel,
      api.setSmartRouterCandidates,
    ])
      expect(fn).not.toHaveBeenCalled();
    expect(confirm).not.toHaveBeenCalled();
    expect(readClipboardText).not.toHaveBeenCalled();
  });

  it.each(['cards', 'list'] as const)(
    'locks %s group switches while the master is off and preserves their saved states',
    async (layout) => {
      groups = groups.map((group) => ({
        ...group,
        masterEnabled: false,
        enabled: false,
        running: false,
      }));
      await open();
      await act(async () => context.setLayout(layout));
      const toggle = () => renderer.root.findByProps({ role: 'switch' });
      expect(toggle().props['aria-checked']).toBe(true);
      expect(toggle().props.disabled).toBe(true);
      expect(toggle().props.className).toContain('disabled:opacity-50');
      await act(async () => button('Second').props.onClick());
      expect(toggle().props['aria-checked']).toBe(false);
      expect(toggle().props.disabled).toBe(true);
      expect(api.setSmartRouterEnabled).not.toHaveBeenCalled();
      expect(api.setSmartRouterGroupEnabled).not.toHaveBeenCalled();
      const displayedModels = context.customModels;
      const candidateReads = vi.mocked(api.getSmartRouterCandidates).mock.calls.length;
      await act(async () => context.setRouterEnabled(true));
      expect(toggle().props.disabled).toBe(false);
      expect(toggle().props['aria-checked']).toBe(false);
      expect(context.activeRouterEnabled).toBe(false);
      expect(context.routers.map((group) => group.groupEnabled)).toEqual([true, false]);
      await act(async () => toggle().props.onClick());
      expect(context.activeRouterEnabled).toBe(true);
      expect(api.setSmartRouterGroupEnabled).toHaveBeenCalledExactlyOnceWith(secondId, true);
      expect(groups[0].groupEnabled).toBe(true);
      await act(async () => context.setRouterEnabled(false));
      expect(toggle().props.disabled).toBe(true);
      expect(toggle().props['aria-checked']).toBe(true);
      expect(context.routers.map((group) => group.groupEnabled)).toEqual([true, true]);
      expect(context.customModels).toBe(displayedModels);
      expect(api.getSmartRouterCandidates).toHaveBeenCalledTimes(candidateReads);
    }
  );

  it.each(['cards', 'list'] as const)(
    'recovers closed %s groups through the visible master switch after a failed enable',
    async (layout) => {
      vi.stubGlobal('__APP_EDITION__', 'full');
      const { Sidebar } = await import('../../components/Sidebar');
      groups = groups.map((group) => ({
        ...group,
        masterEnabled: false,
        groupEnabled: false,
        enabled: false,
        running: false,
      }));
      vi.stubGlobal('localStorage', {
        getItem: (key: string) => (key === 'echobird-router-layout' ? layout : null),
        setItem: vi.fn(),
      });
      function RecoveryHarness() {
        const value = useFreeModels();
        return (
          <>
            <Sidebar
              activePage="freeModels"
              onPageChange={(activePage) => useNavigationStore.setState({ activePage })}
              smartRouterGroups={value.routers}
              smartRouterEnabled={value.routerEnabled}
              smartRouterTogglePending={value.routerTogglePending}
              onSmartRouterChange={(enabled) => void value.setRouterEnabled(enabled)}
            />
            <Harness />
          </>
        );
      }
      await act(async () => {
        renderer = create(
          <FreeModelsProvider>
            <RecoveryHarness />
          </FreeModelsProvider>
        );
      });
      const master = () =>
        renderer.root.findByProps({ role: 'switch', 'aria-label': 'nav.smartRouter' });
      const group = () => renderer.root.findByProps({ role: 'switch', 'aria-label': 'First' });
      const displayedModels = context.customModels;
      expect(master().props['aria-checked']).toBe(false);
      expect(master().props.disabled).toBe(false);
      expect(group().props.disabled).toBe(true);
      expect(api.setSmartRouterEnabled).not.toHaveBeenCalled();
      expect(api.setSmartRouterGroupEnabled).not.toHaveBeenCalled();
      let reject!: (error: Error) => void;
      vi.mocked(api.setSmartRouterEnabled).mockImplementationOnce(
        () => new Promise((_, fail) => (reject = fail))
      );
      await act(async () => master().props.onClick());
      expect(master().props.disabled).toBe(true);
      expect(master().props['aria-busy']).toBe(true);
      await act(async () => master().props.onClick());
      expect(api.setSmartRouterEnabled).toHaveBeenCalledTimes(1);
      await act(async () => reject(new Error('Fixture enable failed')));
      expect(master().props.disabled).toBe(false);
      expect(master().props['aria-checked']).toBe(false);
      expect(group().props.disabled).toBe(true);
      expect(showToast).toHaveBeenCalledExactlyOnceWith('error', 'error.requestFailed');
      await act(async () => master().props.onClick());
      expect(api.setSmartRouterEnabled).toHaveBeenLastCalledWith(true);
      expect(master().props['aria-checked']).toBe(true);
      expect(group().props.disabled).toBe(false);
      expect(context.routers.map((router) => router.groupEnabled)).toEqual([false, false]);
      await act(async () => group().props.onClick());
      expect(api.setSmartRouterGroupEnabled).toHaveBeenCalledExactlyOnceWith('smart-router', true);
      expect(context.activeRouterEnabled).toBe(true);
      expect(context.activeRouterId).toBe('smart-router');
      expect(context.customModels).toBe(displayedModels);
      expect(context.routers[1].groupEnabled).toBe(false);
      expect(context.routers[1].disabledCandidateIds).toEqual(['two']);
      expect(api.saveSmartRouter).not.toHaveBeenCalled();
      expect(api.updateModel).not.toHaveBeenCalled();
      expect(api.setSmartRouterCandidates).not.toHaveBeenCalled();
      expect(confirm).not.toHaveBeenCalled();
    }
  );

  it('toggles the displayed group without applying models or changing groups', async () => {
    await open();
    await act(async () => button('Second').props.onClick());
    const panel = renderer.root.findByProps({ role: 'tabpanel' });
    const displayedModels = context.customModels;
    const candidateReads = vi.mocked(api.getSmartRouterCandidates).mock.calls.length;
    await act(async () => panel.findByProps({ role: 'switch' }).props.onClick());
    expect(api.setSmartRouterGroupEnabled).toHaveBeenCalledExactlyOnceWith(secondId, true);
    expect(context.activeRouterId).toBe(secondId);
    expect(context.customModels.map((model) => model.id)).toEqual(['two']);
    expect(groups[0].groupEnabled).toBe(true);
    expect(api.setSmartRouterEnabled).not.toHaveBeenCalled();
    expect(api.setSmartRouterCandidates).not.toHaveBeenCalled();
    expect(context.customModels).toBe(displayedModels);
    expect(api.getSmartRouterCandidates).toHaveBeenCalledTimes(candidateReads);
  });

  it.each(['cards', 'list'] as const)(
    'keeps %s content stable during a pending group switch and failed save',
    async (layout) => {
      await open();
      await act(async () => context.setLayout(layout));
      const toggle = () => renderer.root.findByProps({ role: 'switch' });
      const renderedSwitch = toggle();
      const displayedModels = context.customModels;
      const candidateReads = vi.mocked(api.getSmartRouterCandidates).mock.calls.length;
      let reject!: (reason: Error) => void;
      vi.mocked(api.setSmartRouterGroupEnabled).mockImplementationOnce(
        () =>
          new Promise((_, fail) => {
            reject = fail;
          })
      );
      act(() => toggle().props.onClick());
      await act(async () => undefined);
      expect(context.groupPending).toBe(true);
      expect(toggle()).toBe(renderedSwitch);
      expect(toggle().props.disabled).toBe(true);
      expect(toggle().props['aria-busy']).toBe(true);
      expect(toggle().props['aria-checked']).toBe(true);
      expect(context.customModels).toBe(displayedModels);
      await act(async () => context.setGroupEnabled('smart-router', false));
      expect(api.setSmartRouterGroupEnabled).toHaveBeenCalledOnce();
      await act(async () => reject(new Error('Fixture save failed')));
      expect(context.groupPending).toBe(false);
      expect(toggle().props.disabled).toBe(false);
      expect(toggle().props['aria-checked']).toBe(true);
      expect(context.customModels).toBe(displayedModels);
      expect(context.activeRouterId).toBe('smart-router');
      expect(showToast).toHaveBeenCalledExactlyOnceWith('error', 'error.requestFailed');
      await act(async () => toggle().props.onClick());
      expect(toggle()).toBe(renderedSwitch);
      expect(toggle().props['aria-checked']).toBe(false);
      expect(context.customModels).toBe(displayedModels);
      expect(api.getSmartRouterCandidates).toHaveBeenCalledTimes(candidateReads);
      expect(api.setSmartRouterCandidates).not.toHaveBeenCalled();
      expect(api.updateModel).not.toHaveBeenCalled();
    }
  );

  it('locks a pending master switch and preserves cached content on failure and retry', async () => {
    await open();
    const displayedModels = context.customModels;
    const candidateReads = vi.mocked(api.getSmartRouterCandidates).mock.calls.length;
    let reject!: (reason: Error) => void;
    vi.mocked(api.setSmartRouterEnabled).mockImplementationOnce(
      () =>
        new Promise((_, fail) => {
          reject = fail;
        })
    );
    let saving!: Promise<void>;
    await act(async () => {
      saving = context.setRouterEnabled(false);
      void context.setRouterEnabled(false);
    });
    expect(context.routerTogglePending).toBe(true);
    expect(context.routerEnabled).toBe(true);
    expect(context.customModels).toBe(displayedModels);
    expect(api.setSmartRouterEnabled).toHaveBeenCalledOnce();
    await act(async () => {
      reject(new Error('Fixture save failed'));
      await saving;
    });
    expect(context.routerTogglePending).toBe(false);
    expect(context.routerEnabled).toBe(true);
    expect(context.customModels).toBe(displayedModels);
    expect(showToast).toHaveBeenCalledExactlyOnceWith('error', 'error.requestFailed');
    await act(async () => context.setRouterEnabled(false));
    expect(context.routerEnabled).toBe(false);
    expect(context.customModels).toBe(displayedModels);
    expect(context.routers.map((group) => group.groupEnabled)).toEqual([true, false]);
    expect(api.getSmartRouterCandidates).toHaveBeenCalledTimes(candidateReads);
    expect(api.updateModel).not.toHaveBeenCalled();
  });

  it('cycles through groups with side arrows without saving settings', async () => {
    await open();
    await act(async () => button('freeModels.router.previous').props.onClick());
    expect(context.activeRouterId).toBe(secondId);
    await act(async () => button('freeModels.router.next').props.onClick());
    expect(context.activeRouterId).toBe('smart-router');
    expect(api.saveSmartRouter).not.toHaveBeenCalled();
    expect(api.setSmartRouterGroupEnabled).not.toHaveBeenCalled();
    expect(confirm).not.toHaveBeenCalled();
  });

  it('uses numbered tabs to replace one fixed detail panel without writes or redundant loads', async () => {
    const firstName = 'First router with a long custom name';
    groups[0].name = firstName;
    await open();
    const panel = () => renderer.root.findByProps({ role: 'tabpanel' });
    const initialReads = vi.mocked(api.getSmartRouterCandidates).mock.calls.length;
    expect(renderer.root.findAllByProps({ role: 'tabpanel' })).toHaveLength(1);
    expect(panel().props['data-router-id']).toBe('smart-router');
    expect(panel().props.onClick).toBeUndefined();
    expect(button(firstName).props['aria-selected']).toBe(true);
    expect(button('Second').props['aria-selected']).toBe(false);
    expect(button(firstName).children).toEqual(['1']);
    expect(button('Second').children).toEqual(['2']);
    expect(
      panel()
        .findAllByType('span')
        .some((node) => node.children.includes(firstName))
    ).toBe(true);
    expect(tabNodes.get('router-group-tab-smart-router')!.focus).not.toHaveBeenCalled();

    await act(async () => button('Second').props.onClick());
    expect(context.activeRouterId).toBe(secondId);
    expect(context.customModels.map((model) => model.id)).toEqual(['two']);
    expect([...context.pausedIds]).toEqual(['two']);
    expect(renderer.root.findAllByProps({ role: 'tabpanel' })).toHaveLength(1);
    expect(panel().props.id).toBe(button('Second').props['aria-controls']);
    expect(panel().props['aria-labelledby']).toBe(button('Second').props.id);
    expect(button('Second').props['aria-selected']).toBe(true);
    expect(api.getSmartRouterCandidates).toHaveBeenCalledTimes(initialReads + 1);
    await act(async () => button('Second').props.onClick());
    expect(api.getSmartRouterCandidates).toHaveBeenCalledTimes(initialReads + 1);
    for (const fn of [
      api.getSmartRouterSettings,
      api.saveSmartRouter,
      api.deleteSmartRouter,
      api.setSmartRouterEnabled,
      api.setSmartRouterGroupEnabled,
      api.setSmartRouterCandidates,
      api.updateModel,
      confirm,
      showToast,
    ])
      expect(fn).not.toHaveBeenCalled();
  });

  it('moves keyboard focus without loading a group and restores focus only after explicit activation', async () => {
    await open();
    const first = tabNodes.get('router-group-tab-smart-router')!;
    const second = tabNodes.get(`router-group-tab-${secondId}`)!;
    const initialReads = vi.mocked(api.getSmartRouterCandidates).mock.calls.length;
    const preventDefault = vi.fn();
    const currentTarget = { parentElement: { querySelectorAll: () => [first, second] } };
    act(() =>
      button('First').props.onKeyDown({ key: 'ArrowRight', currentTarget, preventDefault })
    );
    expect(preventDefault).toHaveBeenCalledOnce();
    expect(second.focus).toHaveBeenCalledOnce();
    expect(context.activeRouterId).toBe('smart-router');
    expect(api.getSmartRouterCandidates).toHaveBeenCalledTimes(initialReads);
    expect(api.getSmartRouterSettings).not.toHaveBeenCalled();

    await act(async () => button('Second').props.onClick());
    expect(context.activeRouterId).toBe(secondId);
    expect(second.focus).toHaveBeenCalledTimes(2);
    expect(restoreFocusRef.current).toBe(false);
    act(() => button('Second').props.onKeyDown({ key: 'Home', currentTarget, preventDefault }));
    expect(first.focus).toHaveBeenCalledOnce();
    expect(context.activeRouterId).toBe(secondId);
    expect(api.getSmartRouterCandidates).toHaveBeenCalledTimes(initialReads + 1);
    expect(api.saveSmartRouter).not.toHaveBeenCalled();
  });

  it('keeps copy and edit actions scoped to the current group', async () => {
    await open();
    await act(async () => button('Second').props.onClick());
    expect(button('freeModels.router.edit Second')).toBeDefined();
    expect(button('freeModels.router.delete Second')).toBeDefined();
    expect(renderer.root.findAllByProps({ 'data-router-id': 'smart-router' })).toHaveLength(0);
    const reads = vi.mocked(api.getSmartRouterCandidates).mock.calls.length;
    const stopPropagation = vi.fn();
    const panel = renderer.root.findByProps({ role: 'tabpanel' });
    await act(async () =>
      panel.findByProps({ 'aria-label': 'btn.copy OpenAI' }).props.onClick({ stopPropagation })
    );
    expect(copyText).toHaveBeenCalledExactlyOnceWith('http://127.0.0.1:53684/v1');
    expect(stopPropagation).toHaveBeenCalledOnce();
    expect(context.activeRouterId).toBe(secondId);
    expect(api.getSmartRouterCandidates).toHaveBeenCalledTimes(reads);
    expect(api.getSmartRouterSettings).not.toHaveBeenCalled();
  });

  it('blocks repeated tab activation while loading and preserves the previous panel on failure', async () => {
    await open();
    let reject!: (reason: Error) => void;
    vi.mocked(api.listSmartRouters).mockImplementationOnce(
      () =>
        new Promise((_, fail) => {
          reject = fail;
        })
    );
    act(() => button('Second').props.onClick());
    await act(async () => undefined);
    expect(context.groupPending).toBe(true);
    expect(button('First').props.disabled).toBe(true);
    expect(button('Second').props.disabled).toBe(true);
    const pendingReads = vi.mocked(api.listSmartRouters).mock.calls.length;
    act(() => button('Second').props.onClick());
    act(() => button('First').props.onClick());
    expect(api.listSmartRouters).toHaveBeenCalledTimes(pendingReads);
    await act(async () => {
      reject(new Error('Fixture read failed'));
    });
    expect(context.activeRouterId).toBe('smart-router');
    expect(context.customModels.map((model) => model.id)).toEqual(['one']);
    expect(renderer.root.findByProps({ role: 'tabpanel' }).props['data-router-id']).toBe(
      'smart-router'
    );
    expect(button('First').props['aria-selected']).toBe(true);
    expect(showToast).toHaveBeenCalledWith('error', 'error.requestFailed');
    expect(restoreFocusRef.current).toBe(false);
    await act(async () => context.setLayout('list'));
    await act(async () => context.setLayout('cards'));
    act(() => renderer.unmount());
    expect(api.listSmartRouters).toHaveBeenCalledTimes(pendingReads);
    expect(api.saveSmartRouter).not.toHaveBeenCalled();
  });
  it('reads the key only on explicit edit and keeps a failed save open for correction', async () => {
    await open();
    await act(async () => button('freeModels.router.edit First').props.onClick());
    expect(api.getSmartRouterSettings).toHaveBeenCalledExactlyOnceWith('smart-router');
    expect(input('router-apiKey').props.value).toBe('private-key');
    expect(input('router-apiKey').props.type).toBe('text');
    expect(api.getSmartRouterAvailablePort).not.toHaveBeenCalled();
    expect(address().props.readOnly).toBe(true);
    expect(address('Anthropic').props.readOnly).toBe(true);
    expect(renderer.root.findAllByProps({ id: 'router-port' })).toHaveLength(0);
    await act(async () => dice().props.onClick());
    await act(async () => input('router-name').props.onChange({ target: { value: 'Renamed' } }));
    expect(api.saveSmartRouter).not.toHaveBeenCalled();
    vi.mocked(api.saveSmartRouter).mockRejectedValueOnce('SMART_ROUTER_PORT_IN_USE');
    await act(async () => renderer.root.findByType('form').props.onSubmit({ preventDefault() {} }));
    expect(renderer.root.findByProps({ role: 'alert' }).children).toEqual([
      'freeModels.router.portInUse',
    ]);
    expect(context.routerBaseUrl).toBe('127.0.0.1:53683/v1');
    vi.mocked(api.saveSmartRouter).mockImplementationOnce(async (settings) => {
      groups[0] = {
        ...groups[0],
        ...settings,
        id: 'smart-router',
        baseUrl: `http://127.0.0.1:${settings.port}/v1`,
      };
      return groups[0];
    });
    await act(async () => renderer.root.findByType('form').props.onSubmit({ preventDefault() {} }));
    expect(api.saveSmartRouter).toHaveBeenLastCalledWith({
      id: 'smart-router',
      name: 'Renamed',
      modelId: 'auto',
      port: 60001,
      apiKey: 'private-key',
    });
    expect(context.routerBaseUrl).toBe('127.0.0.1:60001/v1');
    expect(renderer.root.findAllByProps({ role: 'dialog' })).toHaveLength(0);
    expect(api.updateModel).not.toHaveBeenCalled();
  });

  it.each([
    ['apiKey', ''],
    ['apiKey', '   '],
    ['modelId', ''],
    ['modelId', '   '],
  ] as const)('rejects a blank %s without saving or replacing the draft', async (field, value) => {
    await open();
    await act(async () => button('freeModels.router.edit First').props.onClick());
    await act(async () => {
      if (field === 'apiKey') input('router-apiKey').props.onChange({ target: { value } });
      else input('router-modelId').props.onChange({ target: { value } });
    });
    expect(renderer.root.findByProps({ type: 'submit' }).props.disabled).toBe(true);
    expect(input('router-key-hint').props['aria-hidden']).toBe(field !== 'apiKey');
    if (field === 'apiKey') {
      expect(input('router-key-hint').children).toEqual(['freeModels.router.apiKeyRequired']);
      expect(input('router-apiKey').props['aria-invalid']).toBe(true);
      expect(button('model.copy model.apiKey').props.disabled).toBe(true);
    }
    await act(async () => renderer.root.findByType('form').props.onSubmit({ preventDefault() {} }));
    expect(api.saveSmartRouter).not.toHaveBeenCalled();
    expect(api.encryptSecret).not.toHaveBeenCalled();
    expect(showToast).not.toHaveBeenCalled();
    expect(renderer.root.findAllByProps({ role: 'dialog' })).toHaveLength(1);
    await act(async () => {
      if (field === 'apiKey')
        input('router-apiKey').props.onChange({ target: { value: 'custom-route-key' } });
      else input('router-modelId').props.onChange({ target: { value: 'custom-route' } });
    });
    expect(renderer.root.findByProps({ type: 'submit' }).props.disabled).toBe(false);
    expect(input('router-key-hint').props['aria-hidden']).toBe(true);
    expect(input('router-apiKey').props['aria-invalid']).toBeUndefined();
  });

  it('randomizes only the edited port on dice click and preserves edits on probe failure', async () => {
    await open();
    await act(async () => button('freeModels.router.edit First').props.onClick());
    expect(address().props.value).toBe('127.0.0.1:53683/v1');
    expect(api.getSmartRouterAvailablePort).not.toHaveBeenCalled();
    await act(async () => input('router-name').props.onChange({ target: { value: 'Draft' } }));
    await act(async () => dice().props.onClick());
    expect(api.getSmartRouterAvailablePort).toHaveBeenCalledExactlyOnceWith(53683);
    expect(address().props.value).toBe('127.0.0.1:60001/v1');
    expect(input('router-name').props.value).toBe('Draft');
    expect(input('router-apiKey').props.value).toBe('private-key');
    vi.mocked(api.getSmartRouterAvailablePort).mockRejectedValueOnce(
      'SMART_ROUTER_PORT_UNAVAILABLE'
    );
    await act(async () => dice('Anthropic').props.onClick());
    expect(api.getSmartRouterAvailablePort).toHaveBeenLastCalledWith(60001);
    expect(renderer.root.findByProps({ role: 'alert' }).children).toEqual([
      'freeModels.router.portUnavailable',
    ]);
    expect(address().props.value).toBe('127.0.0.1:60001/v1');
    expect(input('router-name').props.value).toBe('Draft');
    expect(dice().props.disabled).toBe(false);
    expect(api.saveSmartRouter).not.toHaveBeenCalled();
  });

  it('updates both locked addresses from either dice without a manual port field', async () => {
    await open();
    await act(async () => button('freeModels.router.edit First').props.onClick());
    expect(address().props.value).toBe('127.0.0.1:53683/v1');
    expect(address('Anthropic').props.value).toBe('127.0.0.1:53683');
    expect(address().props.readOnly).toBe(true);
    expect(address('Anthropic').props.readOnly).toBe(true);
    expect(renderer.root.findAllByProps({ id: 'router-port' })).toHaveLength(0);
    await act(async () => dice().props.onClick());
    expect(api.getSmartRouterAvailablePort).toHaveBeenCalledExactlyOnceWith(53683);
    expect(address().props.value).toBe('127.0.0.1:60001/v1');
    expect(address('Anthropic').props.value).toBe('127.0.0.1:60001');
    vi.mocked(api.getSmartRouterAvailablePort).mockResolvedValueOnce(60002);
    await act(async () => dice('Anthropic').props.onClick());
    expect(api.getSmartRouterAvailablePort).toHaveBeenLastCalledWith(60001);
    expect(address().props.value).toBe('127.0.0.1:60002/v1');
    expect(address('Anthropic').props.value).toBe('127.0.0.1:60002');
    expect(renderer.root.findByProps({ type: 'submit' }).props.disabled).toBe(false);
    expect(api.saveSmartRouter).not.toHaveBeenCalled();
  });

  it('locks randomization while pending and ignores its result after closing the modal', async () => {
    await open();
    await act(async () => button('freeModels.router.edit First').props.onClick());
    let resolve!: (port: number) => void;
    vi.mocked(api.getSmartRouterAvailablePort).mockImplementationOnce(
      () =>
        new Promise((done) => {
          resolve = done;
        })
    );
    act(() => dice().props.onClick());
    await act(async () => undefined);
    expect(dice().props.disabled).toBe(true);
    expect(dice('Anthropic').props.disabled).toBe(true);
    expect(renderer.root.findByProps({ type: 'submit' }).props.disabled).toBe(true);
    act(() => dice('Anthropic').props.onClick());
    await act(async () => renderer.root.findByType('form').props.onSubmit({ preventDefault() {} }));
    expect(api.getSmartRouterAvailablePort).toHaveBeenCalledOnce();
    expect(api.saveSmartRouter).not.toHaveBeenCalled();
    await act(async () => button('btn.close').props.onClick());
    await act(async () => resolve(60002));
    expect(renderer.root.findAllByProps({ role: 'dialog' })).toHaveLength(0);
    expect(api.saveSmartRouter).not.toHaveBeenCalled();
    expect(showToast).not.toHaveBeenCalled();
  });

  it('offers a free group port, cancels without writing, and leaves one group after deletion', async () => {
    await open();
    await act(async () => button('freeModels.router.add').props.onClick());
    expect(address().props.value).toBe('127.0.0.1:60001/v1');
    expect(api.getSmartRouterAvailablePort).toHaveBeenCalledExactlyOnceWith();
    expect(input('router-name').props.value).toBe('freeModels.router.title #3');
    await act(async () => button('btn.close').props.onClick());
    expect(api.saveSmartRouter).not.toHaveBeenCalled();
    await act(async () => button('freeModels.router.delete First').props.onClick());
    expect(api.deleteSmartRouter).toHaveBeenCalledExactlyOnceWith('smart-router');
    expect(context.activeRouterId).toBe(secondId);
    expect(button('freeModels.router.delete Second').props.disabled).toBe(true);
    await act(async () => context.setLayout('list'));
    expect(button('Second').children).toEqual(['1']);
    expect(button('freeModels.router.add')).toBeDefined();
  });

  it('copies both protocol URLs and the current editable model ID and key without mutating or saving', async () => {
    await open();
    await act(async () => button('freeModels.router.edit First').props.onClick());
    await act(async () => {
      input('router-modelId').props.onChange({ target: { value: 'custom-route' } });
      input('router-apiKey').props.onChange({ target: { value: 'custom-key' } });
    });
    await act(async () => dice().props.onClick());
    for (const [label, value] of [
      ['OpenAI', 'http://127.0.0.1:60001/v1'],
      ['Anthropic', 'http://127.0.0.1:60001'],
      ['model.modelId', 'custom-route'],
      ['model.apiKey', 'custom-key'],
    ]) {
      await act(async () => button(`model.copy ${label}`).props.onClick());
      expect(copyText).toHaveBeenLastCalledWith(value);
      expect(showToast).toHaveBeenLastCalledWith('success', 'feedback.step1.copied');
    }
    expect(input('router-modelId').props.value).toBe('custom-route');
    expect(input('router-apiKey').props.value).toBe('custom-key');
    expect(readClipboardText).not.toHaveBeenCalled();
    expect(renderer.root.findAllByProps({ role: 'combobox' })).toHaveLength(0);
    expect(renderer.root.findAllByProps({ role: 'alert' })).toHaveLength(0);
    expect(api.encryptSecret).not.toHaveBeenCalled();
    expect(api.decryptSecret).not.toHaveBeenCalled();
    expect(api.saveSmartRouter).not.toHaveBeenCalled();
    await act(async () => button('btn.close').props.onClick());
    expect(api.saveSmartRouter).not.toHaveBeenCalled();
  });

  it('copies a locked key as plaintext without unlocking the draft and reuses clipboard failure feedback', async () => {
    await open();
    await act(async () => button('freeModels.router.edit First').props.onClick());
    await act(async () => button('freeModels.router.lockKey').props.onClick());
    await act(async () => button('model.copy model.apiKey').props.onClick());
    expect(api.decryptSecret).toHaveBeenCalledExactlyOnceWith('enc:v1:sealed-fixture');
    expect(copyText).toHaveBeenCalledExactlyOnceWith('private-key');
    expect(input('router-apiKey').props.readOnly).toBe(true);
    expect(input('router-apiKey').props.value).not.toContain('private-key');
    expect(input('router-key-hint').children).toEqual(['key.encrypted']);
    expect(showToast).toHaveBeenLastCalledWith('success', 'feedback.step1.copied');
    vi.mocked(copyText).mockResolvedValueOnce(false);
    await act(async () => button('model.copy model.apiKey').props.onClick());
    expect(showToast).toHaveBeenLastCalledWith('error', 'feedback.step1.failed');
    vi.mocked(api.decryptSecret).mockResolvedValueOnce('');
    await act(async () => button('model.copy model.apiKey').props.onClick());
    expect(copyText).toHaveBeenCalledTimes(2);
    expect(showToast).toHaveBeenLastCalledWith('error', 'feedback.step1.failed');
    expect(api.saveSmartRouter).not.toHaveBeenCalled();
  });

  it('does not copy or toast if a pending key decryption finishes after closing the modal', async () => {
    await open();
    await act(async () => button('freeModels.router.edit First').props.onClick());
    await act(async () => button('freeModels.router.lockKey').props.onClick());
    let resolve!: (value: string) => void;
    vi.mocked(api.decryptSecret).mockReturnValueOnce(
      new Promise((done) => {
        resolve = done;
      })
    );
    let copying!: Promise<void>;
    await act(async () => {
      copying = button('model.copy model.apiKey').props.onClick();
    });
    expect(button('model.copy model.apiKey').props.disabled).toBe(false);
    expect(renderer.root.findByProps({ type: 'submit' }).props.disabled).toBe(false);
    await act(async () => button('btn.close').props.onClick());
    await act(async () => {
      resolve('private-key');
      await copying;
    });
    expect(copyText).not.toHaveBeenCalled();
    expect(showToast).not.toHaveBeenCalled();
    expect(api.saveSmartRouter).not.toHaveBeenCalled();
  });

  it('locks and unlocks the key using the existing crypto APIs without saving the draft', async () => {
    await open();
    await act(async () => button('freeModels.router.edit First').props.onClick());
    expect(input('router-apiKey').props.readOnly).toBe(false);
    await act(async () => button('freeModels.router.lockKey').props.onClick());
    expect(api.encryptSecret).toHaveBeenCalledExactlyOnceWith('private-key');
    expect(input('router-apiKey').props.readOnly).toBe(true);
    expect(input('router-apiKey').props.value).not.toContain('private-key');
    expect(renderer.root.findAllByProps({ 'aria-label': 'model.paste model.apiKey' })).toHaveLength(
      0
    );
    await act(async () => button('freeModels.router.unlockKey').props.onClick());
    expect(api.decryptSecret).toHaveBeenCalledExactlyOnceWith('enc:v1:sealed-fixture');
    expect(input('router-apiKey').props.readOnly).toBe(false);
    expect(input('router-apiKey').props.value).toBe('private-key');
    expect(api.saveSmartRouter).not.toHaveBeenCalled();
  });

  it('decrypts a locked draft before passing it to the existing router save contract', async () => {
    await open();
    await act(async () => button('freeModels.router.edit First').props.onClick());
    await act(async () => button('freeModels.router.lockKey').props.onClick());
    vi.mocked(api.saveSmartRouter).mockResolvedValueOnce(groups[0]);
    await act(async () => renderer.root.findByType('form').props.onSubmit({ preventDefault() {} }));
    expect(api.decryptSecret).toHaveBeenCalledExactlyOnceWith('enc:v1:sealed-fixture');
    expect(api.saveSmartRouter).toHaveBeenCalledExactlyOnceWith({
      id: 'smart-router',
      name: 'First',
      modelId: 'auto',
      port: 53683,
      apiKey: 'private-key',
    });
    expect(renderer.root.findAllByProps({ role: 'dialog' })).toHaveLength(0);
  });

  it('keeps the key draft on failed encryption or decryption and never saves an empty substitute', async () => {
    await open();
    await act(async () => button('freeModels.router.edit First').props.onClick());
    vi.mocked(api.encryptSecret).mockResolvedValueOnce('private-key');
    await act(async () => button('freeModels.router.lockKey').props.onClick());
    expect(input('router-apiKey').props.value).toBe('private-key');
    expect(input('router-apiKey').props.readOnly).toBe(false);
    expect(renderer.root.findByProps({ role: 'alert' }).children).toEqual(['error.requestFailed']);
    await act(async () => button('freeModels.router.lockKey').props.onClick());
    vi.mocked(api.decryptSecret).mockResolvedValueOnce('');
    await act(async () => renderer.root.findByType('form').props.onSubmit({ preventDefault() {} }));
    expect(input('router-apiKey').props.readOnly).toBe(true);
    expect(renderer.root.findByProps({ role: 'alert' }).children).toEqual(['error.requestFailed']);
    expect(api.saveSmartRouter).not.toHaveBeenCalled();
    expect(renderer.root.findByProps({ type: 'submit' }).props.disabled).toBe(false);
  });

  it('does not begin a route write when a pre-save decryption finishes after leaving the modal', async () => {
    await open();
    await act(async () => button('freeModels.router.edit First').props.onClick());
    await act(async () => button('freeModels.router.lockKey').props.onClick());
    let resolve!: (key: string) => void;
    vi.mocked(api.decryptSecret).mockImplementationOnce(
      () =>
        new Promise((done) => {
          resolve = done;
        })
    );
    let saving!: Promise<void>;
    act(() => {
      saving = renderer.root.findByType('form').props.onSubmit({ preventDefault() {} });
    });
    await act(async () => undefined);
    expect(renderer.root.findByProps({ type: 'submit' }).props.disabled).toBe(true);
    act(() => renderer.unmount());
    await act(async () => {
      resolve('private-key');
      await saving;
    });
    expect(api.saveSmartRouter).not.toHaveBeenCalled();
  });

  it.each([7, 8])(
    'hides add at %i groups in both views while preserving existing groups and edit',
    async (count) => {
      populateGroups(count);
      const ids = groups.map((group) => group.id);
      await open();
      expect(renderer.root.findAllByProps({ role: 'tab' })).toHaveLength(count);
      expect(addButtons()).toHaveLength(0);
      await act(async () => context.setLayout('list'));
      expect(addButtons()).toHaveLength(0);
      expect(
        renderer.root
          .findByProps({ role: 'group', 'aria-label': 'freeModels.router.title' })
          .findAllByType('button')
      ).toHaveLength(count);
      expect(context.routers.map((group) => group.id)).toEqual(ids);
      expect(button('freeModels.router.delete First').props.disabled).toBe(false);
      await act(async () => button('freeModels.router.edit First').props.onClick());
      expect(api.getSmartRouterSettings).toHaveBeenCalledExactlyOnceWith('smart-router');
      expect(renderer.root.findAllByProps({ role: 'dialog' })).toHaveLength(1);
      expect(api.saveSmartRouter).not.toHaveBeenCalled();
      expect(api.deleteSmartRouter).not.toHaveBeenCalled();
      expect(confirm).not.toHaveBeenCalled();
    }
  );

  it('hides add after creating the seventh group and restores it after deleting back to six', async () => {
    populateGroups(6);
    const newId = 'smart-router-7d55a193-322a-46b2-aedb-47dbddcdb877';
    vi.mocked(api.saveSmartRouter).mockImplementationOnce(async (settings) => {
      const saved = {
        ...groups[0],
        id: newId,
        name: settings.name,
        modelId: settings.modelId,
        port: settings.port,
        baseUrl: `http://127.0.0.1:${settings.port}/v1`,
      };
      groups = [...groups, saved];
      return saved;
    });
    await open();
    expect(addButtons()).toHaveLength(1);
    await act(async () => button('freeModels.router.add').props.onClick());
    const savedName = input('router-name').props.value;
    await act(async () => renderer.root.findByType('form').props.onSubmit({ preventDefault() {} }));
    expect(context.routers).toHaveLength(7);
    expect(addButtons()).toHaveLength(0);
    await act(async () => context.setLayout('list'));
    expect(addButtons()).toHaveLength(0);
    await act(async () => button(`freeModels.router.delete ${savedName}`).props.onClick());
    expect(api.deleteSmartRouter).toHaveBeenCalledExactlyOnceWith(newId);
    expect(context.routers).toHaveLength(6);
    expect(addButtons()).toHaveLength(1);
    await act(async () => context.setLayout('cards'));
    expect(addButtons()).toHaveLength(1);
    expect(api.saveSmartRouter).toHaveBeenCalledOnce();
    expect(api.updateModel).not.toHaveBeenCalled();
    expect(api.setSmartRouterCandidates).not.toHaveBeenCalled();
  });

  it('blocks candidate edits during a pending selection and retains the old group on load failure', async () => {
    await open();
    let reject!: (reason: Error) => void;
    vi.mocked(api.listSmartRouters).mockImplementationOnce(
      () =>
        new Promise((_, fail) => {
          reject = fail;
        })
    );
    let selection!: Promise<void>;
    await act(async () => {
      selection = context.selectRouter(secondId);
    });
    expect(context.groupPending).toBe(true);
    await expect(
      context.addSelectedModel({
        internalId: 'old-row',
        name: 'Old',
        modelId: 'upstream',
        baseUrl: 'https://fixture.test/v1',
      })
    ).rejects.toThrow();
    await act(async () => {
      reject(new Error('Fixture read failed'));
      await selection;
    });
    expect(context.activeRouterId).toBe('smart-router');
    expect(context.customModels.map((model) => model.id)).toEqual(['one']);
    expect(context.groupPending).toBe(false);
    expect(api.setSmartRouterCandidates).not.toHaveBeenCalled();
    expect(showToast).toHaveBeenCalledWith('error', 'error.requestFailed');
  });
});
