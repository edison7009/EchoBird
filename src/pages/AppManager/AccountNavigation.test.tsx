import React, { useLayoutEffect } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { beforeEach, afterEach, expect, it, vi } from 'vitest';
import * as api from '../../api/tauri';
import { useToolsStore } from '../../stores/toolsStore';
import { useNavigationStore } from '../../stores/navigationStore';
import { AppManagerProvider } from '../../pages/AppManager/AppManagerProvider';
import { useAppManager } from '../../pages/AppManager/context';
import { open as folderPicker } from '@tauri-apps/plugin-dialog';
import { open as shellOpen } from '@tauri-apps/plugin-shell';

vi.hoisted(() => vi.stubGlobal('__APP_EDITION__', 'full'));
vi.mock('../../hooks/useI18n', () => {
  const t = (k: string) => k;
  return { useI18n: () => ({ t, locale: 'en' }) };
});
vi.mock('../../components/ConfirmDialog', () => ({ useConfirm: () => async () => true }));
vi.mock('../../components', () => ({ EFFORT_PULSE_ONESHOT_MS: 0 }));
vi.mock('../../pages/ModelNexus/context', () => {
  const userModels: never[] = [];
  return { useModelNexus: () => ({ userModels }) };
});
vi.mock('../../pages/FreeModels', () => ({ useFreeModels: () => ({ routerEnabled: false }) }));
vi.mock('../../pages/AppManager/ClaudeCodeLoginDialog', () => ({
  ClaudeCodeLoginDialog: () => null,
}));
vi.mock('@tauri-apps/plugin-shell', () => ({ open: vi.fn().mockResolvedValue(undefined) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn().mockResolvedValue('E:/fixture-project'),
}));
vi.mock('../../api/tauri', () => {
  const names = [
    'listZCodeAccounts',
    'startZCodeLogin',
    'pollZCodeLogin',
    'cancelZCodeLogin',
    'switchZCodeAccount',
    'refreshZCodeAccountQuota',
    'deleteZCodeAccount',
    'startCodexLogin',
    'cancelCodexLogin',
    'listCodexAccounts',
    'addCodexAccountViaOAuth',
    'switchCodexAccount',
    'refreshCodexAccountQuota',
    'listClaudeCodeAccounts',
    'startClaudeCodeLogin',
    'cancelClaudeCodeLogin',
    'completeClaudeCodeLogin',
    'refreshClaudeCodeAccountQuota',
    'listWorkBuddyAccounts',
    'startWorkBuddyLogin',
    'cancelWorkBuddyLogin',
    'pollWorkBuddyLogin',
    'refreshWorkBuddyAccountQuota',
    'claimWorkBuddyDailyCredits',
    'switchWorkBuddyAccount',
    'listDeepSeekAccounts',
    'startDeepSeekLogin',
    'cancelDeepSeekLogin',
    'pollDeepSeekLogin',
    'refreshDeepSeekAccountQuota',
    'listGrokAccounts',
    'startGrokLogin',
    'cancelGrokLogin',
    'pollGrokLogin',
    'refreshGrokAccount',
    'switchGrokAccount',
    'deleteGrokAccount',
    'listManusAccounts',
    'startManusLogin',
    'cancelManusLogin',
    'pollManusLogin',
    'refreshManusAccount',
    'switchManusAccount',
    'deleteManusAccount',
    'listCursorAccounts',
    'startCursorLogin',
    'cancelCursorLogin',
    'pollCursorLogin',
    'refreshCursorAccount',
    'listGrokBotAccounts',
    'startGrokBotLogin',
    'cancelGrokBotLogin',
    'pollGrokBotLogin',
    'refreshGrokBotAccount',
    'deleteGrokBotAccount',
    'deleteCursorAccount',
    'listAntigravityAccounts',
    'startAntigravityLogin',
    'cancelAntigravityLogin',
    'pollAntigravityLogin',
    'refreshAntigravityAccount',
    'switchAntigravityAccount',
    'deleteAntigravityAccount',
    'startTool',
    'openExternal',
    'restoreToolToOfficial',
  ];
  return {
    ...Object.fromEntries(names.map((n) => [n, vi.fn()])),
    getModels: vi.fn().mockResolvedValue([]),
    getInstallIndex: vi.fn().mockResolvedValue('{"ids":[]}'),
  };
});

