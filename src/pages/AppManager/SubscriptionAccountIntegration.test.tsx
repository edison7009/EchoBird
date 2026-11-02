import { useLayoutEffect } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { AppManagerProvider } from './AppManagerProvider';
import { useAppManager } from './context';
import { MinimaxAccountSection, MinimaxAccountRegionChoice } from './MinimaxAccountSection';
import { QoderAccountSection } from './QoderAccountSection';
import { AccountSectionRewardButton, AccountSectionRow } from './AccountSectionPrimitives';
import { QuotaCountdown } from './QuotaCountdown';
import { useToolsStore } from '../../stores/toolsStore';
import { useNavigationStore } from '../../stores/navigationStore';
import { AccountCenterTitleActions } from '../AccountCenter/AccountCenter';
import type { MinimaxAccount, QoderAccount } from '../../api/tauri';

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
vi.mock('./ClaudeCodeLoginDialog', () => ({ ClaudeCodeLoginDialog: () => null }));

type Tool = 'minimaxcode' | 'minimaxdesktop' | 'qoder' | 'qodercn';
const tools: Tool[] = ['minimaxcode', 'minimaxdesktop', 'qoder', 'qodercn'];
const mini: MinimaxAccount = {
  id: 'mini-cn',
  email: 'same@example.test',
  region: 'cn',
  active: true,
  plan: 'Pro',
  quota: { credits: 20, windows: [] },
};
const miniGlobal: MinimaxAccount = { ...mini, id: 'mini-global', region: 'global', active: false };
const qoder = (edition: 'qoder' | 'qodercn'): QoderAccount => ({
  id: 'same-id',
  edition,
  name: edition,
  email: null,
  plan: null,
  active: true,
  remaining: null,
  total: null,
  expiresAt: null,
  quotaWindows: [],
});
let state: ReturnType<typeof useAppManager>;
let renderer: ReactTestRenderer;
let handlers: Map<string, (args: Record<string, unknown>) => unknown>;
function Harness() {
  const context = useAppManager();
  useLayoutEffect(() => {
    state = context;
  });
  return (
    <>
      {context.selectedTool?.startsWith('minimax') ? (
        <MinimaxAccountSection />
      ) : (
        <QoderAccountSection />
      )}
      <AccountCenterTitleActions />
    </>
  );
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
async function tick() {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(1);
  });
}
async function mount(tool: Tool, installed = true) {
  useToolsStore.getState().setDetectedTools(
    tools.map((id) => ({
      id,
      name: id,
      category: id === 'minimaxcode' ? 'CLI Code' : 'Desktop',
      installed: installed && id === tool,
      noModelConfig: false,
      launchFile: 'fixture',
    }))
  );
  await act(async () => {
    renderer = create(
      <AppManagerProvider>
        <Harness />
      </AppManagerProvider>
    );
  });
  act(() => {
    state.setSelectedTool(tool);
  });
  await tick();
}
function group(tool: Tool) {
  return tool.startsWith('minimax') ? state.minimaxAccounts : state.qoderAccounts;
}
function calls(command: string) {
  return invoke.mock.calls.filter(([name]) => name === command);
}
function giftButton() {
  return renderer.root
    .findAllByType('button')
    .find((button) =>
      /agent\.(claimDailyCredits|dailyCreditsClaimed)/.test(button.props['aria-label'])
    )!;
}
beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2026-10-13T00:00:00Z'));
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
    ['list_minimax_accounts', () => [mini, miniGlobal]],
    ['list_qoder_accounts', (args) => [qoder(args.edition as 'qoder' | 'qodercn')]],
  ]);
  invoke.mockImplementation(async (command: string, args?: Record<string, unknown>) =>
    handlers.get(command)?.(args ?? {})
  );
  useNavigationStore.setState({ activePage: 'apps' });
});
afterEach(() => {
  act(() => renderer?.unmount());
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

it.each(tools)('%s: passive navigation reads only its installed account group', async (tool) => {
  await mount(tool);
  const minimax = tool.startsWith('minimax');
  const read = minimax ? 'list_minimax_accounts' : 'list_qoder_accounts';
  expect(calls(read)).toHaveLength(1);
  if (!minimax)
    expect(calls(read)[0][1]).toEqual({
      edition: tool.startsWith('qodercn') ? 'qodercn' : 'qoder',
    });
  expect(group(tool).accounts).toHaveLength(minimax ? 2 : 1);
  expect(group(tool).selectedId).toBe(minimax ? mini.id : 'same-id');
  expect(
    invoke.mock.calls.some(([cmd]) =>
      /^(start_.*login|refresh_.*account|switch_.*account|claim_.*credits)$/.test(cmd)
    )
  ).toBe(false);
  expect(openExternal).not.toHaveBeenCalled();
  expect(confirm).not.toHaveBeenCalled();
});

it.each(tools)(
  '%s: an uninstalled client never reads or starts account operations',
  async (tool) => {
    await mount(tool, false);
    await act(async () => {
      await group(tool).add();
    });
    expect(calls('list_minimax_accounts')).toHaveLength(0);
    expect(calls('list_qoder_accounts')).toHaveLength(0);
    expect(calls('start_minimax_login')).toHaveLength(0);
    expect(calls('start_qoder_login')).toHaveLength(0);
  }
);

it('MiniMax keeps both regions and one selection across CLI/Desktop, with explicit model exclusivity', async () => {
  await mount('minimaxcode');
  act(() => state.minimaxAccounts.select(miniGlobal.id));
  act(() => {
    useToolsStore.getState().setDetectedTools(
      tools.map((id) => ({
        id,
        name: id,
        category: 'Desktop',
        installed: id.startsWith('minimax'),
      }))
    );
    state.setSelectedTool('minimaxdesktop');
  });
  await tick();
  expect(state.minimaxAccounts.selectedId).toBe(miniGlobal.id);
  expect(state.minimaxAccounts.accounts.map((a) => a.region)).toEqual(['cn', 'global']);
  act(() => state.handleSelectModel('minimaxdesktop', 'api-model'));
  expect(state.minimaxAccounts.selectedId).toBeNull();
  act(() => state.minimaxAccounts.select(mini.id));
  expect(state.toolModelConfig.minimaxcode).toBeNull();
  expect(state.toolModelConfig.minimaxdesktop).toBeNull();
});

it('Qoder editions retain separate rows and cached selection after a passive failure', async () => {
  await mount('qoder');
  act(() => {
    useToolsStore
      .getState()
      .setDetectedTools(
        tools.map((id) => ({ id, name: id, category: 'Desktop', installed: true }))
      );
    state.setSelectedTool('qodercn');
  });
  await tick();
  expect(state.qoderAccounts.accounts[0].edition).toBe('qodercn');
  handlers.set('list_qoder_accounts', () => Promise.reject(new Error('accountError.read')));
  act(() => state.setSelectedTool('qoder'));
  await tick();
  expect(state.qoderAccounts.accounts[0].edition).toBe('qoder');
  expect(state.applyError).toBeNull();
  expect(calls('refresh_qoder_account_quota')).toHaveLength(0);
  expect(confirm).not.toHaveBeenCalled();
});

it.each(['qoder', 'qodercn'] as const)(
  '%s: selecting an API model and an account are mutually exclusive',
  async (tool) => {
    await mount(tool);
    act(() => state.handleSelectModel(tool, 'api-model'));
    expect(state.qoderAccounts.selectedId).toBeNull();
    expect(state.toolModelConfig[tool]).toBe('api-model');
    act(() => state.qoderAccounts.select('same-id'));
    expect(state.toolModelConfig[tool]).toBeNull();
    expect(state.qoderAccounts.selectedId).toBe('same-id');
    expect(calls('switch_qoder_account')).toHaveLength(0);
    expect(calls('apply_model_to_tool')).toHaveLength(0);
  }
);

it('MiniMax login uses the selected region and cannot change it while authorization is pending', async () => {
  await mount('minimaxcode');
  const poll = deferred<MinimaxAccount | null>();
  handlers.set('start_minimax_login', () => ({
    loginId: 'login',
    verificationUri: 'https://vendor.example/authorize?code=abc',
    userCode: 'abc',
    expiresAt: Date.now() / 1000 + 60,
  }));
  handlers.set('poll_minimax_login', () => poll.promise);
  act(() => state.minimaxAccounts.setRegion('global'));
  let task!: Promise<void>;
  await act(async () => {
    task = state.minimaxAccounts.add();
  });
  expect(calls('start_minimax_login')[0][1]).toEqual({ region: 'global' });
  const choice = renderer.root.findByType(MinimaxAccountRegionChoice).findByType('button');
  expect(choice.props.disabled).toBe(true);
  expect(JSON.stringify(renderer.toJSON())).toContain('abc');
  act(() => state.minimaxAccounts.setRegion('cn'));
  expect(state.minimaxAccounts.region).toBe('global');
  act(() => state.minimaxAccounts.cancelLogin());
  await act(async () => {
    poll.resolve(miniGlobal);
    await task;
  });
  expect(state.minimaxAccounts.selectedId).toBe(mini.id);
  expect(calls('cancel_minimax_login')).toHaveLength(1);
  expect(calls('refresh_minimax_account')).toHaveLength(0);
});

it.each(['minimaxcode', 'qoder', 'qodercn'] as const)(
  '%s: navigation cancels login and ignores late success',
  async (tool) => {
    await mount(tool);
    const miniProvider = tool === 'minimaxcode';
    const poll = deferred<MinimaxAccount | QoderAccount | null>();
    handlers.set(miniProvider ? 'start_minimax_login' : 'start_qoder_login', () => ({
      loginId: 'late-login',
      verificationUri: 'https://vendor.example/authorize',
      userCode: '',
      expiresAt: Date.now() / 1000 + 60,
    }));
    handlers.set(miniProvider ? 'poll_minimax_login' : 'poll_qoder_login', () => poll.promise);
    let task!: Promise<void>;
    await act(async () => {
      task = group(tool).add();
    });
    act(() => useNavigationStore.getState().setActivePage('freeModels'));
    await tick();
    await act(async () => {
      poll.resolve(miniProvider ? { ...mini, id: 'late' } : { ...qoder('qoder'), id: 'late' });
      await task;
    });
    expect(calls(miniProvider ? 'cancel_minimax_login' : 'cancel_qoder_login')).toHaveLength(1);
    expect(group(tool).accounts.some((a) => a.id === 'late')).toBe(false);
    expect(group(tool).busy).toBe(false);
  }
);

it.each(['minimaxcode', 'qoder', 'qodercn'] as const)(
  '%s: refresh/delete actions keep selection and failures visible',
  async (tool) => {
    await mount(tool);
    const miniProvider = tool === 'minimaxcode';
    handlers.set(miniProvider ? 'refresh_minimax_account' : 'refresh_qoder_account_quota', () =>
      Promise.reject(new Error('accountError.network'))
    );
    act(() => {
      if (miniProvider) state.minimaxAccounts.select(miniGlobal.id);
      else state.qoderAccounts.select(null);
    });
    const selected = group(tool).selectedId;
    const row = renderer.root.findAllByType(AccountSectionRow)[0];
    const button = row
      .findAllByType('button')
      .find((b) => b.props['aria-label']?.startsWith('agent.refreshAccount'))!;
    const stopPropagation = vi.fn();
    await act(async () => {
      button.props.onClick({ stopPropagation });
    });
    expect(stopPropagation).toHaveBeenCalled();
    expect(group(tool).selectedId).toBe(selected);
    expect(state.applyError).toBe('accountError.network');
    expect(calls(miniProvider ? 'switch_minimax_account' : 'switch_qoder_account')).toHaveLength(0);
  }
);

it.each(tools)('%s: account apply is explicit and a failed switch never launches', async (tool) => {
  await mount(tool);
  const miniProvider = tool.startsWith('minimax');
  const switchCommand = miniProvider ? 'switch_minimax_account' : 'switch_qoder_account';
  const expected = miniProvider
    ? { accountId: mini.id, toolId: tool }
    : {
        edition: tool.startsWith('qodercn') ? 'qodercn' : 'qoder',
        accountId: 'same-id',
        toolId: tool,
      };
  handlers.set(switchCommand, () => Promise.reject(new Error('accountError.write')));
  await act(async () => {
    await state.handleLaunch();
  });
  expect(calls(switchCommand)[0][1]).toEqual(expected);
  expect(calls('start_tool')).toHaveLength(0);
  expect(calls('apply_model_to_tool')).toHaveLength(0);
  expect(state.applyError).toBe('accountError.write');
  expect(group(tool).selectedId).toBe(miniProvider ? mini.id : 'same-id');
});

it.each(['minimaxcode', 'qoder', 'qodercn'] as const)(
  '%s: login expires at 60 seconds without quota or apply calls',
  async (tool) => {
    await mount(tool);
    const minimax = tool === 'minimaxcode';
    handlers.set(minimax ? 'start_minimax_login' : 'start_qoder_login', () => ({
      loginId: 'deadline',
      verificationUri: 'https://vendor.example/approve',
      userCode: 'code',
      expiresAt: Date.now() / 1000 + 600,
    }));
    handlers.set(minimax ? 'poll_minimax_login' : 'poll_qoder_login', () => null);
    let task!: Promise<void>;
    await act(async () => {
      task = group(tool).add();
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_001);
      await task;
    });
    expect(calls(minimax ? 'cancel_minimax_login' : 'cancel_qoder_login')).toHaveLength(1);
    expect(group(tool).busy).toBe(false);
    expect(state.applyError).toBe('accountError.expired');
    expect(calls(minimax ? 'refresh_minimax_account' : 'refresh_qoder_account_quota')).toHaveLength(
      0
    );
    expect(calls(minimax ? 'switch_minimax_account' : 'switch_qoder_account')).toHaveLength(0);
  }
);

it.each(['minimaxcode', 'qoder', 'qodercn'] as const)(
  '%s: successful login saves and selects an account without native apply or quota reads',
  async (tool) => {
    await mount(tool);
    const minimax = tool === 'minimaxcode';
    const added = minimax
      ? { ...miniGlobal, id: 'added', active: false }
      : { ...qoder(tool === 'qodercn' ? 'qodercn' : 'qoder'), id: 'added', active: false };
    handlers.set(minimax ? 'start_minimax_login' : 'start_qoder_login', () => ({
      loginId: 'new',
      verificationUri: 'https://vendor.example/approve',
      userCode: 'code',
      expiresAt: Date.now() / 1000 + 60,
    }));
    handlers.set(minimax ? 'poll_minimax_login' : 'poll_qoder_login', () => added);
    handlers.set(minimax ? 'list_minimax_accounts' : 'list_qoder_accounts', () => [added]);
    await act(async () => {
      await group(tool).add();
    });
    expect(group(tool).accounts).toEqual([added]);
    expect(group(tool).selectedId).toBe('added');
    expect(calls(minimax ? 'switch_minimax_account' : 'switch_qoder_account')).toHaveLength(0);
    expect(calls(minimax ? 'refresh_minimax_account' : 'refresh_qoder_account_quota')).toHaveLength(
      0
    );
  }
);

it.each(tools)(
  '%s: successful explicit apply forwards the native client ID and follows launch preference',
  async (tool) => {
    await mount(tool);
    const minimax = tool.startsWith('minimax');
    act(() => state.setLaunchAfterApply(false));
    handlers.set(minimax ? 'switch_minimax_account' : 'switch_qoder_account', () => undefined);
    await act(async () => {
      await state.handleLaunch();
    });
    const applied = calls(minimax ? 'switch_minimax_account' : 'switch_qoder_account');
    expect(applied).toHaveLength(1);
    expect(applied[0][1].toolId).toBe(tool);
    expect(calls('start_tool')).toHaveLength(0);
    expect(calls('apply_model_to_tool')).toHaveLength(0);
    expect(state.isLaunching).toBe(false);
  }
);

it('MiniMax CLI selects the common working directory before applying an account', async () => {
  await mount('minimaxcode');
  handlers.set('switch_minimax_account', () => {
    expect(folderPicker).toHaveBeenCalledWith({ directory: true, multiple: false });
  });
  await act(async () => {
    await state.handleLaunch();
  });
  expect(folderPicker).toHaveBeenCalledTimes(1);
  expect(calls('switch_minimax_account')).toHaveLength(1);
  expect(calls('start_tool')).toHaveLength(1);
  expect(calls('start_tool')[0][1]).toMatchObject({
    toolId: 'minimaxcode',
    cwd: 'E:/fixture-project',
  });
  expect(state.isLaunching).toBe(false);
});

it('a fixed launch folder skips the picker and leaves the folder to the backend', async () => {
  await mount('minimaxcode');
  handlers.set('get_cli_launch_settings', () => ({
    minimaxcode: { preset: '', directoryMode: 'fixed', directory: 'D:/work', terminal: 'auto' },
  }));
  await act(async () => {
    await state.handleLaunch();
  });
  expect(folderPicker).not.toHaveBeenCalled();
  expect(calls('switch_minimax_account')).toHaveLength(1);
  expect(calls('start_tool')[0][1]).toMatchObject({ toolId: 'minimaxcode', cwd: null });
});

it('unreadable launch settings fall back to asking for a folder', async () => {
  await mount('minimaxcode');
  handlers.set('get_cli_launch_settings', () => {
    throw new Error('unreadable');
  });
  await act(async () => {
    await state.handleLaunch();
  });
  expect(folderPicker).toHaveBeenCalledTimes(1);
  expect(calls('start_tool')[0][1]).toMatchObject({ cwd: 'E:/fixture-project' });
});

it('MiniMax CLI cancellation prevents both account apply and launch', async () => {
  await mount('minimaxcode');
  folderPicker.mockResolvedValue(null);
  await act(async () => {
    await state.handleLaunch();
  });
  expect(folderPicker).toHaveBeenCalledTimes(1);
  expect(calls('switch_minimax_account')).toHaveLength(0);
  expect(calls('start_tool')).toHaveLength(0);
  expect(calls('apply_model_to_tool')).toHaveLength(0);
  expect(state.minimaxAccounts.selectedId).toBe(mini.id);
  expect(state.isLaunching).toBe(false);
});

it('MiniMax CLI working directory failure is visible without applying an account', async () => {
  await mount('minimaxcode');
  folderPicker.mockRejectedValue(new Error('folder unavailable'));
  await act(async () => {
    await state.handleLaunch();
  });
  expect(calls('switch_minimax_account')).toHaveLength(0);
  expect(calls('start_tool')).toHaveLength(0);
  expect(state.applyError).toBe('accountError.failed');
  expect(state.isLaunching).toBe(false);
});

it.each(['minimaxdesktop', 'qoder', 'qodercn'] as const)(
  '%s: desktop account apply and launch never requests a working directory',
  async (tool) => {
    await mount(tool);
    await act(async () => {
      await state.handleLaunch();
    });
    expect(folderPicker).not.toHaveBeenCalled();
    expect(
      calls(tool === 'minimaxdesktop' ? 'switch_minimax_account' : 'switch_qoder_account')
    ).toHaveLength(1);
    expect(calls('start_tool')).toHaveLength(1);
    expect(calls('start_tool')[0][1].cwd).toBeNull();
  }
);

it('displays a cached Qoder trial account without a fake countdown or automatic refresh', async () => {
  const cached: QoderAccount = {
    ...qoder('qodercn'),
    plan: 'personal_standard',
    expiresAt: 253402214400,
    remaining: 300,
    total: 300,
    quotaWindows: [
      { name: 'Credits', remaining: 0, total: 0 },
      { name: 'Add-on', remaining: 300, total: 300 },
    ],
  };
  handlers.set('list_qoder_accounts', () => [cached]);
  await mount('qodercn');
  const row = renderer.root.findAllByType(AccountSectionRow)[0];
  expect(row.props.plan).toBe('agent.qoderTrial');
  expect(row.props.planPrefix).toBeUndefined();
  expect(renderer.root.findAllByType(QuotaCountdown)).toHaveLength(0);
  expect(calls('refresh_qoder_account_quota')).toHaveLength(0);
  expect(calls('start_qoder_login')).toHaveLength(0);
  expect(calls('switch_qoder_account')).toHaveLength(0);
  expect(group('qodercn').accounts[0].expiresAt).toBe(253402214400);
});

it('places a paid Qoder subscription beside its plan and keeps a single bucket date independent', async () => {
  const future = Math.floor(Date.now() / 1000) + 86400;
  handlers.set('list_qoder_accounts', () => [
    {
      ...qoder('qoder'),
      plan: 'personal_professional',
      expiresAt: future + 86400,
      remaining: 25,
      total: 100,
      quotaWindows: [{ name: 'Credits', remaining: 25, total: 100, expiresAt: future }],
    },
  ]);
  await mount('qoder');
  const row = renderer.root.findAllByType(AccountSectionRow)[0];
  expect(row.props.plan).toBe('Pro');
  expect(row.props.planPrefix.props.resetAt).toBe(future + 86400);
  expect(row.props.planPrefix.props.label).toBe('accountCenter.subscription');
  expect(
    renderer.root
      .findAllByType(QuotaCountdown)
      .map((countdown) => countdown.props.resetAt)
      .sort((a, b) => a - b)
  ).toEqual([future, future + 86400]);
  expect(calls('refresh_qoder_account_quota')).toHaveLength(0);
});

it.each(['qoder', 'qodercn'] as const)(
  '%s: an unknown gift is rechecked explicitly, with selection and refresh isolation',
  async (edition) => {
    await mount(edition);
    act(() => state.qoderAccounts.select(null));
    const claim = deferred<QoderAccount>();
    handlers.set('claim_qoder_daily_credits', () => claim.promise);
    const stopPropagation = vi.fn();
    await act(async () => giftButton().props.onClick({ stopPropagation }));
    expect(stopPropagation).toHaveBeenCalledOnce();
    expect(calls('claim_qoder_daily_credits')[0][1]).toEqual({ edition, accountId: 'same-id' });
    expect(state.qoderAccounts.selectedId).toBeNull();
    expect(giftButton().props.disabled).toBe(true);
    await act(async () => {
      await state.qoderAccounts.claimDaily(state.qoderAccounts.accounts[0]);
      await state.qoderAccounts.refresh(state.qoderAccounts.accounts[0]);
    });
    expect(calls('claim_qoder_daily_credits')).toHaveLength(1);
    expect(calls('refresh_qoder_account_quota')).toHaveLength(0);
    await act(async () => {
      claim.resolve({
        ...qoder(edition),
        remaining: 100,
        dailyReward: {
          status: 'claimed',
          checkedAt: Date.now() / 1000,
          resetAt: Date.now() / 1000 + 7200,
        },
      });
    });
    expect(state.qoderAccounts.accounts[0].remaining).toBe(100);
    expect(giftButton().props['aria-label']).toBe(`agent.dailyCreditsClaimed ${edition}`);
    expect(giftButton().props.disabled).toBe(true);
    await act(async () => {
      await state.qoderAccounts.claimDaily(qoder(edition));
    });
    expect(calls('claim_qoder_daily_credits')).toHaveLength(1);
    expect(calls('switch_qoder_account')).toHaveLength(0);
    expect(calls('start_qoder_login')).toHaveLength(0);
    expect(state.qoderAccounts.selectedId).toBeNull();
  }
);

it.each(['claimed', 'unavailable'] as const)(
  'a confirmed %s gift is disabled without a new status or quota request',
  async (status) => {
    handlers.set('list_qoder_accounts', () => [
      {
        ...qoder('qoder'),
        dailyReward: { status, checkedAt: Date.now() / 1000, resetAt: Date.now() / 1000 + 7200 },
      },
    ]);
    await mount('qoder');
    expect(giftButton().props.disabled).toBe(true);
    expect(giftButton().props['aria-label']).toBe(
      `${status === 'claimed' ? 'agent.dailyCreditsClaimed' : 'agent.claimDailyCredits'} qoder`
    );
    await act(async () => {
      await state.qoderAccounts.claimDaily(state.qoderAccounts.accounts[0]);
    });
    expect(calls('claim_qoder_daily_credits')).toHaveLength(0);
    expect(calls('refresh_qoder_account_quota')).toHaveLength(0);
  }
);

it('locally expires the gift check at 10:00 UTC+8 without assuming eligibility or querying status', async () => {
  vi.setSystemTime(new Date('2026-10-13T01:59:30Z'));
  const resetAt = Date.parse('2026-10-13T02:00:00Z') / 1000;
  handlers.set('list_qoder_accounts', () => [
    {
      ...qoder('qodercn'),
      dailyReward: { status: 'claimed', checkedAt: Date.now() / 1000, resetAt },
    },
  ]);
  await mount('qodercn');
  expect(giftButton().props.disabled).toBe(true);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_000);
  });
  expect(giftButton().props['aria-label']).toBe('agent.claimDailyCredits qodercn');
  expect(giftButton().props.disabled).toBe(false);
  expect(calls('claim_qoder_daily_credits')).toHaveLength(0);
  expect(calls('refresh_qoder_account_quota')).toHaveLength(0);
  expect(state.qoderAccounts.accounts[0].dailyReward?.status).toBe('claimed');
});

