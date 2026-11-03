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
import museCodeDefinition from '../../../tools/musecode/paths.json';
import * as devinApi from '../../api/devin';
import * as pluginApi from '../../api/plugins';
import { usePluginsStore } from '../../stores/pluginsStore';
import type { InstalledPlugin } from '../../api/plugins';
import cursorCliDefinition from '../../../tools/cursorcli/paths.json';
import { enableAccountPlugins } from '../../test/accountPlugins';

vi.mock('../../api/plugins', () => ({
  listPlugins: vi.fn().mockResolvedValue([]),
  launchPlugin: vi.fn().mockResolvedValue(undefined),
}));

vi.mock('../../api/devin', () => ({
  listDevinAccounts: vi.fn().mockResolvedValue([]),
  startDevinLogin: vi.fn(),
  pollDevinLogin: vi.fn(),
  cancelDevinLogin: vi.fn().mockResolvedValue(undefined),
  deleteDevinAccount: vi.fn(),
  refreshDevinAccount: vi.fn(),
}));

describe('Cursor CLI account integration', () => {
  let renderer: ReactTestRenderer;
  let context: ReturnType<typeof useAppManager>;
  const account = {
    id: 'cursor-cli-fixture',
    email: 'fixture@example.test',
    active: true,
    usage: null,
  };
  const tools = (installed: boolean) => [
    { ...cursorCliDefinition, id: 'cursorcli', installed },
    { id: 'cursor', name: 'Cursor', category: 'Desktop', installed: true, noModelConfig: true },
  ];
  function Harness() {
    const state = useAppManager();
    useLayoutEffect(() => {
      context = state;
    });
    return null;
  }
  async function mount(installed = true) {
    useToolsStore.getState().setDetectedTools(tools(installed));
    await act(async () => {
      renderer = create(
        <AppManagerProvider>
          <Harness />
        </AppManagerProvider>
      );
    });
    act(() => {
      context.setViewMode('desktop');
      context.setSelectedTool('cursorcli');
    });
    await act(async () => {
      await vi.runOnlyPendingTimersAsync();
    });
  }
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    useNavigationStore.getState().setActivePage('apps');
    vi.mocked(api.listCursorAccounts).mockResolvedValue([account]);
    vi.mocked(api.switchCursorAccount).mockResolvedValue(account);
    vi.mocked(openDialog).mockResolvedValue('/fixture');
  });
  afterEach(() => {
    act(() => renderer?.unmount());
    useToolsStore.getState().setDetectedTools([]);
    vi.mocked(api.listCursorAccounts).mockRejectedValue(new Error('accountError.read'));
    vi.useRealTimers();
  });
  it('loads only installed CLI accounts without login, quota or configuration changes', async () => {
    await mount(false);
    expect(api.listCursorAccounts).not.toHaveBeenCalled();
    act(() => useToolsStore.getState().setDetectedTools(tools(true)));
    await act(async () => {
      await vi.runOnlyPendingTimersAsync();
    });
    expect(api.listCursorAccounts).toHaveBeenCalledWith('cursorcli');
    expect(api.startCursorLogin).not.toHaveBeenCalled();
    expect(api.refreshCursorAccount).not.toHaveBeenCalled();
    expect(api.switchCursorAccount).not.toHaveBeenCalled();
    expect(api.applyModelToTool).not.toHaveBeenCalled();
    expect(api.startTool).not.toHaveBeenCalled();
  });
  it('applies the shared account only to CLI and launches its verified command in the chosen folder', async () => {
    await mount();
    expect(context.cursorAccounts.selectedId).toBe(account.id);
    await act(async () => {
      await context.handleLaunch();
    });
    expect(api.switchCursorAccount).toHaveBeenCalledExactlyOnceWith(account.id, 'cursorcli');
    expect(api.startTool).toHaveBeenCalledExactlyOnceWith('cursorcli', 'cursor-agent', '/fixture');
    expect(api.applyModelToTool).not.toHaveBeenCalled();
    expect(api.restoreToolToOfficial).not.toHaveBeenCalled();
  });
  it('does not write a login or launch when the folder dialog is cancelled', async () => {
    await mount();
    vi.mocked(openDialog).mockResolvedValueOnce(null);
    await act(async () => {
      await context.handleLaunch();
    });
    expect(api.switchCursorAccount).not.toHaveBeenCalled();
    expect(api.startTool).not.toHaveBeenCalled();
    expect(context.isLaunching).toBe(false);
  });
  it('ignores a late CLI apply result after switching to the editor and reports explicit failures', async () => {
    await mount();
    vi.mocked(api.switchCursorAccount).mockRejectedValueOnce(new Error('accountError.write'));
    await act(async () => {
      await context.handleLaunch();
    });
    expect(context.applyError).toBe('accountError.write');
    expect(api.startTool).not.toHaveBeenCalled();
    let finish!: (value: api.CursorAccount) => void;
    vi.mocked(api.switchCursorAccount).mockReturnValueOnce(
      new Promise((resolve) => {
        finish = resolve;
      })
    );
    let operation!: Promise<void>;
    await act(async () => {
      operation = context.handleLaunch();
    });
    act(() => context.setSelectedTool('cursor'));
    await act(async () => {
      finish(account);
      await operation;
    });
    expect(api.startTool).not.toHaveBeenCalled();
  });
});

describe('installed plugin desktop lifecycle', () => {
  let renderer: ReactTestRenderer;
  let context: ReturnType<typeof useAppManager>;
  const plugin: InstalledPlugin = {
    id: 'fixture.game',
    enabled: true,
    directory: 'packages/fixture.game/one',
    icon: null,
    source: '/fixture',
    revision: null,
    manifest: {
      name: 'game',
      publisher: 'fixture',
      version: '1.0.0',
      displayName: 'Game',
      description: 'Fixture',
      license: 'MIT',
      repository: 'https://example.com/plugin',
      echobird: {
        manifestVersion: 1,
        apiVersion: '0.1-draft',
        platforms: ['windows', 'macos', 'linux'],
        permissions: ['model.request'],
        contributes: { pages: [{ id: 'game', title: 'Game', entry: 'ui/index.html' }] },
      },
    },
  };
  const id = 'plugin:fixture.game:game';
  function Harness() {
    const current = useAppManager();
    useLayoutEffect(() => {
      context = current;
    });
    return null;
  }
  beforeEach(async () => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    vi.stubGlobal('localStorage', { getItem: () => null, setItem: vi.fn() });
    useNavigationStore.getState().setDeveloperMode(true);
    useNavigationStore.getState().setActivePage('apps');
    usePluginsStore.setState(usePluginsStore.getInitialState(), true);
    vi.mocked(pluginApi.listPlugins).mockResolvedValue([plugin]);
    vi.mocked(api.getModels).mockResolvedValue([
      {
        internalId: 'chosen-model',
        name: 'Chosen model',
        modelId: 'fixture',
        baseUrl: 'http://127.0.0.1:1/v1',
        apiKey: 'fixture-key',
      },
    ]);
    useToolsStore.getState().setDetectedTools([
      { id: 'reversi', name: 'Demo', category: 'Game', installed: true },
      { id: 'translator', name: 'Translator', category: 'Utility', installed: true },
    ]);
    await act(async () => {
      renderer = create(
        <AppManagerProvider>
          <Harness />
        </AppManagerProvider>
      );
    });
  });
  afterEach(() => {
    act(() => renderer.unmount());
    usePluginsStore.setState(usePluginsStore.getInitialState(), true);
    useNavigationStore.getState().setDeveloperMode(false);
    vi.mocked(pluginApi.listPlugins).mockResolvedValue([]);
    vi.clearAllTimers();
    vi.useRealTimers();
  });

  it('reuses model selection and launch while leaving native configuration and account operations untouched', async () => {
    expect(context.detectedTools.some((tool) => tool.id === id)).toBe(true);
    expect(useToolsStore.getState().detectedTools.some((tool) => tool.id === 'reversi')).toBe(true);
    act(() => {
      context.setSelectedTool(id);
      context.handleSelectModel(id, 'chosen-model');
    });
    expect(pluginApi.launchPlugin).not.toHaveBeenCalled();
    expect(api.applyModelToTool).not.toHaveBeenCalled();
    expect(api.startTool).not.toHaveBeenCalled();
    await act(async () => context.handleLaunch());
    expect(pluginApi.launchPlugin).toHaveBeenCalledExactlyOnceWith(
      'fixture.game',
      'game',
      expect.objectContaining({ internalId: 'chosen-model' })
    );
    expect(api.applyModelToTool).not.toHaveBeenCalled();
    expect(api.startTool).not.toHaveBeenCalled();
    expect(openDialog).not.toHaveBeenCalled();
  });

  it('removes disabled plugins from desktop selection and preserves the built-in demo tool', () => {
    act(() => context.setSelectedTool(id));
    act(() => usePluginsStore.setState({ installed: [{ ...plugin, enabled: false }] }));
    expect(context.detectedTools.some((tool) => tool.id === id)).toBe(false);
    expect(context.selectedTool).toBeNull();
    expect(useToolsStore.getState().detectedTools.some((tool) => tool.id === 'reversi')).toBe(true);
  });

  it.each(['reversi', 'translator'])(
    'keeps the %s project demo while hiding its bundled desktop selection',
    (tool) => {
      act(() => context.setSelectedTool(tool));
      expect(context.selectedTool).toBeNull();
      expect(useToolsStore.getState().detectedTools.some((entry) => entry.id === tool)).toBe(true);
      expect(pluginApi.launchPlugin).not.toHaveBeenCalled();
      expect(api.startTool).not.toHaveBeenCalled();
    }
  );

  it('clears a pending developer launch on pause and ignores its late error after reenabling', async () => {
    act(() => {
      context.setSelectedTool(id);
      context.handleSelectModel(id, 'chosen-model');
    });
    let fail!: (error: Error) => void;
    vi.mocked(pluginApi.launchPlugin).mockImplementationOnce(
      () =>
        new Promise((_, reject) => {
          fail = reject;
        })
    );
    let pending!: Promise<void>;
    await act(async () => {
      pending = context.handleLaunch();
    });
    expect(context.isLaunching).toBe(true);
    await act(async () => useNavigationStore.getState().setDeveloperMode(false));
    expect(context.isLaunching).toBe(false);
    expect(context.detectedTools.some((tool) => tool.id === id)).toBe(false);
    await act(async () => useNavigationStore.getState().setDeveloperMode(true));
    await act(async () => {
      fail(new Error('old launch failed'));
      await pending;
    });
    expect(context.applyError).toBeNull();
    expect(context.isLaunching).toBe(false);
  });
});

