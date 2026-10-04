import { useLayoutEffect } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import * as api from '../../api/tauri';
import { useToolsStore } from '../../stores/toolsStore';
import { useNavigationStore } from '../../stores/navigationStore';
import { AppManagerProvider } from './AppManagerProvider';
import { AppManagerContext, useAppManager } from './context';
import { AppManagerBottom } from './AppManagerComponents';
import { open as openDialog } from '@tauri-apps/plugin-dialog';

vi.hoisted(() => vi.stubGlobal('__APP_EDITION__', 'full'));
vi.mock('../../utils/platform', () => ({ IS_WINDOWS: false }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn().mockResolvedValue('/fixture') }));
vi.mock('../../hooks/useI18n', () => {
  const t = (key: string) => key;
  return { useI18n: () => ({ t, locale: 'en' }) };
});
vi.mock('../../components/ConfirmDialog', () => ({ useConfirm: () => async () => true }));
vi.mock('../../components', () => ({ EFFORT_PULSE_ONESHOT_MS: 0 }));
vi.mock('../ModelNexus/context', () => {
  const userModels: never[] = [];
  return { useModelNexus: () => ({ userModels }) };
});
vi.mock('../FreeModels', () => ({ useFreeModels: () => ({ routerEnabled: false }) }));
vi.mock('./useClaudeCodeAccounts', () => ({ useClaudeCodeAccounts: () => ({}) }));
vi.mock('./useWorkBuddyAccounts', () => ({ useWorkBuddyAccounts: () => ({}) }));
vi.mock('./useDeepSeekAccounts', () => ({ useDeepSeekAccounts: () => ({}) }));
vi.mock('../../api/tauri', () => ({
  listZCodeAccounts: vi.fn(),
  startZCodeLogin: vi.fn(),
  pollZCodeLogin: vi.fn(),
  cancelZCodeLogin: vi.fn(),
  switchZCodeAccount: vi.fn(),
  refreshZCodeAccountQuota: vi.fn(),
  deleteZCodeAccount: vi.fn(),

  applyModelToTool: vi.fn(),
  startTool: vi.fn().mockResolvedValue({ success: true }),
  listCodexAccounts: vi.fn(),
  cancelCodexLogin: vi.fn(),
  getModels: vi.fn().mockResolvedValue([]),
  getInstallIndex: vi.fn().mockResolvedValue('{"ids":[]}'),
  listGrokBotAccounts: vi.fn().mockRejectedValue(new Error('accountError.read')),
  listCursorAccounts: vi.fn().mockRejectedValue(new Error('accountError.read')),
  listAntigravityAccounts: vi.fn().mockRejectedValue(new Error('accountError.read')),
  startAntigravityLogin: vi.fn(),
  pollAntigravityLogin: vi.fn(),
  cancelAntigravityLogin: vi.fn().mockResolvedValue(undefined),
  deleteAntigravityAccount: vi.fn(),
  switchAntigravityAccount: vi.fn(),
  refreshAntigravityAccount: vi.fn(),
  openExternal: vi.fn().mockResolvedValue(undefined),
  startGrokBotLogin: vi.fn(),
  pollGrokBotLogin: vi.fn(),
  cancelGrokBotLogin: vi.fn(),
  deleteGrokBotAccount: vi.fn(),
  startCursorLogin: vi.fn(),
  pollCursorLogin: vi.fn(),
  cancelCursorLogin: vi.fn(),
  deleteCursorAccount: vi.fn(),
  refreshGrokBotAccount: vi.fn(),
  refreshCursorAccount: vi.fn(),
  listGrokAccounts: vi.fn().mockRejectedValue(new Error('accountError.read')),
  startGrokLogin: vi.fn(),
  pollGrokLogin: vi.fn(),
  cancelGrokLogin: vi.fn(),
  deleteGrokAccount: vi.fn(),
  switchGrokAccount: vi.fn(),
  refreshGrokAccount: vi.fn(),
  listManusAccounts: vi.fn().mockRejectedValue(new Error('accountError.read')),
  startManusLogin: vi.fn(),
  pollManusLogin: vi.fn(),
  cancelManusLogin: vi.fn(),
  deleteManusAccount: vi.fn(),
  switchManusAccount: vi.fn(),
  refreshManusAccount: vi.fn(),
}));