it('keeps the previous gift status and displays an explicit claim failure', async () => {
  await mount('qoder');
  handlers.set('claim_qoder_daily_credits', () =>
    Promise.reject(new Error('accountError.network'))
  );
  await act(async () => {
    await state.qoderAccounts.claimDaily(state.qoderAccounts.accounts[0]);
  });
  expect(state.applyError).toBe('accountError.network');
  expect(state.qoderAccounts.accounts[0].dailyReward).toBeUndefined();
  expect(state.qoderAccounts.refreshing.size).toBe(0);
  expect(giftButton().props.disabled).toBe(false);
  expect(calls('refresh_qoder_account_quota')).toHaveLength(0);
});

it('ignores late claim results after leaving the account tool', async () => {
  await mount('qoder');
  const claim = deferred<QoderAccount>();
  handlers.set('claim_qoder_daily_credits', () => claim.promise);
  let pending!: ReturnType<typeof state.qoderAccounts.claimDaily>;
  await act(async () => {
    pending = state.qoderAccounts.claimDaily(state.qoderAccounts.accounts[0]);
  });
  act(() => useNavigationStore.getState().setActivePage('freeModels'));
  await tick();
  await act(async () => {
    claim.resolve({ ...qoder('qoder'), remaining: 100 });
    await pending;
  });
  expect(state.qoderAccountGroups.qoder.accounts[0].remaining).toBeNull();
  expect(state.qoderAccountGroups.qoder.refreshing.size).toBe(0);
  expect(calls('refresh_qoder_account_quota')).toHaveLength(0);
});