vi.hoisted(() => vi.stubGlobal('__APP_EDITION__', 'full'));
vi.mock('../../utils/platform', () => ({ IS_WINDOWS: false }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn().mockResolvedValue('/fixture') }));
vi.mock('../../hooks/useI18n', () => {
  const t = (key: string) =>
    key === 'accountError.format' ? 'Localized configuration format error' : key;
  return { useI18n: () => ({ t, locale: 'en' }) };
});
vi.mock('../../components/ConfirmDialog', () => ({ useConfirm: () => async () => true }));
vi.mock('../../components', () => ({ getModelIcon: () => null }));
vi.mock('../ModelNexus/context', () => {
  const userModels: never[] = [];
  return { useModelNexus: () => ({ userModels }) };
});
const routerState = vi.hoisted(() => ({ routerEnabled: false, routers: [] as { id: string }[] }));
vi.mock('../FreeModels/provider', () => ({ useFreeModels: () => routerState }));
afterEach(() => {
  routerState.routerEnabled = false;
  routerState.routers = [];
});
vi.mock('./useClaudeCodeAccounts', async () => {
  const { useState } = await import('react');
  return {
    useClaudeCodeAccounts: () => {
      const [selectedId, setSelectedId] = useState<string | null>(null);
      return { accounts: [], selectedId, setSelectedId };
    },
  };
});
vi.mock('./useWorkBuddyAccounts', async () => {
  const { useState } = await import('react');
  return {
    useWorkBuddyAccounts: () => {
      const [selectedId, select] = useState<string | null>(null);
      return { accounts: [], selectedId, select };
    },
  };
});
vi.mock('./useDeepSeekAccounts', async () => {
  const { useState } = await import('react');
  return {
    useDeepSeekAccounts: (_enabled: boolean, _hasModel: boolean, clearModel: () => void) => {
      const [selectedId, setSelectedId] = useState<string | null>(null);
      return {
        accounts: [],
        selectedId,
        select: (id: string | null) => {
          setSelectedId(id);
          if (id) clearModel();
        },
      };
    },
  };
});
vi.mock('../../api/tauri', () => ({
  getCliLaunchSettings: vi.fn().mockResolvedValue({}),
  listQoderAccounts: vi.fn().mockResolvedValue([]),
  startQoderLogin: vi.fn(),
  refreshQoderAccountQuota: vi.fn(),
  listMinimaxAccounts: vi
    .fn()
    .mockResolvedValue([{ id: 'fixture-account', email: 'fixture@example.test', region: 'cn' }]),
  startMinimaxLogin: vi.fn(),
  refreshMinimaxAccount: vi.fn(),
  switchDevinAccount: vi.fn(),
  listOpenDesignAccounts: vi.fn().mockResolvedValue([]),
  startOpenDesignLogin: vi.fn(),
  pollOpenDesignLogin: vi.fn(),
  cancelOpenDesignLogin: vi.fn(),
  switchOpenDesignAccount: vi.fn(),
  refreshOpenDesignAccount: vi.fn(),
  deleteOpenDesignAccount: vi.fn(),
  listClineAccounts: vi.fn().mockResolvedValue([]),
  startClineLogin: vi.fn(),
  pollClineLogin: vi.fn(),
  cancelClineLogin: vi.fn().mockResolvedValue(undefined),
  switchClineAccount: vi.fn(),
  refreshClineAccount: vi.fn(),
  deleteClineAccount: vi.fn(),
  listKiloAccounts: vi.fn().mockResolvedValue([]),
  startKiloLogin: vi.fn(),
  pollKiloLogin: vi.fn(),
  cancelKiloLogin: vi.fn().mockResolvedValue(undefined),
  switchKiloAccount: vi.fn(),
  refreshKiloAccount: vi.fn(),
  deleteKiloAccount: vi.fn(),
  listCopilotAccounts: vi.fn().mockResolvedValue([]),
  startCopilotLogin: vi.fn(),
  pollCopilotLogin: vi.fn(),
  cancelCopilotLogin: vi.fn().mockResolvedValue(undefined),
  switchCopilotAccount: vi.fn(),
  refreshCopilotAccount: vi.fn(),
  deleteCopilotAccount: vi.fn(),
  listKiroAccounts: vi.fn().mockResolvedValue([]),
  startKiroLogin: vi.fn(),
  pollKiroLogin: vi.fn(),
  cancelKiroLogin: vi.fn().mockResolvedValue(undefined),
  switchKiroAccount: vi.fn(),
  refreshKiroAccountQuota: vi.fn(),
  deleteKiroAccount: vi.fn(),
  listTraeAccounts: vi.fn().mockResolvedValue([]),
  startTraeLogin: vi.fn(),
  pollTraeLogin: vi.fn(),
  cancelTraeLogin: vi.fn().mockResolvedValue(undefined),
  switchTraeAccount: vi.fn(),
  refreshTraeAccountQuota: vi.fn(),
  claimTraeDailyCredits: vi.fn(),
  deleteTraeAccount: vi.fn(),
  listZCodeAccounts: vi.fn(),
  startZCodeLogin: vi.fn(),
  pollZCodeLogin: vi.fn(),
  cancelZCodeLogin: vi.fn(),
  switchZCodeAccount: vi.fn(),
  refreshZCodeAccountQuota: vi.fn(),
  deleteZCodeAccount: vi.fn(),

  applyModelToTool: vi.fn(),
  restoreToolToOfficial: vi.fn(),
  startTool: vi.fn().mockResolvedValue({ success: true }),
  listCodexAccounts: vi.fn(),
  cancelCodexLogin: vi.fn(),
  startCodexLogin: vi.fn(),
  refreshCodexAccountQuota: vi.fn(),
  getModels: vi.fn().mockResolvedValue([]),
  getInstallIndex: vi.fn().mockResolvedValue('{"ids":[]}'),
  listGrokBotAccounts: vi.fn().mockRejectedValue(new Error('accountError.read')),
  listCursorAccounts: vi.fn().mockRejectedValue(new Error('accountError.read')),
  switchCursorAccount: vi.fn(),
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
  listCueAccounts: vi.fn().mockRejectedValue(new Error('accountError.read')),
  startCueLogin: vi.fn(),
  pollCueLogin: vi.fn(),
  cancelCueLogin: vi.fn(),
  switchCueAccount: vi.fn(),
  deleteCueAccount: vi.fn(),
  refreshCueAccount: vi.fn(),
  startGrokLogin: vi.fn(),
  pollGrokLogin: vi.fn(),
  cancelGrokLogin: vi.fn(),
  deleteGrokAccount: vi.fn(),
  switchGrokAccount: vi.fn(),
  refreshGrokAccount: vi.fn(),
  listClaudeDesktopAccounts: vi.fn().mockResolvedValue([]),
  startClaudeDesktopLogin: vi.fn(),
  pollClaudeDesktopLogin: vi.fn(),
  cancelClaudeDesktopLogin: vi.fn(),
  refreshClaudeDesktopAccount: vi.fn(),
  switchClaudeDesktopAccount: vi.fn(),
  deleteClaudeDesktopAccount: vi.fn(),
  listManusAccounts: vi.fn().mockRejectedValue(new Error('accountError.read')),
  startManusLogin: vi.fn(),
  pollManusLogin: vi.fn(),
  cancelManusLogin: vi.fn(),
  deleteManusAccount: vi.fn(),
  switchManusAccount: vi.fn(),
  refreshManusAccount: vi.fn(),
}));

describe('Devin application account apply', () => {
  let renderer: ReactTestRenderer;
  let context: ReturnType<typeof useAppManager>;
  const account: devinApi.DevinAccount = {
    id: 'devin-saved',
    email: 'user@example.test',
    active: true,
    plan: 'Free',
    quotaWindows: [],
  };
  function Harness() {
    const value = useAppManager();
    useLayoutEffect(() => {
      context = value;
    });
    return null;
  }
  beforeEach(() => {
    vi.clearAllMocks();
    const storage = new Map<string, string>();
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => storage.set(key, value),
    });
    vi.mocked(devinApi.listDevinAccounts).mockResolvedValue([account]);
    vi.mocked(api.switchDevinAccount).mockResolvedValue(account);
    useNavigationStore.getState().setActivePage('apps');
    useToolsStore.getState().setDetectedTools([
      { id: 'devin', name: 'Devin', category: 'Desktop', installed: true, noModelConfig: true },
      {
        id: 'devincli',
        name: 'Devin CLI',
        category: 'CLI Code',
        installed: true,
        noModelConfig: true,
        startCommand: 'devin',
      },
    ]);
  });
  afterEach(() => {
    act(() => renderer?.unmount());
    useToolsStore.getState().setDetectedTools([]);
    vi.mocked(devinApi.listDevinAccounts).mockResolvedValue([]);
  });
  async function mount(tool: 'devin' | 'devincli') {
    await act(async () => {
      renderer = create(
        <AppManagerProvider>
          <Harness />
        </AppManagerProvider>
      );
    });
    act(() => context.setSelectedTool(tool));
    await act(async () => {
      await new Promise((done) => setTimeout(done, 0));
    });
  }
  it.each(['devin', 'devincli'] as const)(
    '%s applies only on explicit action and follows the native account launch flow',
    async (tool) => {
      await mount(tool);
      expect(context.devinAccounts.selectedId).toBe(account.id);
      expect(api.switchDevinAccount).not.toHaveBeenCalled();
      expect(devinApi.refreshDevinAccount).not.toHaveBeenCalled();
      act(() => context.setLaunchAfterApply(false));
      await act(async () => {
        await context.handleLaunch();
      });
      expect(api.switchDevinAccount).toHaveBeenCalledExactlyOnceWith(account.id, tool);
      expect(api.startTool).not.toHaveBeenCalled();
      expect(openDialog).not.toHaveBeenCalled();
      act(() => context.setLaunchAfterApply(true));
      await act(async () => {
        await context.handleLaunch();
      });
      expect(api.startTool).toHaveBeenCalledWith(
        tool,
        tool === 'devincli' ? 'devin' : undefined,
        tool === 'devincli' ? '/fixture' : undefined
      );
      expect(context.isLaunching).toBe(false);
    }
  );
  it('stops launch on failed switching and ignores a late switch after changing applications', async () => {
    await mount('devin');
    vi.mocked(api.switchDevinAccount).mockRejectedValueOnce(new Error('accountError.write'));
    await act(async () => {
      await context.handleLaunch();
    });
    expect(context.applyError).toBe('accountError.write');
    expect(api.startTool).not.toHaveBeenCalled();
    let resolve!: (value: devinApi.DevinAccount) => void;
    vi.mocked(api.switchDevinAccount).mockReturnValueOnce(
      new Promise((done) => {
        resolve = done;
      })
    );
    let operation!: Promise<void>;
    await act(async () => {
      operation = context.handleLaunch();
    });
    act(() => context.setSelectedTool('devincli'));
    await act(async () => {
      resolve(account);
      await operation;
    });
    expect(api.startTool).not.toHaveBeenCalled();
  });
});

describe('Hermes native model apply', () => {
  let renderer: ReactTestRenderer;
  let context: ReturnType<typeof useAppManager>;
  function Harness() {
    context = useAppManager();
    return null;
  }
  beforeEach(async () => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    vi.stubGlobal('localStorage', { getItem: () => null, setItem: vi.fn() });
    useNavigationStore.getState().setActivePage('apps');
    useToolsStore.getState().setDetectedTools([
      {
        id: 'hermes',
        name: 'Hermes Desktop',
        category: 'Desktop',
        installed: true,
        apiProtocol: ['openai'],
      },
    ]);
    vi.mocked(api.getModels).mockResolvedValue([
      {
        internalId: 'hermes-fixture',
        name: 'Fixture',
        modelId: 'native-model',
        baseUrl: 'https://provider.example/api/v4',
        apiKey: '',
      },
    ]);
    vi.mocked(api.applyModelToTool).mockResolvedValue({ success: true, message: 'ok' });
    await act(async () => {
      renderer = create(
        <AppManagerProvider>
          <Harness />
        </AppManagerProvider>
      );
    });
    await act(async () => context.setSelectedTool('hermes'));
    act(() => {
      context.handleSelectModel('hermes', 'hermes-fixture');
      context.setAgreedConfigPolicy(true);
      context.setLaunchAfterApply(true);
    });
  });
  afterEach(() => {
    act(() => renderer?.unmount());
    useToolsStore.getState().setDetectedTools([]);
    vi.mocked(api.getModels).mockResolvedValue([]);
    vi.stubGlobal('localStorage', undefined);
    vi.useRealTimers();
  });
  it('keeps navigation passive and applies the selected endpoint and empty key only on launch', async () => {
    act(() => context.setViewMode('install'));
    act(() => context.setViewMode('desktop'));
    await act(async () => context.setSelectedTool(null));
    await act(async () => context.setSelectedTool('hermes'));
    expect(api.applyModelToTool).not.toHaveBeenCalled();
    expect(api.restoreToolToOfficial).not.toHaveBeenCalled();
    expect(api.startTool).not.toHaveBeenCalled();
    expect(openDialog).not.toHaveBeenCalled();
    await act(async () => {
      await context.handleLaunch();
    });
    expect(api.applyModelToTool).toHaveBeenCalledExactlyOnceWith(
      'hermes',
      expect.objectContaining({
        model: 'native-model',
        baseUrl: 'https://provider.example/api/v4',
        apiKey: '',
        protocol: 'openai',
      })
    );
    expect(api.startTool).toHaveBeenCalledExactlyOnceWith('hermes', undefined, undefined);
  });
  it('shows an explicit configuration error without starting the client', async () => {
    vi.mocked(api.applyModelToTool).mockResolvedValueOnce({
      success: false,
      message: 'accountError.format',
    });
    await act(async () => {
      await context.handleLaunch();
    });
    expect(context.applyError).toBe('Localized configuration format error');
    expect(api.startTool).not.toHaveBeenCalled();
  });
  it('does not repeat an apply or launch the client while native configuration is pending', async () => {
    let finish!: (value: { success: boolean; message: string }) => void;
    vi.mocked(api.applyModelToTool).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        })
    );
    let pending!: Promise<void>;
    await act(async () => {
      pending = context.handleLaunch();
    });
    await act(async () => {
      await context.handleLaunch();
    });
    expect(api.applyModelToTool).toHaveBeenCalledTimes(1);
    expect(api.startTool).not.toHaveBeenCalled();
    await act(async () => {
      finish({ success: true, message: 'ok' });
      await pending;
    });
    expect(api.startTool).toHaveBeenCalledTimes(1);
  });
});

describe('Claude Desktop account apply', () => {
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
    enableAccountPlugins();
    const storage = new Map<string, string>();
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => storage.set(key, value),
    });
    const account = { id: 'desktop-saved', email: 'one@example.test', active: true };
    vi.mocked(api.listClaudeDesktopAccounts).mockResolvedValue([account]);
    vi.mocked(api.switchClaudeDesktopAccount).mockResolvedValue(account);
    vi.mocked(api.restoreToolToOfficial).mockResolvedValue({ success: true, message: 'ok' });
    useNavigationStore.getState().setActivePage('apps');
    useToolsStore
      .getState()
      .setDetectedTools([
        { id: 'claudedesktop', name: 'Claude', category: 'Desktop', installed: true },
      ]);
  });
  afterEach(() => {
    act(() => renderer?.unmount());
    useToolsStore.getState().setDetectedTools([]);
    vi.mocked(api.listClaudeDesktopAccounts).mockResolvedValue([]);
  });
  async function mount() {
    await act(async () => {
      renderer = create(
        <AppManagerProvider>
          <Harness />
        </AppManagerProvider>
      );
    });
    await act(async () => {
      context.setSelectedTool('claudedesktop');
    });
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
  }
  it('requires explicit apply and respects the launch-after-apply preference', async () => {
    await mount();
    expect(context.claudeDesktopAccounts.selectedId).toBe('desktop-saved');
    expect(api.switchClaudeDesktopAccount).not.toHaveBeenCalled();
    expect(api.restoreToolToOfficial).not.toHaveBeenCalled();
    act(() => context.setLaunchAfterApply(false));
    await act(async () => {
      await context.handleLaunch();
    });
    expect(api.switchClaudeDesktopAccount).toHaveBeenCalledWith('desktop-saved');
    expect(api.restoreToolToOfficial).toHaveBeenCalledWith('claudedesktop');
    expect(api.listClaudeDesktopAccounts).toHaveBeenCalledTimes(2);
    expect(api.startTool).not.toHaveBeenCalled();
    expect(api.refreshClaudeDesktopAccount).not.toHaveBeenCalled();
    act(() => context.setLaunchAfterApply(true));
    await act(async () => {
      await context.handleLaunch();
    });
    expect(api.startTool).toHaveBeenCalledWith('claudedesktop', undefined);
    expect(openDialog).not.toHaveBeenCalled();
    expect(context.isLaunching).toBe(false);
  });
  it('does not restore provider settings or launch when automatic desktop shutdown fails', async () => {
    await mount();
    expect(context.claudeDesktopAccounts.selectedId).toBe('desktop-saved');
    vi.mocked(api.switchClaudeDesktopAccount).mockRejectedValueOnce(
      new Error('accountError.stopClient')
    );
    await act(async () => {
      await context.handleLaunch();
    });
    expect(context.applyError).toBe('accountError.stopClient');
    expect(context.isLaunching).toBe(false);
    expect(api.restoreToolToOfficial).not.toHaveBeenCalled();
    expect(api.startTool).not.toHaveBeenCalled();
  });
});

