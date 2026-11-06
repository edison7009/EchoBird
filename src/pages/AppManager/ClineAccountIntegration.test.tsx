import { useLayoutEffect } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ClineAccount } from '../../api/cline';
import { useToolsStore } from '../../stores/toolsStore';
import { useNavigationStore } from '../../stores/navigationStore';
import { AppManagerProvider } from './AppManagerProvider';
import { useAppManager } from './context';
import { ClineAccountSection } from './ClineAccountSection';
import { AccountSectionRow } from './AccountSectionPrimitives';
import { QuotaCountdown } from './QuotaCountdown';

const { invoke, openExternal, confirm, folderPicker } = vi.hoisted(() => ({
  invoke: vi.fn(),
  openExternal: vi.fn().mockResolvedValue(undefined),
  confirm: vi.fn().mockResolvedValue(true),
  folderPicker: vi.fn(),
}));
vi.hoisted(() => vi.stubGlobal('__APP_EDITION__', 'full'));
vi.mock('@tauri-apps/api/core', () => ({ invoke }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: folderPicker }));
vi.mock('../../api/tauri', async (original) => ({
  ...(await original<typeof import('../../api/tauri')>()),
  openExternal,
}));
vi.mock('../../hooks/useI18n', () => ({
  useI18n: () => ({ t: (key: string) => key, locale: 'en' }),
}));
vi.mock('../../components/ConfirmDialog', () => ({ useConfirm: () => confirm }));
vi.mock('../ModelNexus/context', () => {
  const userModels: never[] = [];
  return { useModelNexus: () => ({ userModels }) };
});
vi.mock('../FreeModels/provider', () => ({ useFreeModels: () => ({ routerEnabled: false }) }));

const account: ClineAccount = {
  id: 'saved',
  email: 'saved@example.test',
  name: 'Saved',
  active: true,
  plan: 'ClinePass',
  balanceMicros: 12_340_000,
  subscriptionEndAt: 1793000000,
};
let state: ReturnType<typeof useAppManager>;
let renderer: ReactTestRenderer;
let handlers: Map<string, (args: Record<string, unknown>) => unknown>;

function Harness() {
  const context = useAppManager();
  useLayoutEffect(() => {
    state = context;
  });
  return ['cline', 'clinedesktop'].includes(context.selectedTool ?? '') ? (
    <ClineAccountSection showDivider={false} />
  ) : null;
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

function calls(command: string) {
  return invoke.mock.calls.filter(([name]) => name === command);
}

async function tick() {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(1);
  });
}

async function mount(tool: 'cline' | 'clinedesktop', installed = true) {
  useToolsStore.getState().setDetectedTools(
    (['cline', 'clinedesktop'] as const).map((id) => ({
      id,
      name: id,
      category: id === 'cline' ? 'CLI Code' : 'Desktop',
      installed,
      activeModel: `${id}-api`,
      startCommand: id === 'cline' ? 'cline' : undefined,
      apiProtocol: ['openai', 'anthropic'],
    }))
  );
  await act(async () => {
    renderer = create(
      <AppManagerProvider>
        <Harness />
      </AppManagerProvider>
    );
  });
  act(() => state.setSelectedTool(tool));
  await tick();
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2026-10-14T00:00:00Z'));
  vi.clearAllMocks();
  folderPicker.mockResolvedValue('E:/fixture-project');
  const storage = new Map<string, string>();
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => storage.get(key) ?? null,
    setItem: (key: string, value: string) => void storage.set(key, value),
  });
  handlers = new Map<string, (args: Record<string, unknown>) => unknown>([
    ['get_models', () => []],
    ['get_tool_install_index', () => '{"ids":[]}'],
    ['list_cline_accounts', () => [account]],
    ['switch_cline_account', () => account],
  ]);
  invoke.mockImplementation(async (command: string, args?: Record<string, unknown>) =>
    handlers.get(command)?.(args ?? {})
  );
  useNavigationStore.setState({ activePage: 'apps' });
});

