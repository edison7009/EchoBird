import { enableAccountPlugins } from '../../test/accountPlugins';
import type { ReactNode } from 'react';
import { DndContext, DragOverlay, PointerSensor, KeyboardSensor } from '@dnd-kit/core';
import { SortableContext } from '@dnd-kit/sortable';
import { SortableCard } from '../../components/SortableCard';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { VoiceInputMain, VoiceInputPanel } from './VoiceInput';
import { useVoiceInputStore as voice } from '../../stores/voiceInputStore';
import { useNavigationStore as navigation } from '../../stores/navigationStore';
import { useToolsStore as tools } from '../../stores/toolsStore';
import { voiceSnapshot } from './testFixtures';
import * as api from '../../api/models';
import * as voiceApi from '../../api/voiceInput';
import { open } from '@tauri-apps/plugin-shell';
import { DirectoryRow } from '../../components/DirectoryRow';
import { ViewModeTabs } from '../../components/ViewModeTabs';
import { AccountProxyPanel } from '../FreeModels/AccountProxyPanel';
import { useAccountAccessRiskStore } from '../../stores/accountAccessRiskStore';
import bundled from '../../data/modelDirectory.json';
import remote from '../../../docs/api/model-directory/index.json';

const { context, account, confirm } = vi.hoisted(() => ({
  confirm: vi.fn(),
  context: {
    userModels: [],
    setNewModelForm: vi.fn(),
    setEditingModelId: vi.fn(),
    setKeyDestroyed: vi.fn(),
    setShowApiKey: vi.fn(),
    setModelModalDestination: vi.fn(),
    setShowAddModelModal: vi.fn(),
  },
  account: { accounts: [], add: vi.fn(), cancelLogin: vi.fn(), refresh: vi.fn() },
}));