describe.each(['claudedesktop', 'claudecode'] as const)('%s direct API configuration', (tool) => {
  let renderer: ReactTestRenderer;
  let context: ReturnType<typeof useAppManager>;
  let storage: Map<string, string>;
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
    storage = new Map([
      ['echobird_claudedesktop_relay_mode', 'false'],
      ['echobird_claudecode_relay_mode', 'false'],
    ]);
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => storage.set(key, value),
    });
    useNavigationStore.getState().setActivePage('apps');
    useToolsStore.getState().setDetectedTools(
      ['claudedesktop', 'claudecode'].map((id) => ({
        id,
        name: id,
        category: 'Desktop',
        installed: true,
        apiProtocol: ['anthropic'],
      }))
    );
    vi.mocked(api.listClaudeDesktopAccounts).mockResolvedValue([]);
    vi.mocked(api.getModels).mockResolvedValue([
      {
        internalId: 'claude-fixture',
        name: 'Actual name',
        modelId: 'actual-model',
        baseUrl: '',
        anthropicUrl: 'https://provider.example/anthropic',
        apiKey: 'fixture-key',
      },
    ]);
    vi.mocked(api.applyModelToTool).mockResolvedValue({ success: true, message: 'ok' });
  });
  afterEach(() => {
    act(() => renderer?.unmount());
    useToolsStore.getState().setDetectedTools([]);
    vi.mocked(api.getModels).mockResolvedValue([]);
    vi.stubGlobal('localStorage', undefined);
    vi.useRealTimers();
  });
  async function mount() {
    await act(async () => {
      renderer = create(
        <AppManagerProvider>
          <Harness />
        </AppManagerProvider>
      );
    });
    await act(async () => context.setSelectedTool(tool));
    act(() => {
      context.handleSelectModel(tool, 'claude-fixture');
      context.setAgreedConfigPolicy(true);
      context.setLaunchAfterApply(false);
    });
  }
  it('ignores old routing preferences without applying on navigation or selection', async () => {
    await mount();
    await act(async () =>
      context.setSelectedTool(tool === 'claudecode' ? 'claudedesktop' : 'claudecode')
    );
    act(() => context.setViewMode('install'));
    act(() => context.setViewMode('desktop'));
    expect(api.applyModelToTool).not.toHaveBeenCalled();
    expect(api.restoreToolToOfficial).not.toHaveBeenCalled();
    expect(api.startClaudeDesktopLogin).not.toHaveBeenCalled();
    expect(api.refreshClaudeDesktopAccount).not.toHaveBeenCalled();
    expect(openDialog).not.toHaveBeenCalled();
    expect(storage.get('echobird_claudedesktop_relay_mode')).toBe('false');
    expect(storage.get('echobird_claudecode_relay_mode')).toBe('false');
    await act(async () => context.setSelectedTool(tool));
    await act(async () => {
      await context.handleLaunch();
      await vi.advanceTimersByTimeAsync(3000);
    });
    expect(api.applyModelToTool).toHaveBeenCalledExactlyOnceWith(tool, {
      id: 'claude-fixture',
      name: 'Actual name',
      model: 'actual-model',
      baseUrl: 'https://provider.example/anthropic',
      apiKey: 'fixture-key',
      protocol: 'anthropic',
      oneMContext: false,
      webSearch: true,
    });
  });
  it('keeps 1M independent per client and reports explicit apply failures', async () => {
    await mount();
    const setOneM =
      tool === 'claudedesktop' ? context.setClaudeDesktop1mMode : context.setClaude1mMode;
    await act(async () => setOneM(true));
    expect(api.applyModelToTool).toHaveBeenLastCalledWith(
      tool,
      expect.objectContaining({ oneMContext: true })
    );
    expect(tool === 'claudedesktop' ? context.claude1mMode : context.claudeDesktop1mMode).toBe(
      false
    );
    vi.mocked(api.applyModelToTool).mockResolvedValueOnce({
      success: false,
      message: 'Apply failed',
    });
    await act(async () => setOneM(false));
    expect(api.applyModelToTool).toHaveBeenLastCalledWith(
      tool,
      expect.objectContaining({ oneMContext: false })
    );
    expect(context.applyError).toBe('Apply failed');
  });
  it('persists a separate search preference and retains it on 1M changes and reentry', async () => {
    await mount();
    await act(async () => context.setClaudeWebSearch(tool, false));
    expect(api.applyModelToTool).toHaveBeenCalledExactlyOnceWith(
      tool,
      expect.objectContaining({ webSearch: false })
    );
    expect(storage.get(`echobird_${tool}_web_search`)).toBe('false');
    expect(
      tool === 'claudedesktop' ? context.claudeCodeWebSearch : context.claudeDesktopWebSearch
    ).toBe(true);
    await act(async () =>
      (tool === 'claudedesktop' ? context.setClaudeDesktop1mMode : context.setClaude1mMode)(true)
    );
    expect(api.applyModelToTool).toHaveBeenLastCalledWith(
      tool,
      expect.objectContaining({ oneMContext: true, webSearch: false })
    );
    act(() => renderer.unmount());
    vi.mocked(api.applyModelToTool).mockClear();
    await mount();
    expect(
      tool === 'claudedesktop' ? context.claudeDesktopWebSearch : context.claudeCodeWebSearch
    ).toBe(false);
    expect(api.applyModelToTool).not.toHaveBeenCalled();
  });
  it('blocks repeated search changes and apply while pending, and reports failure', async () => {
    await mount();
    let finish!: (value: { success: boolean; message: string }) => void;
    vi.mocked(api.applyModelToTool).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        })
    );
    act(() => context.setClaudeWebSearch(tool, false));
    expect(context.isLaunching).toBe(true);
    act(() => context.setClaudeWebSearch(tool, true));
    await act(async () => context.handleLaunch());
    expect(api.applyModelToTool).toHaveBeenCalledTimes(1);
    expect(storage.get(`echobird_${tool}_web_search`)).toBe('false');
    await act(async () => finish({ success: false, message: 'Search update failed' }));
    expect(context.isLaunching).toBe(false);
    expect(context.applyError).toBe('Search update failed');
  });
  it('blocks search, repeated 1M changes and apply while a 1M update is pending', async () => {
    await mount();
    let finish!: (value: { success: boolean; message: string }) => void;
    vi.mocked(api.applyModelToTool).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        })
    );
    act(() =>
      (tool === 'claudedesktop' ? context.setClaudeDesktop1mMode : context.setClaude1mMode)(true)
    );
    expect(context.isLaunching).toBe(true);
    act(() => {
      context.setClaudeWebSearch(tool, false);
      (tool === 'claudedesktop' ? context.setClaudeDesktop1mMode : context.setClaude1mMode)(false);
    });
    await act(async () => context.handleLaunch());
    expect(api.applyModelToTool).toHaveBeenCalledExactlyOnceWith(
      tool,
      expect.objectContaining({ oneMContext: true, webSearch: true })
    );
    expect(storage.get(`echobird_${tool}_1m_mode`)).toBe('true');
    expect(
      tool === 'claudedesktop' ? context.claudeDesktopWebSearch : context.claudeCodeWebSearch
    ).toBe(true);
    expect(api.startTool).not.toHaveBeenCalled();
    await act(async () => finish({ success: false, message: '1M update failed' }));
    expect(context.isLaunching).toBe(false);
    expect(context.applyError).toBe('1M update failed');
  });
  it.each(['uninstalled', 'missing-model'])(
    'does not write native preference settings for %s',
    async (reason) => {
      await mount();
      act(() => {
        if (reason === 'uninstalled')
          useToolsStore
            .getState()
            .setDetectedTools([{ id: tool, name: tool, category: 'Desktop', installed: false }]);
        else context.handleSelectModel(tool, '');
      });
      await act(async () => context.setClaudeWebSearch(tool, false));
      await act(async () =>
        (tool === 'claudedesktop' ? context.setClaudeDesktop1mMode : context.setClaude1mMode)(true)
      );
      expect(api.applyModelToTool).not.toHaveBeenCalled();
      expect(api.restoreToolToOfficial).not.toHaveBeenCalled();
    }
  );
  it('does not apply API preferences while an official account is selected', async () => {
    await mount();
    act(() => {
      if (tool === 'claudedesktop') context.claudeDesktopAccounts.select('saved-account');
      else context.claudeCodeAccounts.setSelectedId('saved-account');
    });
    await act(async () => context.setClaudeWebSearch(tool, false));
    await act(async () =>
      (tool === 'claudedesktop' ? context.setClaudeDesktop1mMode : context.setClaude1mMode)(true)
    );
    expect(api.applyModelToTool).not.toHaveBeenCalled();
    expect(api.restoreToolToOfficial).not.toHaveBeenCalled();
  });
});

describe.each(['codex', 'chatgptdesktop'])('%s Web Search preference', (tool) => {
  let renderer: ReactTestRenderer;
  let context: ReturnType<typeof useAppManager>;
  let storage: Map<string, string>;
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
    storage = new Map();
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => storage.set(key, value),
    });
    useNavigationStore.getState().setActivePage('apps');
    useToolsStore.getState().setDetectedTools(
      ['codex', 'chatgptdesktop'].map((id) => ({
        id,
        name: id,
        category: 'Desktop',
        installed: true,
        apiProtocol: ['openai'],
      }))
    );
    vi.mocked(api.listCodexAccounts).mockResolvedValue([]);
    vi.mocked(api.getModels).mockResolvedValue([
      {
        internalId: 'fixture',
        name: 'Fixture',
        modelId: 'fixture-model',
        baseUrl: 'https://provider.example/v1',
        apiKey: 'fixture',
      },
      {
        internalId: 'smart-router',
        name: 'Auto Router',
        modelId: 'auto',
        baseUrl: 'http://127.0.0.1:53683/v1',
        apiKey: 'fixture-router',
      },
      ...['deepseek.com', 'xiaomimimo.com'].map((domain) => ({
        internalId: domain,
        name: domain,
        modelId: 'fixture-model',
        baseUrl: `https://api.${domain}/v1`,
        apiKey: 'fixture',
      })),
    ]);
    vi.mocked(api.applyModelToTool).mockResolvedValue({ success: true, message: 'ok' });
  });
  afterEach(() => {
    act(() => renderer?.unmount());
    useToolsStore.getState().setDetectedTools([]);
    vi.mocked(api.getModels).mockResolvedValue([]);
    vi.stubGlobal('localStorage', undefined);
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
    await act(async () => {
      context.setSelectedTool(tool);
    });
    act(() => {
      context.handleSelectModel(tool, 'fixture');
      context.setAgreedConfigPolicy(true);
      context.setLaunchAfterApply(false);
    });
  };
  it('restores the default-on preference without inferring provider capabilities', async () => {
    await mount();
    for (const id of ['fixture', 'deepseek.com', 'xiaomimimo.com']) {
      act(() => context.handleSelectModel(tool, id));
      expect(context.codexWebSearch).toBe(true);
      await act(async () => {
        await context.handleLaunch();
        await vi.advanceTimersByTimeAsync(3000);
      });
      expect(api.applyModelToTool).toHaveBeenLastCalledWith(
        tool,
        expect.objectContaining({ webSearch: true })
      );
    }
  });

  it('uses the saved search choice for Smart Access and saves explicit changes for both router and direct models', async () => {
    await mount();
    act(() => context.handleSelectModel(tool, 'smart-router'));
    await act(async () => {
      await context.handleLaunch();
      await vi.advanceTimersByTimeAsync(3000);
    });
    expect(api.applyModelToTool).toHaveBeenLastCalledWith(
      tool,
      expect.objectContaining({ model: 'auto', webSearch: true })
    );
    for (const enabled of [false, true]) {
      await act(async () => context.setCodexWebSearch(enabled));
      expect(context.codexWebSearch).toBe(enabled);
      expect(storage.get('echobird_codex_web_search')).toBe(String(enabled));
      expect(api.applyModelToTool).toHaveBeenLastCalledWith(
        tool,
        expect.objectContaining({ model: 'auto', webSearch: enabled })
      );
    }
    act(() => context.handleSelectModel(tool, 'fixture'));
    await act(async () => {
      await context.handleLaunch();
      await vi.advanceTimersByTimeAsync(3000);
    });
    expect(api.applyModelToTool).toHaveBeenLastCalledWith(
      tool,
      expect.objectContaining({ model: 'fixture-model', webSearch: true })
    );
  });

  it('preserves the applied router selection while disabled and clears it only after its group is deleted', async () => {
    routerState.routers = [{ id: 'smart-router' }];
    await mount();
    act(() => context.handleSelectModel(tool, 'smart-router'));
    vi.mocked(api.getModels).mockResolvedValue([]);
    routerState.routerEnabled = true;
    await act(async () =>
      renderer.update(
        <AppManagerProvider>
          <Harness />
        </AppManagerProvider>
      )
    );
    expect(context.toolModelConfig[tool]).toBe('smart-router');
    expect(api.applyModelToTool).not.toHaveBeenCalled();
    routerState.routers = [];
    await act(async () =>
      renderer.update(
        <AppManagerProvider>
          <Harness />
        </AppManagerProvider>
      )
    );
    expect(context.toolModelConfig[tool]).toBeNull();
    expect(api.applyModelToTool).not.toHaveBeenCalled();
  });
  it('keeps navigation passive and preserves off across repeat apply, tool switches and remounts', async () => {
    await mount();
    await act(async () => context.setCodexWebSearch(false));
    expect(api.applyModelToTool).toHaveBeenCalledExactlyOnceWith(
      tool,
      expect.objectContaining({ webSearch: false })
    );
    vi.mocked(api.applyModelToTool).mockClear();
    const other = tool === 'codex' ? 'chatgptdesktop' : 'codex';
    await act(async () => {
      context.setSelectedTool(other);
      context.handleSelectModel(other, 'fixture');
    });
    act(() => context.setViewMode('install'));
    act(() => {
      useNavigationStore.getState().setDeveloperMode(true);
      useNavigationStore.getState().setDeveloperTab('plugins');
      useNavigationStore.getState().setActivePage('developer');
    });
    expect(api.applyModelToTool).not.toHaveBeenCalled();
    expect(api.startTool).not.toHaveBeenCalled();
    expect(api.startCodexLogin).not.toHaveBeenCalled();
    expect(api.refreshCodexAccountQuota).not.toHaveBeenCalled();
    expect(openDialog).not.toHaveBeenCalled();
    expect(context.applyError).toBeNull();
    act(() => renderer.unmount());
    act(() => useNavigationStore.getState().setActivePage('apps'));
    await mount();
    expect(context.codexWebSearch).toBe(false);
    for (let i = 0; i < 2; i++) {
      await act(async () => {
        await context.handleLaunch();
        await vi.advanceTimersByTimeAsync(3000);
      });
      expect(api.applyModelToTool).toHaveBeenLastCalledWith(
        tool,
        expect.objectContaining({ webSearch: false })
      );
    }
    await act(async () => {
      context.setSelectedTool(other);
    });
    act(() => context.handleSelectModel(other, 'fixture'));
    await act(async () => context.setCodexWebSearch(true));
    expect(api.applyModelToTool).toHaveBeenLastCalledWith(
      other,
      expect.objectContaining({ webSearch: true })
    );
    expect(api.startTool).not.toHaveBeenCalled();
  });
  it('shows explicit apply failures without losing the saved choice or launching', async () => {
    await mount();
    vi.mocked(api.applyModelToTool).mockResolvedValueOnce({
      success: false,
      message: 'Config unavailable',
    });
    await act(async () => context.setCodexWebSearch(false));
    expect(context.applyError).toBe('Config unavailable');
    expect(context.isLaunching).toBe(false);
    expect(context.codexWebSearch).toBe(false);
    expect(storage.get('echobird_codex_web_search')).toBe('false');
    expect(api.startTool).not.toHaveBeenCalled();
  });
  it('blocks overlapping flips while the selected API model is being updated', async () => {
    await mount();
    let resolve!: (value: { success: boolean; message: string }) => void;
    vi.mocked(api.applyModelToTool).mockImplementationOnce(
      () =>
        new Promise((r) => {
          resolve = r;
        })
    );
    act(() => context.setCodexWebSearch(false));
    expect(context.isLaunching).toBe(true);
    act(() => context.setCodexWebSearch(true));
    expect(context.codexWebSearch).toBe(false);
    expect(api.applyModelToTool).toHaveBeenCalledTimes(1);
    await act(async () => {
      resolve({ success: true, message: 'ok' });
    });
    expect(context.isLaunching).toBe(false);
  });
  it('only saves the preference when an account or uninstalled client is selected', async () => {
    await mount();
    act(() => context.setSelectedCodexAccountId('saved-account'));
    await act(async () => context.setCodexWebSearch(false));
    expect(api.applyModelToTool).not.toHaveBeenCalled();
    act(() => context.handleSelectModel(tool, 'fixture'));
    act(() =>
      useToolsStore
        .getState()
        .setDetectedTools([{ id: tool, name: tool, category: 'Desktop', installed: false }])
    );
    await act(async () => context.setCodexWebSearch(true));
    expect(api.applyModelToTool).not.toHaveBeenCalled();
    expect(storage.get('echobird_codex_web_search')).toBe('true');
  });
});