describe.each(['cline', 'clinedesktop'])('%s API model workflow', (tool) => {
  let renderer: ReactTestRenderer;
  let context: ReturnType<typeof useAppManager>;
  function Harness() {
    const state = useAppManager();
    useLayoutEffect(() => {
      context = state;
    });
    return null;
  }
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    useNavigationStore.getState().setActivePage('apps');
    useToolsStore.getState().setDetectedTools([
      {
        id: 'cline',
        name: 'Cline CLI',
        category: 'CLI Code',
        installed: true,
        activeModel: 'cli-before',
        apiProtocol: ['openai', 'anthropic'],
        startCommand: 'cline',
      },
      {
        id: 'clinedesktop',
        name: 'Cline Desktop',
        category: 'Desktop',
        installed: true,
        activeModel: 'desktop-before',
        apiProtocol: ['openai', 'anthropic'],
      },
    ]);
    vi.mocked(api.getModels).mockResolvedValue([
      {
        internalId: 'fixture',
        name: 'Fixture',
        modelId: 'fixture-model',
        baseUrl: 'http://127.0.0.1:1234/v1',
        apiKey: 'fixture',
      },
    ]);
  });
  afterEach(() => {
    act(() => renderer?.unmount());
    useToolsStore.getState().setDetectedTools([]);
    vi.mocked(api.getModels).mockResolvedValue([]);
    vi.useRealTimers();
  });
  const mount = async () => {
    await act(async () => {
      renderer = create(
        <AppManagerProvider>
          <Harness />
        </AppManagerProvider>
      );
    });
    act(() => {
      context.setSelectedTool(tool);
      context.setAgreedConfigPolicy(true);
      context.setLaunchAfterApply(true);
    });
  };
  it('keeps tool, tab and page navigation passive, including uninstalled clients', async () => {
    await mount();
    act(() => context.handleSelectModel(tool, 'fixture'));
    act(() => context.setSelectedTool(tool === 'cline' ? 'clinedesktop' : 'cline'));
    act(() => context.setViewMode('install'));
    act(() => useNavigationStore.getState().setActivePage('myProjects'));
    act(() =>
      useToolsStore
        .getState()
        .setDetectedTools([{ id: tool, name: tool, category: 'Desktop', installed: false }])
    );
    act(() => {
      useNavigationStore.getState().setActivePage('apps');
      context.setSelectedTool(tool);
    });
    await act(async () => {
      await vi.runOnlyPendingTimersAsync();
    });
    expect(api.applyModelToTool).not.toHaveBeenCalled();
    expect(api.startTool).not.toHaveBeenCalled();
    expect(openDialog).not.toHaveBeenCalled();
    expect(api.listCodexAccounts).not.toHaveBeenCalled();
    expect(api.listCursorAccounts).not.toHaveBeenCalled();
    expect(api.startGrokLogin).not.toHaveBeenCalled();
    expect(api.refreshGrokAccount).not.toHaveBeenCalled();
    expect(context.applyError).toBeNull();
  });
  it('waits for explicit apply, updates both shared selectors and honors launch unchecked', async () => {
    await mount();
    act(() => {
      context.handleSelectModel(tool, 'fixture');
      context.setLaunchAfterApply(false);
    });
    expect(api.applyModelToTool).not.toHaveBeenCalled();
    let resolve!: (value: { success: boolean; message: string }) => void;
    vi.mocked(api.applyModelToTool).mockImplementationOnce(
      () =>
        new Promise((r) => {
          resolve = r;
        })
    );
    let pending!: Promise<void>;
    act(() => {
      pending = context.handleLaunch();
    });
    expect(context.isLaunching).toBe(true);
    expect(api.startTool).not.toHaveBeenCalled();
    await act(async () => {
      resolve({ success: true, message: 'ok' });
      await pending;
    });
    expect(api.applyModelToTool).toHaveBeenCalledWith(
      tool,
      expect.objectContaining({ model: 'fixture-model', protocol: 'openai' })
    );
    expect(context.detectedTools.find((t) => t.id === tool)?.activeModel).toBe('fixture-model');
    const other = tool === 'cline' ? 'clinedesktop' : 'cline';
    expect(context.detectedTools.find((t) => t.id === other)?.activeModel).toBe('fixture-model');
    expect(context.toolModelConfig[other]).toBe('fixture');
    expect(api.startTool).not.toHaveBeenCalled();
    expect(openDialog).not.toHaveBeenCalled();
  });
  it('does not launch or change the applied model when configuration fails', async () => {
    await mount();
    act(() => context.handleSelectModel(tool, 'fixture'));
    vi.mocked(api.applyModelToTool).mockResolvedValueOnce({
      success: false,
      message: 'Native storage unavailable',
    });
    await act(async () => {
      await context.handleLaunch();
    });
    expect(context.applyError).toBe('Native storage unavailable');
    expect(context.isLaunching).toBe(false);
    expect(api.startTool).not.toHaveBeenCalled();
    expect(openDialog).not.toHaveBeenCalled();
    expect(context.detectedTools.find((t) => t.id === tool)?.activeModel).toBe(
      tool === 'cline' ? 'cli-before' : 'desktop-before'
    );
  });
  it('uses the shared launch flow and asks for a folder only for CLI', async () => {
    await mount();
    act(() => context.handleSelectModel(tool, 'fixture'));
    vi.mocked(api.applyModelToTool).mockResolvedValueOnce({ success: true, message: 'ok' });
    await act(async () => {
      await context.handleLaunch();
    });
    expect(api.startTool).toHaveBeenCalledWith(
      tool,
      tool === 'cline' ? 'cline' : undefined,
      tool === 'cline' ? '/fixture' : undefined
    );
    expect(openDialog).toHaveBeenCalledTimes(tool === 'cline' ? 1 : 0);
  });
});