it('batch-claims eligible Qoder accounts independently and skips confirmed unavailable rows', async () => {
  const now = Date.now() / 1000;
  handlers.set('list_qoder_accounts', () =>
    ['claimable', 'claimable', 'claimed', 'unavailable'].map((status, index) => ({
      ...qoder('qodercn'),
      id: String(index),
      dailyReward: { status, checkedAt: now, resetAt: now + 7200 },
    }))
  );
  handlers.set('claim_qoder_daily_credits', (args) => ({
    ...qoder('qodercn'),
    id: args.accountId,
    dailyReward: { status: 'claimed', checkedAt: now, resetAt: now + 7200 },
  }));
  await mount('qodercn');
  act(() => useNavigationStore.getState().setActivePage('accounts'));
  await tick();
  expect(calls('claim_qoder_daily_credits')).toHaveLength(0);
  await act(async () => {
    await renderer.root.findByProps({ 'aria-label': 'accountCenter.claimAll' }).props.onClick();
  });
  expect(calls('claim_qoder_daily_credits').map((call) => call[1])).toEqual([
    { edition: 'qodercn', accountId: '0' },
    { edition: 'qodercn', accountId: '1' },
  ]);
  expect(calls('refresh_qoder_account_quota')).toHaveLength(0);
  expect(calls('switch_qoder_account')).toHaveLength(0);
});