let renderer: ReactTestRenderer;
let state: ReturnType<typeof useAppManager>;
function Harness() {
  const ctx = useAppManager();
  useLayoutEffect(() => {
    state = ctx;
  });
  return null;
}
const listNames = [
  'listZCodeAccounts',
  'listCodexAccounts',
  'listClaudeCodeAccounts',
  'listWorkBuddyAccounts',
  'listDeepSeekAccounts',
  'listGrokAccounts',
  'listManusAccounts',
  'listCursorAccounts',
  'listGrokBotAccounts',
  'listAntigravityAccounts',
] as const;
const listFor = {
  zcode: 'listZCodeAccounts',
  codex: 'listCodexAccounts',
  chatgptdesktop: 'listCodexAccounts',
  claudecode: 'listClaudeCodeAccounts',
  workbuddy: 'listWorkBuddyAccounts',
  workbuddyai: 'listWorkBuddyAccounts',
  dsh: 'listDeepSeekAccounts',
  grok: 'listGrokAccounts',
  manus: 'listManusAccounts',
  cursor: 'listCursorAccounts',
  grokbot: 'listGrokBotAccounts',
} as const;
type Tool = keyof typeof listFor;
async function tick() {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(1);
  });
}
async function mount(tool: Tool, installed = true) {
  useToolsStore
    .getState()
    .setDetectedTools([
      { id: tool, name: tool, category: tool === 'grok' ? 'CLI Code' : 'Desktop', installed },
    ]);
  await act(async () => {
    renderer = create(
      <AppManagerProvider>
        <Harness />
      </AppManagerProvider>
    );
  });
  act(() => {
    if (!installed) state.setViewMode('install');
    state.setSelectedTool(tool);
  });
  await tick();
}
beforeEach(() => {
  vi.clearAllMocks();
  vi.useFakeTimers();
  for (const name of listNames) vi.mocked(api[name]).mockResolvedValue([]);
  vi.mocked(api.cancelClaudeCodeLogin).mockResolvedValue(undefined);
  vi.mocked(api.startCodexLogin).mockResolvedValue('fixture-login');
  vi.mocked(api.cancelCodexLogin).mockResolvedValue(undefined);
  vi.mocked(api.restoreToolToOfficial).mockResolvedValue({ success: true, message: '' });
  vi.mocked(api.startTool).mockResolvedValue(undefined);
  vi.mocked(folderPicker).mockResolvedValue('E:/fixture-project');
  useNavigationStore.getState().setActivePage('apps');
});

it.each(['claudecode', 'workbuddy', 'dsh', 'chatgptdesktop'] as const)(
  '%s: leaving the page cancels login and ignores its late failure',
  async (tool) => {
    let reject!: (error: Error) => void;
    const pending = new Promise<never>((_, fail) => {
      reject = fail;
    });
    const expiresAt = Date.now() / 1000 + 60;
    vi.mocked(api.startClaudeCodeLogin).mockResolvedValue({
      loginId: 'login',
      authorizationUrl: 'https://example.test',
      expiresAt,
    });
    vi.mocked(api.startWorkBuddyLogin).mockResolvedValue({
      loginId: 'login',
      verificationUri: 'https://example.test',
      expiresAt,
    });
    vi.mocked(api.startDeepSeekLogin).mockResolvedValue({
      loginId: 'login',
      verificationUri: 'https://example.test',
      expiresAt,
    });
    vi.mocked(api.cancelWorkBuddyLogin).mockResolvedValue(undefined);
    vi.mocked(api.cancelDeepSeekLogin).mockResolvedValue(undefined);
    await mount(tool);
    let task!: Promise<void>;
    await act(async () => {
      if (tool === 'claudecode') {
        await state.claudeCodeAccounts.add();
        vi.mocked(api.completeClaudeCodeLogin).mockReturnValueOnce(pending);
        task = state.claudeCodeAccounts.completeLogin('fixture-code');
      } else if (tool === 'workbuddy') {
        vi.mocked(api.pollWorkBuddyLogin).mockReturnValueOnce(pending);
        task = state.workBuddyAccounts.add();
      } else if (tool === 'dsh') {
        vi.mocked(api.pollDeepSeekLogin).mockReturnValueOnce(pending);
        task = state.deepSeekAccounts.add();
      } else {
        vi.mocked(api.addCodexAccountViaOAuth).mockReturnValueOnce(pending);
        task = state.addCodexAccount();
      }
    });
    act(() => useNavigationStore.getState().setActivePage('models'));
    await tick();
    const cancel = {
      claudecode: api.cancelClaudeCodeLogin,
      workbuddy: api.cancelWorkBuddyLogin,
      dsh: api.cancelDeepSeekLogin,
      chatgptdesktop: api.cancelCodexLogin,
    }[tool];
    expect(cancel).toHaveBeenCalledWith(tool === 'chatgptdesktop' ? 'fixture-login' : 'login');
    await act(async () => {
      reject(new Error('late failure'));
      await task;
    });
    expect(state.applyError).toBeNull();
    expect(state.claudeCodeAccounts.login).toBeNull();
  }
);