afterEach(() => {
  act(() => renderer?.unmount());
  useToolsStore.getState().setDetectedTools([]);
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('Cline shared account workflow', () => {
  it.each(['cline', 'clinedesktop'] as const)(
    '%s only reads saved metadata during passive navigation',
    async (tool) => {
      await mount(tool);
      expect(calls('list_cline_accounts')).toHaveLength(1);
      expect(state.clineAccounts.selectedId).toBe(account.id);
      expect(invoke.mock.calls.filter(([command]) => /cline/.test(command))).toEqual([
        ['list_cline_accounts'],
      ]);
      expect(openExternal).not.toHaveBeenCalled();
      expect(confirm).not.toHaveBeenCalled();
      expect(folderPicker).not.toHaveBeenCalled();
      expect(JSON.stringify(renderer.toJSON())).toContain('$12.34');
      expect(renderer.root.findByType(QuotaCountdown).props.resetAt).toBe(
        account.subscriptionEndAt
      );
    }
  );

  it('does not load accounts or start login for uninstalled clients', async () => {
    await mount('cline', false);
    await act(async () => {
      await state.clineAccounts.add();
    });
    expect(calls('list_cline_accounts')).toHaveLength(0);
    expect(calls('start_cline_login')).toHaveLength(0);
  });

  it.each([undefined, null, 0])(
    'renders missing Cline balance %s through the shared quota placeholder and preserves real zero',
    async (balanceMicros) => {
      handlers.set('list_cline_accounts', () => [{ ...account, balanceMicros }]);
      await mount('clinedesktop');
      const markup = JSON.stringify(renderer.toJSON());
      expect(markup).toContain(balanceMicros == null ? 'model.noUsageData' : '$0.00');
      expect(markup).not.toContain('progressbar');
      expect(renderer.root.findByProps({ role: 'radio' }).props.className).toContain('grid h-14');
      expect(calls('refresh_cline_account')).toHaveLength(0);
      expect(calls('switch_cline_account')).toHaveLength(0);
    }
  );

  it('Account Center loads the shared store once without requesting quota or account changes', async () => {
    await mount('cline');
    invoke.mockClear();
    act(() => useNavigationStore.getState().setActivePage('accounts'));
    await tick();
    expect(calls('list_cline_accounts')).toHaveLength(1);
    expect(invoke.mock.calls.filter(([command]) => /cline/.test(command))).toEqual([
      ['list_cline_accounts'],
    ]);
    expect(openExternal).not.toHaveBeenCalled();
    expect(folderPicker).not.toHaveBeenCalled();
    expect(confirm).not.toHaveBeenCalled();
  });

  it('shares selection across both tools and preserves rows when a passive reread fails', async () => {
    await mount('cline');
    act(() => state.handleSelectModel('cline', 'cli-api'));
    expect(state.clineAccounts.selectedId).toBeNull();
    act(() => state.handleSelectModel('clinedesktop', 'desktop-api'));
    act(() => state.clineAccounts.select(account.id));
    expect(state.toolModelConfig.cline).toBeNull();
    expect(state.toolModelConfig.clinedesktop).toBeNull();
    handlers.set('list_cline_accounts', () => Promise.reject(new Error('accountError.read')));
    act(() => state.setSelectedTool('clinedesktop'));
    await tick();
    expect(state.clineAccounts.accounts).toEqual([account]);
    expect(state.clineAccounts.selectedId).toBe(account.id);
    expect(state.applyError).toBeNull();
    expect(calls('refresh_cline_account')).toHaveLength(0);
    expect(calls('switch_cline_account')).toHaveLength(0);
  });

  it('shows the device code, cancels at 60 seconds and ignores a late approval', async () => {
    await mount('cline');
    const poll = deferred<ClineAccount | null>();
    handlers.set('start_cline_login', () => ({
      loginId: 'device-login',
      verificationUri: 'https://auth.example.test/device',
      userCode: 'ABCD-EFGH',
      expiresAt: Date.now() / 1000 + 300,
    }));
    handlers.set('poll_cline_login', () => poll.promise);
    let operation!: Promise<void>;
    await act(async () => {
      operation = state.clineAccounts.add();
    });
    expect(openExternal).toHaveBeenCalledWith('https://auth.example.test/device');
    expect(JSON.stringify(renderer.toJSON())).toContain('ABCD-EFGH');
    await act(async () => {
      await state.handleLaunch();
      await vi.advanceTimersByTimeAsync(60_000);
    });
    expect(calls('switch_cline_account')).toHaveLength(0);
    expect(calls('cancel_cline_login')[0][1]).toEqual({ loginId: 'device-login' });
    expect(state.clineAccounts.busy).toBe(false);
    expect(state.applyError).toBe('accountError.expired');
    await act(async () => {
      poll.resolve({ ...account, id: 'late' });
      await operation;
    });
    expect(state.clineAccounts.accounts).toEqual([account]);
  });

  it.each(['cline', 'clinedesktop'] as const)(
    '%s applies only on an explicit action and honors the launch preference',
    async (tool) => {
      await mount(tool);
      act(() => state.setLaunchAfterApply(false));
      await act(async () => {
        await state.handleLaunch();
      });
      expect(calls('switch_cline_account')[0][1]).toEqual({ accountId: account.id, toolId: tool });
      expect(calls('apply_model_to_tool')).toHaveLength(0);
      expect(calls('restore_tool_to_official')).toHaveLength(0);
      expect(calls('start_tool')).toHaveLength(0);
      expect(folderPicker).not.toHaveBeenCalled();
      expect(state.detectedTools.every((entry) => entry.activeModel === '')).toBe(true);
      expect(state.toolModelConfig.cline).toBeNull();
      expect(state.toolModelConfig.clinedesktop).toBeNull();
      act(() => state.setLaunchAfterApply(true));
      await act(async () => {
        await state.handleLaunch();
      });
      expect(calls('start_tool')[0][1]).toEqual({
        toolId: tool,
        startCommand: tool === 'cline' ? 'cline' : null,
        cwd: tool === 'cline' ? 'E:/fixture-project' : null,
      });
      expect(folderPicker).toHaveBeenCalledTimes(tool === 'cline' ? 1 : 0);
      expect(state.isLaunching).toBe(false);
    }
  );

  it('cancelling the CLI folder picker prevents both account writes and launch', async () => {
    await mount('cline');
    folderPicker.mockResolvedValue(null);
    await act(async () => {
      await state.handleLaunch();
    });
    expect(calls('switch_cline_account')).toHaveLength(0);
    expect(calls('start_tool')).toHaveLength(0);
    expect(state.isLaunching).toBe(false);
  });

  it('does not clear a new API selection or launch after an earlier account apply completes', async () => {
    await mount('clinedesktop');
    const pending = deferred<ClineAccount>();
    handlers.set('switch_cline_account', () => pending.promise);
    let operation!: Promise<void>;
    await act(async () => {
      operation = state.handleLaunch();
    });
    act(() => state.handleSelectModel('clinedesktop', 'new-api'));
    await act(async () => {
      pending.resolve(account);
      await operation;
    });
    expect(state.toolModelConfig.clinedesktop).toBe('new-api');
    expect(state.clineAccounts.selectedId).toBeNull();
    expect(calls('start_tool')).toHaveLength(0);
    expect(state.isLaunching).toBe(false);
  });

  it('leaving and returning while an account apply is pending does not resume its launch', async () => {
    await mount('clinedesktop');
    const pending = deferred<ClineAccount>();
    handlers.set('switch_cline_account', () => pending.promise);
    let operation!: Promise<void>;
    await act(async () => {
      operation = state.handleLaunch();
    });
    act(() => useNavigationStore.getState().setActivePage('models'));
    await tick();
    act(() => useNavigationStore.getState().setActivePage('apps'));
    await tick();
    await act(async () => {
      pending.resolve(account);
      await operation;
    });
    expect(calls('switch_cline_account')).toHaveLength(1);
    expect(calls('start_tool')).toHaveLength(0);
    expect(state.isLaunching).toBe(false);
    expect(state.applyError).toBeNull();
  });

  it('abandons an old folder choice after changing tools, and exposes explicit apply failures', async () => {
    await mount('cline');
    const folder = deferred<string>();
    folderPicker.mockReturnValue(folder.promise);
    let operation!: Promise<void>;
    await act(async () => {
      operation = state.handleLaunch();
    });
    act(() => state.setSelectedTool('clinedesktop'));
    await tick();
    await act(async () => {
      folder.resolve('E:/old-choice');
      await operation;
    });
    expect(calls('switch_cline_account')).toHaveLength(0);
    handlers.set('switch_cline_account', () =>
      Promise.reject(new Error('accountError.closeClient'))
    );
    await act(async () => {
      await state.handleLaunch();
    });
    expect(state.applyError).toBe('accountError.closeClient');
    expect(calls('start_tool')).toHaveLength(0);
    expect(state.clineAccounts.accounts).toEqual([account]);
  });

  it('nested refresh and delete controls never select their account row', async () => {
    const second = { ...account, id: 'second', email: 'second@example.test', active: false };
    handlers.set('list_cline_accounts', () => [account, second]);
    handlers.set('refresh_cline_account', () => ({ ...second, balanceMicros: 0 }));
    await mount('cline');
    const row = renderer.root.findAllByType(AccountSectionRow)[1];
    const stopPropagation = vi.fn();
    const refresh = row.findByProps({ 'aria-label': 'agent.refreshAccount second@example.test' });
    await act(async () => {
      refresh.props.onClick({ stopPropagation });
    });
    expect(stopPropagation).toHaveBeenCalled();
    expect(state.clineAccounts.selectedId).toBe(account.id);
    expect(state.clineAccounts.accounts[1].balanceMicros).toBe(0);
    await act(async () => {
      row
        .findByProps({ 'aria-label': 'btn.delete second@example.test' })
        .props.onClick({ stopPropagation });
    });
    expect(confirm).toHaveBeenCalled();
    expect(calls('delete_cline_account')[0][1]).toEqual({ accountId: second.id });
    expect(state.clineAccounts.selectedId).toBe(account.id);
  });
});