it.each(['Free', 'Go', 'Explore', 'Build', 'custom plan'])(
  'shows the MiniMax %s tier without changing cached plan or quota',
  async (tier) => {
    const plan = tier === 'custom plan' ? tier : `M Plan ${tier}`;
    handlers.set('list_minimax_accounts', () => [{ ...mini, plan }]);
    await mount('minimaxdesktop');
    expect(
      renderer.root
        .findByType(AccountSectionRow)
        .findAllByType('span')
        .some((span) => span.children.includes(tier))
    ).toBe(true);
    expect(state.minimaxAccounts.accounts[0].plan).toBe(plan);
    expect(state.minimaxAccounts.accounts[0].quota?.credits).toBe(20);
    expect(calls('claim_minimax_daily_credits')).toHaveLength(0);
    expect(calls('refresh_minimax_account')).toHaveLength(0);
  }
);

it('renders MiniMax categorized credits without changing selection or fetching quota on entry', async () => {
  const expiry = Math.floor(Date.now() / 1000) + 15 * 86400;
  const purchasedExpiry = expiry + 86400;
  handlers.set('list_minimax_accounts', () => [
    {
      ...mini,
      quota: {
        credits: 1500,
        purchasedCredits: 1100,
        grantedCredits: 400,
        purchasedExpiresAt: purchasedExpiry,
        grantedExpiresAt: expiry,
        windows: [],
      },
    },
  ]);
  await mount('minimaxdesktop');
  const row = renderer.root.findAllByType(AccountSectionRow)[0];
  const text = renderToStaticMarkup(row.props.secondary);
  expect(text).toContain('agent.minimaxPurchasedCredits');
  expect(text).toContain('agent.grantedCredits');
  expect(text).toContain('1,100');
  expect(text).toContain('400');
  expect(text).not.toContain('1,500');
  expect(text.match(/lucide-gift/g)).toHaveLength(1);
  expect(text).toMatch(/aria-label="agent.grantedCredits">.*?lucide-gift.*?<\/svg>400<\/span>/);
  expect(text).toContain('aria-label="agent.minimaxPurchasedCredits">1,100</span>');
  expect(row.findByType(AccountSectionRewardButton).props.claimed).toBe(false);
  expect(renderer.root.findAllByType(QuotaCountdown).map((item) => item.props.resetAt)).toEqual([
    purchasedExpiry,
    expiry,
  ]);
  expect(calls('refresh_minimax_account')).toHaveLength(0);
  expect(calls('claim_minimax_daily_credits')).toHaveLength(0);
  expect(calls('switch_minimax_account')).toHaveLength(0);
});