vi.mock('react-dom', () => ({ createPortal: (children: ReactNode) => children }));
vi.mock('@dnd-kit/core', async (original) => ({
  ...(await original<typeof import('@dnd-kit/core')>()),
  DndContext: ({ children }: { children: ReactNode }) => children,
  DragOverlay: ({ children }: { children: ReactNode }) => children,
}));
vi.mock('@dnd-kit/sortable', async (original) => ({
  ...(await original<typeof import('@dnd-kit/sortable')>()),
  SortableContext: ({ children }: { children: ReactNode }) => children,
  useSortable: () => ({
    attributes: { tabIndex: 0 },
    listeners: { onPointerDown: vi.fn(), onKeyDown: vi.fn() },
    setNodeRef: vi.fn(),
    setActivatorNodeRef: vi.fn(),
    transform: null,
    transition: undefined,
    isDragging: false,
  }),
}));
vi.mock('../../hooks/useI18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }));
vi.mock('../ModelNexus/context', () => ({ useModelNexus: () => context }));
vi.mock('../MotherAgent/context', () => ({ useMotherAgent: () => ({ models: [] }) }));
vi.mock('./provider', () => ({ useVoiceAccounts: () => account }));
vi.mock('./VoiceIsland', () => ({ IslandContent: () => null }));
vi.mock('../../components/BirdEasterEgg', () => ({ BirdEasterEgg: () => null }));
vi.mock('../../components/Toast', () => ({ useToast: () => ({ showToast: vi.fn() }) }));
vi.mock('../../components/ConfirmDialog', () => ({ useConfirm: () => confirm }));
vi.mock('../../api/models', () => ({ getModelDirectory: vi.fn(), addModel: vi.fn() }));
vi.mock('../../api/voiceInput', async (original) => ({
  ...(await original<typeof import('../../api/voiceInput')>()),
  getVoiceModels: vi.fn(),
  saveVoiceConfig: vi.fn(),
  startVoice: vi.fn(),
  recordVoice: vi.fn(),
  getVoiceState: vi.fn(),
  getVoiceDevices: vi.fn(),
}));
vi.mock('@tauri-apps/plugin-shell', () => ({ open: vi.fn().mockResolvedValue(undefined) }));
let renderer: ReactTestRenderer;
let stored: Map<string, string>;
let setItem: ReturnType<typeof vi.fn>;
let Sidebar: typeof import('../../components/Sidebar').Sidebar;
beforeAll(async () => {
  vi.stubGlobal('__APP_EDITION__', 'full');
  ({ Sidebar } = await import('../../components/Sidebar'));
});
beforeEach(() => {
  enableAccountPlugins();
  vi.clearAllMocks();
  useAccountAccessRiskStore.setState({ acknowledged: false, hydrated: false });
  stored = new Map();
  setItem = vi.fn((key: string, value: string) => stored.set(key, value));
  vi.stubGlobal('localStorage', { getItem: (key: string) => stored.get(key) ?? null, setItem });
  confirm.mockResolvedValue(true);
  vi.stubGlobal('document', { addEventListener: vi.fn(), removeEventListener: vi.fn() });
  vi.stubGlobal('navigator', { platform: 'Win32' });
  vi.stubGlobal('window', {
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    open: vi.fn(),
  });
  voice.setState({ ...voice.getInitialState(), snapshot: voiceSnapshot() });
  navigation.setState({ activePage: 'voice' });
  tools.setState({
    detectedTools: [{ id: 'workbuddy', name: 'WorkBuddy', category: 'desktop', installed: true }],
  });
  vi.mocked(api.getModelDirectory).mockResolvedValue(null);
  vi.mocked(voiceApi.getVoiceModels).mockResolvedValue([]);
});
afterEach(() => {
  act(() => renderer?.unmount());
  vi.unstubAllGlobals();
});
async function mount() {
  await act(async () => {
    renderer = create(<VoiceInputPanel />);
  });
}
async function modelTab() {
  await act(async () =>
    renderer.root.findByType(VoiceInputPanel).findByType(ViewModeTabs).props.onChange('model')
  );
}
it('has two compact tabs; navigation does not open a form, log in, refresh quota, record or write', async () => {
  await mount();
  expect(
    renderer.root
      .findByType(ViewModeTabs)
      .props.options.map((option: { label: string }) => option.label)
  ).toEqual(['voice.accountInput', 'btn.addModel']);
  await modelTab();
  expect(renderer.root.findAllByType(DirectoryRow)).toHaveLength(12);
  expect(context.setShowAddModelModal).not.toHaveBeenCalled();
  expect(account.add).not.toHaveBeenCalled();
  expect(account.refresh).not.toHaveBeenCalled();
  expect(api.addModel).not.toHaveBeenCalled();
  expect(voiceApi.saveVoiceConfig).not.toHaveBeenCalled();
  expect(voiceApi.startVoice).not.toHaveBeenCalled();
});
it('opens the shared account form only on explicit selection and uses the speech destination', async () => {
  await mount();
  act(() =>
    renderer.root.findByProps({ type: 'checkbox' }).props.onChange({ target: { checked: true } })
  );
  act(() =>
    renderer.root.findByProps({ 'aria-label': 'voice.accountInput: WorkBuddy' }).props.onClick()
  );
  expect(context.setNewModelForm).toHaveBeenCalledWith(
    expect.objectContaining({ accountSource: { provider: 'workBuddy', accountId: '' } })
  );
  expect(context.setModelModalDestination).toHaveBeenCalledExactlyOnceWith('voiceInput');
  expect(context.setShowAddModelModal).toHaveBeenCalledWith(true);
  expect(account.add).not.toHaveBeenCalled();
});
it.each([0, 3])(
  'shares risk confirmation and revocation with smart access with %i sources',
  async (count) => {
    voice.setState({
      snapshot: voiceSnapshot({
        config: {
          ...voiceSnapshot().config,
          channels: Array.from({ length: count }, (_, index) => ({
            kind: 'model' as const,
            id: `speech-${index}`,
          })),
        },
      }),
    });
    await act(async () => {
      renderer = create(
        <>
          <VoiceInputPanel />
          <AccountProxyPanel />
        </>
      );
    });
    const checkboxes = () => renderer.root.findAllByProps({ type: 'checkbox' });
    const voiceRow = () =>
      renderer.root.findByProps({ 'aria-label': 'voice.accountInput: WorkBuddy' });
    const routerRow = () =>
      renderer.root.findByProps({ 'aria-label': 'freeModels.addToRouter: WorkBuddy' });
    expect(checkboxes().map((box) => box.props.checked)).toEqual([false, false]);
    expect(voiceRow().props.disabled).toBe(true);
    expect(routerRow().props.disabled).toBe(true);
    act(() => {
      voiceRow().props.onClick();
      routerRow().props.onClick();
    });
    expect(context.setShowAddModelModal).not.toHaveBeenCalled();
    await act(async () => checkboxes()[0].props.onChange({ target: { checked: true } }));
    expect(checkboxes().map((box) => box.props.checked)).toEqual([true, true]);
    expect(voiceRow().props.disabled).toBe(false);
    expect(routerRow().props.disabled).toBe(false);
    const notices = renderer.root.findAllByProps({ 'aria-haspopup': 'dialog' });
    await act(async () => {
      await notices[0].props.onClick();
      await notices[1].props.onClick();
    });
    expect(confirm).toHaveBeenCalledTimes(2);
    expect(confirm.mock.calls[0]).toEqual(confirm.mock.calls[1]);
    expect(confirm.mock.calls[0][0].message).toBe('accountProxy.riskMessage');
    await act(async () => checkboxes()[1].props.onChange({ target: { checked: false } }));
    expect(checkboxes().map((box) => box.props.checked)).toEqual([false, false]);
    expect(voiceRow().props.disabled).toBe(true);
    expect(routerRow().props.disabled).toBe(true);
    expect(setItem.mock.calls).toEqual([
      ['echobird-account-proxy-risk-acknowledged', 'true'],
      ['echobird-account-proxy-risk-acknowledged', 'false'],
    ]);
    expect(api.addModel).not.toHaveBeenCalled();
    expect(voiceApi.saveVoiceConfig).not.toHaveBeenCalled();
    expect(voiceApi.startVoice).not.toHaveBeenCalled();
  }
);
it('opens account and API model forms with three sources without changing the configured sources', async () => {
  const config = {
    ...voiceSnapshot().config,
    channels: [1, 2, 3].map((index) => ({ kind: 'model' as const, id: `speech-${index}` })),
  };
  voice.setState({ snapshot: voiceSnapshot({ config }) });
  useAccountAccessRiskStore.setState({ acknowledged: true, hydrated: true });
  await mount();
  const row = renderer.root.findByProps({ 'aria-label': 'voice.accountInput: WorkBuddy' });
  expect(row.props.disabled).toBe(false);
  expect(renderer.root.findByProps({ 'aria-label': 'freeModels.customAdd' }).props.disabled).toBe(
    false
  );
  act(() => row.props.onClick());
  expect(context.setShowAddModelModal).toHaveBeenCalledOnce();
  await modelTab();
  const model = renderer.root.findAllByType(DirectoryRow)[0];
  expect(model.props.add.disabled).toBe(false);
  act(() => model.props.add.onClick());
  expect(context.setShowAddModelModal).toHaveBeenCalledTimes(2);
  expect(voice.getState().snapshot?.config).toEqual(config);
  expect(api.addModel).not.toHaveBeenCalled();
  expect(voiceApi.saveVoiceConfig).not.toHaveBeenCalled();
  expect(account.add).not.toHaveBeenCalled();
  expect(account.refresh).not.toHaveBeenCalled();
});
it('prefills speech URL/model/protocol from plus; the website side only opens the homepage', async () => {
  await mount();
  await modelTab();
  const row = renderer.root
    .findAllByType(DirectoryRow)
    .find((node) => node.props.name.includes('MiMo'))!;
  act(() => row.props.add.onClick());
  expect(context.setNewModelForm).toHaveBeenCalledWith(
    expect.objectContaining({
      baseUrl: 'https://api.xiaomimimo.com/v1',
      anthropicUrl: '',
      modelId: 'mimo-v2.5-asr',
      voiceOptions: { protocol: 'mimo' },
    })
  );
  context.setNewModelForm.mockClear();
  await act(async () => row.props.onOpen());
  expect(open).toHaveBeenCalledExactlyOnceWith('https://platform.xiaomimimo.com/');
  expect(context.setNewModelForm).not.toHaveBeenCalled();
  expect(voiceApi.saveVoiceConfig).not.toHaveBeenCalled();
});
it('preserves bundled speech providers when the current remote directory has no speech list', async () => {
  vi.mocked(api.getModelDirectory).mockResolvedValue({ providers: [], relays: [] });
  await mount();
  await modelTab();
  expect(renderer.root.findAllByType(DirectoryRow)).toHaveLength(12);
  expect(bundled.speechProviders).toEqual(remote.speechProviders);
});

it.each([
  [false, 'account', false],
  [true, 'account', false],
  [false, 'model', false],
  [true, 'model', false],
  [false, 'account', true],
  [true, 'model', true],
] as const)(
  'keeps the page stable for master=%s, tab=%s, failed=%s during a delayed sidebar toggle',
  async (shortcutEnabled, tab, failed) => {
    useAccountAccessRiskStore.setState({ acknowledged: true, hydrated: true });
    const config = {
      ...voiceSnapshot().config,
      shortcutEnabled,
      optimizationEnabled: true,
      channels: [
        { kind: 'model' as const, id: 'speech-one' },
        { kind: 'model' as const, id: 'speech-two' },
      ],
    };
    voice.setState({ snapshot: voiceSnapshot({ config }) });
    let finish!: (snapshot: ReturnType<typeof voiceSnapshot>) => void;
    let fail!: (reason: Error) => void;
    vi.mocked(voiceApi.saveVoiceConfig).mockReturnValueOnce(
      new Promise((resolve, reject) => {
        finish = resolve;
        fail = reject;
      })
    );
    await act(async () => {
      renderer = create(
        <>
          <Sidebar
            activePage="voice"
            onPageChange={vi.fn()}
            smartRouterGroups={[]}
            smartRouterEnabled={false}
            smartRouterTogglePending={false}
            onSmartRouterChange={vi.fn()}
          />
          <VoiceInputMain />
          <VoiceInputPanel />
        </>
      );
    });
    if (tab === 'model') await modelTab();
    const main = renderer.root.findByProps({ className: 'voice-grid' });
    const panel = renderer.root.findByProps({
      className: 'voice-input-panel flex h-full flex-col overflow-hidden',
    });
    const sections = renderer.root.findAllByType('section');
    const channels = renderer.root.findAllByProps({ role: 'listitem' });
    const tabs = renderer.root.findByType(VoiceInputPanel).findByType(ViewModeTabs);
    const toggle = renderer.root.findByProps({
      role: 'switch',
      'aria-label': 'voice.enableInput',
    });
    const route = renderer.root.findByProps({ className: 'free-model-route-arrow' }).parent!;
    expect(route.props.className).toBe(
      `voice-routing-lines${shortcutEnabled ? '' : ' is-disabled'}`
    );
    const remove = renderer.root.findAllByProps({ 'aria-label': 'voice.removeChannel' });
    const add = renderer.root.findByProps({ 'aria-label': 'freeModels.customAdd' });
    expect(remove.every((button) => !button.props.disabled)).toBe(true);
    expect(add.props.disabled).toBe(false);
    act(() => {
      void toggle.props.onClick();
    });
    expect(voice.getState().saving).toBe(true);
    expect(main.props['data-saving']).toBe(true);
    expect(panel.props['data-saving']).toBe(true);
    expect(toggle.props['aria-checked']).toBe(shortcutEnabled);
    expect(route.props.className).toBe(
      `voice-routing-lines${shortcutEnabled ? '' : ' is-disabled'}`
    );
    expect(toggle.props.disabled).toBe(true);
    expect(remove.every((button) => button.props.disabled)).toBe(true);
    expect(add.props.disabled).toBe(true);
    act(() => {
      toggle.props.onClick();
      remove[0].props.onClick();
      add.props.onClick();
    });
    const saved = { ...config, shortcutEnabled: !shortcutEnabled };
    expect(voiceApi.saveVoiceConfig).toHaveBeenCalledExactlyOnceWith(saved);
    await act(async () => {
      if (failed) fail(new Error('save failed'));
      else finish(voiceSnapshot({ config: saved, revision: 2 }));
    });
    expect(toggle.props['aria-checked']).toBe(failed ? shortcutEnabled : !shortcutEnabled);
    const enabled = failed ? shortcutEnabled : !shortcutEnabled;
    expect(route.props.className).toBe(`voice-routing-lines${enabled ? '' : ' is-disabled'}`);
    expect(renderer.root.findByProps({ className: 'free-model-route-arrow' }).parent).toBe(route);
    if (failed)
      expect(renderer.root.findByProps({ role: 'alert' }).children).toContain('Error: save failed');
    expect(toggle.props.disabled).toBe(false);
    expect(main.props['data-saving']).toBe(false);
    expect(panel.props['data-saving']).toBe(false);
    expect(renderer.root.findAllByType('section')).toEqual(sections);
    expect(renderer.root.findAllByProps({ role: 'listitem' })).toEqual(channels);
    expect(renderer.root.findByType(VoiceInputPanel).findByType(ViewModeTabs)).toBe(tabs);
    expect(tabs.props.value).toBe(tab);
    expect(renderer.root.findByProps({ className: 'voice-grid' })).toBe(main);
    expect(
      renderer.root.findByProps({
        className: 'voice-input-panel flex h-full flex-col overflow-hidden',
      })
    ).toBe(panel);
    expect(voiceApi.getVoiceModels).toHaveBeenCalledTimes(1);
    expect(api.getModelDirectory).toHaveBeenCalledTimes(1);
    expect(voiceApi.getVoiceState).not.toHaveBeenCalled();
    expect(voiceApi.getVoiceDevices).not.toHaveBeenCalled();
    expect(voiceApi.startVoice).not.toHaveBeenCalled();
    expect(voiceApi.recordVoice).not.toHaveBeenCalled();
    expect(context.setShowAddModelModal).not.toHaveBeenCalled();
    expect(account.add).not.toHaveBeenCalled();
    expect(account.refresh).not.toHaveBeenCalled();
  }
);

it.each(['recording', 'pending', 'risk'] as const)(
  'preserves unavailable panel feedback for %s even while saving',
  async (reason) => {
    useAccountAccessRiskStore.setState({ acknowledged: reason !== 'risk', hydrated: true });
    voice.setState({
      saving: true,
      pending: reason === 'pending',
      snapshot: voiceSnapshot({
        phase: reason === 'recording' ? 'recording' : 'idle',
        config: {
          ...voiceSnapshot().config,
          channels: Array.from({ length: 1 }, (_, index) => ({
            kind: 'model' as const,
            id: `speech-${index}`,
          })),
        },
      }),
    });
    await mount();
    const panel = renderer.root.findByProps({
      className: 'voice-input-panel flex h-full flex-col overflow-hidden',
    });
    expect(panel.props['data-saving']).toBe(false);
    expect(
      renderer.root.findByProps({ 'aria-label': 'voice.accountInput: WorkBuddy' }).props.disabled
    ).toBe(true);
    expect(voiceApi.saveVoiceConfig).not.toHaveBeenCalled();
  }
);

it('uses saved account access when native clients are uninstalled', async () => {
  tools.setState({ detectedTools: [] });
  useAccountAccessRiskStore.setState({ acknowledged: true, hydrated: true });
  await mount();
  const row = renderer.root.findByProps({ 'aria-label': 'voice.accountInput: WorkBuddy AI' });
  expect(row.props.disabled).toBe(false);
  act(() => row.props.onClick());
  expect(context.setNewModelForm).toHaveBeenCalledWith(
    expect.objectContaining({ accountSource: { provider: 'workBuddyAi', accountId: '' } })
  );
  expect(account.add).not.toHaveBeenCalled();
  expect(account.refresh).not.toHaveBeenCalled();
  expect(voiceApi.saveVoiceConfig).not.toHaveBeenCalled();
});

it('follows the actual active voice source through failover, completion and disabling', async () => {
  const config = {
    ...voiceSnapshot().config,
    shortcutEnabled: true,
    channels: [
      { kind: 'model' as const, id: 'speech-one' },
      { kind: 'model' as const, id: 'speech-two' },
    ],
  };
  voice.setState({ snapshot: voiceSnapshot({ config, phase: 'transcribing', activeChannel: 0 }) });
  await act(async () => {
    renderer = create(<VoiceInputMain />);
  });
  const highlighted = () =>
    renderer.root
      .findAllByProps({ role: 'listitem' })
      .flatMap((card, index) => (card.props.className.includes('is-active') ? [index] : []));
  expect(highlighted()).toEqual([0]);
  await act(async () => {
    voice.setState({
      snapshot: voiceSnapshot({ config, phase: 'transcribing', activeChannel: 1 }),
    });
  });
  expect(highlighted()).toEqual([1]);
  await act(async () => {
    voice.setState({ snapshot: voiceSnapshot({ config }) });
  });
  expect(highlighted()).toEqual([]);
  await act(async () => {
    voice.setState({
      snapshot: voiceSnapshot({ config: { ...config, shortcutEnabled: false }, activeChannel: 1 }),
    });
  });
  expect(highlighted()).toEqual([]);
  expect(voiceApi.saveVoiceConfig).not.toHaveBeenCalled();
  expect(voiceApi.recordVoice).not.toHaveBeenCalled();
});

describe('voice source card ordering', () => {
  const channels = [
    { kind: 'workbuddy' as const, id: 'a'.repeat(64) },
    { kind: 'model' as const, id: 'speech-one' },
    { kind: 'model' as const, id: 'speech-two' },
  ];
  const key = (index: number) => `${channels[index].kind}:${channels[index].id}`;
  const drag = () => renderer.root.findByType(DndContext);
  const order = () => renderer.root.findByType(SortableContext).props.items;
  const event = (from: string, to: string | null) => ({
    active: { id: from },
    over: to ? { id: to } : null,
  });
  const mountSources = (count = 3) => {
    voice.setState({
      snapshot: voiceSnapshot({
        config: { ...voiceSnapshot().config, channels: channels.slice(0, count) },
      }),
    });
    act(() => {
      renderer = create(<VoiceInputMain />);
    });
  };

  it('keeps drag ordering and removal while omitting the old upward shortcut', () => {
    mountSources();
    expect(renderer.root.findAllByProps({ 'aria-label': 'voice.makePrimary' })).toHaveLength(0);
    expect(renderer.root.findAllByProps({ 'aria-label': 'voice.removeChannel' })).toHaveLength(3);
    expect(order()).toEqual(channels.map((channel) => `${channel.kind}:${channel.id}`));
    expect(voiceApi.saveVoiceConfig).not.toHaveBeenCalled();
  });

  it('keeps empty cards blank except for their numbers and disables empty-slot dragging', () => {
    mountSources(0);
    const cards = renderer.root.findAllByProps({ role: 'listitem' });
    expect(cards).toHaveLength(3);
    cards.forEach((card, index) => {
      expect(card.findAllByType('span').flatMap((span) => span.children)).toEqual([
        String(index + 1),
      ]);
      expect(card.findAllByType('svg')).toHaveLength(0);
      expect(card.findAllByType('button')).toHaveLength(0);
    });
    expect(renderer.root.findAllByType(SortableCard).every((card) => card.props.disabled)).toBe(
      true
    );
    expect(voiceApi.saveVoiceConfig).not.toHaveBeenCalled();
  });

  it('reuses the smart-access pointer threshold, keyboard sensor, vertical motion and overlay', () => {
    mountSources();
    expect(drag().props.sensors).toEqual([
      expect.objectContaining({
        sensor: PointerSensor,
        options: { activationConstraint: { distance: 5 } },
      }),
      expect.objectContaining({ sensor: KeyboardSensor }),
    ]);
    const transform = { x: 22, y: 90, scaleX: 1, scaleY: 1 };
    expect(drag().props.modifiers[0]({ transform })).toEqual({ ...transform, x: 0 });
    expect(drag().props.autoScroll).toBe(false);
    act(() => drag().props.onDragStart({ active: { id: key(1) } }));
    expect(renderer.root.findByType(DragOverlay).props.dropAnimation).toBeNull();
    expect(
      renderer.root
        .findAllByProps({ 'aria-hidden': true })
        .some((node) => String(node.props.className).includes('is-dragging'))
    ).toBe(true);
    expect(voiceApi.saveVoiceConfig).not.toHaveBeenCalled();
    act(() => drag().props.onDragCancel());
    expect(order()).toEqual([key(0), key(1), key(2)]);
    expect(voiceApi.saveVoiceConfig).not.toHaveBeenCalled();
  });

  it.each([
    [0, 2, [1, 2, 0]],
    [2, 0, [2, 0, 1]],
  ] as const)('persists moving source %s to %s once', async (from, to, expected) => {
    mountSources();
    let finish!: (snapshot: ReturnType<typeof voiceSnapshot>) => void;
    vi.mocked(voiceApi.saveVoiceConfig).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        })
    );
    act(() => {
      void drag().props.onDragEnd(event(key(from), key(to)));
    });
    const next = expected.map((index) => channels[index]);
    expect(order()).toEqual(expected.map(key));
    expect(voice.getState().saving).toBe(true);
    const config = { ...voiceSnapshot().config, channels: next };
    expect(voiceApi.saveVoiceConfig).toHaveBeenCalledExactlyOnceWith(config);
    await act(async () => finish(voiceSnapshot({ config, revision: 2 })));
    expect(voice.getState().snapshot!.config.channels).toEqual(next);
    expect(order()).toEqual(expected.map(key));
    expect(voiceApi.startVoice).not.toHaveBeenCalled();
    expect(voiceApi.recordVoice).not.toHaveBeenCalled();
  });

  it('restores the previous order and shows a failed-save error', async () => {
    mountSources();
    vi.mocked(voiceApi.saveVoiceConfig).mockRejectedValueOnce(new Error('order save failed'));
    await act(async () => drag().props.onDragEnd(event(key(2), key(0))));
    expect(order()).toEqual([key(0), key(1), key(2)]);
    expect(voice.getState().snapshot!.config.channels).toEqual(channels);
    expect(renderer.root.findByProps({ role: 'alert' }).children).toContain(
      'Error: order save failed'
    );
  });

  it('ignores unchanged, outside, empty and stale drops, and blocks busy or hidden pages', async () => {
    mountSources(2);
    for (const drop of [
      event(key(0), key(0)),
      event(key(0), null),
      event(key(0), 'empty:2'),
      event('missing', key(0)),
    ]) {
      await act(async () => drag().props.onDragEnd(drop));
    }
    for (const changes of [
      { saving: true },
      { saving: false, pending: true },
      {
        pending: false,
        snapshot: voiceSnapshot({
          phase: 'recording',
          config: { ...voiceSnapshot().config, channels },
        }),
      },
    ]) {
      act(() => voice.setState(changes));
      expect(renderer.root.findAllByType(SortableCard).every((card) => card.props.disabled)).toBe(
        true
      );
      await act(async () => drag().props.onDragEnd(event(key(0), key(1))));
    }
    act(() => {
      voice.setState({
        snapshot: voiceSnapshot({ config: { ...voiceSnapshot().config, channels } }),
      });
      navigation.setState({ activePage: 'models' });
    });
    await act(async () => drag().props.onDragEnd(event(key(0), key(1))));
    expect(voiceApi.saveVoiceConfig).not.toHaveBeenCalled();
    expect(voiceApi.startVoice).not.toHaveBeenCalled();
  });
});