const accountTools = [
  'grokbot',
  'cursor',
  'grok',
  'manus',
  'antigravity',
  'antigravitydesktop',
] as const;
describe.each(accountTools)('%s navigation account loading', (tool) => {
  let renderer: ReactTestRenderer;
  let context: ReturnType<typeof useAppManager>;
  const list = {
    grokbot: api.listGrokBotAccounts,
    cursor: api.listCursorAccounts,
    grok: api.listGrokAccounts,
    manus: api.listManusAccounts,
    antigravity: api.listAntigravityAccounts,
    antigravitydesktop: api.listAntigravityAccounts,
  }[tool];
  const setInstalled = (installed: boolean) =>
    useToolsStore
      .getState()
      .setDetectedTools([{ id: tool, name: tool, category: 'Desktop', installed }]);
  function Harness() {
    const state = useAppManager();
    useLayoutEffect(() => {
      context = state;
    });
    return null;
  }
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    useNavigationStore.getState().setActivePage('apps');
    setInstalled(false);
  });
  afterEach(() => {
    act(() => renderer?.unmount());
    useToolsStore.getState().setDetectedTools([]);
    vi.useRealTimers();
  });

  it('skips uninstalled clients and never turns navigation read failures into dialogs or quota refreshes', async () => {
    await act(async () => {
      renderer = create(
        <AppManagerProvider>
          <Harness />
        </AppManagerProvider>
      );
    });
    act(() => {
      context.setViewMode('install');
      context.setSelectedTool(tool);
    });
    await act(async () => {
      await vi.runOnlyPendingTimersAsync();
    });
    expect(list).not.toHaveBeenCalled();
    expect(context.applyError).toBeNull();

    act(() => {
      setInstalled(true);
      context.setViewMode('desktop');
      context.setSelectedTool(tool);
    });
    await act(async () => {
      await vi.runOnlyPendingTimersAsync();
    });
    expect(list).toHaveBeenCalledTimes(1);
    expect(context.applyError).toBeNull();

    act(() => context.setViewMode('install'));
    await act(async () => {
      await vi.runOnlyPendingTimersAsync();
    });
    expect(list).toHaveBeenCalledTimes(1);
    expect(api.refreshGrokBotAccount).not.toHaveBeenCalled();
    expect(api.refreshCursorAccount).not.toHaveBeenCalled();
    expect(api.refreshGrokAccount).not.toHaveBeenCalled();
    expect(api.startGrokLogin).not.toHaveBeenCalled();
    expect(api.switchGrokAccount).not.toHaveBeenCalled();
    expect(api.refreshManusAccount).not.toHaveBeenCalled();
    expect(api.startManusLogin).not.toHaveBeenCalled();
    expect(api.switchManusAccount).not.toHaveBeenCalled();
    expect(api.refreshAntigravityAccount).not.toHaveBeenCalled();
    expect(api.startAntigravityLogin).not.toHaveBeenCalled();
    expect(api.switchAntigravityAccount).not.toHaveBeenCalled();
  });
});