it('keeps MiniMax gift requests separate from API selection, login region and pending refresh', async () => {
  await mount('minimaxdesktop');
  act(() => {
    state.handleSelectModel('minimaxdesktop', 'api-model');
    state.minimaxAccounts.setRegion('global');
  });
  const claim = deferred<MinimaxAccount>();
  handlers.set('claim_minimax_daily_credits', () => claim.promise);
  const stopPropagation = vi.fn();
  await act(async () => giftButton().props.onClick({ stopPropagation }));
  expect(stopPropagation).toHaveBeenCalledOnce();
  expect(calls('claim_minimax_daily_credits')[0][1]).toEqual({ accountId: mini.id });
  expect(giftButton().props.disabled).toBe(true);
  await act(async () => {
    await state.minimaxAccounts.claimDaily(mini);
    await state.minimaxAccounts.refresh(mini);
    await state.minimaxAccounts.claimDaily({ ...mini, region: 'global' });
  });
  expect(calls('claim_minimax_daily_credits')).toHaveLength(1);
  expect(calls('refresh_minimax_account')).toHaveLength(0);
  await act(async () => {
    claim.resolve({
      ...mini,
      dailyReward: { status: 'claimed', checkedAt: Date.now() / 1000 },
    });
  });
  expect(giftButton().props['aria-label']).toBe(`agent.dailyCreditsClaimed ${mini.email}`);
  expect(giftButton().props.disabled).toBe(true);
  expect(state.toolModelConfig.minimaxdesktop).toBe('api-model');
  expect(state.minimaxAccounts.region).toBe('global');
  expect(state.minimaxAccounts.selectedId).toBeNull();
  await act(async () => {
    await state.minimaxAccounts.claimDaily(mini);
  });
  expect(calls('claim_minimax_daily_credits')).toHaveLength(1);
  expect(calls('switch_minimax_account')).toHaveLength(0);
  expect(calls('start_minimax_login')).toHaveLength(0);
});