describe('ZCode model capability preferences', () => {
  let renderer: ReactTestRenderer;
  let context: ReturnType<typeof useAppManager>;
  let storage: Map<string, string>;
  function Harness() {
    const state = useAppManager();
    useLayoutEffect(() => {
      context = state;
    });
    return null;
  }
  beforeEach(() => {
    vi.clearAllMocks();
    storage = new Map();
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => storage.set(key, value),
    });
    useNavigationStore.getState().setActivePage('apps');
    useToolsStore.getState().setDetectedTools([
      { id: 'zcode', name: 'ZCode', category: 'Desktop', installed: true, apiProtocol: ['openai'] },
      { id: 'codex', name: 'Codex', category: 'CLI', installed: true, apiProtocol: ['openai'] },
    ]);
    vi.mocked(api.listZCodeAccounts).mockResolvedValue([]);
    vi.mocked(api.listCodexAccounts).mockResolvedValue([]);
    vi.mocked(api.getModels).mockResolvedValue([
      {
        internalId: 'fixture',
        name: 'Fixture',
        modelId: 'vision/fixture',
        baseUrl: 'https://example.test/v1',
        apiKey: 'fixture',
      },
    ]);
    vi.mocked(api.applyModelToTool).mockResolvedValue({ success: true, message: 'ok' });
  });
  afterEach(() => {
    act(() => renderer?.unmount());
    useToolsStore.getState().setDetectedTools([]);
    vi.mocked(api.getModels).mockResolvedValue([]);
    vi.stubGlobal('localStorage', undefined);
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
    await act(async () => context.setSelectedTool('zcode'));
  };

  it('restores independent preferences without applying or logging in during navigation', async () => {
    storage.set('echobird_zcode_web_search', 'true');
    storage.set('echobird_zcode_image_input', 'false');
    await mount();
    expect(context.zcodeWebSearch).toBe(true);
    expect(context.zcodeImageInput).toBe(false);
    expect(api.applyModelToTool).not.toHaveBeenCalled();
    expect(api.startZCodeLogin).not.toHaveBeenCalled();
    expect(api.refreshZCodeAccountQuota).not.toHaveBeenCalled();
    expect(context.applyError).toBeNull();
    await act(async () => context.setSelectedTool('codex'));
    await act(async () => context.setSelectedTool('zcode'));
    expect(api.applyModelToTool).not.toHaveBeenCalled();
    expect(context.zcodeImageInput).toBe(false);
    expect(context.codexWebSearch).toBe(true);
  });

  it('persists without an API model, applies explicit flips with both values, and isolates tools', async () => {
    await mount();
    expect(context.zcodeWebSearch).toBe(false);
    expect(context.zcodeImageInput).toBe(true);
    act(() => context.setZCodeCapability('webSearch', true));
    expect(api.applyModelToTool).not.toHaveBeenCalled();
    expect(storage.get('echobird_zcode_web_search')).toBe('true');
    act(() => context.handleSelectModel('zcode', 'fixture'));
    await act(async () => context.setZCodeCapability('imageInput', false));
    expect(api.applyModelToTool).toHaveBeenLastCalledWith(
      'zcode',
      expect.objectContaining({
        model: 'vision/fixture',
        webSearch: true,
        imageInput: false,
      })
    );
    await act(async () => context.setZCodeCapability('webSearch', false));
    expect(api.applyModelToTool).toHaveBeenLastCalledWith(
      'zcode',
      expect.objectContaining({
        webSearch: false,
        imageInput: false,
      })
    );
    expect(storage.get('echobird_zcode_image_input')).toBe('false');
    const calls = vi.mocked(api.applyModelToTool).mock.calls.length;
    await act(async () => context.setSelectedTool('codex'));
    act(() => context.setZCodeCapability('imageInput', true));
    expect(api.applyModelToTool).toHaveBeenCalledTimes(calls);
    expect(context.zcodeImageInput).toBe(false);
    expect(storage.has('echobird_codex_web_search')).toBe(false);
  });

  it('locks repeated flips until completion and exposes an explicit apply failure', async () => {
    await mount();
    act(() => context.handleSelectModel('zcode', 'fixture'));
    let finish!: (value: { success: boolean; message: string }) => void;
    vi.mocked(api.applyModelToTool).mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      })
    );
    act(() => context.setZCodeCapability('webSearch', true));
    expect(context.isLaunching).toBe(true);
    act(() => context.setZCodeCapability('imageInput', false));
    expect(api.applyModelToTool).toHaveBeenCalledTimes(1);
    expect(context.zcodeImageInput).toBe(true);
    await act(async () => finish({ success: false, message: 'Fixture apply failed' }));
    expect(context.isLaunching).toBe(false);
    expect(context.applyError).toBe('Fixture apply failed');
  });

  it.each(['account', 'uninstalled'])('does not apply capabilities in %s mode', async (mode) => {
    const account: api.ZCodeAccount = {
      id: 'saved',
      email: 'fixture@example.test',
      provider: 'bigmodel',
      active: true,
      plan: null,
      remainingPercent: null,
      resetAt: null,
    };
    vi.mocked(api.listZCodeAccounts).mockResolvedValue([account]);
    if (mode === 'uninstalled') {
      useToolsStore
        .getState()
        .setDetectedTools([{ id: 'zcode', name: 'ZCode', category: 'Desktop', installed: false }]);
    }
    await mount();
    if (mode === 'account') act(() => context.zcodeAccounts.select(account.id));
    await act(async () => context.setZCodeCapability('webSearch', true));
    expect(storage.get('echobird_zcode_web_search')).toBe('true');
    expect(api.applyModelToTool).not.toHaveBeenCalled();
    expect(api.switchZCodeAccount).not.toHaveBeenCalled();
    expect(api.refreshZCodeAccountQuota).not.toHaveBeenCalled();
    expect(api.startTool).not.toHaveBeenCalled();
  });

  it('passes saved capabilities through the ordinary apply button', async () => {
    vi.useFakeTimers();
    storage.set('echobird_zcode_web_search', 'true');
    storage.set('echobird_zcode_image_input', 'false');
    await mount();
    act(() => {
      context.handleSelectModel('zcode', 'fixture');
      context.setAgreedConfigPolicy(true);
      context.setLaunchAfterApply(false);
    });
    await act(async () => {
      await context.handleLaunch();
      await vi.advanceTimersByTimeAsync(3000);
    });
    expect(api.applyModelToTool).toHaveBeenLastCalledWith(
      'zcode',
      expect.objectContaining({
        webSearch: true,
        imageInput: false,
      })
    );
    expect(api.startTool).not.toHaveBeenCalled();
  });
});

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
    act(() => {
      useNavigationStore.getState().setDeveloperMode(true);
      useNavigationStore.getState().setDeveloperTab('plugins');
      useNavigationStore.getState().setActivePage('developer');
    });
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

describe('Muse Code API model workflow', () => {
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
    const storage = new Map<string, string>();
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => storage.set(key, value),
    });
    useNavigationStore.getState().setActivePage('apps');
    useToolsStore.getState().setDetectedTools([
      { ...museCodeDefinition, id: 'musecode', installed: true, activeModel: 'before' },
      { id: 'muse', name: 'Muse', category: 'Cloud Agent', installed: true, noModelConfig: true },
    ]);
    vi.mocked(api.getModels).mockResolvedValue([
      {
        internalId: 'fixture',
        name: 'Fixture',
        modelId: 'fixture-model',
        baseUrl: 'https://example.test/v1',
        apiKey: 'fixture-key',
      },
    ]);
    vi.mocked(api.applyModelToTool).mockResolvedValue({ success: true, message: 'ok' });
    vi.mocked(openDialog).mockResolvedValue('/fixture');
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
    act(() => context.setSelectedTool('musecode'));
  };

  it('keeps plugin and statistics navigation passive and reloads models on returning to applications', async () => {
    await mount();
    vi.mocked(api.getModels).mockClear();
    await act(async () => {
      useNavigationStore.getState().setDeveloperMode(true);
      useNavigationStore.getState().setDeveloperTab('plugins');
      useNavigationStore.getState().setActivePage('developer');
    });
    await act(async () => useNavigationStore.getState().setDeveloperTab('statistics'));
    expect(api.getModels).not.toHaveBeenCalled();
    await act(async () => useNavigationStore.getState().setActivePage('apps'));
    expect(api.getModels).toHaveBeenCalledOnce();
    expect(context.userModels.some((model) => model.internalId === 'fixture')).toBe(true);
    await act(async () => useNavigationStore.getState().setDeveloperTab('plugins'));
    await act(async () => useNavigationStore.getState().setDeveloperMode(false));
    expect(useNavigationStore.getState().activePage).toBe('apps');
    expect(api.applyModelToTool).not.toHaveBeenCalled();
    expect(api.startTool).not.toHaveBeenCalled();
    expect(api.startCodexLogin).not.toHaveBeenCalled();
    expect(api.refreshCodexAccountQuota).not.toHaveBeenCalled();
    expect(openDialog).not.toHaveBeenCalled();
  });

  it('keeps selection, tool, tab and page navigation passive, including an uninstalled client', async () => {
    await mount();
    act(() => context.handleSelectModel('musecode', 'fixture'));
    act(() => context.setSelectedTool('muse'));
    act(() => context.setViewMode('install'));
    act(() => useNavigationStore.getState().setActivePage('models'));
    act(() =>
      useToolsStore
        .getState()
        .setDetectedTools([{ ...museCodeDefinition, id: 'musecode', installed: false }])
    );
    act(() => {
      useNavigationStore.getState().setActivePage('apps');
      context.setSelectedTool('musecode');
    });
    await act(async () => {
      await vi.runOnlyPendingTimersAsync();
    });
    expect(api.applyModelToTool).not.toHaveBeenCalled();
    expect(api.restoreToolToOfficial).not.toHaveBeenCalled();
    expect(api.startTool).not.toHaveBeenCalled();
    expect(openDialog).not.toHaveBeenCalled();
    expect(api.listCodexAccounts).not.toHaveBeenCalled();
    expect(api.listCursorAccounts).not.toHaveBeenCalled();
    expect(api.startCodexLogin).not.toHaveBeenCalled();
    expect(api.refreshCodexAccountQuota).not.toHaveBeenCalled();
    expect(context.applyError).toBeNull();
  });

  it('applies the pending model explicitly without launching when launch is unchecked', async () => {
    await mount();
    act(() => {
      context.handleSelectModel('musecode', 'fixture');
      context.setLaunchAfterApply(false);
    });
    expect(api.applyModelToTool).not.toHaveBeenCalled();
    await act(async () => {
      await context.handleLaunch();
    });
    expect(api.applyModelToTool).toHaveBeenCalledExactlyOnceWith('musecode', {
      id: 'fixture',
      name: 'Fixture',
      baseUrl: 'https://example.test/v1',
      apiKey: 'fixture-key',
      model: 'fixture-model',
      protocol: 'openai',
    });
    expect(context.detectedTools.find((tool) => tool.id === 'musecode')?.activeModel).toBe(
      'fixture-model'
    );
    expect(context.toolModelConfig.muse).toBeUndefined();
    expect(api.startTool).not.toHaveBeenCalled();
    expect(openDialog).not.toHaveBeenCalled();
  });

  it('waits for configuration before choosing a working folder and launching', async () => {
    await mount();
    act(() => context.handleSelectModel('musecode', 'fixture'));
    let resolve!: (value: { success: boolean; message: string }) => void;
    vi.mocked(api.applyModelToTool).mockImplementationOnce(() => new Promise((r) => (resolve = r)));
    let pending!: Promise<void>;
    act(() => {
      pending = context.handleLaunch();
    });
    expect(context.isLaunching).toBe(true);
    expect(api.startTool).not.toHaveBeenCalled();
    expect(openDialog).not.toHaveBeenCalled();
    await act(async () => {
      resolve({ success: true, message: 'ok' });
      await pending;
    });
    expect(api.startTool).toHaveBeenCalledExactlyOnceWith('musecode', 'muse', '/fixture');
    expect(openDialog).toHaveBeenCalledExactlyOnceWith({ directory: true, multiple: false });
    expect(vi.mocked(api.applyModelToTool).mock.invocationCallOrder[0]).toBeLessThan(
      vi.mocked(api.startTool).mock.invocationCallOrder[0]
    );
  });

  it('preserves the applied model and stops launch when configuration fails', async () => {
    await mount();
    act(() => context.handleSelectModel('musecode', 'fixture'));
    vi.mocked(api.applyModelToTool).mockResolvedValueOnce({
      success: false,
      message: 'Native storage unavailable',
    });
    await act(async () => {
      await context.handleLaunch();
    });
    expect(context.applyError).toBe('Native storage unavailable');
    expect(context.isLaunching).toBe(false);
    expect(context.toolModelConfig.musecode).toBe('fixture');
    expect(context.detectedTools.find((tool) => tool.id === 'musecode')?.activeModel).toBe(
      'before'
    );
    expect(api.startTool).not.toHaveBeenCalled();
    expect(openDialog).not.toHaveBeenCalled();
  });

  it('can launch without a model and aborts when the working folder is cancelled', async () => {
    await mount();
    vi.mocked(openDialog).mockResolvedValueOnce(null);
    await act(async () => {
      await context.handleLaunch();
    });
    expect(api.applyModelToTool).not.toHaveBeenCalled();
    expect(api.startTool).not.toHaveBeenCalled();
    expect(context.isLaunching).toBe(false);
    await act(async () => {
      await context.handleLaunch();
    });
    expect(api.startTool).toHaveBeenCalledExactlyOnceWith('musecode', 'muse', '/fixture');
  });
});