it.each(['claudecode', 'workbuddy', 'dsh', 'chatgptdesktop'] as const)(
  '%s: the 60 second deadline cancels an in-flight login request',
  async (tool) => {
    let resolve!: (account: never) => void;
    const pending = new Promise<never>((done) => {
      resolve = done;
    });
    const login = {
      loginId: 'login',
      verificationUri: 'https://example.test',
      expiresAt: Date.now() / 1000 + 600,
    };
    vi.mocked(api.startWorkBuddyLogin).mockResolvedValue(login);
    vi.mocked(api.startDeepSeekLogin).mockResolvedValue(login);
    vi.mocked(api.startClaudeCodeLogin).mockResolvedValue({
      ...login,
      authorizationUrl: 'https://example.test',
    });
    vi.mocked(api.cancelWorkBuddyLogin).mockResolvedValue(undefined);
    vi.mocked(api.cancelDeepSeekLogin).mockResolvedValue(undefined);
    await mount(tool);
    let task!: Promise<void>;
    await act(async () => {
      if (tool === 'claudecode') {
        await state.claudeCodeAccounts.add();
        vi.mocked(api.completeClaudeCodeLogin).mockReturnValueOnce(pending);
        task = state.claudeCodeAccounts.completeLogin('fixture-code');
      } else if (tool === 'workbuddy') {
        vi.mocked(api.pollWorkBuddyLogin).mockReturnValueOnce(pending);
        task = state.workBuddyAccounts.add();
      } else if (tool === 'dsh') {
        vi.mocked(api.pollDeepSeekLogin).mockReturnValueOnce(pending);
        task = state.deepSeekAccounts.add();
      } else {
        vi.mocked(api.addCodexAccountViaOAuth).mockReturnValueOnce(pending);
        task = state.addCodexAccount();
      }
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    expect(state.applyError).toBe('accountError.expired');
    const cancel = {
      claudecode: api.cancelClaudeCodeLogin,
      workbuddy: api.cancelWorkBuddyLogin,
      dsh: api.cancelDeepSeekLogin,
      chatgptdesktop: api.cancelCodexLogin,
    }[tool];
    expect(cancel).toHaveBeenCalled();
    expect(
      state.claudeCodeAccounts.busy ||
        state.workBuddyAccounts.busy ||
        state.deepSeekAccounts.busy ||
        state.isAddingCodexAccount
    ).toBe(false);
    const before = vi.mocked(api[listFor[tool]]).mock.calls.length;
    await act(async () => {
      resolve({ id: 'late' } as never);
      await task;
    });
    expect(api[listFor[tool]]).toHaveBeenCalledTimes(before);
  }
);

it.each(['claudecode', 'workbuddy', 'dsh', 'chatgptdesktop'] as const)(
  '%s: explicit refresh reports errors and prevents duplicate requests',
  async (tool) => {
    await mount(tool);
    let reject!: (error: Error) => void;
    const pending = new Promise<never>((_, fail) => {
      reject = fail;
    });
    const quota = {
      claudecode: api.refreshClaudeCodeAccountQuota,
      workbuddy: api.refreshWorkBuddyAccountQuota,
      dsh: api.refreshDeepSeekAccountQuota,
      chatgptdesktop: api.refreshCodexAccountQuota,
    }[tool];
    vi.mocked(quota).mockReturnValueOnce(pending);
    const row = {
      id: 'quota-fixture',
      edition: 'workbuddy',
      email: 'fixture@example.test',
    } as never;
    const refresh = {
      claudecode: state.claudeCodeAccounts.refresh,
      workbuddy: state.workBuddyAccounts.refresh,
      dsh: state.deepSeekAccounts.refresh,
      chatgptdesktop: state.refreshCodexAccountQuota,
    }[tool];
    let task!: Promise<void>;
    act(() => {
      task = refresh(row);
      void refresh(row);
    });
    expect(quota).toHaveBeenCalledTimes(1);
    await act(async () => {
      reject(new Error('accountError.network'));
      await task;
    });
    expect(state.applyError).not.toBeNull();
  }
);

it.each(['workbuddy', 'dsh'] as const)(
  '%s: login expiry does not leave a concurrent quota refresh disabled',
  async (tool) => {
    const row = { id: 'fixture', edition: 'workbuddy' } as never;
    const login = {
      loginId: 'login',
      verificationUri: 'https://example.test',
      expiresAt: Date.now() / 1000 + 600,
    };
    let finishRefresh!: (row: never) => void;
    let finishLogin!: (row: null) => void;
    const quota = new Promise<never>((resolve) => {
      finishRefresh = resolve;
    });
    const pending = new Promise<null>((resolve) => {
      finishLogin = resolve;
    });
    vi.mocked(api.startWorkBuddyLogin).mockResolvedValue(login);
    vi.mocked(api.startDeepSeekLogin).mockResolvedValue(login);
    vi.mocked(api.cancelWorkBuddyLogin).mockResolvedValue(undefined);
    vi.mocked(api.cancelDeepSeekLogin).mockResolvedValue(undefined);
    vi.mocked(api.pollWorkBuddyLogin).mockReturnValue(pending);
    vi.mocked(api.pollDeepSeekLogin).mockReturnValue(pending);
    vi.mocked(
      tool === 'workbuddy' ? api.refreshWorkBuddyAccountQuota : api.refreshDeepSeekAccountQuota
    ).mockReturnValueOnce(quota);
    await mount(tool);
    const accountState = () =>
      tool === 'workbuddy' ? state.workBuddyAccounts : state.deepSeekAccounts;
    let loginTask!: Promise<void>;
    let refreshTask!: Promise<void>;
    await act(async () => {
      loginTask = accountState().add();
      refreshTask = accountState().refresh(row);
    });
    expect(accountState().refreshing.has('fixture')).toBe(true);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    await act(async () => {
      finishRefresh(row);
      finishLogin(null);
      await Promise.all([refreshTask, loginTask]);
    });
    expect(accountState().refreshing.size).toBe(0);
  }
);

it('Claude Code: timeout cancels a late login initialization without opening the browser', async () => {
  let resolve!: (login: api.ClaudeCodeLogin) => void;
  vi.mocked(api.startClaudeCodeLogin).mockReturnValueOnce(
    new Promise((done) => {
      resolve = done;
    })
  );
  await mount('claudecode');
  let task!: Promise<void>;
  act(() => {
    task = state.claudeCodeAccounts.add();
  });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_000);
  });
  expect(state.claudeCodeAccounts.busy).toBe(false);
  await act(async () => {
    resolve({
      loginId: 'late',
      authorizationUrl: 'https://example.test',
      expiresAt: Date.now() / 1000 + 60,
    });
    await task;
  });
  expect(api.cancelClaudeCodeLogin).toHaveBeenCalledWith('late');
  expect(state.claudeCodeAccounts.login).toBeNull();
  expect(shellOpen).not.toHaveBeenCalled();
});