it.each(['claimed', 'unavailable'] as const)(
  'retains a confirmed MiniMax %s state until its five-minute cache expires locally',
  async (status) => {
    handlers.set('list_minimax_accounts', () => [
      { ...mini, dailyReward: { status, checkedAt: Date.now() / 1000 } },
    ]);
    await mount('minimaxdesktop');
    expect(giftButton().props.disabled).toBe(true);
    await act(async () => {
      await state.minimaxAccounts.claimDaily(state.minimaxAccounts.accounts[0]);
    });
    expect(calls('claim_minimax_daily_credits')).toHaveLength(0);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(300_000);
    });
    expect(giftButton().props['aria-label']).toBe(`agent.claimDailyCredits ${mini.email}`);
    expect(giftButton().props.disabled).toBe(false);
    expect(state.minimaxAccounts.accounts[0].dailyReward?.status).toBe(status);
    expect(calls('claim_minimax_daily_credits')).toHaveLength(0);
    expect(calls('refresh_minimax_account')).toHaveLength(0);
  }
);

it('preserves MiniMax cached status and quota when an explicit gift claim fails', async () => {
  const dailyReward = { status: 'claimable' as const, checkedAt: Date.now() / 1000 };
  handlers.set('list_minimax_accounts', () => [{ ...mini, dailyReward }]);
  await mount('minimaxdesktop');
  handlers.set('claim_minimax_daily_credits', () =>
    Promise.reject(new Error('accountError.network'))
  );
  await act(async () => {
    await state.minimaxAccounts.claimDaily(state.minimaxAccounts.accounts[0]);
  });
  expect(state.applyError).toBe('accountError.network');
  expect(state.minimaxAccounts.accounts[0].dailyReward).toEqual(dailyReward);
  expect(state.minimaxAccounts.accounts[0].quota?.credits).toBe(20);
  expect(state.minimaxAccounts.refreshing.size).toBe(0);
  expect(calls('refresh_minimax_account')).toHaveLength(0);
});