describe('VS Code API model workflow', () => {
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
    const storage = new Map<string, string>();
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => storage.set(key, value),
    });
    useNavigationStore.getState().setActivePage('apps');
    useToolsStore.getState().setDetectedTools([
      {
        id: 'vscode',
        name: 'Visual Studio Code',
        category: 'IDE',
        installed: true,
        activeModel: 'before',
        apiProtocol: ['openai', 'anthropic'],
      },
      {
        id: 'cline',
        name: 'Cline',
        category: 'CLI Code',
        installed: true,
        activeModel: 'cline-before',
      },
      {
        id: 'copilotdesktop',
        name: 'GitHub Copilot',
        category: 'Desktop',
        installed: true,
      },
    ]);
    vi.mocked(api.getModels).mockResolvedValue([
      {
        internalId: 'fixture',
        name: 'Fixture',
        modelId: 'fixture-model',
        baseUrl: 'https://example.test/v1',
        apiKey: 'fixture-key',
      },
      {
        internalId: 'anthropic-fixture',
        name: 'Anthropic Fixture',
        modelId: 'anthropic-model',
        baseUrl: '',
        anthropicUrl: 'https://example.test/anthropic',
        apiKey: 'fixture-key',
      },
    ]);
    vi.mocked(api.applyModelToTool).mockResolvedValue({ success: true, message: 'ok' });
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
    act(() => context.setSelectedTool('vscode'));
  };

  it('keeps selection and tool, tab and page navigation passive, including an uninstalled client', async () => {
    await mount();
    act(() => context.handleSelectModel('vscode', 'fixture'));
    act(() => context.setSelectedTool('cline'));
    act(() => context.setViewMode('install'));
    act(() => {
      useNavigationStore.getState().setDeveloperMode(true);
      useNavigationStore.getState().setDeveloperTab('plugins');
      useNavigationStore.getState().setActivePage('developer');
    });
    act(() =>
      useToolsStore
        .getState()
        .setDetectedTools([
          { id: 'vscode', name: 'Visual Studio Code', category: 'IDE', installed: false },
        ])
    );
    act(() => {
      useNavigationStore.getState().setActivePage('apps');
      context.setViewMode('desktop');
      context.setSelectedTool('vscode');
    });
    await act(async () => {
      await vi.runOnlyPendingTimersAsync();
    });
    expect(api.applyModelToTool).not.toHaveBeenCalled();
    expect(api.restoreToolToOfficial).not.toHaveBeenCalled();
    expect(api.startTool).not.toHaveBeenCalled();
    expect(api.listCodexAccounts).not.toHaveBeenCalled();
    expect(api.listCursorAccounts).not.toHaveBeenCalled();
    expect(api.startCodexLogin).not.toHaveBeenCalled();
    expect(api.refreshCodexAccountQuota).not.toHaveBeenCalled();
    expect(openDialog).not.toHaveBeenCalled();
    expect(context.applyError).toBeNull();
  });

  it.each([
    ['fixture', 'openai', 'https://example.test/v1', 'fixture-model'],
    ['anthropic-fixture', 'anthropic', 'https://example.test/anthropic', 'anthropic-model'],
  ])(
    'applies %s explicitly and updates only VS Code without launching',
    async (id, protocol, baseUrl, model) => {
      await mount();
      act(() => {
        context.handleSelectModel('vscode', id);
        context.setLaunchAfterApply(false);
      });
      expect(api.applyModelToTool).not.toHaveBeenCalled();
      await act(async () => {
        await context.handleLaunch();
      });
      expect(api.applyModelToTool).toHaveBeenCalledWith('vscode', {
        id,
        name: id === 'fixture' ? 'Fixture' : 'Anthropic Fixture',
        baseUrl,
        apiKey: 'fixture-key',
        model,
        protocol,
      });
      expect(context.detectedTools.find((tool) => tool.id === 'vscode')?.activeModel).toBe(model);
      expect(context.detectedTools.find((tool) => tool.id === 'cline')?.activeModel).toBe(
        'cline-before'
      );
      expect(context.toolModelConfig.cline).toBeUndefined();
      expect(api.startTool).not.toHaveBeenCalled();
      expect(openDialog).not.toHaveBeenCalled();
    }
  );

  it('waits for configuration to finish before starting the existing IDE launch flow', async () => {
    await mount();
    act(() => context.handleSelectModel('vscode', 'fixture'));
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
      await vi.advanceTimersByTimeAsync(3500);
    });
    expect(context.isLaunching).toBe(true);
    await act(async () => {
      await context.handleLaunch();
    });
    expect(api.applyModelToTool).toHaveBeenCalledOnce();
    await act(async () => {
      resolve({ success: true, message: 'ok' });
      await pending;
    });
    expect(api.startTool).toHaveBeenCalledWith('vscode', undefined, undefined);
    expect(openDialog).not.toHaveBeenCalled();
    expect(context.isLaunching).toBe(true);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    expect(context.isLaunching).toBe(false);
  });

  it('keeps the action pending until IDE launch finishes', async () => {
    await mount();
    act(() => context.handleSelectModel('vscode', 'fixture'));
    let resolve!: () => void;
    vi.mocked(api.startTool).mockImplementationOnce(
      () =>
        new Promise<void>((r) => {
          resolve = r;
        })
    );
    let pending!: Promise<void>;
    await act(async () => {
      pending = context.handleLaunch();
    });
    expect(api.startTool).toHaveBeenCalledOnce();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(3500);
      await context.handleLaunch();
    });
    expect(context.isLaunching).toBe(true);
    expect(api.applyModelToTool).toHaveBeenCalledOnce();
    expect(api.startTool).toHaveBeenCalledOnce();
    await act(async () => {
      resolve();
      await pending;
      await vi.advanceTimersByTimeAsync(3000);
    });
    expect(context.isLaunching).toBe(false);
    expect(openDialog).not.toHaveBeenCalled();
  });

  it.each(['vscode', 'copilotdesktop'])(
    'ignores the previous failed tool cooldown while %s is pending',
    async (tool) => {
      await mount();
      act(() => {
        context.setSelectedTool('cline');
        context.handleSelectModel('cline', 'fixture');
      });
      vi.mocked(api.applyModelToTool).mockResolvedValueOnce({
        success: false,
        message: 'Unavailable',
      });
      await act(async () => {
        await context.handleLaunch();
      });
      expect(context.isLaunching).toBe(false);
      act(() => {
        context.setSelectedTool(tool);
        context.handleSelectModel(tool, 'fixture');
      });
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
      await act(async () => {
        await vi.advanceTimersByTimeAsync(3500);
      });
      expect(context.isLaunching).toBe(true);
      await act(async () => {
        await context.handleLaunch();
      });
      expect(api.applyModelToTool).toHaveBeenCalledTimes(2);
      expect(api.startTool).not.toHaveBeenCalled();
      await act(async () => {
        resolve({ success: true, message: 'ok' });
        await pending;
        await vi.advanceTimersByTimeAsync(3000);
      });
      expect(context.isLaunching).toBe(false);
      expect(api.startTool).toHaveBeenCalledExactlyOnceWith(tool, undefined, undefined);
    }
  );

  it('ignores a previous tool failure while VS Code configuration is pending', async () => {
    await mount();
    act(() => {
      context.setSelectedTool('cline');
      context.handleSelectModel('cline', 'fixture');
      context.setLaunchAfterApply(false);
    });
    let resolvePrevious!: (value: { success: boolean; message: string }) => void;
    vi.mocked(api.applyModelToTool).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolvePrevious = resolve;
        })
    );
    let previousPending!: Promise<void>;
    act(() => {
      previousPending = context.handleLaunch();
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(3500);
    });
    expect(context.isLaunching).toBe(false);
    act(() => {
      context.setSelectedTool('vscode');
      context.handleSelectModel('vscode', 'fixture');
    });
    let resolveCurrent!: (value: { success: boolean; message: string }) => void;
    vi.mocked(api.applyModelToTool).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveCurrent = resolve;
        })
    );
    let currentPending!: Promise<void>;
    act(() => {
      currentPending = context.handleLaunch();
    });
    expect(context.isLaunching).toBe(true);
    await act(async () => {
      resolvePrevious({ success: false, message: 'Unavailable' });
      await previousPending;
    });
    await act(async () => {
      await context.handleLaunch();
    });
    const applyCountWhilePending = vi.mocked(api.applyModelToTool).mock.calls.length;
    await act(async () => {
      resolveCurrent({ success: true, message: 'ok' });
      await currentPending;
    });
    expect(applyCountWhilePending).toBe(2);
    expect(api.startTool).not.toHaveBeenCalled();
    expect(openDialog).not.toHaveBeenCalled();
  });

  it.each(['cancel', 'error'])(
    'ignores the previous tool folder picker %s while VS Code configuration is pending',
    async (outcome) => {
      await mount();
      act(() => {
        context.setSelectedTool('cline');
        context.handleSelectModel('cline', 'fixture');
      });
      let resolvePrevious!: (value: { success: boolean; message: string }) => void;
      vi.mocked(api.applyModelToTool).mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            resolvePrevious = resolve;
          })
      );
      let previousPending!: Promise<void>;
      act(() => {
        previousPending = context.handleLaunch();
      });
      await act(async () => {
        await vi.advanceTimersByTimeAsync(3500);
      });
      expect(context.isLaunching).toBe(false);
      act(() => {
        context.setSelectedTool('vscode');
        context.handleSelectModel('vscode', 'fixture');
        context.setLaunchAfterApply(false);
      });
      let resolveCurrent!: (value: { success: boolean; message: string }) => void;
      vi.mocked(api.applyModelToTool).mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            resolveCurrent = resolve;
          })
      );
      let currentPending!: Promise<void>;
      act(() => {
        currentPending = context.handleLaunch();
      });
      expect(context.isLaunching).toBe(true);
      if (outcome === 'cancel') vi.mocked(openDialog).mockResolvedValueOnce(null);
      else vi.mocked(openDialog).mockRejectedValueOnce(new Error('Picker unavailable'));
      await act(async () => {
        resolvePrevious({ success: true, message: 'ok' });
        await previousPending;
      });
      expect(openDialog).toHaveBeenCalledWith({ directory: true, multiple: false });
      const launchingAfterPicker = context.isLaunching;
      await act(async () => {
        await context.handleLaunch();
      });
      const applyCountWhilePending = vi.mocked(api.applyModelToTool).mock.calls.length;
      await act(async () => {
        resolveCurrent({ success: true, message: 'ok' });
        await currentPending;
      });
      expect(applyCountWhilePending).toBe(2);
      expect(launchingAfterPicker).toBe(true);
      expect(api.startTool).not.toHaveBeenCalled();
    }
  );

  it('preserves the applied model and prevents launch when configuration fails', async () => {
    await mount();
    act(() => context.handleSelectModel('vscode', 'fixture'));
    vi.mocked(api.applyModelToTool).mockResolvedValueOnce({
      success: false,
      message: 'Invalid settings',
    });
    await act(async () => {
      await context.handleLaunch();
    });
    expect(context.applyError).toBe('Invalid settings');
    expect(context.isLaunching).toBe(false);
    expect(context.detectedTools.find((tool) => tool.id === 'vscode')?.activeModel).toBe('before');
    expect(api.startTool).not.toHaveBeenCalled();
    expect(openDialog).not.toHaveBeenCalled();
  });

  it('translates DSH Auto Router configuration failures and only launches after a successful retry', async () => {
    useToolsStore.getState().setDetectedTools([
      {
        id: 'dsh',
        name: 'DeepSeek Harness',
        category: 'Desktop',
        installed: true,
        activeModel: 'before',
        apiProtocol: ['anthropic', 'openai'],
        noCwdPrompt: true,
      },
    ]);
    vi.mocked(api.getModels).mockResolvedValue([
      {
        internalId: 'smart-router',
        name: 'Auto Router',
        modelId: 'auto',
        baseUrl: 'http://127.0.0.1:53683/v1',
        anthropicUrl: 'http://127.0.0.1:53683',
        apiKey: 'router-fixture-key',
      },
    ]);
    await mount();
    act(() => {
      context.setSelectedTool('dsh');
      context.handleSelectModel('dsh', 'smart-router');
    });
    expect(api.applyModelToTool).not.toHaveBeenCalled();
    expect(api.startTool).not.toHaveBeenCalled();
    expect(context.applyError).toBeNull();

    vi.mocked(api.applyModelToTool).mockResolvedValueOnce({
      success: false,
      message: 'accountError.format',
    });
    await act(async () => {
      await context.handleLaunch();
    });
    expect(context.applyError).toBe('Localized configuration format error');
    expect(context.isLaunching).toBe(false);
    expect(context.detectedTools.find((tool) => tool.id === 'dsh')?.activeModel).toBe('before');
    expect(api.startTool).not.toHaveBeenCalled();
    expect(openDialog).not.toHaveBeenCalled();

    await act(async () => {
      await context.handleLaunch();
    });
    expect(api.applyModelToTool).toHaveBeenLastCalledWith(
      'dsh',
      expect.objectContaining({
        model: 'auto',
        baseUrl: 'http://127.0.0.1:53683',
        protocol: 'anthropic',
        apiKey: 'router-fixture-key',
      })
    );
    expect(api.startTool).toHaveBeenCalledExactlyOnceWith('dsh', undefined, undefined);
    const applyOrder = vi.mocked(api.applyModelToTool).mock.invocationCallOrder;
    expect(applyOrder[applyOrder.length - 1]).toBeLessThan(
      vi.mocked(api.startTool).mock.invocationCallOrder[0]
    );
    expect(openDialog).not.toHaveBeenCalled();
  });

  it('launches without writing configuration when the write policy is unchecked', async () => {
    await mount();
    act(() => {
      context.handleSelectModel('vscode', 'fixture');
      context.setAgreedConfigPolicy(false);
    });
    await act(async () => {
      await context.handleLaunch();
    });
    expect(api.applyModelToTool).not.toHaveBeenCalled();
    expect(api.startTool).toHaveBeenCalledWith('vscode', undefined, undefined);
    expect(openDialog).not.toHaveBeenCalled();
    expect(context.detectedTools.find((tool) => tool.id === 'vscode')?.activeModel).toBe('before');
  });
});