it('Grok Build: cancelling the directory picker prevents launch after account apply', async () => {
  vi.mocked(api.listGrokAccounts).mockResolvedValue([{ id: 'fixture', active: true }] as never);
  vi.mocked(folderPicker).mockResolvedValue(null);
  await mount('grok');
  await act(async () => {
    await state.handleLaunch();
  });
  expect(api.switchGrokAccount).toHaveBeenCalledWith('fixture');
  expect(folderPicker).toHaveBeenCalled();
  expect(api.startTool).not.toHaveBeenCalled();
});

it('Grok Build: apply-only does not ask for a directory or launch', async () => {
  vi.mocked(api.listGrokAccounts).mockResolvedValue([{ id: 'fixture', active: true }] as never);
  await mount('grok');
  act(() => state.setLaunchAfterApply(false));
  await act(async () => {
    await state.handleLaunch();
  });
  expect(api.switchGrokAccount).toHaveBeenCalledWith('fixture');
  expect(folderPicker).not.toHaveBeenCalled();
  expect(api.startTool).not.toHaveBeenCalled();
});
afterEach(() => {
  act(() => renderer?.unmount());
  useToolsStore.getState().setDetectedTools([]);
  vi.useRealTimers();
});

it.each(Object.keys(listFor) as Tool[])(
  '%s: uninstalled navigation must not read native accounts',
  async (tool) => {
    await mount(tool, false);
    expect(api[listFor[tool]]).not.toHaveBeenCalled();
  }
);
it.each(Object.keys(listFor) as Tool[])(
  '%s: installed navigation only reads accounts',
  async (tool) => {
    await mount(tool);
    expect(api[listFor[tool]]).toHaveBeenCalledTimes(1);
    const actions = [
      api.startZCodeLogin,
      api.refreshZCodeAccountQuota,
      api.switchZCodeAccount,
      api.deleteZCodeAccount,
      api.startCodexLogin,
      api.startClaudeCodeLogin,
      api.startWorkBuddyLogin,
      api.startDeepSeekLogin,
      api.startGrokLogin,
      api.startCursorLogin,
      api.startGrokBotLogin,
      api.refreshCodexAccountQuota,
      api.refreshClaudeCodeAccountQuota,
      api.refreshWorkBuddyAccountQuota,
      api.claimWorkBuddyDailyCredits,
      api.refreshDeepSeekAccountQuota,
      api.refreshGrokAccount,
      api.refreshCursorAccount,
      api.refreshGrokBotAccount,
      api.switchCodexAccount,
      api.switchWorkBuddyAccount,
      api.switchGrokAccount,
      api.restoreToolToOfficial,
      api.startTool,
    ];
    for (const action of actions) expect(action).not.toHaveBeenCalled();
    act(() => state.setViewMode('install'));
    await tick();
    expect(api[listFor[tool]]).toHaveBeenCalledTimes(1);
    for (const action of actions) expect(action).not.toHaveBeenCalled();
  }
);
it.each(['claudecode', 'workbuddy', 'dsh', 'chatgptdesktop'] as const)(
  '%s: failed passive reload preserves cached accounts',
  async (tool) => {
    const row = { id: 'cached', active: true, edition: 'workbuddy' };
    vi.mocked(api[listFor[tool]]).mockResolvedValueOnce([row] as never);
    await mount(tool);
    const rows = () =>
      ({
        claudecode: state.claudeCodeAccounts.accounts,
        workbuddy: state.workBuddyAccounts.accounts,
        dsh: state.deepSeekAccounts.accounts,
        chatgptdesktop: state.codexAccounts,
      })[tool];
    expect(rows()).toEqual([row]);
    act(() => useNavigationStore.getState().setActivePage('models'));
    await tick();
    vi.mocked(api[listFor[tool]]).mockRejectedValueOnce(new Error('accountError.read'));
    act(() => useNavigationStore.getState().setActivePage('apps'));
    await tick();
    expect(api[listFor[tool]]).toHaveBeenCalledTimes(2);
    expect(rows()).toEqual([row]);
    expect(state.applyError).toBeNull();
  }
);
it.each(['claudecode', 'workbuddy', 'dsh', 'zcode'] as Tool[])(
  '%s: passive read failure must not show dialog',
  async (tool) => {
    vi.mocked(api[listFor[tool]]).mockRejectedValueOnce(new Error('accountError.read'));
    await mount(tool);
    expect(api[listFor[tool]]).toHaveBeenCalledTimes(1);
    expect(state.applyError).toBeNull();
  }
);
it.each(['stay', 'leave', 'retry'] as const)(
  'Claude Code: a cancellation failure is only shown for its current session (%s)',
  async (next) => {
    vi.mocked(api.startClaudeCodeLogin).mockResolvedValue({
      loginId: 'login',
      authorizationUrl: 'https://example.test',
      expiresAt: Date.now() / 1000 + 60,
    });
    await mount('claudecode');
    await act(async () => {
      await state.claudeCodeAccounts.add();
    });
    let reject!: (error: Error) => void;
    vi.mocked(api.cancelClaudeCodeLogin).mockReturnValueOnce(
      new Promise((_, fail) => {
        reject = fail;
      })
    );
    act(() => state.claudeCodeAccounts.cancelLogin());
    if (next === 'leave') {
      act(() => useNavigationStore.getState().setActivePage('models'));
      await tick();
    } else if (next === 'retry') {
      vi.mocked(api.startClaudeCodeLogin).mockResolvedValueOnce({
        loginId: 'new-login',
        authorizationUrl: 'https://example.test',
        expiresAt: Date.now() / 1000 + 60,
      });
      await act(async () => {
        await state.claudeCodeAccounts.add();
      });
    }
    await act(async () => {
      reject(new Error('accountError.network'));
    });
    if (next === 'stay') expect(state.applyError).not.toBeNull();
    else expect(state.applyError).toBeNull();
    if (next === 'retry') expect(state.claudeCodeAccounts.login?.loginId).toBe('new-login');
  }
);