it('ignores MiniMax gift results after navigation without starting a quota refresh', async () => {
  await mount('minimaxdesktop');
  const claim = deferred<MinimaxAccount>();
  handlers.set('claim_minimax_daily_credits', () => claim.promise);
  let pending!: ReturnType<typeof state.minimaxAccounts.claimDaily>;
  await act(async () => {
    pending = state.minimaxAccounts.claimDaily(mini);
  });
  act(() => useNavigationStore.getState().setActivePage('freeModels'));
  await tick();
  await act(async () => {
    claim.resolve({ ...mini, dailyReward: { status: 'claimed', checkedAt: Date.now() / 1000 } });
    await pending;
  });
  expect(state.minimaxAccounts.accounts[0].dailyReward).toBeUndefined();
  expect(state.minimaxAccounts.refreshing.size).toBe(0);
  expect(calls('refresh_minimax_account')).toHaveLength(0);
});

it('batch-claims both MiniMax regions independently without applying or refreshing an account', async () => {
  handlers.set('claim_minimax_daily_credits', (args) => ({
    ...(args.accountId === mini.id ? mini : miniGlobal),
    dailyReward: { status: 'claimed', checkedAt: Date.now() / 1000 },
  }));
  await mount('minimaxdesktop');
  act(() => useNavigationStore.getState().setActivePage('accounts'));
  await tick();
  expect(calls('claim_minimax_daily_credits')).toHaveLength(0);
  await act(async () => {
    await renderer.root.findByProps({ 'aria-label': 'accountCenter.claimAll' }).props.onClick();
  });
  expect(calls('claim_minimax_daily_credits').map((call) => call[1])).toEqual([
    { accountId: mini.id },
    { accountId: miniGlobal.id },
  ]);
  expect(calls('refresh_minimax_account')).toHaveLength(0);
  expect(calls('switch_minimax_account')).toHaveLength(0);
});