const accountTools = [
  'grokbot',
  'cursor',
  'grok',
  'manus',
  'cue',
  'antigravity',
  'antigravitydesktop',
] as const;
describe.each(['copilotdesktop', 'workbuddy', 'workbuddyai'])('%s pending API apply', (tool) => {
  let renderer: ReactTestRenderer;
  let context: ReturnType<typeof useAppManager>;
  function Harness() {
    const state = useAppManager();
    useLayoutEffect(() => {
      context = state;
    });
    return null;
  }
  beforeEach(async () => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    vi.mocked(api.listCopilotAccounts).mockResolvedValue([]);
    vi.mocked(api.getModels).mockResolvedValue([
      { internalId: 'fixture', name: 'Fixture', baseUrl: 'https://example.com/v1', apiKey: 'key' },
    ]);
    vi.mocked(api.applyModelToTool).mockResolvedValue({ success: true, message: 'ok' });
    useNavigationStore.getState().setActivePage('apps');
    useToolsStore
      .getState()
      .setDetectedTools([{ id: tool, name: tool, category: 'Desktop', installed: true }]);
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
      context.handleSelectModel(tool, 'fixture');
    });
  });
  afterEach(() => {
    act(() => renderer?.unmount());
    useToolsStore.getState().setDetectedTools([]);
    vi.mocked(api.getModels).mockResolvedValue([]);
    vi.useRealTimers();
  });
  it('blocks repeat applies until native initialization and optional launch finish', async () => {
    let finishApply!: (value: { success: boolean; message: string }) => void;
    let finishLaunch!: () => void;
    vi.mocked(api.applyModelToTool).mockReturnValueOnce(
      new Promise((resolve) => {
        finishApply = resolve;
      })
    );
    vi.mocked(api.startTool).mockReturnValueOnce(
      new Promise((resolve) => {
        finishLaunch = resolve;
      })
    );
    let pending!: Promise<void>;
    await act(async () => {
      pending = context.handleLaunch();
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(4000);
    });
    expect(context.isLaunching).toBe(true);
    await act(async () => {
      await context.handleLaunch();
    });
    expect(api.applyModelToTool).toHaveBeenCalledTimes(1);
    expect(api.startTool).not.toHaveBeenCalled();
    await act(async () => {
      finishApply({ success: true, message: 'ok' });
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(4000);
    });
    expect(context.isLaunching).toBe(true);
    expect(api.startTool).toHaveBeenCalledTimes(1);
    await act(async () => {
      finishLaunch();
      await pending;
    });
    expect(context.isLaunching).toBe(true);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    expect(context.isLaunching).toBe(false);
  });
  it('retains pending state for configuration-only changes and recovers after success', async () => {
    act(() => context.setLaunchAfterApply(false));
    let finishApply!: (value: { success: boolean; message: string }) => void;
    vi.mocked(api.applyModelToTool).mockReturnValueOnce(
      new Promise((resolve) => {
        finishApply = resolve;
      })
    );
    let pending!: Promise<void>;
    await act(async () => {
      pending = context.handleLaunch();
      await vi.advanceTimersByTimeAsync(4000);
    });
    expect(context.isLaunching).toBe(true);
    await act(async () => {
      await context.handleLaunch();
    });
    expect(api.applyModelToTool).toHaveBeenCalledTimes(1);
    await act(async () => {
      finishApply({ success: true, message: 'ok' });
      await pending;
    });
    expect(api.startTool).not.toHaveBeenCalled();
    expect(context.isLaunching).toBe(true);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    expect(context.isLaunching).toBe(false);
    await act(async () => {
      await context.handleLaunch();
    });
    expect(api.applyModelToTool).toHaveBeenCalledTimes(2);
    expect(api.startTool).not.toHaveBeenCalled();
  });
  it('releases pending state after configuration failure and allows a retry', async () => {
    let finishApply!: (value: { success: boolean; message: string }) => void;
    vi.mocked(api.applyModelToTool).mockReturnValueOnce(
      new Promise((resolve) => {
        finishApply = resolve;
      })
    );
    let pending!: Promise<void>;
    await act(async () => {
      pending = context.handleLaunch();
      await vi.advanceTimersByTimeAsync(4000);
    });
    expect(context.isLaunching).toBe(true);
    await act(async () => {
      finishApply({ success: false, message: 'Configuration unavailable' });
      await pending;
    });
    expect(context.isLaunching).toBe(false);
    expect(context.applyError).toBe('Configuration unavailable');
    expect(api.startTool).not.toHaveBeenCalled();
    await act(async () => {
      await context.handleLaunch();
    });
    expect(api.applyModelToTool).toHaveBeenCalledTimes(2);
    expect(api.startTool).toHaveBeenCalledExactlyOnceWith(tool, undefined, undefined);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    expect(context.isLaunching).toBe(false);
  });
});

describe.each(accountTools)('%s navigation account loading', (tool) => {
  let renderer: ReactTestRenderer;
  let context: ReturnType<typeof useAppManager>;
  const list = {
    grokbot: api.listGrokBotAccounts,
    cursor: api.listCursorAccounts,
    grok: api.listGrokAccounts,
    manus: api.listManusAccounts,
    cue: api.listCueAccounts,
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
    expect(api.refreshCueAccount).not.toHaveBeenCalled();
    expect(api.startCueLogin).not.toHaveBeenCalled();
    expect(api.switchCueAccount).not.toHaveBeenCalled();
    expect(api.refreshAntigravityAccount).not.toHaveBeenCalled();
    expect(api.startAntigravityLogin).not.toHaveBeenCalled();
    expect(api.switchAntigravityAccount).not.toHaveBeenCalled();
  });
});