describe.each(['antigravity', 'antigravitydesktop'] as const)('%s account apply', (tool) => {
  let renderer: ReactTestRenderer;
  let context: ReturnType<typeof useAppManager>;
  function Harness() {
    const state = useAppManager();
    useLayoutEffect(() => {
      context = state;
    });
    return null;
  }
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.listAntigravityAccounts).mockResolvedValue([
      { id: 'saved', email: 'one@example.test', active: true, plan: 'Pro', quotas: [] },
    ]);
    vi.mocked(api.switchAntigravityAccount).mockResolvedValue({
      id: 'saved',
      email: 'one@example.test',
      active: true,
      plan: 'Pro',
      quotas: [],
    });
    useNavigationStore.getState().setActivePage('apps');
    useToolsStore.getState().setDetectedTools([
      {
        id: tool,
        name: tool,
        category: tool === 'antigravity' ? 'CLI Code' : 'Desktop',
        installed: true,
        noModelConfig: true,
        startCommand: tool === 'antigravity' ? 'agy' : undefined,
      },
    ]);
  });
  afterEach(() => {
    act(() => renderer?.unmount());
    useToolsStore.getState().setDetectedTools([]);
  });
  it('loads passively and applies only on explicit launch', async () => {
    await act(async () => {
      renderer = create(
        <AppManagerProvider>
          <Harness />
        </AppManagerProvider>
      );
    });
    act(() => context.setSelectedTool(tool));
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    expect(context.antigravityAccounts.selectedId).toBe('saved');
    expect(api.switchAntigravityAccount).not.toHaveBeenCalled();
    expect(api.refreshAntigravityAccount).not.toHaveBeenCalled();
    vi.mocked(api.switchAntigravityAccount).mockRejectedValueOnce(new Error('accountError.write'));
    await act(async () => {
      await context.handleLaunch();
    });
    expect(api.startTool).not.toHaveBeenCalled();
    expect(context.applyError).toBe('accountError.write');
    await act(async () => {
      await context.handleLaunch();
    });
    expect(api.switchAntigravityAccount).toHaveBeenCalledWith('saved');
    expect(api.startTool).toHaveBeenCalledWith(
      tool,
      tool === 'antigravity' ? 'agy' : undefined,
      tool === 'antigravity' ? '/fixture' : undefined
    );
  });

  it('opens browser authorization without launching the client or disabling Launch', async () => {
    vi.mocked(api.startAntigravityLogin).mockResolvedValueOnce({
      loginId: 'browser-login',
      verificationUri: 'https://accounts.google.com/o/oauth2/v2/auth?state=fixture',
      expiresAt: Date.now() / 1000 + 60,
    });
    vi.mocked(api.pollAntigravityLogin).mockImplementation(() => new Promise(() => {}));
    await act(async () => {
      renderer = create(
        <AppManagerProvider>
          <Harness />
        </AppManagerProvider>
      );
    });
    act(() => context.setSelectedTool(tool));
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    act(() => {
      void context.antigravityAccounts.add();
    });
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    expect(api.openExternal).toHaveBeenCalledWith(
      'https://accounts.google.com/o/oauth2/v2/auth?state=fixture'
    );
    expect(api.startTool).not.toHaveBeenCalled();
    expect(api.switchAntigravityAccount).not.toHaveBeenCalled();
    expect(context.antigravityAccounts.busy).toBe(true);
    let bottom!: ReactTestRenderer;
    act(() => {
      bottom = create(
        <AppManagerContext.Provider value={context}>
          <AppManagerBottom />
        </AppManagerContext.Provider>
      );
    });
    const launch = bottom.root
      .findAllByType('button')
      .find((button) => button.children.includes('btn.launchApp'));
    expect(launch?.props.disabled).toBe(false);
    act(() => bottom.unmount());
  });
});