it('Claude Code: leaving tool cancels pending login', async () => {
  vi.mocked(api.startClaudeCodeLogin).mockResolvedValue({
    loginId: 'fixture-login',
    authorizationUrl: 'https://example.test',
    expiresAt: Date.now() / 1000 + 60,
  });
  await mount('claudecode');
  await act(async () => {
    await state.claudeCodeAccounts.add();
  });
  expect(state.claudeCodeAccounts.login?.loginId).toBe('fixture-login');
  expect(shellOpen).toHaveBeenCalledWith('https://example.test');
  act(() => state.setSelectedTool('cursor'));
  await tick();
  expect(api.cancelClaudeCodeLogin).toHaveBeenCalledWith('fixture-login');
  expect(state.claudeCodeAccounts.login).toBeNull();
});
it('ChatGPT: a late login failure must not open a dialog on another tool', async () => {
  let reject!: (e: Error) => void;
  vi.mocked(api.addCodexAccountViaOAuth).mockReturnValueOnce(
    new Promise((_, r) => {
      reject = r;
    })
  );
  await mount('chatgptdesktop');
  let task!: Promise<void>;
  await act(async () => {
    task = state.addCodexAccount();
  });
  act(() => state.setSelectedTool('cursor'));
  await act(async () => {
    reject(new Error('accountError.expired'));
    await task;
  });
  expect(state.applyError).toBeNull();
});
it('Grok Build: account launch must use the common CLI folder picker', async () => {
  vi.mocked(api.listGrokAccounts).mockResolvedValue([
    { id: 'fixture', email: 'fixture@example.test', active: true },
  ] as never);
  await mount('grok');
  expect(state.grokAccounts.selectedId).toBe('fixture');
  await act(async () => {
    await state.handleLaunch();
  });
  expect(folderPicker).toHaveBeenCalledTimes(1);
  expect(api.startTool).toHaveBeenCalledWith('grok', undefined, 'E:/fixture-project');
});
it('positive control: passive Grok navigation does not refresh, switch, or log in', async () => {
  await mount('grok');
  expect(api.listGrokAccounts).toHaveBeenCalledTimes(1);
  expect(api.refreshGrokAccount).not.toHaveBeenCalled();
  expect(api.switchGrokAccount).not.toHaveBeenCalled();
  expect(api.startGrokLogin).not.toHaveBeenCalled();
});
it('WorkBuddy account selection preserves custom model configuration', async () => {
  vi.mocked(api.listWorkBuddyAccounts).mockResolvedValue([
    { id: 'fixture', name: 'fixture', edition: 'workbuddy', active: true },
  ] as never);
  await mount('workbuddy');
  await act(async () => {
    await state.handleLaunch();
  });
  expect(api.switchWorkBuddyAccount).toHaveBeenCalledWith('workbuddy', 'fixture');
  expect(api.restoreToolToOfficial).not.toHaveBeenCalled();
});
it('positive control: Grok Build without account selection uses the CLI folder picker', async () => {
  await mount('grok');
  expect(state.grokAccounts.selectedId).toBeNull();
  await act(async () => {
    await state.handleLaunch();
  });
  expect(folderPicker).toHaveBeenCalledTimes(1);
  expect(api.startTool).toHaveBeenCalledWith('grok', undefined, 'E:/fixture-project');
});
it('Claude Code: adding an account must not automatically refresh quota', async () => {
  vi.mocked(api.startClaudeCodeLogin).mockResolvedValue({
    loginId: 'login',
    authorizationUrl: 'https://example.test',
    expiresAt: Date.now() / 1000 + 60,
  });
  vi.mocked(api.completeClaudeCodeLogin).mockResolvedValue({
    id: 'fixture',
    email: 'fixture@example.test',
  } as never);
  await mount('claudecode');
  await act(async () => {
    await state.claudeCodeAccounts.add();
  });
  await act(async () => {
    await state.claudeCodeAccounts.completeLogin('fixture-code');
  });
  expect(api.refreshClaudeCodeAccountQuota).not.toHaveBeenCalled();
});
it('WorkBuddy: adding an account must not automatically refresh quota', async () => {
  vi.mocked(api.startWorkBuddyLogin).mockResolvedValue({
    loginId: 'login',
    verificationUri: 'https://example.test',
    expiresAt: Date.now() / 1000 + 60,
  });
  vi.mocked(api.pollWorkBuddyLogin).mockResolvedValue({
    id: 'fixture',
    edition: 'workbuddy',
  } as never);
  vi.mocked(api.cancelWorkBuddyLogin).mockResolvedValue(undefined);
  await mount('workbuddy');
  await act(async () => {
    await state.workBuddyAccounts.add();
  });
  expect(api.refreshWorkBuddyAccountQuota).not.toHaveBeenCalled();
});
it('DeepSeek: adding an account must not automatically refresh quota', async () => {
  vi.mocked(api.startDeepSeekLogin).mockResolvedValue({
    loginId: 'login',
    verificationUri: 'https://example.test',
    expiresAt: Date.now() / 1000 + 60,
  });
  vi.mocked(api.pollDeepSeekLogin).mockResolvedValue({ id: 'fixture' } as never);
  vi.mocked(api.cancelDeepSeekLogin).mockResolvedValue(undefined);
  await mount('dsh');
  await act(async () => {
    await state.deepSeekAccounts.add();
  });
  expect(api.refreshDeepSeekAccountQuota).not.toHaveBeenCalled();
});