describe('Cue account apply', () => {
  let renderer: ReactTestRenderer;
  let context: ReturnType<typeof useAppManager>;
  const account = {
    id: 'cue-saved',
    email: 'cue@example.test',
    active: true,
    plan: 'Free',
    credits: null,
  };
  function Harness() {
    const state = useAppManager();
    useLayoutEffect(() => {
      context = state;
    });
    return null;
  }
  beforeEach(async () => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    vi.mocked(api.listCueAccounts).mockResolvedValue([account]);
    vi.mocked(api.switchCueAccount).mockResolvedValue(account);
    vi.mocked(api.cancelCueLogin).mockResolvedValue(undefined);
    useNavigationStore.getState().setActivePage('apps');
    useToolsStore
      .getState()
      .setDetectedTools([
        { id: 'cue', name: 'Cue', category: 'Cloud Agent', installed: true, noModelConfig: true },
      ]);
    await act(async () => {
      renderer = create(
        <AppManagerProvider>
          <Harness />
        </AppManagerProvider>
      );
    });
    await act(async () => {
      context.setSelectedTool('cue');
    });
    await act(async () => {
      await vi.runOnlyPendingTimersAsync();
    });
    expect(context.cueAccounts.selectedId).toBe('cue-saved');
  });
  afterEach(() => {
    act(() => renderer?.unmount());
    useToolsStore.getState().setDetectedTools([]);
    vi.useRealTimers();
  });
  it('applies the selected Cue account before launch without touching Manus or API models', async () => {
    expect(api.switchCueAccount).not.toHaveBeenCalled();
    await act(async () => {
      await context.handleLaunch();
    });
    expect(api.switchCueAccount).toHaveBeenCalledExactlyOnceWith('cue-saved');
    expect(api.startTool).toHaveBeenCalledExactlyOnceWith('cue');
    expect(vi.mocked(api.switchCueAccount).mock.invocationCallOrder[0]).toBeLessThan(
      vi.mocked(api.startTool).mock.invocationCallOrder[0]
    );
    expect(api.switchManusAccount).not.toHaveBeenCalled();
    expect(api.applyModelToTool).not.toHaveBeenCalled();
    expect(api.refreshCueAccount).not.toHaveBeenCalled();
  });
  it('preserves selection and does not launch after a failed switch', async () => {
    vi.mocked(api.switchCueAccount).mockRejectedValueOnce(new Error('accountError.closeClient'));
    await act(async () => {
      await context.handleLaunch();
    });
    expect(api.startTool).not.toHaveBeenCalled();
    expect(context.cueAccounts.selectedId).toBe('cue-saved');
    expect(context.applyError).not.toBeNull();
    expect(context.isLaunching).toBe(false);
  });
  it('blocks applying or launching while the Cue login is pending', async () => {
    vi.mocked(api.startCueLogin).mockResolvedValue({
      loginId: 'cue-pending',
      expiresAt: Date.now() / 1000 + 60,
    });
    vi.mocked(api.pollCueLogin).mockReturnValue(new Promise(() => {}));
    await act(async () => {
      void context.cueAccounts.add();
    });
    expect(context.cueAccounts.busy).toBe(true);
    await act(async () => {
      await context.handleLaunch();
    });
    expect(api.switchCueAccount).not.toHaveBeenCalled();
    expect(api.startTool).not.toHaveBeenCalled();
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

describe.each(['workbuddy', 'workbuddyai'] as const)('%s image capability preference', (tool) => {
  let renderer: ReactTestRenderer;
  let context: ReturnType<typeof useAppManager>;
  let storage: Map<string, string>;
  const other = tool === 'workbuddy' ? 'workbuddyai' : 'workbuddy';
  function Harness() {
    const state = useAppManager();
    useLayoutEffect(() => {
      context = state;
    });
    return null;
  }
  beforeEach(() => {
    vi.clearAllMocks();
    storage = new Map();
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => storage.set(key, value),
    });
    useNavigationStore.getState().setActivePage('apps');
    useToolsStore.getState().setDetectedTools(
      [tool, other].map((id) => ({
        id,
        name: id,
        category: 'Desktop',
        installed: true,
        apiProtocol: ['openai'],
      }))
    );
    vi.mocked(api.getModels).mockResolvedValue([
      {
        internalId: 'fixture',
        name: 'Fixture',
        modelId: 'any/model',
        baseUrl: 'https://example.test/v1',
        apiKey: 'fixture',
      },
    ]);
    vi.mocked(api.applyModelToTool).mockResolvedValue({ success: true, message: 'ok' });
  });
  afterEach(() => {
    act(() => renderer?.unmount());
    useToolsStore.getState().setDetectedTools([]);
    vi.mocked(api.getModels).mockResolvedValue([]);
    vi.stubGlobal('localStorage', undefined);
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
    await act(async () => context.setSelectedTool(tool));
  };
  it('restores preferences across passive navigation and remounts without applying', async () => {
    storage.set(`echobird_${tool}_image_input`, 'false');
    storage.set(`echobird_${tool}_1m_mode`, 'true');
    await mount();
    expect(context.workBuddyImageInput).toBe(false);
    expect(context.workBuddy1mMode).toBe(true);
    await act(async () => context.setSelectedTool(other));
    expect(context.workBuddyImageInput).toBe(true);
    expect(context.workBuddy1mMode).toBe(false);
    act(() => context.setWorkBuddyImageInput(false));
    expect(storage.get(`echobird_${other}_image_input`)).toBe('false');
    await act(async () => context.setSelectedTool(tool));
    act(() => useNavigationStore.getState().setActivePage('models'));
    act(() => useNavigationStore.getState().setActivePage('apps'));
    expect(context.workBuddyImageInput).toBe(false);
    act(() => renderer.unmount());
    await mount();
    expect(context.workBuddyImageInput).toBe(false);
    expect(context.workBuddy1mMode).toBe(true);
    expect(api.applyModelToTool).not.toHaveBeenCalled();
    expect(api.startTool).not.toHaveBeenCalled();
    expect(context.zcodeImageInput).toBe(true);
    expect(storage.has('echobird_zcode_image_input')).toBe(false);
  });
  it('passes the saved value through normal apply and explicit flips without launching', async () => {
    vi.useFakeTimers();
    storage.set(`echobird_${tool}_image_input`, 'false');
    storage.set(`echobird_${tool}_1m_mode`, 'true');
    await mount();
    act(() => {
      context.handleSelectModel(tool, 'fixture');
      context.setAgreedConfigPolicy(true);
      context.setLaunchAfterApply(false);
    });
    await act(async () => {
      await context.handleLaunch();
      await vi.advanceTimersByTimeAsync(3000);
    });
    expect(api.applyModelToTool).toHaveBeenLastCalledWith(
      tool,
      expect.objectContaining({ imageInput: false, oneMContext: true })
    );
    await act(async () => context.setWorkBuddyImageInput(true));
    expect(api.applyModelToTool).toHaveBeenLastCalledWith(
      tool,
      expect.objectContaining({ imageInput: true, oneMContext: true })
    );
    await act(async () => context.setWorkBuddy1mMode(false));
    expect(api.applyModelToTool).toHaveBeenLastCalledWith(
      tool,
      expect.objectContaining({ imageInput: true, oneMContext: false })
    );
    expect(storage.get(`echobird_${tool}_1m_mode`)).toBe('false');
    expect(storage.has(`echobird_${other}_1m_mode`)).toBe(false);
    expect(storage.get(`echobird_${tool}_image_input`)).toBe('true');
    expect(storage.has(`echobird_${other}_image_input`)).toBe(false);
    expect(api.startTool).not.toHaveBeenCalled();
  });
  it('blocks repeated flips while pending and shows explicit failures', async () => {
    await mount();
    act(() => context.handleSelectModel(tool, 'fixture'));
    let finish!: (value: { success: boolean; message: string }) => void;
    vi.mocked(api.applyModelToTool).mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      })
    );
    act(() => context.setWorkBuddyImageInput(false));
    expect(context.isLaunching).toBe(true);
    act(() => context.setWorkBuddyImageInput(true));
    act(() => context.setWorkBuddy1mMode(true));
    expect(context.workBuddy1mMode).toBe(false);
    expect(api.applyModelToTool).toHaveBeenCalledTimes(1);
    expect(context.workBuddyImageInput).toBe(false);
    await act(async () => finish({ success: false, message: 'Fixture apply failed' }));
    expect(context.isLaunching).toBe(false);
    expect(context.applyError).toBe('Fixture apply failed');
    await act(async () => context.setSelectedTool('unrelated'));
    act(() => context.setWorkBuddyImageInput(true));
    expect(api.applyModelToTool).toHaveBeenCalledTimes(1);
    expect(storage.get(`echobird_${tool}_image_input`)).toBe('false');
  });
  it.each(['no-model', 'account', 'uninstalled'])(
    'only saves preference in %s mode',
    async (mode) => {
      await mount();
      if (mode !== 'no-model') act(() => context.handleSelectModel(tool, 'fixture'));
      if (mode === 'account') act(() => context.workBuddyAccounts.select('saved'));
      if (mode === 'uninstalled')
        act(() =>
          useToolsStore
            .getState()
            .setDetectedTools([{ id: tool, name: tool, category: 'Desktop', installed: false }])
        );
      await act(async () => context.setWorkBuddyImageInput(false));
      expect(storage.get(`echobird_${tool}_image_input`)).toBe('false');
      await act(async () => context.setWorkBuddy1mMode(true));
      expect(storage.get(`echobird_${tool}_1m_mode`)).toBe('true');
      expect(api.applyModelToTool).not.toHaveBeenCalled();
      expect(api.startTool).not.toHaveBeenCalled();
    }
  );
});

describe.each(['mimocode', 'mimodesktop'] as const)('%s model capabilities', (tool) => {
  let renderer: ReactTestRenderer;
  let context: ReturnType<typeof useAppManager>;
  let storage: Map<string, string>;
  const other = tool === 'mimocode' ? 'mimodesktop' : 'mimocode';
  function Harness() {
    const state = useAppManager();
    useLayoutEffect(() => {
      context = state;
    });
    return null;
  }
  beforeEach(() => {
    vi.clearAllMocks();
    storage = new Map();
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => storage.set(key, value),
    });
    useNavigationStore.getState().setActivePage('apps');
    useToolsStore.getState().setDetectedTools(
      [tool, other].map((id) => ({
        id,
        name: id,
        category: 'Desktop',
        installed: true,
        apiProtocol: ['openai'],
      }))
    );
    vi.mocked(api.getModels).mockResolvedValue([
      {
        internalId: 'fixture',
        name: 'Fixture',
        modelId: 'any/model',
        baseUrl: 'https://example.test/v1',
        apiKey: 'fixture',
      },
    ]);
    vi.mocked(api.applyModelToTool).mockResolvedValue({ success: true, message: 'ok' });
  });
  afterEach(() => {
    act(() => renderer?.unmount());
    useToolsStore.getState().setDetectedTools([]);
    vi.mocked(api.getModels).mockResolvedValue([]);
    vi.stubGlobal('localStorage', undefined);
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
    await act(async () => context.setSelectedTool(tool));
  };
  it('restores isolated values without applying on navigation or remount', async () => {
    storage.set(`echobird_${tool}_image_input`, 'false');
    storage.set(`echobird_${tool}_1m_mode`, 'true');
    await mount();
    expect(context.mimoImageInput).toBe(false);
    expect(context.mimo1mMode).toBe(true);
    await act(async () => context.setSelectedTool(other));
    expect(context.mimoImageInput).toBe(true);
    expect(context.mimo1mMode).toBe(false);
    await act(async () => context.setSelectedTool(tool));
    act(() => useNavigationStore.getState().setActivePage('models'));
    act(() => useNavigationStore.getState().setActivePage('apps'));
    act(() => renderer.unmount());
    await mount();
    expect(context.mimoImageInput).toBe(false);
    expect(context.mimo1mMode).toBe(true);
    expect(context.zcodeImageInput).toBe(true);
    expect(context.claude1mMode).toBe(false);
    expect(api.applyModelToTool).not.toHaveBeenCalled();
    expect(api.startTool).not.toHaveBeenCalled();
  });
  it('uses both saved values for explicit flips and the normal apply button', async () => {
    vi.useFakeTimers();
    await mount();
    act(() => context.handleSelectModel(tool, 'fixture'));
    await act(async () => context.setMimoCapability('imageInput', false));
    expect(api.applyModelToTool).toHaveBeenLastCalledWith(
      tool,
      expect.objectContaining({ imageInput: false, oneMContext: false })
    );
    await act(async () => context.setMimoCapability('oneMContext', true));
    expect(api.applyModelToTool).toHaveBeenLastCalledWith(
      tool,
      expect.objectContaining({ imageInput: false, oneMContext: true })
    );
    expect(storage.get(`echobird_${tool}_image_input`)).toBe('false');
    expect(storage.get(`echobird_${tool}_1m_mode`)).toBe('true');
    expect(storage.has(`echobird_${other}_1m_mode`)).toBe(false);
    act(() => {
      context.setAgreedConfigPolicy(true);
      context.setLaunchAfterApply(false);
    });
    await act(async () => {
      await context.handleLaunch();
      await vi.advanceTimersByTimeAsync(3000);
    });
    expect(api.applyModelToTool).toHaveBeenLastCalledWith(
      tool,
      expect.objectContaining({ imageInput: false, oneMContext: true })
    );
    expect(api.startTool).not.toHaveBeenCalled();
  });
  it('locks both controls while pending and reports failure without launching', async () => {
    await mount();
    act(() => context.handleSelectModel(tool, 'fixture'));
    let finish!: (value: { success: boolean; message: string }) => void;
    vi.mocked(api.applyModelToTool).mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      })
    );
    act(() => context.setMimoCapability('oneMContext', true));
    expect(context.isLaunching).toBe(true);
    act(() => context.setMimoCapability('imageInput', false));
    expect(context.mimoImageInput).toBe(true);
    expect(api.applyModelToTool).toHaveBeenCalledTimes(1);
    await act(async () => finish({ success: false, message: 'Fixture MiMo error' }));
    expect(context.applyError).toBe('Fixture MiMo error');
    expect(context.isLaunching).toBe(false);
    expect(api.startTool).not.toHaveBeenCalled();
  });
  it.each(['no-model', 'uninstalled'])('only saves preferences with %s', async (mode) => {
    await mount();
    if (mode === 'uninstalled') {
      act(() => context.handleSelectModel(tool, 'fixture'));
      act(() =>
        useToolsStore
          .getState()
          .setDetectedTools([{ id: tool, name: tool, category: 'Desktop', installed: false }])
      );
    }
    await act(async () => context.setMimoCapability('oneMContext', true));
    expect(storage.get(`echobird_${tool}_1m_mode`)).toBe('true');
    expect(api.applyModelToTool).not.toHaveBeenCalled();
    expect(api.startTool).not.toHaveBeenCalled();
    await act(async () => context.setSelectedTool('unrelated'));
    act(() => context.setMimoCapability('oneMContext', false));
    expect(storage.get(`echobird_${tool}_1m_mode`)).toBe('true');
  });
});

describe.each(['minimaxcode', 'minimaxdesktop'] as const)('%s model capabilities', (tool) => {
  let renderer: ReactTestRenderer;
  let context: ReturnType<typeof useAppManager>;
  let storage: Map<string, string>;
  const other = tool === 'minimaxcode' ? 'minimaxdesktop' : 'minimaxcode';
  function Harness() {
    const state = useAppManager();
    useLayoutEffect(() => {
      context = state;
    });
    return null;
  }
  beforeEach(() => {
    vi.clearAllMocks();
    storage = new Map();
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => storage.set(key, value),
    });
    useNavigationStore.getState().setActivePage('apps');
    useToolsStore.getState().setDetectedTools(
      [tool, other].map((id) => ({
        id,
        name: id,
        category: 'Desktop',
        installed: true,
        apiProtocol: ['openai'],
      }))
    );
    vi.mocked(api.getModels).mockResolvedValue([
      {
        internalId: 'fixture',
        name: 'Fixture',
        modelId: 'any/model',
        baseUrl: 'https://example.test/v1',
        apiKey: 'fixture',
      },
    ]);
    vi.mocked(api.applyModelToTool).mockResolvedValue({ success: true, message: 'ok' });
  });
  afterEach(() => {
    act(() => renderer?.unmount());
    useToolsStore.getState().setDetectedTools([]);
    vi.mocked(api.getModels).mockResolvedValue([]);
    vi.stubGlobal('localStorage', undefined);
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
    await act(async () => context.setSelectedTool(tool));
  };
  it('shares saved capabilities across both entries without applying during navigation', async () => {
    storage.set('echobird_minimax_image_input', 'false');
    storage.set('echobird_minimax_1m_mode', 'true');
    await mount();
    expect(context.minimaxImageInput).toBe(false);
    expect(context.minimax1mMode).toBe(true);
    await act(async () => context.setSelectedTool(other));
    expect(context.minimaxImageInput).toBe(false);
    expect(context.minimax1mMode).toBe(true);
    act(() => useNavigationStore.getState().setActivePage('models'));
    act(() => useNavigationStore.getState().setActivePage('apps'));
    act(() => renderer.unmount());
    await mount();
    expect(context.minimax1mMode).toBe(true);
    expect(context.mimo1mMode).toBe(false);
    expect(api.applyModelToTool).not.toHaveBeenCalled();
    expect(api.startTool).not.toHaveBeenCalled();
    expect(api.startMinimaxLogin).not.toHaveBeenCalled();
    expect(api.refreshMinimaxAccount).not.toHaveBeenCalled();
  });
  it('passes both flags on flips and normal apply, including through the other entry', async () => {
    vi.useFakeTimers();
    await mount();
    act(() => context.handleSelectModel(tool, 'fixture'));
    await act(async () => context.setMinimaxCapability('imageInput', false));
    expect(api.applyModelToTool).toHaveBeenLastCalledWith(
      tool,
      expect.objectContaining({ imageInput: false, oneMContext: false })
    );
    await act(async () => context.setMinimaxCapability('oneMContext', true));
    expect(api.applyModelToTool).toHaveBeenLastCalledWith(
      tool,
      expect.objectContaining({ imageInput: false, oneMContext: true })
    );
    expect(storage.get('echobird_minimax_image_input')).toBe('false');
    expect(storage.get('echobird_minimax_1m_mode')).toBe('true');
    await act(async () => context.setSelectedTool(other));
    expect(context.minimax1mMode).toBe(true);
    act(() => {
      context.setAgreedConfigPolicy(true);
      context.setLaunchAfterApply(false);
    });
    await act(async () => {
      await context.handleLaunch();
      await vi.advanceTimersByTimeAsync(3000);
    });
    expect(api.applyModelToTool).toHaveBeenLastCalledWith(
      other,
      expect.objectContaining({ imageInput: false, oneMContext: true })
    );
    expect(api.startTool).not.toHaveBeenCalled();
  });
  it('prevents duplicate flips while pending and exposes an apply failure', async () => {
    await mount();
    act(() => context.handleSelectModel(tool, 'fixture'));
    let finish!: (value: { success: boolean; message: string }) => void;
    vi.mocked(api.applyModelToTool).mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      })
    );
    act(() => context.setMinimaxCapability('oneMContext', true));
    expect(context.isLaunching).toBe(true);
    act(() => context.setMinimaxCapability('imageInput', false));
    expect(context.minimaxImageInput).toBe(true);
    expect(api.applyModelToTool).toHaveBeenCalledTimes(1);
    await act(async () => finish({ success: false, message: 'Fixture MiniMax error' }));
    expect(context.applyError).toBe('Fixture MiniMax error');
    expect(context.isLaunching).toBe(false);
    expect(api.startTool).not.toHaveBeenCalled();
  });
  it.each(['no-model', 'uninstalled', 'account'])(
    'only stores preferences for %s',
    async (mode) => {
      await mount();
      if (mode === 'uninstalled') {
        act(() => context.handleSelectModel(tool, 'fixture'));
        act(() =>
          useToolsStore
            .getState()
            .setDetectedTools([{ id: tool, name: tool, category: 'Desktop', installed: false }])
        );
      }
      if (mode === 'account') {
        await act(async () => context.minimaxAccounts.select('fixture-account'));
      }
      await act(async () => context.setMinimaxCapability('oneMContext', true));
      expect(storage.get('echobird_minimax_1m_mode')).toBe('true');
      expect(api.applyModelToTool).not.toHaveBeenCalled();
      expect(api.startTool).not.toHaveBeenCalled();
      await act(async () => context.setSelectedTool('unrelated'));
      act(() => context.setMinimaxCapability('oneMContext', false));
      expect(storage.get('echobird_minimax_1m_mode')).toBe('true');
    }
  );
});

describe.each(['qoder', 'qodercn'] as const)('%s model capabilities', (tool) => {
  let renderer: ReactTestRenderer;
  let context: ReturnType<typeof useAppManager>;
  let storage: Map<string, string>;
  const other = tool === 'qoder' ? 'qodercn' : 'qoder';
  function Harness() {
    const state = useAppManager();
    useLayoutEffect(() => {
      context = state;
    });
    return null;
  }
  beforeEach(() => {
    vi.clearAllMocks();
    storage = new Map();
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => storage.set(key, value),
    });
    useNavigationStore.getState().setActivePage('apps');
    useToolsStore.getState().setDetectedTools(
      [tool, other].map((id) => ({
        id,
        name: id,
        category: 'Desktop',
        installed: true,
        apiProtocol: ['openai'],
      }))
    );
    vi.mocked(api.getModels).mockResolvedValue([
      {
        internalId: 'fixture',
        name: 'Fixture',
        modelId: 'any/model',
        baseUrl: 'https://example.test/v1',
        apiKey: 'fixture',
      },
    ]);
    vi.mocked(api.applyModelToTool).mockResolvedValue({ success: true, message: 'ok' });
  });
  afterEach(() => {
    act(() => renderer?.unmount());
    useToolsStore.getState().setDetectedTools([]);
    vi.mocked(api.getModels).mockResolvedValue([]);
    vi.stubGlobal('localStorage', undefined);
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
    await act(async () => context.setSelectedTool(tool));
  };
  it('restores isolated values without applying on navigation or remount', async () => {
    storage.set(`echobird_${tool}_image_input`, 'false');
    storage.set(`echobird_${tool}_1m_mode`, 'true');
    await mount();
    expect(context.qoderImageInput).toBe(false);
    expect(context.qoder1mMode).toBe(true);
    await act(async () => context.setSelectedTool(other));
    expect(context.qoderImageInput).toBe(true);
    expect(context.qoder1mMode).toBe(false);
    await act(async () => context.setSelectedTool(tool));
    act(() => useNavigationStore.getState().setActivePage('models'));
    act(() => useNavigationStore.getState().setActivePage('apps'));
    act(() => renderer.unmount());
    await mount();
    expect(context.qoderImageInput).toBe(false);
    expect(context.qoder1mMode).toBe(true);
    expect(context.zcodeImageInput).toBe(true);
    expect(context.claude1mMode).toBe(false);
    expect(api.applyModelToTool).not.toHaveBeenCalled();
    expect(api.startTool).not.toHaveBeenCalled();
  });
  it('uses both saved values for explicit flips and the normal apply button', async () => {
    vi.useFakeTimers();
    await mount();
    act(() => context.handleSelectModel(tool, 'fixture'));
    await act(async () => context.setQoderCapability('imageInput', false));
    expect(api.applyModelToTool).toHaveBeenLastCalledWith(
      tool,
      expect.objectContaining({ imageInput: false, oneMContext: false })
    );
    await act(async () => context.setQoderCapability('oneMContext', true));
    expect(api.applyModelToTool).toHaveBeenLastCalledWith(
      tool,
      expect.objectContaining({ imageInput: false, oneMContext: true })
    );
    expect(storage.get(`echobird_${tool}_image_input`)).toBe('false');
    expect(storage.get(`echobird_${tool}_1m_mode`)).toBe('true');
    expect(storage.has(`echobird_${other}_1m_mode`)).toBe(false);
    act(() => {
      context.setAgreedConfigPolicy(true);
      context.setLaunchAfterApply(false);
    });
    await act(async () => {
      await context.handleLaunch();
      await vi.advanceTimersByTimeAsync(3000);
    });
    expect(api.applyModelToTool).toHaveBeenLastCalledWith(
      tool,
      expect.objectContaining({ imageInput: false, oneMContext: true })
    );
    expect(api.startTool).toHaveBeenCalledTimes(1);
  });
  it('locks both controls while pending and reports failure without launching', async () => {
    await mount();
    act(() => context.handleSelectModel(tool, 'fixture'));
    let finish!: (value: { success: boolean; message: string }) => void;
    vi.mocked(api.applyModelToTool).mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      })
    );
    act(() => context.setQoderCapability('oneMContext', true));
    expect(context.isLaunching).toBe(true);
    act(() => context.setQoderCapability('imageInput', false));
    expect(context.qoderImageInput).toBe(true);
    expect(api.applyModelToTool).toHaveBeenCalledTimes(1);
    await act(async () => finish({ success: false, message: 'Fixture Qoder error' }));
    expect(context.applyError).toBe('Fixture Qoder error');
    expect(context.isLaunching).toBe(false);
    expect(api.startTool).not.toHaveBeenCalled();
  });
  it.each(['no-model', 'uninstalled', 'account'])(
    'only saves preferences with %s',
    async (mode) => {
      await mount();
      if (mode === 'uninstalled') {
        act(() => context.handleSelectModel(tool, 'fixture'));
        act(() =>
          useToolsStore
            .getState()
            .setDetectedTools([{ id: tool, name: tool, category: 'Desktop', installed: false }])
        );
      }
      if (mode === 'account') {
        act(() => context.handleSelectModel(tool, 'fixture'));
        await act(async () => context.qoderAccounts.select('fixture-account'));
        expect(context.qoderAccounts.selectedId).toBe('fixture-account');
        expect(context.toolModelConfig[tool]).toBeNull();
      }
      await act(async () => context.setQoderCapability('oneMContext', true));
      expect(storage.get(`echobird_${tool}_1m_mode`)).toBe('true');
      expect(api.applyModelToTool).not.toHaveBeenCalled();
      expect(api.startTool).not.toHaveBeenCalled();
      await act(async () => context.setSelectedTool('unrelated'));
      act(() => context.setQoderCapability('oneMContext', false));
      expect(storage.get(`echobird_${tool}_1m_mode`)).toBe('true');
    }
  );
});

describe.each(['dsh'] as const)('%s model capabilities', (tool) => {
  let renderer: ReactTestRenderer;
  let context: ReturnType<typeof useAppManager>;
  let storage: Map<string, string>;
  const other = 'qoder';
  function Harness() {
    const state = useAppManager();
    useLayoutEffect(() => {
      context = state;
    });
    return null;
  }
  beforeEach(() => {
    vi.clearAllMocks();
    storage = new Map();
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => storage.set(key, value),
    });
    useNavigationStore.getState().setActivePage('apps');
    useToolsStore.getState().setDetectedTools(
      [tool, other].map((id) => ({
        id,
        name: id,
        category: 'Desktop',
        installed: true,
        apiProtocol: ['openai'],
      }))
    );
    vi.mocked(api.getModels).mockResolvedValue([
      {
        internalId: 'fixture',
        name: 'Fixture',
        modelId: 'any/model',
        baseUrl: 'https://example.test/v1',
        apiKey: 'fixture',
      },
    ]);
    vi.mocked(api.applyModelToTool).mockResolvedValue({ success: true, message: 'ok' });
  });
  afterEach(() => {
    act(() => renderer?.unmount());
    useToolsStore.getState().setDetectedTools([]);
    vi.mocked(api.getModels).mockResolvedValue([]);
    vi.stubGlobal('localStorage', undefined);
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
    await act(async () => context.setSelectedTool(tool));
  };
  it('restores isolated values without applying on navigation or remount', async () => {
    storage.set(`echobird_${tool}_image_input`, 'false');
    storage.set(`echobird_${tool}_1m_mode`, 'true');
    await mount();
    expect(context.dshImageInput).toBe(false);
    expect(context.dsh1mMode).toBe(true);
    await act(async () => context.setSelectedTool(other));
    expect(context.qoderImageInput).toBe(true);
    expect(context.qoder1mMode).toBe(false);
    await act(async () => context.setSelectedTool(tool));
    act(() => useNavigationStore.getState().setActivePage('models'));
    act(() => useNavigationStore.getState().setActivePage('apps'));
    act(() => renderer.unmount());
    await mount();
    expect(context.dshImageInput).toBe(false);
    expect(context.dsh1mMode).toBe(true);
    expect(context.zcodeImageInput).toBe(true);
    expect(context.claude1mMode).toBe(false);
    expect(api.applyModelToTool).not.toHaveBeenCalled();
    expect(api.startTool).not.toHaveBeenCalled();
  });
  it('uses both saved values for explicit flips and the normal apply button', async () => {
    vi.useFakeTimers();
    await mount();
    act(() => context.handleSelectModel(tool, 'fixture'));
    await act(async () => context.setDshCapability('imageInput', false));
    expect(api.applyModelToTool).toHaveBeenLastCalledWith(
      tool,
      expect.objectContaining({ imageInput: false, oneMContext: false })
    );
    await act(async () => context.setDshCapability('oneMContext', true));
    expect(api.applyModelToTool).toHaveBeenLastCalledWith(
      tool,
      expect.objectContaining({ imageInput: false, oneMContext: true })
    );
    expect(storage.get(`echobird_${tool}_image_input`)).toBe('false');
    expect(storage.get(`echobird_${tool}_1m_mode`)).toBe('true');
    expect(storage.has(`echobird_${other}_1m_mode`)).toBe(false);
    act(() => {
      context.setAgreedConfigPolicy(true);
      context.setLaunchAfterApply(false);
    });
    await act(async () => {
      await context.handleLaunch();
      await vi.advanceTimersByTimeAsync(3000);
    });
    expect(api.applyModelToTool).toHaveBeenLastCalledWith(
      tool,
      expect.objectContaining({ imageInput: false, oneMContext: true })
    );
    expect(api.startTool).not.toHaveBeenCalled();
  });
  it('locks both controls while pending and reports failure without launching', async () => {
    await mount();
    act(() => context.handleSelectModel(tool, 'fixture'));
    let finish!: (value: { success: boolean; message: string }) => void;
    vi.mocked(api.applyModelToTool).mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      })
    );
    act(() => context.setDshCapability('oneMContext', true));
    expect(context.isLaunching).toBe(true);
    act(() => context.setDshCapability('imageInput', false));
    expect(context.dshImageInput).toBe(true);
    expect(api.applyModelToTool).toHaveBeenCalledTimes(1);
    await act(async () => finish({ success: false, message: 'Fixture Harness error' }));
    expect(context.applyError).toBe('Fixture Harness error');
    expect(context.isLaunching).toBe(false);
    expect(api.startTool).not.toHaveBeenCalled();
  });
  it.each(['no-model', 'uninstalled', 'account'])(
    'only saves preferences with %s',
    async (mode) => {
      await mount();
      if (mode === 'uninstalled') {
        act(() => context.handleSelectModel(tool, 'fixture'));
        act(() =>
          useToolsStore
            .getState()
            .setDetectedTools([{ id: tool, name: tool, category: 'Desktop', installed: false }])
        );
      }
      if (mode === 'account') {
        act(() => context.handleSelectModel(tool, 'fixture'));
        await act(async () => context.deepSeekAccounts.select('fixture-account'));
        expect(context.toolModelConfig[tool]).toBeNull();
        expect(context.deepSeekAccounts.selectedId).toBe('fixture-account');
      }
      await act(async () => context.setDshCapability('oneMContext', true));
      expect(storage.get(`echobird_${tool}_1m_mode`)).toBe('true');
      expect(api.applyModelToTool).not.toHaveBeenCalled();
      expect(api.startTool).not.toHaveBeenCalled();
      await act(async () => context.setSelectedTool('unrelated'));
      act(() => context.setDshCapability('oneMContext', false));
      expect(storage.get(`echobird_${tool}_1m_mode`)).toBe('true');
    }
  );
});