const zcodeRow: api.ZCodeAccount = {
  id: 'bigmodel:one',
  provider: 'bigmodel',
  email: 'one@example.test',
  active: true,
  plan: 'Pro',
  remainingPercent: 40,
  resetAt: null,
  subscriptionEndAt: 1900000000,
  quotaWindows: [
    { remainingPercent: 40, resetAt: 1890000000 },
    { remainingPercent: 70, resetAt: 1890200000 },
  ],
};
function zcodeLogin(): api.ZCodeLogin {
  return {
    loginId: 'zcode-login',
    verificationUri: 'https://bigmodel.cn/login',
    expiresAt: Date.now() / 1000 + 60,
    pollIntervalSeconds: 1,
  };
}
it('ZCode: region choice is passive, login saves without refresh or apply, selection excludes API models', async () => {
  await mount('zcode');
  act(() => state.zcodeAccounts.setProvider('zai'));
  expect(api.startZCodeLogin).not.toHaveBeenCalled();
  expect(api.listZCodeAccounts).toHaveBeenCalledTimes(1);
  act(() => state.handleSelectModel('zcode', 'api-model'));
  vi.mocked(api.startZCodeLogin).mockResolvedValue(zcodeLogin());
  vi.mocked(api.pollZCodeLogin).mockResolvedValue(zcodeRow);
  vi.mocked(api.cancelZCodeLogin).mockResolvedValue(undefined);
  vi.mocked(api.listZCodeAccounts).mockResolvedValue([zcodeRow]);
  await act(async () => {
    await state.zcodeAccounts.add();
  });
  expect(api.startZCodeLogin).toHaveBeenCalledWith('zai');
  expect(api.openExternal).toHaveBeenCalledWith('https://bigmodel.cn/login');
  expect(state.zcodeAccounts.selectedId).toBe(zcodeRow.id);
  expect(state.toolModelConfig.zcode).toBeNull();
  expect(api.switchZCodeAccount).not.toHaveBeenCalled();
  expect(api.refreshZCodeAccountQuota).not.toHaveBeenCalled();
  act(() => state.handleSelectModel('zcode', 'api-model'));
  expect(state.zcodeAccounts.selectedId).toBeNull();
  act(() => state.zcodeAccounts.select(zcodeRow.id));
  expect(state.toolModelConfig.zcode).toBeNull();
});
it.each(['page', 'tool', 'timeout'] as const)(
  'ZCode: %s cancels login and ignores a late result',
  async (exit) => {
    let finish!: (row: api.ZCodeAccount) => void;
    vi.mocked(api.startZCodeLogin).mockResolvedValue(zcodeLogin());
    vi.mocked(api.pollZCodeLogin).mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      })
    );
    vi.mocked(api.cancelZCodeLogin).mockResolvedValue(undefined);
    await mount('zcode');
    let task!: Promise<void>;
    await act(async () => {
      task = state.zcodeAccounts.add();
    });
    act(() => state.zcodeAccounts.setProvider('zai'));
    expect(state.zcodeAccounts.provider).toBe('bigmodel');
    if (exit === 'timeout') {
      await act(async () => {
        await vi.advanceTimersByTimeAsync(60_000);
      });
      expect(state.applyError).toBe('accountError.expired');
    } else {
      act(() =>
        exit === 'page'
          ? useNavigationStore.getState().setActivePage('models')
          : state.setSelectedTool(null)
      );
      await tick();
    }
    expect(api.cancelZCodeLogin).toHaveBeenCalledWith('zcode-login');
    await act(async () => {
      finish(zcodeRow);
      await task;
    });
    expect(state.zcodeAccounts.accounts).toEqual([]);
    expect(api.listZCodeAccounts).toHaveBeenCalledTimes(1);
    expect(api.switchZCodeAccount).not.toHaveBeenCalled();
  }
);
it('ZCode: passive reload preserves cached rows and apply failure does not launch', async () => {
  vi.mocked(api.listZCodeAccounts).mockResolvedValue([zcodeRow]);
  await mount('zcode');
  act(() => useNavigationStore.getState().setActivePage('models'));
  await tick();
  vi.mocked(api.listZCodeAccounts).mockRejectedValueOnce(new Error('accountError.read'));
  act(() => useNavigationStore.getState().setActivePage('apps'));
  await tick();
  expect(state.zcodeAccounts.accounts).toEqual([zcodeRow]);
  expect(state.applyError).toBeNull();
  act(() => {
    state.zcodeAccounts.select(zcodeRow.id);
    state.setLaunchAfterApply(true);
  });
  vi.mocked(api.switchZCodeAccount).mockRejectedValueOnce(new Error('accountError.write'));
  await act(async () => {
    await state.handleLaunch();
  });
  expect(state.applyError).toBe('accountError.write');
  expect(api.startTool).not.toHaveBeenCalled();
  vi.mocked(api.switchZCodeAccount).mockResolvedValue(zcodeRow);
  await act(async () => {
    await state.handleLaunch();
  });
  expect(api.switchZCodeAccount).toHaveBeenCalledWith(zcodeRow.id);
  expect(api.startTool).toHaveBeenCalledWith('zcode', undefined);
  expect(api.refreshZCodeAccountQuota).not.toHaveBeenCalled();
});
it('ZCode: explicit quota failure remains visible and cached, delete uses the shared confirmation flow', async () => {
  vi.mocked(api.listZCodeAccounts).mockResolvedValue([zcodeRow]);
  await mount('zcode');
  vi.mocked(api.refreshZCodeAccountQuota).mockRejectedValueOnce(new Error('accountError.network'));
  await act(async () => {
    await state.zcodeAccounts.refresh(zcodeRow);
  });
  expect(state.applyError).toBe('accountError.network');
  expect(state.zcodeAccounts.accounts).toEqual([zcodeRow]);
  let finish!: (row: api.ZCodeAccount) => void;
  vi.mocked(api.refreshZCodeAccountQuota).mockReturnValueOnce(
    new Promise((resolve) => {
      finish = resolve;
    })
  );
  let refresh!: Promise<void>;
  await act(async () => {
    refresh = state.zcodeAccounts.refresh(zcodeRow);
  });
  expect(state.zcodeAccounts.refreshing.has(zcodeRow.id)).toBe(true);
  const updated = { ...zcodeRow, plan: 'Max', subscriptionEndAt: 1910000000 };
  await act(async () => {
    finish(updated);
    await refresh;
  });
  expect(state.zcodeAccounts.refreshing.size).toBe(0);
  expect(state.zcodeAccounts.accounts).toEqual([updated]);
  expect(api.startZCodeLogin).not.toHaveBeenCalled();
  expect(api.switchZCodeAccount).not.toHaveBeenCalled();
  vi.mocked(api.deleteZCodeAccount).mockResolvedValue(undefined);
  await act(async () => {
    await state.zcodeAccounts.remove(zcodeRow);
  });
  expect(api.deleteZCodeAccount).toHaveBeenCalledWith(zcodeRow.id);
  expect(state.zcodeAccounts.accounts).toEqual([]);
  expect(state.zcodeAccounts.selectedId).toBeNull();
});
