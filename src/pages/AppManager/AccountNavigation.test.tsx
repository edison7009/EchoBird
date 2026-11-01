import { usePluginsStore } from '../../stores/pluginsStore';
import { enableAccountPlugins } from '../../test/accountPlugins';
import React, { useLayoutEffect } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { beforeEach, afterEach, expect, it, vi } from 'vitest';
import * as api from '../../api/tauri';
import * as kimiApi from '../../api/kimiAccounts';
import * as codeArtsApi from '../../api/codeArtsAccounts';
import { useToolsStore } from '../../stores/toolsStore';
import { useNavigationStore } from '../../stores/navigationStore';
import { useUsageIslandStore } from '../../stores/usageIslandStore';
import { useAccountPrivacyStore } from '../../stores/accountPrivacyStore';
import { AppManagerProvider } from '../../pages/AppManager/AppManagerProvider';
import { useAppManager } from '../../pages/AppManager/context';
import { AppManagerErrorModal, AppManagerPanel, CodexAccountSection } from './AppManagerComponents';
import { AccountSectionButton, AccountSectionRow } from './AccountSectionPrimitives';
import { DirectoryRow } from '../../components/DirectoryRow';
import {
  AccountCenterMain,
  AccountCenterPanel,
  AccountCenterTitleActions,
  AccountCard,
} from '../AccountCenter/AccountCenter';
import { open as folderPicker } from '@tauri-apps/plugin-dialog';
import { open as shellOpen } from '@tauri-apps/plugin-shell';
import { TraeAccountSection } from './TraeAccountSection';

vi.hoisted(() => vi.stubGlobal('__APP_EDITION__', 'full'));
vi.mock('../../api/codeArtsAccounts', async (original) => ({
  ...(await original<typeof import('../../api/codeArtsAccounts')>()),
  listCodeArtsAccounts: vi.fn(),
}));
vi.mock('../../api/kimiAccounts', () => ({
  listKimiAccounts: vi.fn(),
  startKimiLogin: vi.fn(),
  pollKimiLogin: vi.fn(),
  cancelKimiLogin: vi.fn(),
  switchKimiAccount: vi.fn(),
  refreshKimiAccount: vi.fn(),
  deleteKimiAccount: vi.fn(),
}));
vi.mock('../../hooks/useI18n', () => {
  const summaries: Record<string, string> = {
    'accountCenter.batchAuthFailed': 'Authorization failed: {n}',
    'accountCenter.batchRefreshFailed': 'Refresh failed: {n}',
    'accountCenter.batchClaimFailed': 'Claim failed: {n}',
  };
  const t = (k: string) => summaries[k] ?? k;
  return { useI18n: () => ({ t, locale: 'en' }) };
});
vi.mock('../../components/ConfirmDialog', () => ({ useConfirm: () => async () => true }));
vi.mock('../../components', () => ({
  getModelIcon: () => '/fixture.svg',
}));
vi.mock('../../pages/ModelNexus/context', () => {
  const userModels: never[] = [];
  return { useModelNexus: () => ({ userModels }) };
});
vi.mock('../../pages/FreeModels/provider', () => ({
  useFreeModels: () => ({ routerEnabled: false }),
}));
vi.mock('../../pages/AppManager/ClaudeCodeLoginDialog', () => ({
  ClaudeCodeLoginDialog: () => null,
}));
vi.mock('@tauri-apps/plugin-shell', () => ({ open: vi.fn().mockResolvedValue(undefined) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn().mockResolvedValue('E:/fixture-project'),
}));
vi.mock('../../api/tauri', () => {
  const names = [
    'listDuMateAccounts',
    'listDoubaoAccounts',
    'listOpenDesignAccounts',
    'startOpenDesignLogin',
    'pollOpenDesignLogin',
    'cancelOpenDesignLogin',
    'switchOpenDesignAccount',
    'refreshOpenDesignAccount',
    'deleteOpenDesignAccount',
    'listClineAccounts',
    'startClineLogin',
    'pollClineLogin',
    'cancelClineLogin',
    'switchClineAccount',
    'refreshClineAccount',
    'deleteClineAccount',
    'listZCodeAccounts',
    'listKiloAccounts',
    'startKiloLogin',
    'pollKiloLogin',
    'cancelKiloLogin',
    'switchKiloAccount',
    'refreshKiloAccount',
    'deleteKiloAccount',
    'listCopilotAccounts',
    'startCopilotLogin',
    'pollCopilotLogin',
    'cancelCopilotLogin',
    'switchCopilotAccount',
    'refreshCopilotAccount',
    'deleteCopilotAccount',
    'startZCodeLogin',
    'pollZCodeLogin',
    'cancelZCodeLogin',
    'switchZCodeAccount',
    'refreshZCodeAccountQuota',
    'deleteZCodeAccount',
    'startCodexLogin',
    'cancelCodexLogin',
    'listCodexAccounts',
    'deleteCodexAccount',
    'addCodexAccountViaOAuth',
    'switchCodexAccount',
    'refreshCodexAccountQuota',
    'listClaudeCodeAccounts',
    'listClaudeDesktopAccounts',
    'deleteClaudeCodeAccount',
    'startClaudeCodeLogin',
    'cancelClaudeCodeLogin',
    'completeClaudeCodeLogin',
    'refreshClaudeCodeAccountQuota',
    'listWorkBuddyAccounts',
    'listKiroAccounts',
    'startKiroLogin',
    'pollKiroLogin',
    'cancelKiroLogin',
    'switchKiroAccount',
    'refreshKiroAccountQuota',
    'deleteKiroAccount',
    'applyModelToTool',
    'listTraeAccounts',
    'startTraeLogin',
    'pollTraeLogin',
    'cancelTraeLogin',
    'switchTraeAccount',
    'refreshTraeAccountQuota',
    'claimTraeDailyCredits',
    'deleteTraeAccount',
    'deleteWorkBuddyAccount',
    'startWorkBuddyLogin',
    'cancelWorkBuddyLogin',
    'pollWorkBuddyLogin',
    'refreshWorkBuddyAccountQuota',
    'claimWorkBuddyDailyCredits',
    'switchWorkBuddyAccount',
    'listDeepSeekAccounts',
    'deleteDeepSeekAccount',
    'startDeepSeekLogin',
    'cancelDeepSeekLogin',
    'pollDeepSeekLogin',
    'refreshDeepSeekAccountQuota',
    'switchDeepSeekAccount',
    'listGrokAccounts',
    'startGrokLogin',
    'cancelGrokLogin',
    'pollGrokLogin',
    'refreshGrokAccount',
    'switchGrokAccount',
    'deleteGrokAccount',
    'startClaudeDesktopLogin',
    'cancelClaudeDesktopLogin',
    'pollClaudeDesktopLogin',
    'refreshClaudeDesktopAccount',
    'switchClaudeDesktopAccount',
    'deleteClaudeDesktopAccount',
    'listManusAccounts',
    'startManusLogin',
    'cancelManusLogin',
    'pollManusLogin',
    'refreshManusAccount',
    'switchManusAccount',
    'deleteManusAccount',
    'listCueAccounts',
    'startCueLogin',
    'cancelCueLogin',
    'pollCueLogin',
    'refreshCueAccount',
    'switchCueAccount',
    'deleteCueAccount',
    'listCursorAccounts',
    'startCursorLogin',
    'cancelCursorLogin',
    'pollCursorLogin',
    'refreshCursorAccount',
    'switchCursorAccount',
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
    'getSettings',
    'saveSettings',
  ];
  return {
    ...Object.fromEntries(names.map((n) => [n, vi.fn()])),
    getModels: vi.fn().mockResolvedValue([]),
    getInstallIndex: vi.fn().mockResolvedValue('{"ids":[]}'),
  };
});

let renderer: ReactTestRenderer;
let state: ReturnType<typeof useAppManager>;
const applyErrorEvents: (string | null)[] = [];
function Harness({
  center = false,
  desktop = false,
  panel = false,
}: {
  center?: boolean;
  desktop?: boolean;
  panel?: boolean;
}) {
  const ctx = useAppManager();
  useLayoutEffect(() => {
    state = ctx;
  });
  useLayoutEffect(() => {
    applyErrorEvents.push(ctx.applyError);
  }, [ctx.applyError]);
  return center || desktop || panel ? (
    <>
      {center && (
        <>
          <AccountCenterTitleActions />
          <AccountCenterMain />
          <AccountCenterPanel />
          <AppManagerErrorModal />
        </>
      )}
      {desktop && <CodexAccountSection />}
      {panel && <AppManagerPanel />}
    </>
  ) : null;
}
const listNames = [
  'listOpenDesignAccounts',
  'listZCodeAccounts',
  'listCodexAccounts',
  'listClaudeCodeAccounts',
  'listClaudeDesktopAccounts',
  'listWorkBuddyAccounts',
  'listKiroAccounts',
  'listTraeAccounts',
  'listDeepSeekAccounts',
  'listGrokAccounts',
  'listManusAccounts',
  'listCueAccounts',
  'listCursorAccounts',
  'listGrokBotAccounts',
  'listAntigravityAccounts',
] as const;
const listFor = {
  opendesign: 'listOpenDesignAccounts',
  zcode: 'listZCodeAccounts',
  codex: 'listCodexAccounts',
  chatgptdesktop: 'listCodexAccounts',
  claudecode: 'listClaudeCodeAccounts',
  claudedesktop: 'listClaudeDesktopAccounts',
  workbuddy: 'listWorkBuddyAccounts',
  workbuddyai: 'listWorkBuddyAccounts',
  trae: 'listTraeAccounts',
  traecn: 'listTraeAccounts',
  kirocrew: 'listKiroAccounts',
  kirocli: 'listKiroAccounts',
  dsh: 'listDeepSeekAccounts',
  grok: 'listGrokAccounts',
  manus: 'listManusAccounts',
  cue: 'listCueAccounts',
  cursor: 'listCursorAccounts',
  grokbot: 'listGrokBotAccounts',
  antigravity: 'listAntigravityAccounts',
  antigravitydesktop: 'listAntigravityAccounts',
} as const;

const openDesignFixture: api.OpenDesignAccount = {
  id: 'design-account',
  email: 'designer@example.test',
  active: false,
  plan: 'go',
  quotaWindows: [
    { label: '5h', remainingPercent: 75, resetAt: 2_000_000_000 },
    { label: '7d', remainingPercent: 60, resetAt: 2_000_100_000 },
  ],
  balanceUsd: 2.5,
};

const kimiFixture: kimiApi.KimiAccount = {
  id: 'cn:kimi-fixture',
  email: 'kimi@example.test',
  region: 'cn',
  plan: 'Plus',
  active: false,
  quotaWindows: [{ name: 'limit_5h', remainingPercent: 75 }],
};

it.each(['kimicode', 'kimidesktop'] as const)(
  'Kimi: %s only loads saved accounts when installed and performs no passive account actions',
  async (tool) => {
    await mount(tool, false);
    expect(kimiApi.listKimiAccounts).not.toHaveBeenCalled();
    act(() => renderer.unmount());
    await mount(tool as Tool);
    expect(kimiApi.listKimiAccounts).toHaveBeenCalledOnce();
    for (const [name, action] of Object.entries(kimiApi))
      if (name !== 'listKimiAccounts' && vi.isMockFunction(action))
        expect(action).not.toHaveBeenCalled();
    expect(api.applyModelToTool).not.toHaveBeenCalled();
    expect(api.restoreToolToOfficial).not.toHaveBeenCalled();
    expect(api.startTool).not.toHaveBeenCalled();
    expect(state.applyError).toBeNull();
  }
);

it.each(['cn', 'global'] as const)(
  'Kimi: %s login saves only; account selection is exclusive with API models and explicit apply switches',
  async (region) => {
    vi.mocked(kimiApi.startKimiLogin).mockResolvedValue({
      loginId: 'kimi-login',
      verificationUri: `https://auth.kimi.${region === 'cn' ? 'com' : 'ai'}/activate`,
      expiresAt: Date.now() / 1000 + 60,
      pollIntervalSeconds: 1,
    });
    const account = { ...kimiFixture, region };
    vi.mocked(kimiApi.pollKimiLogin).mockResolvedValue(account);
    vi.mocked(kimiApi.switchKimiAccount).mockResolvedValue({ ...account, active: true });
    await mount('kimidesktop');
    act(() => state.kimiAccounts.setRegion(region));
    await tick();
    vi.mocked(kimiApi.listKimiAccounts).mockResolvedValue([account]);
    await act(async () => {
      await state.kimiAccounts.add();
    });
    expect(kimiApi.startKimiLogin).toHaveBeenCalledExactlyOnceWith(region);
    expect(kimiApi.refreshKimiAccount).not.toHaveBeenCalled();
    expect(kimiApi.switchKimiAccount).not.toHaveBeenCalled();
    expect(api.startTool).not.toHaveBeenCalled();
    act(() => state.handleSelectModel('kimidesktop', 'fixture-api-model'));
    expect(state.kimiAccounts.selectedId).toBeNull();
    act(() => state.kimiAccounts.select(account.id));
    expect(state.toolModelConfig.kimidesktop).toBeNull();
    await act(async () => {
      await state.handleLaunch();
    });
    expect(kimiApi.switchKimiAccount).toHaveBeenCalledExactlyOnceWith(account.id);
    expect(api.startTool).toHaveBeenCalledExactlyOnceWith('kimidesktop', undefined, undefined);
    expect(api.applyModelToTool).not.toHaveBeenCalled();
    expect(api.restoreToolToOfficial).not.toHaveBeenCalled();
    expect(folderPicker).not.toHaveBeenCalled();
  }
);

it('Kimi: the actual account panel exposes the shared region choice without starting login', async () => {
  await mount('kimidesktop');
  await act(async () => {
    renderer.update(
      <AppManagerProvider>
        <Harness panel />
      </AppManagerProvider>
    );
  });
  const button = renderer.root
    .findAllByType('button')
    .find((item) => item.props['aria-label'] === 'agent.kimiSwitchRegion');
  expect(button).toBeDefined();
  expect(button!.props.disabled).toBeFalsy();
  act(() => button!.props.onClick());
  await tick();
  expect(state.kimiAccounts.region).toBe('global');
  expect(JSON.stringify(renderer.toJSON())).toContain('kimi.ai');
  expect(kimiApi.startKimiLogin).not.toHaveBeenCalled();
  expect(kimiApi.refreshKimiAccount).not.toHaveBeenCalled();
});

it.each([true, false])(
  'Kimi CLI: workspace selection happens before account writes (cancelled=%s)',
  async (cancelled) => {
    vi.mocked(kimiApi.listKimiAccounts).mockResolvedValue([kimiFixture]);
    vi.mocked(kimiApi.switchKimiAccount).mockResolvedValue(kimiFixture);
    vi.mocked(folderPicker).mockResolvedValue(cancelled ? null : 'E:/fixture-project');
    await mount('kimicode');
    useToolsStore
      .getState()
      .setDetectedTools([
        { id: 'kimicode', name: 'Kimi CLI', category: 'CLI Code', installed: true },
      ]);
    await tick();
    act(() => state.kimiAccounts.select(kimiFixture.id));
    await act(async () => {
      await state.handleLaunch();
    });
    expect(folderPicker).toHaveBeenCalledOnce();
    if (cancelled) {
      expect(kimiApi.switchKimiAccount).not.toHaveBeenCalled();
      expect(api.startTool).not.toHaveBeenCalled();
    } else {
      expect(kimiApi.switchKimiAccount).toHaveBeenCalledExactlyOnceWith(kimiFixture.id);
      expect(api.startTool).toHaveBeenCalledExactlyOnceWith(
        'kimicode',
        undefined,
        'E:/fixture-project'
      );
    }
    expect(state.isLaunching).toBe(false);
  }
);

it.each(['account', 'model', 'return'] as const)(
  'Kimi: a pending folder choice cannot apply an old selection after %s changes',
  async (change) => {
    const other = { ...kimiFixture, id: 'kimi-other' };
    vi.mocked(kimiApi.listKimiAccounts).mockResolvedValue([kimiFixture, other]);
    let finishPicker!: (path: string) => void;
    vi.mocked(folderPicker).mockReturnValue(
      new Promise((resolve) => {
        finishPicker = resolve;
      })
    );
    await mount('kimicode');
    useToolsStore
      .getState()
      .setDetectedTools([
        { id: 'kimicode', name: 'Kimi CLI', category: 'CLI Code', installed: true },
      ]);
    await tick();
    act(() => state.kimiAccounts.select(kimiFixture.id));
    let launch!: Promise<void>;
    await act(async () => {
      launch = state.handleLaunch();
    });
    expect(folderPicker).toHaveBeenCalledOnce();
    if (change === 'account') act(() => state.kimiAccounts.select(other.id));
    if (change === 'model') act(() => state.handleSelectModel('kimicode', 'new-api-model'));
    if (change === 'return') {
      act(() => useNavigationStore.getState().setActivePage('models'));
      await tick();
      act(() => useNavigationStore.getState().setActivePage('apps'));
      await tick();
    }
    await act(async () => {
      finishPicker('E:/fixture-project');
      await launch;
    });
    expect(kimiApi.switchKimiAccount).not.toHaveBeenCalled();
    expect(api.startTool).not.toHaveBeenCalled();
    if (change === 'account') expect(state.kimiAccounts.selectedId).toBe(other.id);
    if (change === 'model') expect(state.toolModelConfig.kimicode).toBe('new-api-model');
    expect(state.applyError).toBeNull();
  }
);

it('Kimi: a late native switch does not erase a newer API selection or launch the client', async () => {
  vi.mocked(kimiApi.listKimiAccounts).mockResolvedValue([kimiFixture]);
  let finishSwitch!: (account: kimiApi.KimiAccount) => void;
  vi.mocked(kimiApi.switchKimiAccount).mockReturnValue(
    new Promise((resolve) => {
      finishSwitch = resolve;
    })
  );
  await mount('kimidesktop');
  act(() => state.kimiAccounts.select(kimiFixture.id));
  let launch!: Promise<void>;
  await act(async () => {
    launch = state.handleLaunch();
  });
  expect(kimiApi.switchKimiAccount).toHaveBeenCalledOnce();
  act(() => state.handleSelectModel('kimidesktop', 'new-api-model'));
  await act(async () => {
    finishSwitch(kimiFixture);
    await launch;
  });
  expect(state.toolModelConfig.kimidesktop).toBe('new-api-model');
  expect(api.startTool).not.toHaveBeenCalled();
  expect(state.applyError).toBeNull();
});

it('Kimi: changing tools cancels the pending login, locks its region and ignores a late account', async () => {
  let finishPoll!: (account: kimiApi.KimiAccount | null) => void;
  const poll = new Promise<kimiApi.KimiAccount | null>((resolve) => {
    finishPoll = resolve;
  });
  vi.mocked(kimiApi.startKimiLogin).mockResolvedValue({
    loginId: 'kimi-pending',
    verificationUri: 'https://auth.kimi.com/activate',
    expiresAt: Date.now() / 1000 + 60,
    pollIntervalSeconds: 1,
  });
  vi.mocked(kimiApi.pollKimiLogin).mockReturnValue(poll);
  await mount('kimidesktop');
  let login!: Promise<void>;
  await act(async () => {
    login = state.kimiAccounts.add();
  });
  expect(state.kimiAccounts.busy).toBe(true);
  act(() => state.kimiAccounts.setRegion('global'));
  expect(state.kimiAccounts.region).toBe('cn');
  act(() => state.setSelectedTool('kimicode'));
  await tick();
  expect(kimiApi.cancelKimiLogin).toHaveBeenCalledWith('kimi-pending');
  await act(async () => {
    finishPoll(kimiFixture);
    await login;
  });
  expect(state.kimiAccounts.accounts).toEqual([]);
  expect(state.kimiAccounts.busy).toBe(false);
  expect(kimiApi.switchKimiAccount).not.toHaveBeenCalled();
  expect(kimiApi.refreshKimiAccount).not.toHaveBeenCalled();
  expect(state.applyError).toBeNull();
});

it('Kimi: timeout cancels at 60 seconds without applying or refreshing an account', async () => {
  vi.mocked(kimiApi.startKimiLogin).mockResolvedValue({
    loginId: 'kimi-timeout',
    verificationUri: 'https://auth.kimi.com/activate',
    expiresAt: Date.now() / 1000 + 300,
    pollIntervalSeconds: 1,
  });
  vi.mocked(kimiApi.pollKimiLogin).mockResolvedValue(null);
  await mount('kimicode');
  await act(async () => {
    void state.kimiAccounts.add();
  });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_100);
  });
  expect(state.kimiAccounts.busy).toBe(false);
  expect(kimiApi.cancelKimiLogin).toHaveBeenCalledWith('kimi-timeout');
  expect(kimiApi.switchKimiAccount).not.toHaveBeenCalled();
  expect(kimiApi.refreshKimiAccount).not.toHaveBeenCalled();
});

it.each(['kimicode', 'kimidesktop'] as const)(
  'Kimi: %s explicit refresh accepts an account without quota and opens no error',
  async (tool) => {
    vi.mocked(kimiApi.listKimiAccounts).mockResolvedValue([kimiFixture]);
    await mount(tool);
    const empty = { ...kimiFixture, plan: 'Free', quotaWindows: [], wallet: null };
    vi.mocked(kimiApi.refreshKimiAccount).mockResolvedValue(empty);
    await act(async () => {
      await state.kimiAccounts.refresh(kimiFixture);
    });
    expect(kimiApi.refreshKimiAccount).toHaveBeenCalledExactlyOnceWith(kimiFixture.id);
    expect(state.kimiAccounts.accounts).toEqual([empty]);
    expect(state.kimiAccounts.selectedId).toBeNull();
    expect(state.kimiAccounts.refreshing.size).toBe(0);
    expect(state.applyError).toBeNull();
    expect(kimiApi.switchKimiAccount).not.toHaveBeenCalled();
    expect(api.applyModelToTool).not.toHaveBeenCalled();
    expect(api.startTool).not.toHaveBeenCalled();
  }
);

it('Kimi: passive failure keeps cached rows; failed explicit refresh keeps quota and retry updates it', async () => {
  vi.mocked(kimiApi.listKimiAccounts).mockResolvedValue([kimiFixture]);
  await mount('kimidesktop');
  act(() => useNavigationStore.getState().setActivePage('models'));
  await tick();
  vi.mocked(kimiApi.listKimiAccounts).mockRejectedValueOnce(new Error('accountError.read'));
  act(() => useNavigationStore.getState().setActivePage('apps'));
  await tick();
  expect(state.kimiAccounts.accounts).toEqual([kimiFixture]);
  expect(state.applyError).toBeNull();
  expect(kimiApi.refreshKimiAccount).not.toHaveBeenCalled();
  vi.mocked(kimiApi.refreshKimiAccount).mockRejectedValueOnce(new Error('accountError.quota'));
  await act(async () => {
    await state.kimiAccounts.refresh(kimiFixture);
  });
  expect(state.kimiAccounts.accounts).toEqual([kimiFixture]);
  expect(state.kimiAccounts.selectedId).toBeNull();
  expect(state.applyError).toBe('accountError.quota');
  const updated = {
    ...kimiFixture,
    plan: 'Ultra',
    quotaWindows: [{ name: 'limit_7d', remainingPercent: 35 }],
  };
  vi.mocked(kimiApi.refreshKimiAccount).mockResolvedValue(updated);
  await act(async () => {
    await state.kimiAccounts.refresh(kimiFixture);
  });
  expect(state.kimiAccounts.accounts).toEqual([updated]);
  expect(kimiApi.switchKimiAccount).not.toHaveBeenCalled();
});

it('Open Design: the actual panel keeps accounts and the official model section together without API models', async () => {
  await mount('opendesign');
  await act(async () => {
    renderer.update(
      <AppManagerProvider>
        <Harness panel />
      </AppManagerProvider>
    );
  });
  const markup = JSON.stringify(renderer.toJSON());
  expect(markup).toContain('/icons/tools/opendesign.png');
  expect(markup).toContain('agent.modelSwitch');
  expect(markup).toContain('Open Design Official');
  expect(api.applyModelToTool).not.toHaveBeenCalled();
  expect(api.restoreToolToOfficial).not.toHaveBeenCalled();
});

it('Open Design: account and API selection are exclusive and only explicit apply writes the model', async () => {
  vi.mocked(api.listOpenDesignAccounts).mockResolvedValue([openDesignFixture]);
  vi.mocked(api.getModels).mockResolvedValue([
    {
      internalId: 'design-model',
      name: 'Fixture',
      modelId: 'fixture-model',
      baseUrl: 'https://fixture.test/v1',
      apiKey: 'fixture-key',
    },
  ]);
  vi.mocked(api.applyModelToTool).mockResolvedValue({ success: true, message: '' });
  await mount('opendesign');
  act(() => state.handleSelectModel('opendesign', 'design-model'));
  await tick();
  act(() => state.openDesignAccounts.select(openDesignFixture.id));
  expect(state.toolModelConfig.opendesign).toBeNull();
  act(() => state.handleSelectModel('opendesign', 'design-model'));
  expect(state.openDesignAccounts.selectedId).toBeNull();
  expect(api.applyModelToTool).not.toHaveBeenCalled();
  expect(api.switchOpenDesignAccount).not.toHaveBeenCalled();
  expect(api.restoreToolToOfficial).not.toHaveBeenCalled();
  act(() => state.setLaunchAfterApply(false));
  await act(async () => {
    await state.handleLaunch();
  });
  expect(api.applyModelToTool).toHaveBeenCalledWith(
    'opendesign',
    expect.objectContaining({ model: 'fixture-model', protocol: 'openai' })
  );
  expect(api.switchOpenDesignAccount).not.toHaveBeenCalled();
  expect(api.startTool).not.toHaveBeenCalled();
  expect(folderPicker).not.toHaveBeenCalled();
});

it.each([true, false])(
  'Open Design: restoring official is explicit and failed restore prevents launch (%s)',
  async (success) => {
    await mount('opendesign');
    act(() => state.handleSelectModel('opendesign', '__official__opendesign'));
    expect(api.restoreToolToOfficial).not.toHaveBeenCalled();
    vi.mocked(api.restoreToolToOfficial).mockResolvedValue({
      success,
      message: 'native-settings-error',
    });
    await act(async () => {
      await state.handleLaunch();
    });
    expect(api.restoreToolToOfficial).toHaveBeenCalledExactlyOnceWith('opendesign');
    expect(api.applyModelToTool).not.toHaveBeenCalled();
    expect(api.switchOpenDesignAccount).not.toHaveBeenCalled();
    if (success) expect(api.startTool).toHaveBeenCalledWith('opendesign', undefined, undefined);
    else {
      expect(api.startTool).not.toHaveBeenCalled();
      expect(state.applyError).toBe('native-settings-error');
    }
    expect(folderPicker).not.toHaveBeenCalled();
  }
);

it('Open Design: adding only saves the account; explicit apply switches and launches the desktop client', async () => {
  vi.mocked(api.startOpenDesignLogin).mockResolvedValue({
    loginId: 'design-login',
    verificationUri: 'https://open-design.ai/cloud/cli/activate?code=fixture',
    expiresAt: Date.now() / 1000 + 60,
    pollIntervalSeconds: 1,
  });
  vi.mocked(api.pollOpenDesignLogin).mockResolvedValue(openDesignFixture);
  await mount('opendesign');
  vi.mocked(api.listOpenDesignAccounts).mockResolvedValue([openDesignFixture]);
  await act(async () => {
    await state.openDesignAccounts.add();
  });
  await tick();
  expect(state.openDesignAccounts.accounts).toEqual([openDesignFixture]);
  expect(api.openExternal).toHaveBeenCalledWith(
    'https://open-design.ai/cloud/cli/activate?code=fixture'
  );
  expect(api.refreshOpenDesignAccount).not.toHaveBeenCalled();
  expect(api.switchOpenDesignAccount).not.toHaveBeenCalled();
  expect(api.startTool).not.toHaveBeenCalled();
  act(() => state.openDesignAccounts.select(openDesignFixture.id));
  await act(async () => {
    await state.handleLaunch();
  });
  expect(api.switchOpenDesignAccount).toHaveBeenCalledExactlyOnceWith(openDesignFixture.id);
  expect(api.startTool).toHaveBeenCalledExactlyOnceWith('opendesign');
  expect(api.applyModelToTool).not.toHaveBeenCalled();
  expect(api.restoreToolToOfficial).not.toHaveBeenCalled();
  expect(folderPicker).not.toHaveBeenCalled();
});

it('Open Design: passive read failure keeps the cached identity without actions or dialogs', async () => {
  vi.mocked(api.listOpenDesignAccounts).mockResolvedValueOnce([openDesignFixture]);
  await mount('opendesign');
  expect(state.openDesignAccounts.accounts).toEqual([openDesignFixture]);
  act(() => useNavigationStore.getState().setActivePage('models'));
  await tick();
  vi.mocked(api.listOpenDesignAccounts).mockRejectedValueOnce(new Error('accountError.read'));
  act(() => useNavigationStore.getState().setActivePage('apps'));
  await tick();
  expect(state.openDesignAccounts.accounts).toEqual([openDesignFixture]);
  expect(state.applyError).toBeNull();
  expect(api.refreshOpenDesignAccount).not.toHaveBeenCalled();
  expect(api.startOpenDesignLogin).not.toHaveBeenCalled();
  expect(api.switchOpenDesignAccount).not.toHaveBeenCalled();
});

it('Open Design: refresh and delete controls preserve account selection and unknown quota stays unknown', async () => {
  vi.mocked(api.listOpenDesignAccounts).mockResolvedValue([openDesignFixture]);
  await mount('opendesign');
  act(() =>
    renderer.update(
      <AppManagerProvider>
        <Harness panel />
      </AppManagerProvider>
    )
  );
  act(() => state.openDesignAccounts.select(openDesignFixture.id));
  vi.mocked(api.refreshOpenDesignAccount).mockResolvedValue({
    ...openDesignFixture,
    quotaWindows: [],
    balanceUsd: null,
  });
  const row = () => renderer.root.findByType(AccountSectionRow);
  expect(renderToStaticMarkup(<>{row().props.secondary}</>)).toContain('75%');
  await act(async () => {
    await row().props.onRefresh();
  });
  expect(state.openDesignAccounts.selectedId).toBe(openDesignFixture.id);
  expect(renderToStaticMarkup(<>{row().props.secondary}</>)).toContain('—');
  expect(api.switchOpenDesignAccount).not.toHaveBeenCalled();
  vi.mocked(api.deleteOpenDesignAccount).mockResolvedValue(undefined);
  await act(async () => {
    await row().props.onDelete();
  });
  expect(api.deleteOpenDesignAccount).toHaveBeenCalledWith(openDesignFixture.id);
  expect(api.startTool).not.toHaveBeenCalled();
});
type Tool = keyof typeof listFor;
async function tick() {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(1);
  });
}
async function mount(
  tool: Tool | api.KiloTool | api.CopilotTool | 'kimicode' | 'kimidesktop',
  installed = true
) {
  useToolsStore.getState().setDetectedTools([
    {
      id: tool,
      name: tool,
      category: ['grok', 'antigravity', 'kilo', 'copilot', 'kirocli'].includes(tool)
        ? 'CLI Code'
        : 'Desktop',
      noModelConfig: [
        'antigravity',
        'antigravitydesktop',
        'cursor',
        'cursor',
        'trae',
        'traecn',
        'kirocrew',
        'kirocli',
      ].includes(tool),
      installed,
    },
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
  applyErrorEvents.length = 0;
  vi.clearAllMocks();
  vi.useFakeTimers();
  for (const name of listNames) vi.mocked(api[name]).mockResolvedValue([]);
  vi.mocked(api.listClineAccounts).mockResolvedValue([]);
  vi.mocked(api.listKiloAccounts).mockResolvedValue([]);
  vi.mocked(api.listCopilotAccounts).mockResolvedValue([]);
  vi.mocked(kimiApi.listKimiAccounts).mockResolvedValue([]);
  vi.mocked(kimiApi.cancelKimiLogin).mockResolvedValue(undefined);
  vi.mocked(api.getModels).mockResolvedValue([]);
  for (const [name, action] of Object.entries(api))
    if (name.startsWith('cancel') && vi.isMockFunction(action)) action.mockResolvedValue(undefined);
  vi.mocked(api.startCodexLogin).mockResolvedValue('fixture-login');
  vi.mocked(api.restoreToolToOfficial).mockResolvedValue({ success: true, message: '' });
  vi.mocked(api.startTool).mockResolvedValue(undefined);
  vi.mocked(api.getSettings).mockResolvedValue({ developerMode: false, locale: 'en' });
  vi.mocked(api.saveSettings).mockResolvedValue(undefined);
  vi.mocked(folderPicker).mockResolvedValue('E:/fixture-project');
  useNavigationStore.setState(useNavigationStore.getInitialState(), true);
  enableAccountPlugins();
});

async function showAccountCenter(tools: Tool[]) {
  await mount(tools[0]);
  act(() => {
    useToolsStore.getState().setDetectedTools(
      tools.map((id) => ({
        id,
        name: id,
        category: 'Desktop',
        installed: true,
      }))
    );
    useNavigationStore.getState().setActivePage('accounts');
  });
  await tick();
  await act(async () => {
    renderer.update(
      <AppManagerProvider>
        <Harness center />
      </AppManagerProvider>
    );
  });
}

function mockAccountOrderStorage(raw?: string) {
  const values = new Map(raw ? [['echobird_account_card_order', raw]] : []);
  const setItem = vi.fn((key: string, value: string) => void values.set(key, value));
  const cacheWrites = vi.fn((key: string, value: string) => void values.set(key, value));
  const storage = {
    getItem: vi.fn((key: string) => values.get(key) ?? null),
    setItem: (key: string, value: string) => {
      if (key === 'echobird_account_card_order') setItem(key, value);
      else if (key === 'echobird_account_card_cache') cacheWrites(key, value);
      else values.set(key, value);
    },
  };
  vi.stubGlobal('localStorage', storage);
  return { getItem: storage.getItem, setItem, cacheWrites };
}

const kiloFixture: api.KiloAccount = {
  id: 'kilo-fixture',
  email: 'kilo@example.test',
  active: true,
  plan: null,
  balance: null,
};
const copilotFixture: api.CopilotAccount = {
  id: 'copilot-fixture',
  login: 'github-user',
  host: 'github.com',
  active: true,
  plan: null,
  remaining: null,
  total: null,
  resetAt: null,
};
const clientCases = [
  {
    family: 'kilo',
    tool: 'kilo',
    list: 'listKiloAccounts',
    apply: 'switchKiloAccount',
    refresh: 'refreshKiloAccount',
  },
  {
    family: 'kilo',
    tool: 'kilodesktop',
    list: 'listKiloAccounts',
    apply: 'switchKiloAccount',
    refresh: 'refreshKiloAccount',
  },
  {
    family: 'copilot',
    tool: 'copilot',
    list: 'listCopilotAccounts',
    apply: 'switchCopilotAccount',
    refresh: 'refreshCopilotAccount',
  },
  {
    family: 'copilot',
    tool: 'copilotdesktop',
    list: 'listCopilotAccounts',
    apply: 'switchCopilotAccount',
    refresh: 'refreshCopilotAccount',
  },
] as const;
const clientGroup = (family: 'kilo' | 'copilot') =>
  family === 'kilo' ? state.kiloAccounts : state.copilotAccounts;

it.each(['kilo', 'copilot'] as const)(
  '%s: successful login saves the summary without quota refresh or native apply',
  async (family) => {
    const start = family === 'kilo' ? 'startKiloLogin' : 'startCopilotLogin';
    const poll = family === 'kilo' ? 'pollKiloLogin' : 'pollCopilotLogin';
    const refresh = family === 'kilo' ? 'refreshKiloAccount' : 'refreshCopilotAccount';
    const apply = family === 'kilo' ? 'switchKiloAccount' : 'switchCopilotAccount';
    const row = family === 'kilo' ? kiloFixture : copilotFixture;
    vi.mocked(api[start]).mockResolvedValue({
      loginId: 'approved-login',
      verificationUri: 'https://kilo.ai/login',
      expiresAt: Date.now() / 1000 + 60,
    } as never);
    vi.mocked(api[poll]).mockResolvedValue(row as never);
    await mount(family);
    vi.mocked(
      api[family === 'kilo' ? 'listKiloAccounts' : 'listCopilotAccounts']
    ).mockResolvedValue([row] as never);
    await act(async () => {
      await clientGroup(family).add();
    });
    expect(clientGroup(family).accounts).toEqual([row]);
    expect(api[refresh]).not.toHaveBeenCalled();
    expect(api[apply]).not.toHaveBeenCalled();
    expect(api.startTool).not.toHaveBeenCalled();
    expect(clientGroup(family).busy).toBe(false);
  }
);

it.each(['kilo', 'copilot'] as const)(
  '%s: account rows isolate refresh and nested keyboard actions while preserving selection',
  async (family) => {
    const list = family === 'kilo' ? 'listKiloAccounts' : 'listCopilotAccounts';
    const refreshName = family === 'kilo' ? 'refreshKiloAccount' : 'refreshCopilotAccount';
    const row = family === 'kilo' ? kiloFixture : copilotFixture;
    vi.mocked(api[list]).mockResolvedValue([row] as never);
    await mount(family);
    await act(async () => {
      renderer.update(
        <AppManagerProvider>
          <Harness panel />
        </AppManagerProvider>
      );
    });
    const identity = family === 'kilo' ? kiloFixture.email : copilotFixture.login;
    const radio = renderer.root.findByProps({ role: 'radio' });
    expect(radio.props.className).toContain('h-12');
    const preventDefault = vi.fn();
    act(() =>
      radio.props.onKeyDown({ key: 'Enter', target: {}, currentTarget: radio, preventDefault })
    );
    expect(preventDefault).not.toHaveBeenCalled();
    act(() =>
      radio.props.onKeyDown({ key: ' ', target: radio, currentTarget: radio, preventDefault })
    );
    expect(preventDefault).toHaveBeenCalledOnce();
    expect(clientGroup(family).selectedId).toBe(row.id);
    let complete!: (value: unknown) => void;
    vi.mocked(api[refreshName]).mockReturnValue(
      new Promise((resolve) => {
        complete = resolve;
      }) as never
    );
    const stopPropagation = vi.fn();
    await act(async () => {
      renderer.root
        .findByProps({ 'aria-label': `agent.refreshAccount ${identity}` })
        .props.onClick({ stopPropagation });
    });
    expect(stopPropagation).toHaveBeenCalledOnce();
    expect(
      renderer.root.findByProps({ 'aria-label': `agent.refreshAccount ${identity}` }).props.disabled
    ).toBe(true);
    expect(clientGroup(family).selectedId).toBe(row.id);
    await act(async () => {
      complete(row);
    });
    expect(
      renderer.root.findByProps({ 'aria-label': `agent.refreshAccount ${identity}` }).props.disabled
    ).toBe(false);
    expect(clientGroup(family).selectedId).toBe(row.id);
    expect(api[refreshName]).toHaveBeenCalledExactlyOnceWith(row.id);
  }
);

it.each(clientCases)(
  '$tool: passive navigation preserves cached rows and never applies or refreshes',
  async (client) => {
    const row = client.family === 'kilo' ? kiloFixture : copilotFixture;
    vi.mocked(api[client.list]).mockResolvedValue([row] as never);
    await mount(client.tool);
    expect(api[client.list]).toHaveBeenCalledTimes(1);
    if (client.family === 'copilot')
      expect(api.listCopilotAccounts).toHaveBeenCalledWith(client.tool);
    expect(clientGroup(client.family).accounts).toEqual([row]);
    for (const [name, action] of Object.entries(api)) {
      if (/^(start|refresh|switch|delete|claim|restore)/.test(name) && vi.isMockFunction(action))
        expect(action).not.toHaveBeenCalled();
    }
    act(() => useNavigationStore.getState().setActivePage('models'));
    await tick();
    vi.mocked(api[client.list]).mockRejectedValueOnce(new Error('accountError.read'));
    act(() => useNavigationStore.getState().setActivePage('apps'));
    await tick();
    expect(clientGroup(client.family).accounts).toEqual([row]);
    expect(state.applyError).toBeNull();
    expect(api[client.apply]).not.toHaveBeenCalled();
    expect(api[client.refresh]).not.toHaveBeenCalled();
    expect(folderPicker).not.toHaveBeenCalled();
  }
);

it('Copilot CLI and Desktop load their own saved selection during passive tool switching', async () => {
  const cliRow = { ...copilotFixture, id: 'cli-saved', login: 'cli-user' };
  const desktopRow = { ...copilotFixture, id: 'desktop-saved', login: 'desktop-user' };
  vi.mocked(api.listCopilotAccounts).mockImplementation(async (toolId) =>
    toolId === 'copilotdesktop' ? [desktopRow] : [cliRow]
  );
  await mount('copilot');
  act(() =>
    useToolsStore.getState().setDetectedTools([
      { id: 'copilot', name: 'Copilot CLI', category: 'CLI Code', installed: true },
      { id: 'copilotdesktop', name: 'Copilot Desktop', category: 'Desktop', installed: true },
    ])
  );
  expect(state.copilotAccounts.selectedId).toBe(cliRow.id);
  act(() => state.setSelectedTool('copilotdesktop'));
  await tick();
  expect(api.listCopilotAccounts).toHaveBeenLastCalledWith('copilotdesktop');
  expect(state.copilotAccounts.selectedId).toBe(desktopRow.id);
  act(() => state.setSelectedTool('copilot'));
  await tick();
  expect(api.listCopilotAccounts).toHaveBeenLastCalledWith('copilot');
  expect(state.copilotAccounts.selectedId).toBe(cliRow.id);
  expect(api.startCopilotLogin).not.toHaveBeenCalled();
  expect(api.refreshCopilotAccount).not.toHaveBeenCalled();
  expect(api.switchCopilotAccount).not.toHaveBeenCalled();
  expect(api.startTool).not.toHaveBeenCalled();
});

it.each(clientCases)(
  '$tool: an uninstalled tool loads saved accounts only on Account Management',
  async (client) => {
    await mount(client.tool, false);
    expect(api[client.list]).not.toHaveBeenCalled();
    act(() => useNavigationStore.getState().setActivePage('accounts'));
    await tick();
    // Saved EchoBird accounts stay manageable; nothing logs in, refreshes or applies.
    expect(api[client.list]).toHaveBeenCalled();
    expect(api[client.apply]).not.toHaveBeenCalled();
    expect(api[client.refresh]).not.toHaveBeenCalled();
    expect(api.startTool).not.toHaveBeenCalled();
  }
);

it.each(clientCases)(
  '$tool: positive control applies the account before native launch',
  async (client) => {
    const row = client.family === 'kilo' ? kiloFixture : copilotFixture;
    vi.mocked(api[client.list]).mockResolvedValue([row] as never);
    await mount(client.tool);
    act(() => state.setLaunchAfterApply(true));
    let complete!: (value: unknown) => void;
    vi.mocked(api[client.apply]).mockReturnValueOnce(
      new Promise((resolve) => {
        complete = resolve;
      }) as never
    );
    let task!: Promise<void>;
    act(() => {
      task = state.handleLaunch();
    });
    await tick();
    expect(api[client.apply]).toHaveBeenCalledExactlyOnceWith(row.id, client.tool);
    expect(api.startTool).not.toHaveBeenCalled();
    await act(async () => {
      await state.handleLaunch();
    });
    expect(api[client.apply]).toHaveBeenCalledTimes(1);
    await act(async () => {
      complete(row);
      await task;
    });
    expect(api.startTool).toHaveBeenCalledWith(
      client.tool,
      undefined,
      client.tool === 'kilo' || client.tool === 'copilot' ? 'E:/fixture-project' : undefined
    );
    expect(api[client.refresh]).not.toHaveBeenCalled();
    expect(state.applyError).toBeNull();
  }
);

it.each(clientCases)(
  '$tool: native apply failure remains visible and prevents launch',
  async (client) => {
    const row = client.family === 'kilo' ? kiloFixture : copilotFixture;
    vi.mocked(api[client.list]).mockResolvedValue([row] as never);
    vi.mocked(api[client.apply]).mockRejectedValueOnce(new Error('accountError.write'));
    await mount(client.tool);
    await act(async () => {
      await state.handleLaunch();
    });
    expect(clientGroup(client.family).accounts).toEqual([row]);
    expect(state.applyError).toBe('accountError.write');
    expect(api.startTool).not.toHaveBeenCalled();
  }
);

it.each(clientCases)(
  '$tool: account and API model choices are exclusive without native writes',
  async (client) => {
    vi.mocked(api[client.list]).mockResolvedValue([
      client.family === 'kilo' ? kiloFixture : copilotFixture,
    ] as never);
    await mount(client.tool);
    act(() => state.handleSelectModel(client.tool, 'fixture-model'));
    expect(clientGroup(client.family).selectedId).toBeNull();
    act(() => clientGroup(client.family).select('chosen-account'));
    expect(state.toolModelConfig[client.tool]).toBeNull();
    expect(clientGroup(client.family).selectedId).toBe('chosen-account');
    expect(api[client.apply]).not.toHaveBeenCalled();
  }
);

it.each(['kilo', 'copilot'] as const)(
  '%s: switching between CLI and Desktop cancels login and ignores late completion',
  async (family) => {
    const expiresAt = Date.now() / 1000 + 60;
    const start = family === 'kilo' ? 'startKiloLogin' : 'startCopilotLogin';
    const poll = family === 'kilo' ? 'pollKiloLogin' : 'pollCopilotLogin';
    const cancel = family === 'kilo' ? 'cancelKiloLogin' : 'cancelCopilotLogin';
    vi.mocked(api[start]).mockResolvedValue({
      loginId: 'fixture-login',
      verificationUri: 'https://kilo.ai/login',
      expiresAt,
    } as never);
    let resolve!: (value: unknown) => void;
    vi.mocked(api[poll]).mockReturnValue(
      new Promise((done) => {
        resolve = done;
      }) as never
    );
    await mount(family);
    act(() =>
      useToolsStore.getState().setDetectedTools([
        { id: family, name: family, category: 'CLI Code', installed: true },
        { id: `${family}desktop`, name: family, category: 'Desktop', installed: true },
      ])
    );
    let task!: Promise<void>;
    await act(async () => {
      task = clientGroup(family).add();
    });
    expect(api[start]).toHaveBeenCalledTimes(1);
    if (family === 'kilo') expect(api.openExternal).toHaveBeenCalledWith('https://kilo.ai/login');
    else expect(api.openExternal).not.toHaveBeenCalled();
    act(() => state.setSelectedTool(`${family}desktop`));
    await tick();
    expect(api[cancel]).toHaveBeenCalledWith('fixture-login');
    await act(async () => {
      resolve(family === 'kilo' ? kiloFixture : copilotFixture);
      await task;
    });
    expect(clientGroup(family).accounts).toEqual([]);
    expect(clientGroup(family).busy).toBe(false);
  }
);

it.each(['kilo', 'copilot'] as const)(
  '%s: login has a 60-second deadline and adding never refreshes quota',
  async (family) => {
    const start = family === 'kilo' ? 'startKiloLogin' : 'startCopilotLogin';
    const poll = family === 'kilo' ? 'pollKiloLogin' : 'pollCopilotLogin';
    const cancel = family === 'kilo' ? 'cancelKiloLogin' : 'cancelCopilotLogin';
    const refresh = family === 'kilo' ? 'refreshKiloAccount' : 'refreshCopilotAccount';
    vi.mocked(api[start]).mockResolvedValue({
      loginId: 'deadline-login',
      verificationUri: 'https://kilo.ai/login',
      expiresAt: Date.now() / 1000 + 600,
    } as never);
    vi.mocked(api[poll]).mockResolvedValue(null);
    await mount(family);
    let task!: Promise<void>;
    await act(async () => {
      task = clientGroup(family).add();
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_001);
      await task;
    });
    expect(api[cancel]).toHaveBeenCalledWith('deadline-login');
    expect(clientGroup(family).busy).toBe(false);
    expect(api[refresh]).not.toHaveBeenCalled();
  }
);

function accountCardIds() {
  return renderer.root
    .findByType(AccountCenterMain)
    .findAll((node) => node.type === 'div' && node.props['data-sortable-card'])
    .map((node) => node.props['data-sortable-card']);
}

function accountCardDnd() {
  return renderer.root
    .findByType(AccountCenterMain)
    .find((node) => typeof node.props.onDragEnd === 'function');
}

function dropAccountCard(activeId: string, overId: string | null) {
  act(() => {
    accountCardDnd().props.onDragEnd({
      active: { id: activeId },
      over: overId == null ? null : { id: overId },
    });
  });
}

function pendingAccountList<T>() {
  let resolve!: (rows: T[]) => void;
  const promise = new Promise<T[]>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

async function mountAccountCenter(tools: Tool[]) {
  useToolsStore
    .getState()
    .setDetectedTools(tools.map((id) => ({ id, name: id, category: 'Desktop', installed: true })));
  useNavigationStore.getState().setActivePage('accounts');
  await act(async () => {
    renderer = create(
      <AppManagerProvider>
        <Harness center />
      </AppManagerProvider>
    );
  });
}

function expectPassiveAccountNavigation() {
  for (const [key, action] of Object.entries(api))
    if (/^(start|refresh|switch|delete|claim|restore|apply)/.test(key) && vi.isMockFunction(action))
      expect(action, key).not.toHaveBeenCalled();
  expect(folderPicker).not.toHaveBeenCalled();
  expect(shellOpen).not.toHaveBeenCalled();
  expect(state.applyError).toBeNull();
  expect(applyErrorEvents.filter(Boolean)).toEqual([]);
}

it('Account Center: cached apps wait for current detection before loading accounts', async () => {
  useToolsStore.setState({
    detectedTools: ['codex', 'workbuddy'].map((id) => ({
      id,
      name: id,
      category: 'Desktop',
      installed: true,
    })),
    usingCachedTools: true,
    hasScanned: false,
  });
  useNavigationStore.getState().setActivePage('accounts');
  await act(async () => {
    renderer = create(
      <AppManagerProvider>
        <Harness center />
      </AppManagerProvider>
    );
  });
  await tick();
  expect(api.listCodexAccounts).not.toHaveBeenCalled();
  expect(api.listWorkBuddyAccounts).not.toHaveBeenCalled();
  act(() =>
    useToolsStore.getState().setDetectedTools([
      { id: 'codex', name: 'Codex', category: 'Desktop', installed: false },
      { id: 'workbuddy', name: 'WorkBuddy', category: 'Desktop', installed: true },
    ])
  );
  await tick();
  // Saved accounts load for an uninstalled client too; its backend skips client files.
  expect(api.listCodexAccounts).toHaveBeenCalledTimes(1);
  expect(api.listWorkBuddyAccounts).toHaveBeenCalledWith('workbuddy');
  expectPassiveAccountNavigation();
});

it('Usage Island: only its developer tab loads saved accounts, without querying quota', async () => {
  useToolsStore
    .getState()
    .setDetectedTools([{ id: 'codex', name: 'Codex', category: 'Desktop', installed: true }]);
  useNavigationStore.getState().setDeveloperMode(true);
  useNavigationStore.getState().setActivePage('developer');
  useNavigationStore.getState().setDeveloperTab('statistics');
  await act(async () => {
    renderer = create(
      <AppManagerProvider>
        <Harness center />
      </AppManagerProvider>
    );
  });
  await tick();
  expect(api.listCodexAccounts).not.toHaveBeenCalled();
  act(() => useNavigationStore.getState().setDeveloperTab('usageIsland'));
  await tick();
  expect(api.listCodexAccounts).toHaveBeenCalledTimes(1);
  expectPassiveAccountNavigation();
});

it('Usage Island: the open window loads saved accounts on other pages without querying quota', async () => {
  useToolsStore
    .getState()
    .setDetectedTools([{ id: 'codex', name: 'Codex', category: 'Desktop', installed: true }]);
  useNavigationStore.getState().setDeveloperMode(true);
  useNavigationStore.getState().setActivePage('apps');
  useUsageIslandStore.setState({ enabled: true, sources: ['model:m'] });
  await act(async () => {
    renderer = create(
      <AppManagerProvider>
        <Harness center />
      </AppManagerProvider>
    );
  });
  await tick();
  expect(api.listCodexAccounts).toHaveBeenCalledTimes(1);
  expectPassiveAccountNavigation();
  useUsageIslandStore.setState({ enabled: false, sources: [] });
});

it('Smart Access: cached apps wait for current detection before loading native accounts', async () => {
  vi.mocked(api.listDuMateAccounts).mockResolvedValue([]);
  vi.mocked(api.listDoubaoAccounts).mockResolvedValue([]);
  vi.mocked(codeArtsApi.listCodeArtsAccounts).mockResolvedValue([]);
  const tools = ['dumate', 'doubaowork', 'dola', 'codearts'].map((id) => ({
    id,
    name: id,
    category: 'Desktop',
    installed: true,
  }));
  useToolsStore.setState({ detectedTools: tools, usingCachedTools: true, hasScanned: false });
  useNavigationStore.getState().setActivePage('freeModels');
  await act(async () => {
    renderer = create(
      <AppManagerProvider>
        <Harness />
      </AppManagerProvider>
    );
  });
  await tick();
  expect(api.listDuMateAccounts).not.toHaveBeenCalled();
  expect(api.listDoubaoAccounts).not.toHaveBeenCalled();
  expect(codeArtsApi.listCodeArtsAccounts).not.toHaveBeenCalled();
  act(() =>
    useToolsStore
      .getState()
      .setDetectedTools(tools.map((tool) => ({ ...tool, installed: tool.id !== 'dola' })))
  );
  await tick();
  expect(api.listDuMateAccounts).toHaveBeenCalledOnce();
  expect(api.listDoubaoAccounts).toHaveBeenCalledExactlyOnceWith('doubaowork');
  expect(codeArtsApi.listCodeArtsAccounts).toHaveBeenCalledOnce();
  expectPassiveAccountNavigation();
});

it('Account Center: first load keeps the skeleton until all installed providers settle', async () => {
  const storage = mockAccountOrderStorage();
  const codex = pendingAccountList<api.CodexAccount>();
  const workBuddy = pendingAccountList<api.WorkBuddyAccount>();
  vi.mocked(api.listCodexAccounts).mockReturnValueOnce(codex.promise);
  vi.mocked(api.listWorkBuddyAccounts).mockImplementation((edition) =>
    edition === 'workbuddy' ? workBuddy.promise : Promise.resolve([])
  );
  await mountAccountCenter(['codex', 'workbuddy']);
  expect(accountCardIds()).toEqual([]);
  expect(renderer.root.findByProps({ 'aria-label': 'accountCenter.loading' })).toBeDefined();
  expect(api.listCodexAccounts).not.toHaveBeenCalled();
  expect(api.listWorkBuddyAccounts).not.toHaveBeenCalled();
  await tick();
  expect(api.listCodexAccounts).toHaveBeenCalledTimes(1);
  expect(api.listWorkBuddyAccounts).toHaveBeenCalledWith('workbuddy');
  await act(async () => workBuddy.resolve([rewardAccount]));
  expect(accountCardIds()).toEqual([]);
  expect(renderer.root.findByProps({ 'aria-label': 'accountCenter.loading' })).toBeDefined();
  expect(storage.cacheWrites).not.toHaveBeenCalled();
  const claimAll = renderer.root.findByProps({ 'aria-label': 'accountCenter.claimAll' });
  const refreshAll = renderer.root.findByProps({ 'aria-label': 'accountCenter.refreshAll' });
  expect(claimAll.props.disabled).toBe(true);
  expect(refreshAll.props.disabled).toBe(true);
  await act(async () => {
    await Promise.all([claimAll.props.onClick(), refreshAll.props.onClick()]);
  });
  expect(api.claimWorkBuddyDailyCredits).not.toHaveBeenCalled();
  expect(api.refreshWorkBuddyAccountQuota).not.toHaveBeenCalled();
  expect(api.refreshCodexAccountQuota).not.toHaveBeenCalled();
  await act(async () =>
    codex.resolve([{ id: 'first', email: 'first@example.test', active: false }])
  );
  expect(accountCardIds()).toEqual(['codex:first', 'workbuddy:reward']);
  expect(renderer.root.findAllByProps({ 'aria-label': 'accountCenter.loading' })).toEqual([]);
  expect(claimAll.props.disabled).toBe(false);
  expect(refreshAll.props.disabled).toBe(false);
  expect(storage.cacheWrites).toHaveBeenCalledTimes(1);
  expectPassiveAccountNavigation();
});

it('Account Center: remount shows the persisted complete grid and replaces it only after every background read', async () => {
  mockAccountOrderStorage();
  vi.mocked(api.listCodexAccounts).mockResolvedValue([
    { id: 'first', email: 'first@example.test', active: false },
  ]);
  vi.mocked(api.listDeepSeekAccounts).mockResolvedValue([
    { id: 'second', name: 'Second', active: false, balances: null },
  ]);
  await mountAccountCenter(['codex', 'dsh']);
  await tick();
  expect(accountCardIds()).toEqual(['codex:first', 'dsh:second']);
  act(() => renderer.unmount());
  const codex = pendingAccountList<api.CodexAccount>();
  const deepSeek = pendingAccountList<api.DeepSeekAccount>();
  vi.mocked(api.listCodexAccounts).mockReturnValueOnce(codex.promise);
  vi.mocked(api.listDeepSeekAccounts).mockReturnValueOnce(deepSeek.promise);
  await mountAccountCenter(['codex', 'dsh']);
  expect(accountCardIds()).toEqual(['codex:first', 'dsh:second']);
  expect(renderer.root.findAllByProps({ 'aria-label': 'accountCenter.loading' })).toEqual([]);
  expect(
    renderer.root.findByProps({ 'aria-label': 'agent.refreshAccount first@example.test' }).props
      .disabled
  ).toBe(true);
  await tick();
  await act(async () => codex.resolve([{ id: 'new', email: 'new@example.test', active: false }]));
  expect(accountCardIds()).toEqual(['codex:first', 'dsh:second']);
  expect(renderer.root.findAllByProps({ 'aria-label': 'model.dragSort new@example.test' })).toEqual(
    []
  );
  const claimAll = renderer.root.findByProps({ 'aria-label': 'accountCenter.claimAll' });
  const refreshAll = renderer.root.findByProps({ 'aria-label': 'accountCenter.refreshAll' });
  expect(claimAll.props.disabled).toBe(true);
  expect(refreshAll.props.disabled).toBe(true);
  await act(async () => {
    await claimAll.props.onClick();
    await refreshAll.props.onClick();
  });
  expect(api.refreshCodexAccountQuota).not.toHaveBeenCalled();
  expect(api.refreshDeepSeekAccountQuota).not.toHaveBeenCalled();
  await act(async () =>
    deepSeek.resolve([{ id: 'third', name: 'Third', active: false, balances: null }])
  );
  expect(accountCardIds()).toEqual(['codex:new', 'dsh:third']);
  expect(
    renderer.root.findByProps({ 'aria-label': 'agent.refreshAccount new@example.test' }).props
      .disabled
  ).toBe(false);
  expect(claimAll.props.disabled).toBe(false);
  expect(refreshAll.props.disabled).toBe(false);
  expectPassiveAccountNavigation();
});

it('Account Center: returning before reload starts preserves the complete cache and ignores an older navigation read', async () => {
  const storage = mockAccountOrderStorage();
  vi.mocked(api.listCodexAccounts).mockResolvedValue([
    { id: 'first', email: 'first@example.test', active: false },
  ]);
  vi.mocked(api.listDeepSeekAccounts).mockResolvedValue([
    { id: 'second', name: 'Second', active: false, balances: null },
  ]);
  await mountAccountCenter(['codex', 'dsh']);
  await tick();
  const initialWrites = storage.cacheWrites.mock.calls.length;
  const oldCodex = pendingAccountList<api.CodexAccount>();
  const oldDeepSeek = pendingAccountList<api.DeepSeekAccount>();
  vi.mocked(api.listCodexAccounts).mockReturnValueOnce(oldCodex.promise);
  vi.mocked(api.listDeepSeekAccounts).mockReturnValueOnce(oldDeepSeek.promise);
  act(() => useNavigationStore.getState().setActivePage('models'));
  await tick();
  act(() => useNavigationStore.getState().setActivePage('accounts'));
  expect(accountCardIds()).toEqual(['codex:first', 'dsh:second']);
  expect(api.listCodexAccounts).toHaveBeenCalledTimes(1);
  await tick();
  await act(async () =>
    oldCodex.resolve([{ id: 'stale', email: 'stale@example.test', active: false }])
  );
  expect(accountCardIds()).toEqual(['codex:first', 'dsh:second']);
  act(() => useNavigationStore.getState().setActivePage('models'));
  await tick();
  const codex = pendingAccountList<api.CodexAccount>();
  const deepSeek = pendingAccountList<api.DeepSeekAccount>();
  vi.mocked(api.listCodexAccounts).mockReturnValueOnce(codex.promise);
  vi.mocked(api.listDeepSeekAccounts).mockReturnValueOnce(deepSeek.promise);
  act(() => useNavigationStore.getState().setActivePage('accounts'));
  expect(accountCardIds()).toEqual(['codex:first', 'dsh:second']);
  expect(
    renderer.root.findByProps({ 'aria-label': 'model.dragSort first@example.test' }).props.disabled
  ).toBe(true);
  expect(storage.cacheWrites).toHaveBeenCalledTimes(initialWrites);
  await tick();
  await act(async () =>
    oldDeepSeek.resolve([{ id: 'stale', name: 'Stale', active: false, balances: null }])
  );
  expect(accountCardIds()).toEqual(['codex:first', 'dsh:second']);
  expect(storage.cacheWrites).toHaveBeenCalledTimes(initialWrites);
  await act(async () =>
    codex.resolve([{ id: 'current', email: 'current@example.test', active: false }])
  );
  expect(accountCardIds()).toEqual(['codex:first', 'dsh:second']);
  await act(async () =>
    deepSeek.resolve([{ id: 'current', name: 'Current', active: false, balances: null }])
  );
  expect(accountCardIds()).toEqual(['codex:current', 'dsh:current']);
  expectPassiveAccountNavigation();
});

it('Account Center: saves cross-provider card order, restores it on remount and isolates refresh', async () => {
  const storage = mockAccountOrderStorage();
  const first = { id: 'same', email: 'first@example.test', active: false };
  const second = { id: 'second', email: 'second@example.test', active: false };
  vi.mocked(api.listCodexAccounts).mockResolvedValue([first, second]);
  vi.mocked(api.listDeepSeekAccounts).mockResolvedValue([
    { id: 'same', name: 'DeepSeek', active: false, balances: null },
  ]);
  await showAccountCenter(['codex', 'dsh']);
  expect(accountCardIds()).toEqual(['codex:same', 'codex:second', 'dsh:same']);
  expect(storage.setItem).not.toHaveBeenCalled();
  dropAccountCard('dsh:same', 'codex:same');
  const order = ['dsh:same', 'codex:same', 'codex:second'];
  expect(accountCardIds()).toEqual(order);
  expect(storage.setItem).toHaveBeenCalledExactlyOnceWith(
    'echobird_account_card_order',
    JSON.stringify(order)
  );
  act(() =>
    renderer.update(
      <AppManagerProvider>
        <Harness />
      </AppManagerProvider>
    )
  );
  act(() =>
    renderer.update(
      <AppManagerProvider>
        <Harness center />
      </AppManagerProvider>
    )
  );
  expect(accountCardIds()).toEqual(order);
  for (const [key, action] of Object.entries(api))
    if (/^(start|refresh|switch|delete|claim|restore)/.test(key) && vi.isMockFunction(action))
      expect(action).not.toHaveBeenCalled();
  const grip = renderer.root.findByProps({ 'aria-label': 'model.dragSort first@example.test' });
  const refresh = renderer.root.findByProps({
    'aria-label': 'agent.refreshAccount first@example.test',
  });
  expect(grip.props.onPointerDown).toBeTypeOf('function');
  expect(refresh.props.onPointerDown).toBeUndefined();
  vi.mocked(api.refreshCodexAccountQuota).mockResolvedValue({ ...first, quotaPercent: 80 });
  await act(async () => refresh.props.onClick({ stopPropagation: vi.fn() }));
  expect(api.refreshCodexAccountQuota).toHaveBeenCalledExactlyOnceWith('same');
  expect(accountCardIds()).toEqual(order);
  expect(storage.setItem).toHaveBeenCalledTimes(1);
});

it('Account Center: saved ordering ignores deleted cards and appends new accounts after reload', async () => {
  const storage = mockAccountOrderStorage(
    JSON.stringify(['dsh:deleted', 'codex:second', 'codex:first'])
  );
  vi.mocked(api.listCodexAccounts).mockResolvedValue([
    { id: 'first', email: 'first@example.test', active: false },
    { id: 'second', email: 'second@example.test', active: false },
    { id: 'new', email: 'new@example.test', active: false },
  ]);
  await showAccountCenter(['codex']);
  expect(accountCardIds()).toEqual(['codex:second', 'codex:first', 'codex:new']);
  expect(storage.setItem).not.toHaveBeenCalled();
  vi.mocked(api.listCodexAccounts).mockResolvedValue([
    { id: 'first', email: 'first@example.test', active: false },
    { id: 'new', email: 'new@example.test', active: false },
  ]);
  act(() => useNavigationStore.getState().setActivePage('models'));
  await tick();
  act(() => useNavigationStore.getState().setActivePage('accounts'));
  await tick();
  expect(accountCardIds()).toEqual(['codex:first', 'codex:new']);
  expect(storage.setItem).not.toHaveBeenCalled();
  expect(api.refreshCodexAccountQuota).not.toHaveBeenCalled();
  expect(api.deleteCodexAccount).not.toHaveBeenCalled();
});

it('Account Center: desktop rows follow a live reorder and aliases without changing selection or native accounts', async () => {
  const storage = mockAccountOrderStorage();
  const rows: api.CodexAccount[] = [
    { id: 'first', email: 'first@example.test', active: true },
    { id: 'second', email: 'second@example.test', active: false },
  ];
  vi.mocked(api.listCodexAccounts).mockResolvedValue(rows);
  await showAccountCenter(['codex', 'chatgptdesktop']);
  act(() =>
    renderer.update(
      <AppManagerProvider>
        <Harness center desktop />
      </AppManagerProvider>
    )
  );
  const desktopRows = () =>
    renderer.root.findByType(CodexAccountSection).findAllByType(AccountSectionRow);
  expect(desktopRows().map((row) => row.props.email)).toEqual(rows.map((row) => row.email));
  expect(state.selectedCodexAccountId).toBe('first');
  dropAccountCard('codex:second', 'codex:first');
  expect(desktopRows().map((row) => row.props.email)).toEqual([
    'second@example.test',
    'first@example.test',
  ]);
  expect(desktopRows().map((row) => row.props.selected)).toEqual([false, true]);
  expect(rows.map((row) => row.id)).toEqual(['first', 'second']);
  expect(storage.setItem).toHaveBeenCalledTimes(1);
  act(() => {
    useNavigationStore.getState().setActivePage('apps');
    state.setSelectedTool('chatgptdesktop');
    renderer.update(
      <AppManagerProvider>
        <Harness desktop />
      </AppManagerProvider>
    );
  });
  await tick();
  expect(desktopRows().map((row) => row.props.email)).toEqual([
    'second@example.test',
    'first@example.test',
  ]);
  expect(state.selectedCodexAccountId).toBe('first');
  for (const [key, action] of Object.entries(api))
    if (/^(start|refresh|switch|delete|claim|restore)/.test(key) && vi.isMockFunction(action))
      expect(action).not.toHaveBeenCalled();
  expect(storage.setItem).toHaveBeenCalledTimes(1);
  vi.mocked(api.refreshCodexAccountQuota).mockResolvedValue({ ...rows[0], quotaPercent: 80 });
  await act(async () => desktopRows()[1].props.onRefresh());
  expect(api.refreshCodexAccountQuota).toHaveBeenCalledExactlyOnceWith('first');
  expect(state.codexAccounts.map((row) => row.id)).toEqual(['second', 'first']);
  expect(state.codexAccounts[1].quotaPercent).toBe(80);
  act(() => useNavigationStore.getState().setActivePage('accounts'));
  await tick();
  act(() =>
    renderer.update(
      <AppManagerProvider>
        <Harness center />
      </AppManagerProvider>
    )
  );
  expect(accountCardIds()).toEqual(['codex:second', 'codex:first']);
  act(() => renderer.unmount());
  await mount('chatgptdesktop');
  act(() =>
    renderer.update(
      <AppManagerProvider>
        <Harness desktop />
      </AppManagerProvider>
    )
  );
  expect(desktopRows().map((row) => row.props.email)).toEqual([
    'second@example.test',
    'first@example.test',
  ]);
  expect(storage.setItem).toHaveBeenCalledTimes(1);
});

it('App Desktop: saved order applies to every provider and edition with duplicate IDs, leaving new rows at the end', async () => {
  const scopes = [
    'codex',
    'claudecode',
    'workbuddy',
    'workbuddyai',
    'dsh',
    'antigravity',
    'cursor',
    'zcode',
    'grok',
    'grokbot',
    'manus',
  ];
  const expected = (index: number) =>
    index % 2 ? ['first', 'second', 'new'] : ['second', 'first', 'new'];
  const storage = mockAccountOrderStorage(
    JSON.stringify(
      scopes.flatMap((scope, index) =>
        expected(index)
          .slice(0, 2)
          .map((id) => `${scope}:${id}`)
      )
    )
  );
  const rows = ['first', 'second', 'new'].map((id) => ({
    id,
    email: `${id}@example.test`,
    name: id,
    active: false,
    plan: null,
  }));
  vi.mocked(api.listCodexAccounts).mockResolvedValue(rows as never);
  vi.mocked(api.listClaudeCodeAccounts).mockResolvedValue(rows as never);
  vi.mocked(api.listWorkBuddyAccounts).mockImplementation(async (edition) =>
    rows.map((row) => ({ ...rewardAccount, ...row, edition }))
  );
  vi.mocked(api.listDeepSeekAccounts).mockResolvedValue(
    rows.map((row) => ({ ...row, balances: null }))
  );
  vi.mocked(api.listAntigravityAccounts).mockResolvedValue(
    rows.map((row) => ({ ...row, quotas: [] }))
  );
  vi.mocked(api.listCursorAccounts).mockResolvedValue(rows as never);
  vi.mocked(api.listZCodeAccounts).mockResolvedValue(rows.map((row) => ({ ...zcodeRow, ...row })));
  vi.mocked(api.listGrokAccounts).mockResolvedValue(rows as never);
  vi.mocked(api.listGrokBotAccounts).mockResolvedValue(rows as never);
  vi.mocked(api.listManusAccounts).mockResolvedValue(rows as never);
  await showAccountCenter(Object.keys(listFor) as Tool[]);
  const groups = [
    state.codexAccounts,
    state.claudeCodeAccounts.accounts,
    state.workBuddyAccountGroups.workbuddy.accounts,
    state.workBuddyAccountGroups.workbuddyai.accounts,
    state.deepSeekAccounts.accounts,
    state.antigravityAccounts.accounts,
    state.cursorAccounts.accounts,
    state.zcodeAccounts.accounts,
    state.grokAccounts.accounts,
    state.grokBotAccounts.accounts,
    state.manusAccounts.accounts,
  ];
  groups.forEach((accounts, index) =>
    expect(accounts.map((account) => account.id)).toEqual(expected(index))
  );
  for (const [tool, scope] of [
    ['workbuddy', 'workbuddy'],
    ['workbuddyai', 'workbuddyai'],
  ] as const) {
    act(() => state.setSelectedTool(tool));
    await tick();
    expect(state.workBuddyAccounts.accounts.map((account) => account.id)).toEqual(
      expected(scopes.indexOf(scope))
    );
  }
  expect(rows.map((row) => row.id)).toEqual(['first', 'second', 'new']);
  expect(storage.setItem).not.toHaveBeenCalled();
  for (const [key, action] of Object.entries(api))
    if (/^(start|refresh|switch|delete|claim|restore)/.test(key) && vi.isMockFunction(action))
      expect(action).not.toHaveBeenCalled();
});

it('Account Center: cancelled, invalid and late drops do not save an order', async () => {
  const storage = mockAccountOrderStorage();
  vi.mocked(api.listCodexAccounts).mockResolvedValue([
    { id: 'first', email: 'first@example.test', active: false },
    { id: 'second', email: 'second@example.test', active: false },
  ]);
  await showAccountCenter(['codex']);
  const dnd = accountCardDnd();
  act(() => {
    dnd.props.onDragStart({ active: { id: 'codex:first' } });
    dnd.props.onDragCancel();
  });
  for (const target of [null, 'codex:missing', 'codex:first'])
    dropAccountCard('codex:first', target);
  act(() => useNavigationStore.getState().setActivePage('models'));
  dropAccountCard('codex:first', 'codex:second');
  await tick();
  act(() => useNavigationStore.getState().setActivePage('accounts'));
  await tick();
  expect(accountCardIds()).toEqual(['codex:first', 'codex:second']);
  expect(storage.setItem).not.toHaveBeenCalled();
});

it('Account Center: pending account reload disables sorting without rewriting the saved order', async () => {
  const storage = mockAccountOrderStorage();
  const rows = [
    { id: 'first', email: 'first@example.test', active: false },
    { id: 'second', email: 'second@example.test', active: false },
  ];
  vi.mocked(api.listCodexAccounts).mockResolvedValue(rows);
  await showAccountCenter(['codex']);
  act(() => useNavigationStore.getState().setActivePage('models'));
  await tick();
  let resolve!: (rows: api.CodexAccount[]) => void;
  vi.mocked(api.listCodexAccounts).mockReturnValueOnce(
    new Promise((done) => {
      resolve = done;
    })
  );
  act(() => useNavigationStore.getState().setActivePage('accounts'));
  await tick();
  const grip = renderer.root.findByProps({ 'aria-label': 'model.dragSort first@example.test' });
  expect(grip.props.disabled).toBe(true);
  dropAccountCard('codex:first', 'codex:second');
  expect(accountCardIds()).toEqual(['codex:first', 'codex:second']);
  expect(storage.setItem).not.toHaveBeenCalled();
  await act(async () => resolve(rows));
  expect(grip.props.disabled).toBe(false);
});

it.each(['{invalid', '{}', '[12,null]'])(
  'Account Center: malformed saved card order falls back without a write (%s)',
  async (raw) => {
    const storage = mockAccountOrderStorage(raw);
    vi.mocked(api.listCodexAccounts).mockResolvedValue([
      { id: 'first', email: 'first@example.test', active: false },
      { id: 'second', email: 'second@example.test', active: false },
    ]);
    await showAccountCenter(['codex']);
    expect(accountCardIds()).toEqual(['codex:first', 'codex:second']);
    expect(storage.setItem).not.toHaveBeenCalled();
    expect(state.applyError).toBeNull();
  }
);

it('Account Center: unavailable order storage keeps sorting usable for the current session', async () => {
  const storage = mockAccountOrderStorage();
  storage.getItem.mockImplementation(() => {
    throw new Error('unavailable');
  });
  storage.setItem.mockImplementation(() => {
    throw new Error('unavailable');
  });
  vi.mocked(api.listCodexAccounts).mockResolvedValue([
    { id: 'first', email: 'first@example.test', active: false },
    { id: 'second', email: 'second@example.test', active: false },
  ]);
  await showAccountCenter(['codex']);
  dropAccountCard('codex:first', 'codex:second');
  expect(accountCardIds()).toEqual(['codex:second', 'codex:first']);
  expect(state.codexAccounts.map((account) => account.id)).toEqual(['second', 'first']);
  act(() =>
    renderer.update(
      <AppManagerProvider>
        <Harness />
      </AppManagerProvider>
    )
  );
  act(() =>
    renderer.update(
      <AppManagerProvider>
        <Harness center />
      </AppManagerProvider>
    )
  );
  expect(accountCardIds()).toEqual(['codex:second', 'codex:first']);
  expect(state.applyError).toBeNull();
  expect(api.refreshCodexAccountQuota).not.toHaveBeenCalled();
});

const rewardAccount: api.WorkBuddyAccount = {
  id: 'reward',
  name: 'Reward',
  edition: 'workbuddy',
  active: false,
  plan: 'Free',
  remaining: 50,
  total: 500,
  rewardRemaining: 50,
  rewardTotal: 500,
  expiresAt: null,
};

const refreshFor = {
  opendesign: 'refreshOpenDesignAccount',
  codex: 'refreshCodexAccountQuota',
  chatgptdesktop: 'refreshCodexAccountQuota',
  claudecode: 'refreshClaudeCodeAccountQuota',
  claudedesktop: 'refreshClaudeDesktopAccount',
  dsh: 'refreshDeepSeekAccountQuota',
  workbuddy: 'refreshWorkBuddyAccountQuota',
  workbuddyai: 'refreshWorkBuddyAccountQuota',
  trae: 'refreshTraeAccountQuota',
  traecn: 'refreshTraeAccountQuota',
  kirocrew: 'refreshKiroAccountQuota',
  kirocli: 'refreshKiroAccountQuota',
  antigravity: 'refreshAntigravityAccount',
  antigravitydesktop: 'refreshAntigravityAccount',
  cursor: 'refreshCursorAccount',
  grokbot: 'refreshGrokBotAccount',
  zcode: 'refreshZCodeAccountQuota',
  grok: 'refreshGrokAccount',
  manus: 'refreshManusAccount',
  cue: 'refreshCueAccount',
} as const;

it.each(Object.keys(listFor) as Tool[])(
  'desktop auth status %s: hides stale quota, shares the status across pages and restores it after a successful retry',
  async (tool) => {
    const resetAt = Date.now() / 1000 + 86400;
    const saved = {
      ...rewardAccount,
      id: 'saved',
      name: 'Saved user',
      email: 'saved@example.test',
      active: true,
      edition:
        tool === 'trae' || tool === 'traecn' || tool === 'kirocrew' || tool === 'kirocli'
          ? tool
          : tool === 'workbuddyai'
            ? 'workbuddyai'
            : 'workbuddy',
      provider: 'bigmodel',
      plan: 'Plus',
      quotaPercent: 64,
      quotaResetAt: resetAt,
      quotaWindows: [{ label: '7d', remainingPercent: 64, resetAt }],
      remainingPercent: 64,
      resetAt,
      fiveHour: { remainingPercent: 64, resetAt },
      sevenDay: { remainingPercent: 64, resetAt },
      balances: [{ currency: 'CNY', amount: 12.34 }],
      quotas: [{ name: 'gemini-pro', remainingPercent: 64, resetAt }],
      quota: [{ label: 'credits', remaining: 64, total: 100, resetAt }],
      usage: { remainingPercent: 64, resetAt, plan: 'Plus' },
      credits: { total: 500 },
    };
    const healthy = {
      ...saved,
      id: 'healthy',
      name: 'Healthy user',
      email: 'healthy@example.test',
      active: false,
    };
    vi.mocked(api[listFor[tool]]).mockResolvedValue([saved, healthy] as never);
    await mount(tool);
    act(() =>
      renderer.update(
        <AppManagerProvider>
          <Harness panel />
        </AppManagerProvider>
      )
    );
    const rows = () => renderer.root.findByType(AppManagerPanel).findAllByType(AccountSectionRow);
    const markup = (index: number) =>
      renderToStaticMarkup(
        <AccountSectionRow
          {...(rows()[index].props as React.ComponentProps<typeof AccountSectionRow>)}
        />
      );
    const before = markup(0);
    const healthyQuota = renderToStaticMarkup(<>{rows()[1].props.secondary}</>);
    expect(rows()).toHaveLength(2);
    expect(rows()[0].findAllByProps({ role: 'status' })).toHaveLength(0);
    expect(api[refreshFor[tool]]).not.toHaveBeenCalled();
    act(() => rows()[1].findByProps({ role: 'radio' }).props.onClick());
    const stopPropagation = vi.fn();
    const clickRefresh = (index: number) => {
      const row = rows()[index];
      const button = row
        .findAllByType('button')
        .find((button) => String(button.props['aria-label']).startsWith('agent.refreshAccount '))!;
      button.props.onClick({ stopPropagation });
    };
    vi.mocked(api[refreshFor[tool]]).mockRejectedValueOnce('accountError.network');
    await act(async () => {
      clickRefresh(1);
    });
    expect(rows()[1].findAllByProps({ role: 'status' })).toHaveLength(0);
    expect(rows()[1].props.selected).toBe(true);
    expect(renderToStaticMarkup(<>{rows()[1].props.secondary}</>)).toBe(healthyQuota);
    vi.mocked(api[refreshFor[tool]]).mockRejectedValueOnce('accountError.network|HTTP 401');
    await act(async () => {
      clickRefresh(0);
    });
    expect(stopPropagation).toHaveBeenCalledTimes(2);
    expect(rows()[0].findByProps({ role: 'status' }).children).toEqual([
      'accountCenter.authFailed',
    ]);
    expect(rows()[0].findAllByProps({ role: 'progressbar' })).toHaveLength(0);
    expect(rows()[0].props.selected).toBe(false);
    expect(rows()[1].props.selected).toBe(true);
    expect(rows()[1].findAllByProps({ role: 'status' })).toHaveLength(0);
    const failed = markup(0);
    expect(failed).not.toContain('64%');
    expect(failed).not.toContain('12.34');
    expect(failed).not.toContain('>500');
    expect(failed).toContain(rows()[0].props.email);
    if (tool !== 'dsh') expect(failed).toContain('Plus');
    expect(failed).toContain('agent.refreshAccount');
    expect(failed).toContain('btn.delete');
    act(() => {
      state.setApplyError(null);
      useNavigationStore.getState().setActivePage('accounts');
      renderer.update(
        <AppManagerProvider>
          <Harness center />
        </AppManagerProvider>
      );
    });
    await tick();
    const centerCard = renderer.root.findAllByType(AccountCard).find(
      (card) =>
        card.props.provider.id ===
          ({
            chatgptdesktop: 'codex',
            kirocrew: 'kiro',
            kirocli: 'kiro',
            antigravitydesktop: 'antigravity',
          }[tool as string] ?? tool) && card.props.account.id === 'saved'
    )!;
    expect(centerCard.findByProps({ role: 'status' }).children).toEqual([
      'accountCenter.authFailed',
    ]);
    expect(api[refreshFor[tool]]).toHaveBeenCalledTimes(2);
    act(() => {
      useNavigationStore.getState().setActivePage('apps');
      renderer.update(
        <AppManagerProvider>
          <Harness panel />
        </AppManagerProvider>
      );
    });
    await tick();
    expect(rows()[0].findAllByProps({ role: 'status' })).toHaveLength(1);
    expect(api[refreshFor[tool]]).toHaveBeenCalledTimes(2);
    const radio = rows()[0].findByProps({ role: 'radio' });
    const preventDefault = vi.fn();
    act(() =>
      radio.props.onKeyDown({ key: ' ', target: radio, currentTarget: radio, preventDefault })
    );
    expect(preventDefault).toHaveBeenCalledOnce();
    expect(rows()[0].props.selected).toBe(true);
    let resolve!: (value: never) => void;
    vi.mocked(api[refreshFor[tool]]).mockReturnValueOnce(
      new Promise<never>((done) => {
        resolve = done;
      })
    );
    act(() => clickRefresh(0));
    expect(rows()[0].props.refreshing).toBe(true);
    expect(rows()[0].findAllByProps({ role: 'status' })).toHaveLength(1);
    await act(async () => {
      resolve((tool === 'cursor' || tool === 'grokbot' ? saved.usage : saved) as never);
    });
    expect(rows()[0].props.refreshing).toBe(false);
    expect(rows()[0].findAllByProps({ role: 'status' })).toHaveLength(0);
    expect(markup(0)).toBe(before);
    for (const [key, action] of Object.entries(api))
      if (/^(start|switch|delete|claim|restore)/.test(key) && vi.isMockFunction(action))
        expect(action).not.toHaveBeenCalled();
  }
);

it('Account Center: a batch marks only authorization failures, preserves them on return and restores quota after retry', async () => {
  const failed: api.CodexAccount = {
    id: 'same',
    email: 'failed@example.test',
    plan: 'Pro',
    active: false,
    quotaWindows: [{ label: '7d', remainingPercent: 25 }],
  };
  const healthy = { ...failed, id: 'healthy', email: 'healthy@example.test' };
  const buddy = { ...rewardAccount, id: 'same' };
  const balance: api.DeepSeekAccount = {
    id: 'same',
    name: 'Balance',
    active: false,
    balances: [{ currency: 'CNY', amount: 12.5 }],
  };
  vi.mocked(api.listCodexAccounts).mockResolvedValue([failed, healthy]);
  vi.mocked(api.listWorkBuddyAccounts).mockImplementation(async (edition) =>
    edition === 'workbuddy' ? [buddy] : []
  );
  vi.mocked(api.listDeepSeekAccounts).mockResolvedValue([balance]);
  vi.mocked(api.refreshCodexAccountQuota).mockImplementation(async (id) => {
    if (id === 'same') throw new Error('accountError.loginRequired');
    return healthy;
  });
  vi.mocked(api.refreshWorkBuddyAccountQuota).mockRejectedValue('accountError.auth|HTTP 503');
  vi.mocked(api.refreshDeepSeekAccountQuota).mockRejectedValue('accountError.network');
  await showAccountCenter(['codex', 'workbuddy', 'dsh']);
  const card = (label: string) => renderer.root.findByProps({ 'aria-label': label });
  const failedCard = () => card('ChatGPT / Codex failed@example.test');
  expect(api.refreshCodexAccountQuota).not.toHaveBeenCalled();
  await act(async () => {
    await renderer.root.findByProps({ 'aria-label': 'accountCenter.refreshAll' }).props.onClick();
  });
  expect(failedCard().findByProps({ role: 'status' }).children).toEqual([
    'accountCenter.authFailed',
  ]);
  expect(failedCard().findAllByProps({ role: 'progressbar' })).toHaveLength(0);
  expect(
    card('ChatGPT / Codex healthy@example.test').findAllByProps({ role: 'status' })
  ).toHaveLength(0);
  expect(
    card('ChatGPT / Codex healthy@example.test').findAllByProps({ role: 'progressbar' })
  ).toHaveLength(1);
  expect(card('WorkBuddy Reward').findAllByProps({ role: 'status' })).toHaveLength(0);
  expect(card('DeepSeek Harness Balance').findAllByProps({ role: 'status' })).toHaveLength(0);
  expect(state.codexAuthorizationFailedIds.has('same')).toBe(true);
  expect(state.workBuddyAccountGroups.workbuddy.authorizationFailedIds.size).toBe(0);
  expect(state.deepSeekAccounts.authorizationFailedIds.size).toBe(0);
  expect(state.deepSeekAccounts.accounts).toEqual([balance]);
  expect(state.workBuddyAccountGroups.workbuddy.accounts).toEqual([buddy]);
  const summary =
    'Authorization failed: 1 (ChatGPT / Codex · failed@example.test) Refresh failed: 2 (DeepSeek Harness · Balance, WorkBuddy · Reward)';
  expect(state.applyError).toBe(summary);
  expect(applyErrorEvents).toEqual([null, summary]);
  act(() => useNavigationStore.getState().setActivePage('models'));
  await tick();
  act(() => useNavigationStore.getState().setActivePage('accounts'));
  await tick();
  expect(failedCard().findAllByProps({ role: 'status' })).toHaveLength(1);
  expect(api.refreshCodexAccountQuota).toHaveBeenCalledTimes(2);
  expect(api.refreshWorkBuddyAccountQuota).toHaveBeenCalledTimes(1);
  expect(api.refreshDeepSeekAccountQuota).toHaveBeenCalledTimes(1);
  let resolve!: (account: api.CodexAccount) => void;
  vi.mocked(api.refreshCodexAccountQuota).mockReturnValueOnce(
    new Promise((done) => {
      resolve = done;
    })
  );
  let task!: Promise<unknown>;
  act(() => {
    task = state.refreshCodexAccountQuota(state.codexAccounts.find((row) => row.id === 'same')!);
  });
  expect(failedCard().findAllByProps({ role: 'status' })).toHaveLength(1);
  expect(state.refreshingCodexAccountIds.has('same')).toBe(true);
  await act(async () => {
    resolve({ ...failed, quotaWindows: [{ label: '7d', remainingPercent: 80 }] });
    await task;
  });
  expect(failedCard().findAllByProps({ role: 'status' })).toHaveLength(0);
  expect(failedCard().findByProps({ role: 'progressbar' }).props['aria-valuenow']).toBe(80);
  expect(state.codexAuthorizationFailedIds.size).toBe(0);
  expect(api.startCodexLogin).not.toHaveBeenCalled();
  expect(api.switchCodexAccount).not.toHaveBeenCalled();
  expect(api.claimWorkBuddyDailyCredits).not.toHaveBeenCalled();
  expect(api.deleteCodexAccount).not.toHaveBeenCalled();
  expect(api.startTool).not.toHaveBeenCalled();
});

it.each([
  [
    'authorization',
    'accountError.loginRequired',
    'accountError.network|HTTP 401',
    'Authorization failed: 2 (ChatGPT / Codex · first@example.test, ChatGPT / Codex · second@example.test)',
  ],
  [
    'connection',
    'accountError.network',
    'accountError.auth|HTTP 429',
    'Refresh failed: 2 (ChatGPT / Codex · first@example.test, ChatGPT / Codex · second@example.test)',
  ],
  [
    'mixed',
    'accountError.auth|HTTP 403',
    'accountError.quotaTimeout',
    'Authorization failed: 1 (ChatGPT / Codex · first@example.test) Refresh failed: 1 (ChatGPT / Codex · second@example.test)',
  ],
  ['success', null, null, null],
])(
  'Account Center: shows one summary only after all bulk refreshes finish (%s)',
  async (_kind, firstError, secondError, summary) => {
    const rows: api.CodexAccount[] = ['first', 'second', 'last'].map((id) => ({
      id,
      email: `${id}@example.test`,
      active: false,
      quotaPercent: 25,
    }));
    vi.mocked(api.listCodexAccounts).mockResolvedValue(rows);
    let resolve!: (row: api.CodexAccount) => void;
    vi.mocked(api.refreshCodexAccountQuota).mockImplementation(async (id) => {
      if (id === 'last')
        return new Promise((done) => {
          resolve = done;
        });
      const error = id === 'first' ? firstError : secondError;
      if (error) throw new Error(error);
      return { ...rows.find((row) => row.id === id)!, quotaPercent: 80 };
    });
    await showAccountCenter(['codex']);
    const refreshAll = renderer.root.findByProps({ 'aria-label': 'accountCenter.refreshAll' });
    let task!: Promise<void>;
    act(() => {
      task = refreshAll.props.onClick();
      void refreshAll.props.onClick();
    });
    await tick();
    expect(vi.mocked(api.refreshCodexAccountQuota).mock.calls).toEqual([
      ['first'],
      ['second'],
      ['last'],
    ]);
    expect(state.applyError).toBeNull();
    expect(applyErrorEvents).toEqual([null]);
    expect(renderer.root.findByType(AppManagerErrorModal).findAllByType('button')).toHaveLength(0);
    expect(refreshAll.props.disabled).toBe(true);
    expect(state.codexAccounts[0].quotaPercent).toBe(firstError ? 25 : 80);
    expect(state.codexAccounts[1].quotaPercent).toBe(secondError ? 25 : 80);
    await act(async () => {
      resolve({ ...rows[2], quotaPercent: 90 });
      await task;
    });
    expect(state.codexAccounts[2].quotaPercent).toBe(90);
    expect(state.applyError).toBe(summary);
    expect(applyErrorEvents).toEqual(summary ? [null, summary] : [null]);
    const modal = renderer.root.findByType(AppManagerErrorModal);
    expect(modal.findAllByType('button')).toHaveLength(summary ? 1 : 0);
    if (summary) {
      expect(modal.findByType('p').children).toEqual([summary]);
      act(() => modal.findByType('button').props.onClick());
      await tick();
      expect(state.applyError).toBeNull();
      expect(modal.findAllByType('button')).toHaveLength(0);
      expect(api.refreshCodexAccountQuota).toHaveBeenCalledTimes(3);
    }
    expect(refreshAll.props.disabled).toBe(false);
    vi.mocked(api.refreshCodexAccountQuota).mockImplementation(async (id) => ({
      ...rows.find((row) => row.id === id)!,
      quotaPercent: 100,
    }));
    await act(async () => {
      await refreshAll.props.onClick();
    });
    expect(state.applyError).toBeNull();
    expect(state.codexAuthorizationFailedIds.size).toBe(0);
    expect(api.refreshCodexAccountQuota).toHaveBeenCalledTimes(6);
    expect(api.startCodexLogin).not.toHaveBeenCalled();
    expect(api.switchCodexAccount).not.toHaveBeenCalled();
    expect(api.claimWorkBuddyDailyCredits).not.toHaveBeenCalled();
  }
);

it.each(['leave', 'reenter', 'unmount'])(
  'Account Center: discards collected batch failures on %s and permits a new batch',
  async (next) => {
    const rows: api.CodexAccount[] = ['failed', 'pending', 'queued'].map((id) => ({
      id,
      email: `${id}@example.test`,
      active: false,
      quotaPercent: 25,
    }));
    vi.mocked(api.listCodexAccounts).mockResolvedValue(rows);
    let reject!: (error: Error) => void;
    vi.mocked(api.refreshCodexAccountQuota).mockImplementation(async (id) => {
      if (id === 'failed') throw new Error('accountError.loginRequired');
      return new Promise((_, fail) => {
        reject = fail;
      });
    });
    await showAccountCenter(['codex']);
    let task!: Promise<void>;
    act(() => {
      task = renderer.root
        .findByProps({ 'aria-label': 'accountCenter.refreshAll' })
        .props.onClick();
    });
    await tick();
    expect([...state.codexAuthorizationFailedIds]).toEqual(['failed']);
    expect(state.applyError).toBeNull();
    if (next === 'unmount') {
      act(() => {
        renderer.update(
          <AppManagerProvider>
            <Harness />
          </AppManagerProvider>
        );
      });
    } else {
      act(() => useNavigationStore.getState().setActivePage('models'));
      await tick();
      if (next === 'reenter') {
        act(() => useNavigationStore.getState().setActivePage('accounts'));
        await tick();
      }
    }
    await act(async () => {
      reject(new Error('accountError.network'));
      await task;
    });
    expect(vi.mocked(api.refreshCodexAccountQuota).mock.calls).toEqual([['failed'], ['pending']]);
    expect(state.applyError).toBeNull();
    expect(applyErrorEvents).toEqual([null]);
    if (next === 'unmount') {
      act(() => {
        renderer.update(
          <AppManagerProvider>
            <Harness center />
          </AppManagerProvider>
        );
      });
    } else if (next === 'leave') {
      act(() => useNavigationStore.getState().setActivePage('accounts'));
      await tick();
    }
    vi.mocked(api.refreshCodexAccountQuota).mockImplementation(async (id) => ({
      ...rows.find((row) => row.id === id)!,
      quotaPercent: 100,
    }));
    await act(async () => {
      await renderer.root.findByProps({ 'aria-label': 'accountCenter.refreshAll' }).props.onClick();
    });
    expect(api.refreshCodexAccountQuota).toHaveBeenCalledTimes(5);
    expect(state.codexAuthorizationFailedIds.size).toBe(0);
    expect(state.applyError).toBeNull();
    expect(applyErrorEvents).toEqual([null]);
  }
);

it('Account Center: title actions stay enabled with no saved accounts and do not start automatic actions', async () => {
  await showAccountCenter(['workbuddy']);
  const claimAll = renderer.root.findByProps({ 'aria-label': 'accountCenter.claimAll' });
  const refreshAll = renderer.root.findByProps({ 'aria-label': 'accountCenter.refreshAll' });
  expect(claimAll.props.disabled).toBe(false);
  expect(refreshAll.props.disabled).toBe(false);
  await act(async () => {
    await claimAll.props.onClick();
    await refreshAll.props.onClick();
  });
  for (const [key, action] of Object.entries(api))
    if (/^(start|refresh|switch|delete|claim|restore)/.test(key) && vi.isMockFunction(action))
      expect(action).not.toHaveBeenCalled();
  expect(state.applyError).toBeNull();
});

it.each(['refresh', 'claim'] as const)(
  'Account Center: %s stays available during page-entry reload and only locks for explicit requests',
  async (action) => {
    const row = {
      ...rewardAccount,
      dailyClaimedAt: action === 'claim' ? Date.now() / 1000 : null,
    };
    vi.mocked(api.listWorkBuddyAccounts).mockImplementation(async (edition) =>
      edition === 'workbuddy' ? [row] : []
    );
    await showAccountCenter(['workbuddy']);
    const claimAll = renderer.root.findByProps({ 'aria-label': 'accountCenter.claimAll' });
    const refreshAll = renderer.root.findByProps({ 'aria-label': 'accountCenter.refreshAll' });
    expect(claimAll.props.disabled).toBe(false);
    expect(refreshAll.props.disabled).toBe(false);
    act(() => useNavigationStore.getState().setActivePage('models'));
    await tick();
    let resolveList!: (rows: api.WorkBuddyAccount[]) => void;
    vi.mocked(api.listWorkBuddyAccounts).mockReturnValueOnce(
      new Promise((done) => {
        resolveList = done;
      })
    );
    act(() => useNavigationStore.getState().setActivePage('accounts'));
    await tick();
    expect(state.workBuddyAccountGroups.workbuddy.loading).toBe(true);
    expect(claimAll.props.disabled).toBe(false);
    expect(refreshAll.props.disabled).toBe(false);
    for (const [key, operation] of Object.entries(api))
      if (/^(start|refresh|switch|delete|claim|restore)/.test(key) && vi.isMockFunction(operation))
        expect(operation).not.toHaveBeenCalled();
    let resolveAction!: (row: api.WorkBuddyAccount) => void;
    const pending = new Promise<api.WorkBuddyAccount>((done) => {
      resolveAction = done;
    });
    const operation =
      action === 'claim' ? api.claimWorkBuddyDailyCredits : api.refreshWorkBuddyAccountQuota;
    vi.mocked(operation).mockReturnValueOnce(pending);
    const button = action === 'claim' ? claimAll : refreshAll;
    act(() => {
      void button.props.onClick();
      void button.props.onClick();
    });
    expect(operation).toHaveBeenCalledExactlyOnceWith('workbuddy', row.id);
    expect(claimAll.props.disabled).toBe(true);
    expect(refreshAll.props.disabled).toBe(true);
    const updated = { ...row, remaining: 200, rewardRemaining: 300 };
    await act(async () => resolveAction(updated));
    await act(async () => resolveList([row]));
    expect(state.workBuddyAccountGroups.workbuddy.accounts).toEqual([updated]);
    expect(claimAll.props.disabled).toBe(false);
    expect(refreshAll.props.disabled).toBe(false);
    expect(api.switchWorkBuddyAccount).not.toHaveBeenCalled();
    expect(api.startTool).not.toHaveBeenCalled();
    expect(state.applyError).toBeNull();
  }
);

it('Account Center: individual/batch rewards share locks, retry claimed accounts, exclude international accounts and preserve partial failures', async () => {
  const now = Date.now() / 1000;
  const rows = ['manual', 'failed', 'success', 'claimed'].map((id) => ({
    ...rewardAccount,
    id,
    name: id,
    dailyClaimedAt: id === 'claimed' ? now : null,
  }));
  const international = {
    ...rewardAccount,
    id: 'global',
    name: 'global',
    edition: 'workbuddyai' as const,
  };
  vi.mocked(api.listWorkBuddyAccounts).mockImplementation(async (edition) =>
    edition === 'workbuddy' ? rows : [international]
  );
  await showAccountCenter(['workbuddy', 'workbuddyai']);
  expect(api.claimWorkBuddyDailyCredits).not.toHaveBeenCalled();
  const claimAll = renderer.root.findByProps({ 'aria-label': 'accountCenter.claimAll' });
  expect(claimAll.props.disabled).toBe(false);
  expect(
    renderer.root.findAll(
      (node) =>
        node.type === 'button' && node.props['aria-label'] === 'agent.claimDailyCredits global'
    )
  ).toHaveLength(0);
  expect(
    renderer.root.findByProps({ 'aria-label': 'agent.dailyCreditsClaimed claimed' }).props.disabled
  ).toBe(true);
  let resolve!: (row: api.WorkBuddyAccount) => void;
  vi.mocked(api.claimWorkBuddyDailyCredits).mockReturnValueOnce(
    new Promise((done) => {
      resolve = done;
    })
  );
  const manual = renderer.root.findByProps({ 'aria-label': 'agent.claimDailyCredits manual' });
  const stopPropagation = vi.fn();
  act(() => manual.props.onClick({ stopPropagation }));
  expect(stopPropagation).toHaveBeenCalledOnce();
  expect(claimAll.props.disabled).toBe(true);
  await act(async () => {
    await claimAll.props.onClick();
    await state.workBuddyAccountGroups.workbuddy.claimDaily(rows[0]);
    await state.workBuddyAccountGroups.workbuddy.refresh(rows[0]);
  });
  expect(api.claimWorkBuddyDailyCredits).toHaveBeenCalledExactlyOnceWith('workbuddy', 'manual');
  expect(api.refreshWorkBuddyAccountQuota).not.toHaveBeenCalled();
  await act(async () => {
    resolve({ ...rows[0], rewardRemaining: 100, dailyClaimedAt: now });
  });
  expect(
    renderer.root.findByProps({ 'aria-label': 'agent.dailyCreditsClaimed manual' }).props.disabled
  ).toBe(true);
  vi.mocked(api.claimWorkBuddyDailyCredits).mockImplementation(async (_edition, id) => {
    if (id === 'failed') throw new Error('accountError.claim');
    return { ...rows.find((row) => row.id === id)!, rewardRemaining: 200, dailyClaimedAt: now };
  });
  await act(async () => {
    await claimAll.props.onClick();
  });
  expect(vi.mocked(api.claimWorkBuddyDailyCredits).mock.calls).toEqual([
    ['workbuddy', 'manual'],
    ['workbuddy', 'manual'],
    ['workbuddy', 'failed'],
    ['workbuddy', 'success'],
    ['workbuddy', 'claimed'],
  ]);
  expect(
    state.workBuddyAccountGroups.workbuddy.accounts.find((row) => row.id === 'failed')
  ).toEqual(rows[1]);
  expect(
    state.workBuddyAccountGroups.workbuddy.accounts.find((row) => row.id === 'success')
      ?.rewardRemaining
  ).toBe(200);
  expect(state.applyError).toBe('Claim failed: 1 (WorkBuddy · failed)');
  expect(claimAll.props.disabled).toBe(false);
  expect(api.switchWorkBuddyAccount).not.toHaveBeenCalled();
  expect(api.startTool).not.toHaveBeenCalled();
  expect(api.openExternal).not.toHaveBeenCalled();
});

it('Account Center: refreshes all account groups sequentially once and keeps failed rows', async () => {
  const first: api.CodexAccount = {
    id: 'first',
    email: 'first@example.test',
    active: false,
    quotaPercent: 10,
  };
  const second: api.CodexAccount = {
    id: 'second',
    email: 'second@example.test',
    active: false,
    quotaPercent: 20,
  };
  const generic = {
    id: 'saved',
    email: 'saved@example.test',
    name: 'saved',
    active: false,
    plan: null,
  };
  vi.mocked(api.listCodexAccounts).mockResolvedValue([first, second]);
  vi.mocked(api.listWorkBuddyAccounts).mockImplementation(async (edition) => [
    { ...rewardAccount, edition },
  ]);
  vi.mocked(api.listClaudeCodeAccounts).mockResolvedValue([generic] as never);
  vi.mocked(api.listDeepSeekAccounts).mockResolvedValue([{ ...generic, balances: null }]);
  vi.mocked(api.listAntigravityAccounts).mockResolvedValue([{ ...generic, quotas: [] }]);
  vi.mocked(api.listCursorAccounts).mockResolvedValue([generic] as never);
  vi.mocked(api.listZCodeAccounts).mockResolvedValue([zcodeRow]);
  vi.mocked(api.listGrokAccounts).mockResolvedValue([generic] as never);
  vi.mocked(api.listGrokBotAccounts).mockResolvedValue([generic] as never);
  vi.mocked(api.listManusAccounts).mockResolvedValue([generic] as never);
  await showAccountCenter(Object.keys(listFor) as Tool[]);
  act(() =>
    useToolsStore
      .getState()
      .setDetectedTools(
        useToolsStore
          .getState()
          .detectedTools.map((tool) => ({ ...tool, installed: tool.id !== 'cursor' }))
      )
  );
  await tick();
  const actions = [
    'refreshClaudeCodeAccountQuota',
    'refreshDeepSeekAccountQuota',
    'refreshWorkBuddyAccountQuota',
    'refreshAntigravityAccount',
    'refreshZCodeAccountQuota',
    'refreshGrokAccount',
    'refreshGrokBotAccount',
    'refreshManusAccount',
  ] as const;
  for (const name of actions) expect(api[name]).not.toHaveBeenCalled();
  let reject!: (error: Error) => void;
  vi.mocked(api.refreshCodexAccountQuota)
    .mockReturnValueOnce(
      new Promise((_, fail) => {
        reject = fail;
      })
    )
    .mockResolvedValue({ ...second, quotaPercent: 80 });
  vi.mocked(api.refreshClaudeCodeAccountQuota).mockResolvedValue(generic as never);
  vi.mocked(api.refreshDeepSeekAccountQuota).mockResolvedValue({ ...generic, balances: [] });
  vi.mocked(api.refreshWorkBuddyAccountQuota).mockImplementation(async (edition) => ({
    ...rewardAccount,
    edition,
    remaining: 200,
  }));
  vi.mocked(api.refreshAntigravityAccount).mockResolvedValue({ ...generic, quotas: [] });
  vi.mocked(api.refreshZCodeAccountQuota).mockResolvedValue(zcodeRow);
  vi.mocked(api.refreshGrokAccount).mockResolvedValue(generic as never);
  vi.mocked(api.refreshGrokBotAccount).mockResolvedValue(generic as never);
  vi.mocked(api.refreshManusAccount).mockResolvedValue(generic as never);
  const refreshAll = renderer.root.findByProps({ 'aria-label': 'accountCenter.refreshAll' });
  let task!: Promise<void>;
  act(() => {
    task = refreshAll.props.onClick();
    void refreshAll.props.onClick();
  });
  expect(api.refreshCodexAccountQuota).toHaveBeenCalledExactlyOnceWith('first');
  for (const name of actions) expect(api[name]).not.toHaveBeenCalled();
  expect(refreshAll.props.disabled).toBe(true);
  expect(renderer.root.findByProps({ 'aria-label': 'accountCenter.claimAll' }).props.disabled).toBe(
    true
  );
  await act(async () => {
    reject(new Error('accountError.quota'));
    await task;
  });
  expect(vi.mocked(api.refreshCodexAccountQuota).mock.calls).toEqual([['first'], ['second']]);
  expect(state.codexAccounts[0]).toEqual(first);
  expect(state.codexAccounts[1].quotaPercent).toBe(80);
  for (const name of actions)
    expect(api[name]).toHaveBeenCalledTimes(name === 'refreshWorkBuddyAccountQuota' ? 2 : 1);
  expect(vi.mocked(api.refreshWorkBuddyAccountQuota).mock.calls).toEqual([
    ['workbuddy', rewardAccount.id],
    ['workbuddyai', rewardAccount.id],
  ]);
  expect(api.refreshDeepSeekAccountQuota).toHaveBeenCalledWith('saved', 'en');
  // An uninstalled client's saved account is refreshed too.
  expect(api.refreshCursorAccount).toHaveBeenCalledTimes(1);
  expect(api.claimWorkBuddyDailyCredits).not.toHaveBeenCalled();
  expect(api.startTool).not.toHaveBeenCalled();
  expect(api.openExternal).not.toHaveBeenCalled();
  for (const [key, action] of Object.entries(api))
    if (key.startsWith('switch') && vi.isMockFunction(action))
      expect(action).not.toHaveBeenCalled();
  expect(state.applyError).toBe('Refresh failed: 1 (ChatGPT / Codex · first@example.test)');
  expect(refreshAll.props.disabled).toBe(false);
});

it.each([false, true])(
  'Account Center: leaving stops a batch and ignores its late result, including after re-entry (failure=%s)',
  async (failure) => {
    const rows: api.CodexAccount[] = [
      { id: 'first', email: 'first@example.test', active: false, quotaPercent: 10 },
      { id: 'second', email: 'second@example.test', active: false, quotaPercent: 20 },
    ];
    vi.mocked(api.listCodexAccounts).mockResolvedValue(rows);
    await showAccountCenter(['codex']);
    let resolve!: (row: api.CodexAccount) => void;
    vi.mocked(api.refreshCodexAccountQuota).mockImplementationOnce(async () => {
      const result = await new Promise<api.CodexAccount>((done) => {
        resolve = done;
      });
      if (failure) throw new Error('accountError.quota');
      return result;
    });
    const refreshAll = renderer.root.findByProps({ 'aria-label': 'accountCenter.refreshAll' });
    let task!: Promise<void>;
    act(() => {
      task = refreshAll.props.onClick();
    });
    act(() => useNavigationStore.getState().setActivePage('models'));
    await tick();
    act(() => useNavigationStore.getState().setActivePage('accounts'));
    await tick();
    await act(async () => {
      resolve({ ...rows[0], quotaPercent: 99 });
      await task;
    });
    expect(api.refreshCodexAccountQuota).toHaveBeenCalledExactlyOnceWith('first');
    expect(state.codexAccounts).toEqual(rows);
    expect(state.applyError).toBeNull();
    expect(refreshAll.props.disabled).toBe(false);
    vi.mocked(api.refreshCodexAccountQuota).mockImplementation(async (id) => ({
      ...rows.find((row) => row.id === id)!,
      quotaPercent: 100,
    }));
    await act(async () => {
      await refreshAll.props.onClick();
    });
    expect(vi.mocked(api.refreshCodexAccountQuota).mock.calls).toEqual([
      ['first'],
      ['first'],
      ['second'],
    ]);
  }
);

it('Account Center: China day rollover enables rewards without auto claiming or refreshing', async () => {
  vi.setSystemTime(new Date('2026-10-07T15:59:30Z'));
  const row = { ...rewardAccount, dailyClaimedAt: Date.now() / 1000 };
  vi.mocked(api.listWorkBuddyAccounts).mockResolvedValue([row]);
  await showAccountCenter(['workbuddy']);
  expect(
    renderer.root.findByProps({ 'aria-label': 'agent.dailyCreditsClaimed Reward' }).props.disabled
  ).toBe(true);
  const claimAll = renderer.root.findByProps({ 'aria-label': 'accountCenter.claimAll' });
  expect(claimAll.props.disabled).toBe(false);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_000);
  });
  expect(
    renderer.root.findByProps({ 'aria-label': 'agent.claimDailyCredits Reward' }).props.disabled
  ).toBe(false);
  expect(claimAll.props.disabled).toBe(false);
  expect(api.claimWorkBuddyDailyCredits).not.toHaveBeenCalled();
  expect(api.refreshWorkBuddyAccountQuota).not.toHaveBeenCalled();
  expect(api.startTool).not.toHaveBeenCalled();
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
    let task!: Promise<unknown>;
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
    let refreshTask!: Promise<unknown>;
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
  vi.stubGlobal('localStorage', undefined);
  vi.useRealTimers();
});

it.each(Object.keys(listFor) as Tool[])(
  '%s: uninstalled navigation must not read native accounts',
  async (tool) => {
    await mount(tool, false);
    expect(api[listFor[tool]]).not.toHaveBeenCalled();
  }
);

const cancelCases = [
  ['opendesign', 'startOpenDesignLogin', 'pollOpenDesignLogin', 'cancelOpenDesignLogin'],
  ['codex', 'startCodexLogin', 'addCodexAccountViaOAuth', 'cancelCodexLogin'],
  ['chatgptdesktop', 'startCodexLogin', 'addCodexAccountViaOAuth', 'cancelCodexLogin'],
  ['claudecode', 'startClaudeCodeLogin', 'completeClaudeCodeLogin', 'cancelClaudeCodeLogin'],
  [
    'claudedesktop',
    'startClaudeDesktopLogin',
    'pollClaudeDesktopLogin',
    'cancelClaudeDesktopLogin',
  ],
  ['workbuddy', 'startWorkBuddyLogin', 'pollWorkBuddyLogin', 'cancelWorkBuddyLogin'],
  ['workbuddyai', 'startWorkBuddyLogin', 'pollWorkBuddyLogin', 'cancelWorkBuddyLogin'],
  ['dsh', 'startDeepSeekLogin', 'pollDeepSeekLogin', 'cancelDeepSeekLogin'],
  ['grok', 'startGrokLogin', 'pollGrokLogin', 'cancelGrokLogin'],
  ['manus', 'startManusLogin', 'pollManusLogin', 'cancelManusLogin'],
  ['cue', 'startCueLogin', 'pollCueLogin', 'cancelCueLogin'],
  ['cursor', 'startCursorLogin', 'pollCursorLogin', 'cancelCursorLogin'],
  ['grokbot', 'startGrokBotLogin', 'pollGrokBotLogin', 'cancelGrokBotLogin'],
  ['antigravity', 'startAntigravityLogin', 'pollAntigravityLogin', 'cancelAntigravityLogin'],
  ['antigravitydesktop', 'startAntigravityLogin', 'pollAntigravityLogin', 'cancelAntigravityLogin'],
  ['zcode', 'startZCodeLogin', 'pollZCodeLogin', 'cancelZCodeLogin'],
] as const;

const communityProviderTools = [
  'claudecode',
  'claudedesktop',
  'doubaowork',
  'dola',
  'workbuddy',
  'workbuddyai',
] as const;

it.each(communityProviderTools)(
  'apps: %s account login opens the complete plugin list without starting a login',
  async (tool) => {
    useNavigationStore.setState({ developerMode: false });
    usePluginsStore.setState({ installed: [] });
    await mount(tool as Tool);
    act(() =>
      renderer.update(
        <AppManagerProvider>
          <Harness panel />
        </AppManagerProvider>
      )
    );
    await act(async () => {
      expect(renderer.root.findByType(AccountSectionButton).props.idleLabel).toBe(
        'plugins.category.account'
      );
      renderer.root.findByType(AccountSectionButton).props.onClick();
    });
    expect(useNavigationStore.getState()).toMatchObject({
      developerMode: true,
      activePage: 'developer',
      developerTab: 'plugins',
      accountPluginsRequest: expect.any(Number),
    });
    expect(api.saveSettings).toHaveBeenCalledWith({ developerMode: true, locale: 'en' });
    expect(api.openExternal).not.toHaveBeenCalled();
    for (const action of [
      api.startClaudeCodeLogin,
      api.startClaudeDesktopLogin,
      api.startWorkBuddyLogin,
    ])
      expect(action).not.toHaveBeenCalled();
    expect(api.startTool).not.toHaveBeenCalled();
  }
);

for (const page of ['apps', 'accounts'] as const) {
  const supportedCancelCases = cancelCases.filter(
    ([tool]) => !communityProviderTools.includes(tool as (typeof communityProviderTools)[number])
  );
  it.each(supportedCancelCases)(
    `${page}: %s cancel button ends the current native login and allows an immediate retry`,
    async (tool, startName, pollName, cancelName) => {
      let attempt = 0;
      vi.mocked(api[startName]).mockImplementation(async () => {
        const loginId = `login-${++attempt}`;
        return (
          startName === 'startCodexLogin'
            ? loginId
            : {
                loginId,
                authorizationUrl: 'https://example.test',
                verificationUri: 'https://example.test',
                expiresAt: Date.now() / 1000 + 60,
                pollIntervalSeconds: 1,
              }
        ) as never;
      });
      vi.mocked(api[cancelName]).mockResolvedValue(undefined);
      vi.mocked(api[pollName]).mockImplementation(() => new Promise<never>(() => {}));
      try {
        if (page === 'accounts') await showAccountCenter([tool]);
        else {
          await mount(tool);
          act(() =>
            renderer.update(
              <AppManagerProvider>
                <Harness panel />
              </AppManagerProvider>
            )
          );
        }
        const clickAdd = () => {
          if (page === 'apps') renderer.root.findByType(AccountSectionButton).props.onClick();
          else
            renderer.root
              .findAllByType(DirectoryRow)
              .find((row) => !row.props.add?.disabled)!
              .props.add.onClick();
        };
        await act(async () => clickAdd());
        await tick();
        const browserCalls = vi.mocked(api.openExternal).mock.calls.length;
        if (tool === 'claudedesktop' || tool === 'manus' || tool === 'cue')
          expect(browserCalls).toBe(0);
        const clickCancel = () => {
          const button = renderer.root.findByProps({ 'aria-label': 'btn.cancel' });
          expect(button.props.disabled).not.toBe(true);
          button.props.onClick({ stopPropagation: vi.fn() });
        };
        await act(async () => clickCancel());
        expect(api[cancelName]).toHaveBeenCalledExactlyOnceWith('login-1');
        expect(renderer.root.findAllByProps({ 'aria-label': 'btn.cancel' })).toHaveLength(0);
        await act(async () => clickAdd());
        await tick();
        expect(api[startName]).toHaveBeenCalledTimes(2);
        await act(async () => clickCancel());
        expect(api[cancelName]).toHaveBeenLastCalledWith('login-2');
        expect(renderer.root.findAllByProps({ 'aria-label': 'btn.cancel' })).toHaveLength(0);
        expect(state.applyError).toBeNull();
        expect(api.startTool).not.toHaveBeenCalled();
        expect(api.openExternal).toHaveBeenCalledTimes(browserCalls * 2);
        for (const [key, action] of Object.entries(api))
          if (/^(refresh|switch|delete)/.test(key) && vi.isMockFunction(action))
            expect(action).not.toHaveBeenCalled();
      } finally {
        vi.mocked(api[startName]).mockReset();
        vi.mocked(api[pollName]).mockReset();
        vi.mocked(api[cancelName]).mockReset();
      }
    }
  );
}

it.each(Object.keys(listFor) as Tool[])(
  '%s: installed navigation only reads accounts',
  async (tool) => {
    await mount(tool);
    expect(api[listFor[tool]]).toHaveBeenCalledTimes(1);
    const actions = [
      api.startOpenDesignLogin,
      api.refreshOpenDesignAccount,
      api.switchOpenDesignAccount,
      api.deleteOpenDesignAccount,
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
      api.switchDeepSeekAccount,
      api.switchCursorAccount,
      api.switchAntigravityAccount,
      api.startAntigravityLogin,
      api.refreshAntigravityAccount,
      api.restoreToolToOfficial,
      api.startTool,
    ];
    for (const action of actions) expect(action).not.toHaveBeenCalled();
    const requests = Object.values(api).flatMap((request) =>
      vi.isMockFunction(request) ? [[request, request.mock.calls.length] as const] : []
    );
    for (const hidden of [true, false]) {
      act(() => useAccountPrivacyStore.getState().setHidden(hidden));
      await tick();
      for (const [request, count] of requests) expect(request).toHaveBeenCalledTimes(count);
      expect(state.applyError).toBeNull();
    }
    act(() => state.setViewMode('install'));
    await tick();
    expect(api[listFor[tool]]).toHaveBeenCalledTimes(1);
    for (const action of actions) expect(action).not.toHaveBeenCalled();
  }
);
it.each(['claudecode', 'claudedesktop', 'workbuddy', 'dsh', 'chatgptdesktop'] as const)(
  '%s: failed passive reload preserves cached accounts',
  async (tool) => {
    const row = { id: 'cached', active: true, edition: 'workbuddy' };
    vi.mocked(api[listFor[tool]]).mockResolvedValueOnce([row] as never);
    await mount(tool);
    const rows = () =>
      ({
        claudecode: state.claudeCodeAccounts.accounts,
        claudedesktop: state.claudeDesktopAccounts.accounts,
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

it('Account Center: loads installed groups once, shared stores once, and WorkBuddy editions separately', async () => {
  await mount('dsh');
  act(() =>
    useToolsStore.getState().setDetectedTools(
      (Object.keys(listFor) as Tool[]).map((id) => ({
        id,
        name: id,
        category: 'Desktop',
        installed: true,
      }))
    )
  );
  vi.clearAllMocks();
  act(() => useNavigationStore.getState().setActivePage('accounts'));
  await tick();
  for (const list of listNames)
    expect(api[list]).toHaveBeenCalledTimes(
      ['listWorkBuddyAccounts', 'listTraeAccounts'].includes(list) ? 2 : 1
    );
  expect(api.listWorkBuddyAccounts).toHaveBeenCalledWith('workbuddy');
  expect(api.listWorkBuddyAccounts).toHaveBeenCalledWith('workbuddyai');
  expect(api.listTraeAccounts).toHaveBeenCalledWith('trae');
  expect(api.listTraeAccounts).toHaveBeenCalledWith('traecn');
  for (const name of ['start', 'refresh', 'switch', 'delete', 'claim', 'apply'] as const) {
    for (const [key, action] of Object.entries(api))
      if (key.startsWith(name) && vi.isMockFunction(action)) expect(action).not.toHaveBeenCalled();
  }
  expect(api.startTool).not.toHaveBeenCalled();
  expect(folderPicker).not.toHaveBeenCalled();
  expect(state.applyError).toBeNull();
});

it('Account Center: uninstalled clients list saved accounts without other actions', async () => {
  await mount('dsh', false);
  act(() => useNavigationStore.getState().setActivePage('accounts'));
  await tick();
  // Built-in backends skip client files when uninstalled; plugin-provided
  // accounts list outside that guard, so they still wait for the client.
  const pluginLists: string[] = [
    'listClaudeCodeAccounts',
    'listClaudeDesktopAccounts',
    'listWorkBuddyAccounts',
    'listDoubaoAccounts',
    'listTraeAccounts',
  ];
  for (const list of listNames)
    if (pluginLists.includes(list)) expect(api[list]).not.toHaveBeenCalled();
    else expect(api[list]).toHaveBeenCalled();
  expectPassiveAccountNavigation();
});

it('Account Center: returning after a passive failure preserves both WorkBuddy editions', async () => {
  const cn = { id: 'cn', name: 'CN', active: true, edition: 'workbuddy' };
  const global = { id: 'global', name: 'Global', active: true, edition: 'workbuddyai' };
  vi.mocked(api.listWorkBuddyAccounts).mockImplementation(
    async (edition) => [edition === 'workbuddy' ? cn : global] as never
  );
  await mount('workbuddy');
  act(() =>
    useToolsStore.getState().setDetectedTools(
      ['workbuddy', 'workbuddyai'].map((id) => ({
        id,
        name: id,
        category: 'Desktop',
        installed: true,
      }))
    )
  );
  act(() => useNavigationStore.getState().setActivePage('accounts'));
  await tick();
  act(() => useNavigationStore.getState().setActivePage('models'));
  await tick();
  vi.mocked(api.listWorkBuddyAccounts).mockRejectedValue(new Error('accountError.read'));
  act(() => useNavigationStore.getState().setActivePage('accounts'));
  await tick();
  expect(state.workBuddyAccountGroups.workbuddy.accounts).toEqual([cn]);
  expect(state.workBuddyAccountGroups.workbuddyai.accounts).toEqual([global]);
  expect(state.applyError).toBeNull();
  vi.mocked(api.listWorkBuddyAccounts).mockResolvedValue([]);
});

it('Account Center: entering cancels App Manager login even when the group stays enabled', async () => {
  vi.mocked(api.startClaudeCodeLogin).mockResolvedValue({
    loginId: 'apps-login',
    authorizationUrl: 'https://example.test',
    expiresAt: Date.now() / 1000 + 60,
  });
  await mount('claudecode');
  await act(async () => {
    await state.claudeCodeAccounts.add();
  });
  act(() => useNavigationStore.getState().setActivePage('accounts'));
  await tick();
  expect(api.cancelClaudeCodeLogin).toHaveBeenCalledWith('apps-login');
  expect(state.claudeCodeAccounts.login).toBeNull();
});

it('Account Center: leaving cancels pending login and ignores a late saved account', async () => {
  let resolve!: (account: api.DeepSeekAccount | null) => void;
  vi.mocked(api.startDeepSeekLogin).mockResolvedValue({
    loginId: 'center-login',
    verificationUri: 'https://example.test',
    expiresAt: Date.now() / 1000 + 60,
  });
  vi.mocked(api.pollDeepSeekLogin).mockReturnValue(
    new Promise((done) => {
      resolve = done;
    })
  );
  await mount('dsh');
  act(() => useNavigationStore.getState().setActivePage('accounts'));
  await tick();
  let task!: Promise<void>;
  await act(async () => {
    task = state.deepSeekAccounts.add();
  });
  act(() => useNavigationStore.getState().setActivePage('apps'));
  await tick();
  expect(api.cancelDeepSeekLogin).toHaveBeenCalledWith('center-login');
  await act(async () => {
    resolve({ id: 'late', name: 'late', active: false, balances: null });
    await task;
  });
  expect(state.deepSeekAccounts.accounts).toEqual([]);
  expect(state.applyError).toBeNull();
});

it('Account Center: login saves an account without clearing the desktop API selection or applying it', async () => {
  const model = {
    internalId: 'fixture-model',
    name: 'Fixture',
    baseUrl: 'https://example.test/v1',
    apiKey: 'fixture',
  };
  vi.mocked(api.getModels).mockResolvedValue([model]);
  const account = { id: 'new', name: 'New', active: false, balances: null };
  vi.mocked(api.startDeepSeekLogin).mockResolvedValue({
    loginId: 'center-login',
    verificationUri: 'https://example.test',
    expiresAt: Date.now() / 1000 + 60,
  });
  vi.mocked(api.pollDeepSeekLogin).mockResolvedValue(account);
  await mount('dsh');
  act(() => state.handleSelectModel('dsh', 'fixture-model'));
  act(() => useNavigationStore.getState().setActivePage('accounts'));
  await tick();
  vi.mocked(api.listDeepSeekAccounts).mockResolvedValue([account]);
  await act(async () => {
    renderer.update(
      <AppManagerProvider>
        <Harness center />
      </AppManagerProvider>
    );
  });
  const section = renderer.root.findAll(
    (node) => node.type === 'section' && node.props['aria-label'] === 'DeepSeek Harness'
  )[0];
  await act(async () => {
    section
      .findAll(
        (node) =>
          node.type === 'button' &&
          node.props['aria-label'] === 'agent.addCurrentAccount DeepSeek Harness'
      )[0]
      .props.onClick();
  });
  await tick();
  expect(state.deepSeekAccounts.accounts).toEqual([account]);
  expect(state.toolModelConfig.dsh).toBe('fixture-model');
  expect(api.switchDeepSeekAccount).not.toHaveBeenCalled();
  expect(api.refreshDeepSeekAccountQuota).not.toHaveBeenCalled();
  expect(api.startTool).not.toHaveBeenCalled();
  vi.mocked(api.getModels).mockResolvedValue([]);
});

it('Account Center: ZCode choice is passive, website follows the region and add uses that region', async () => {
  await mount('zcode');
  act(() => useNavigationStore.getState().setActivePage('accounts'));
  await tick();
  await act(async () => {
    renderer.update(
      <AppManagerProvider>
        <Harness center />
      </AppManagerProvider>
    );
  });
  const section = renderer.root.findAll(
    (node) => node.type === 'section' && node.props['aria-label'] === 'ZCode'
  )[0];
  const choice = section.findAll(
    (node) => node.type === 'button' && node.props['aria-label'] === 'agent.zcodeSwitchProvider'
  )[0];
  const website = section.findAll(
    (node) => node.type === 'button' && node.props['aria-label'] === 'accountCenter.website'
  )[0];
  expect(
    section.findAll((node) => node.type === 'span' && node.children.includes('bigmodel.cn'))
  ).toHaveLength(1);
  await act(async () => website.props.onClick());
  expect(api.openExternal).toHaveBeenCalledWith('https://bigmodel.cn/glm-coding');
  expect(api.startZCodeLogin).not.toHaveBeenCalled();
  vi.mocked(api.openExternal).mockClear();
  act(() => choice.props.onClick());
  expect(state.zcodeAccounts.provider).toBe('zai');
  expect(api.startZCodeLogin).not.toHaveBeenCalled();
  expect(api.openExternal).not.toHaveBeenCalled();
  expect(
    section.findAll((node) => node.type === 'span' && node.children.includes('z.ai'))
  ).toHaveLength(1);
  await act(async () => website.props.onClick());
  expect(api.openExternal).toHaveBeenCalledWith('https://z.ai/subscribe');
  expect(api.startZCodeLogin).not.toHaveBeenCalled();
  expect(api.refreshZCodeAccountQuota).not.toHaveBeenCalled();
  expect(api.switchZCodeAccount).not.toHaveBeenCalled();
  expect(api.startTool).not.toHaveBeenCalled();
  vi.mocked(api.openExternal).mockClear();
  act(() => choice.props.onClick());
  expect(state.zcodeAccounts.provider).toBe('bigmodel');
  expect(
    section.findAll((node) => node.type === 'span' && node.children.includes('bigmodel.cn'))
  ).toHaveLength(1);
  expect(api.openExternal).not.toHaveBeenCalled();
  act(() => choice.props.onClick());
  vi.mocked(api.startZCodeLogin).mockResolvedValue(zcodeLogin());
  vi.mocked(api.pollZCodeLogin).mockResolvedValue(zcodeRow);
  await act(async () => {
    section
      .findAll(
        (node) =>
          node.type === 'button' && node.props['aria-label'] === 'agent.addCurrentAccount ZCode'
      )[0]
      .props.onClick();
  });
  await tick();
  expect(api.startZCodeLogin).toHaveBeenCalledWith('zai');
  expect(api.refreshZCodeAccountQuota).not.toHaveBeenCalled();
  expect(api.switchZCodeAccount).not.toHaveBeenCalled();
});

it('Account Center: website and refresh actions are separate, and a refresh failure remains visible', async () => {
  const row = { id: 'saved', name: 'Saved', active: false, balances: null };
  vi.mocked(api.listDeepSeekAccounts).mockResolvedValue([row]);
  vi.mocked(api.openExternal).mockResolvedValue(undefined);
  await mount('dsh');
  act(() =>
    useToolsStore.getState().setDetectedTools([
      {
        id: 'dsh',
        name: 'DeepSeek Harness',
        category: 'CLI Code',
        installed: true,
        website: 'https://www.deepseek.com/harness/',
      },
    ])
  );
  act(() => useNavigationStore.getState().setActivePage('accounts'));
  await tick();
  await act(async () => {
    renderer.update(
      <AppManagerProvider>
        <Harness center />
      </AppManagerProvider>
    );
  });
  const providerSection = renderer.root.findAll(
    (node) => node.type === 'section' && node.props['aria-label'] === 'DeepSeek Harness'
  )[0];
  const website = providerSection.findAll(
    (node) => node.type === 'button' && node.props['aria-label'] === 'accountCenter.website'
  )[0];
  expect(website.props.disabled).toBe(false);
  await act(async () => {
    await website.props.onClick();
  });
  expect(api.openExternal).toHaveBeenCalledWith('https://www.deepseek.com/harness/');
  expect(api.startDeepSeekLogin).not.toHaveBeenCalled();
  expect(api.refreshDeepSeekAccountQuota).not.toHaveBeenCalled();
  vi.mocked(api.refreshDeepSeekAccountQuota).mockRejectedValueOnce(new Error('accountError.quota'));
  const refresh = renderer.root.findAll(
    (node) => node.type === 'button' && node.props['aria-label'] === 'agent.refreshAccount Saved'
  )[0];
  await act(async () => {
    refresh.props.onClick({ stopPropagation: vi.fn() });
  });
  expect(api.refreshDeepSeekAccountQuota).toHaveBeenCalledWith('saved', 'en');
  expect(state.applyError).toBe('accountError.quota');
  expect(state.deepSeekAccounts.accounts).toEqual([row]);
});

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
it.each(['workbuddy', 'workbuddyai'] as const)(
  '%s: account selection preserves custom model configuration',
  async (edition) => {
    vi.mocked(api.listWorkBuddyAccounts).mockResolvedValue([
      { id: 'fixture', name: 'fixture', edition, active: true },
    ] as never);
    await mount(edition);
    expect(api.switchWorkBuddyAccount).not.toHaveBeenCalled();
    await act(async () => {
      await state.handleLaunch();
    });
    expect(api.switchWorkBuddyAccount).toHaveBeenCalledExactlyOnceWith(edition, 'fixture');
    expect(api.restoreToolToOfficial).not.toHaveBeenCalled();
  }
);
it.each(['workbuddy', 'workbuddyai'] as const)(
  '%s: failed history/account apply is visible and prevents launch',
  async (edition) => {
    vi.mocked(api.listWorkBuddyAccounts).mockResolvedValue([
      { id: 'fixture', name: 'fixture', edition, active: true },
    ] as never);
    await mount(edition);
    const loaded = vi.mocked(api.listWorkBuddyAccounts).mock.calls.length;
    vi.mocked(api.switchWorkBuddyAccount).mockRejectedValueOnce(
      new Error('accountError.write|WorkBuddy history: database is locked')
    );
    await act(async () => {
      await state.handleLaunch();
    });
    expect(state.applyError).toBe('accountError.write');
    expect(api.startTool).not.toHaveBeenCalled();
    expect(api.listWorkBuddyAccounts).toHaveBeenCalledTimes(loaded);
    expect(api.refreshWorkBuddyAccountQuota).not.toHaveBeenCalled();
    expect(api.restoreToolToOfficial).not.toHaveBeenCalled();
  }
);
const sharedHistoryTools = [
  ['zcode', 'listZCodeAccounts', 'switchZCodeAccount'],
  ['dsh', 'listDeepSeekAccounts', 'switchDeepSeekAccount'],
  ['grok', 'listGrokAccounts', 'switchGrokAccount'],
  ['cursor', 'listCursorAccounts', 'switchCursorAccount'],
  ['antigravity', 'listAntigravityAccounts', 'switchAntigravityAccount'],
  ['antigravitydesktop', 'listAntigravityAccounts', 'switchAntigravityAccount'],
] as const;
it.each(sharedHistoryTools)(
  '%s: explicit account apply completes before reload and native launch',
  async (tool, list, apply) => {
    const row = { id: 'fixture', email: 'fixture@example.test', active: true };
    vi.mocked(api[list]).mockResolvedValue([row] as never);
    await mount(tool);
    act(() => state.setLaunchAfterApply(true));
    const loaded = vi.mocked(api[list]).mock.calls.length;
    let complete!: (result: never) => void;
    vi.mocked(api[apply]).mockReturnValueOnce(
      new Promise((resolve) => {
        complete = resolve;
      }) as never
    );
    let task!: Promise<void>;
    act(() => {
      task = state.handleLaunch();
    });
    await tick();
    expect(api[apply]).toHaveBeenCalledTimes(1);
    expect(vi.mocked(api[apply]).mock.calls[0][0]).toBe('fixture');
    expect(api[list]).toHaveBeenCalledTimes(loaded);
    expect(api.startTool).not.toHaveBeenCalled();
    await act(async () => {
      await state.handleLaunch();
    });
    expect(api[apply]).toHaveBeenCalledTimes(1);
    await act(async () => {
      complete(row as never);
      await task;
    });
    expect(api[list]).toHaveBeenCalledTimes(loaded + 1);
    expect(api.startTool).toHaveBeenCalledTimes(1);
    expect(vi.mocked(api.startTool).mock.calls[0][0]).toBe(tool);
    expect(state.applyError).toBeNull();
  }
);
it.each(sharedHistoryTools)(
  '%s: failed account apply preserves cached rows and prevents launch',
  async (tool, list, apply) => {
    const row = { id: 'fixture', email: 'fixture@example.test', active: true };
    vi.mocked(api[list]).mockResolvedValue([row] as never);
    await mount(tool);
    const loaded = vi.mocked(api[list]).mock.calls.length;
    vi.mocked(api[apply]).mockRejectedValueOnce(new Error('accountError.write'));
    await act(async () => {
      await state.handleLaunch();
    });
    expect(api[apply]).toHaveBeenCalledTimes(1);
    expect(state.applyError).toBe('accountError.write');
    expect(api[list]).toHaveBeenCalledTimes(loaded);
    expect(api.startTool).not.toHaveBeenCalled();
    const rows = {
      zcode: state.zcodeAccounts.accounts,
      dsh: state.deepSeekAccounts.accounts,
      grok: state.grokAccounts.accounts,
      cursor: state.cursorAccounts.accounts,
      antigravity: state.antigravityAccounts.accounts,
      antigravitydesktop: state.antigravityAccounts.accounts,
    };
    expect(rows[tool]).toEqual([row]);
    expect(api.refreshZCodeAccountQuota).not.toHaveBeenCalled();
    expect(api.refreshDeepSeekAccountQuota).not.toHaveBeenCalled();
    expect(api.refreshGrokAccount).not.toHaveBeenCalled();
    expect(api.refreshCursorAccount).not.toHaveBeenCalled();
    expect(api.refreshAntigravityAccount).not.toHaveBeenCalled();
  }
);
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
  let refresh!: Promise<unknown>;
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

const traeRow: api.TraeAccount = {
  id: 'same',
  edition: 'traecn',
  name: 'CN account',
  plan: 'Pro',
  active: true,
  remaining: 20,
};

it.each(['trae', 'traecn'] as const)(
  '%s: account-only panel and passive navigation preserve native account/model state',
  async (edition) => {
    const storage = mockAccountOrderStorage();
    vi.mocked(api.listTraeAccounts).mockImplementation(async (scope) => [
      { ...traeRow, edition: scope, name: scope },
    ]);
    await mount(edition);
    act(() =>
      renderer.update(
        <AppManagerProvider>
          <Harness panel />
        </AppManagerProvider>
      )
    );
    expect(renderer.root.findAllByType(TraeAccountSection)).toHaveLength(1);
    for (const mode of ['install', 'desktop'] as const) {
      act(() => state.setViewMode(mode));
      await tick();
    }
    for (const page of ['models', 'accounts', 'apps'] as const) {
      act(() => useNavigationStore.getState().setActivePage(page));
      await tick();
    }
    expect(state.traeAccountGroups[edition].accounts[0].edition).toBe(edition);
    expect(storage.setItem).not.toHaveBeenCalled();
    for (const [key, action] of Object.entries(api))
      if (/^(start|refresh|switch|delete|claim|restore)/.test(key) && vi.isMockFunction(action))
        expect(action).not.toHaveBeenCalled();
    expect(folderPicker).not.toHaveBeenCalled();
    expect(state.applyError).toBeNull();
  }
);

it.each(['trae', 'traecn'] as const)(
  '%s: explicit account apply switches its own region and launches without model restore',
  async (edition) => {
    vi.mocked(api.listTraeAccounts).mockImplementation(async (scope) => [
      { ...traeRow, edition: scope },
    ]);
    vi.mocked(api.switchTraeAccount).mockResolvedValue(undefined);
    await mount(edition);
    await act(async () => {
      await state.handleLaunch();
    });
    expect(api.switchTraeAccount).toHaveBeenCalledExactlyOnceWith(edition, traeRow.id);
    expect(api.startTool).toHaveBeenCalledExactlyOnceWith(edition, undefined);
    expect(api.restoreToolToOfficial).not.toHaveBeenCalled();
    expect(api.refreshTraeAccountQuota).not.toHaveBeenCalled();
    expect(api.claimTraeDailyCredits).not.toHaveBeenCalled();
    expect(folderPicker).not.toHaveBeenCalled();
    expect(state.isLaunching).toBe(false);
  }
);

it('TRAE: pending account apply cannot be repeated after the normal launch cooldown', async () => {
  vi.mocked(api.listTraeAccounts).mockResolvedValue([traeRow]);
  await mount('traecn');
  let resolve!: () => void;
  vi.mocked(api.switchTraeAccount).mockReturnValueOnce(
    new Promise<void>((done) => {
      resolve = done;
    })
  );
  let applying!: Promise<void>;
  act(() => {
    applying = state.handleLaunch();
  });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(3100);
    await state.handleLaunch();
  });
  expect(state.isLaunching).toBe(true);
  expect(api.switchTraeAccount).toHaveBeenCalledTimes(1);
  expect(api.startTool).not.toHaveBeenCalled();
  await act(async () => {
    resolve();
    await applying;
  });
  expect(api.startTool).toHaveBeenCalledExactlyOnceWith('traecn', undefined);
});

it('TRAE: explicit switch failure is visible and prevents launch', async () => {
  vi.mocked(api.listTraeAccounts).mockResolvedValue([traeRow]);
  vi.mocked(api.switchTraeAccount).mockRejectedValueOnce(new Error('accountError.write'));
  await mount('traecn');
  await act(async () => {
    await state.handleLaunch();
  });
  expect(state.applyError).toBe('accountError.write');
  expect(state.traeAccounts.accounts).toEqual([traeRow]);
  expect(api.startTool).not.toHaveBeenCalled();
  expect(api.restoreToolToOfficial).not.toHaveBeenCalled();
});

it('Account Center: TRAE regions stay separate and the shared reward batch skips unavailable rows', async () => {
  const now = Date.now() / 1000;
  const claimed = { ...traeRow, id: 'claimed', name: 'Claimed', dailyClaimedAt: now };
  const eligible = { ...traeRow, id: 'eligible', name: 'Eligible', active: false };
  const disabled = { ...traeRow, id: 'disabled', name: 'Disabled', rewardEnabled: false };
  vi.mocked(api.listTraeAccounts).mockImplementation(async (edition) =>
    edition === 'traecn'
      ? [claimed, eligible, disabled]
      : [{ ...traeRow, edition: 'trae', name: 'Global' }]
  );
  vi.mocked(api.claimTraeDailyCredits).mockResolvedValue({
    ...eligible,
    dailyClaimedAt: now,
    deviceClaimedAt: now,
    remaining: 120,
  });
  await showAccountCenter(['traecn', 'trae']);
  expect(state.traeAccountGroups.trae.accounts[0].edition).toBe('trae');
  expect(state.traeAccountGroups.traecn.accounts).toHaveLength(3);
  expect(api.claimTraeDailyCredits).not.toHaveBeenCalled();
  expect(
    renderer.root
      .findAllByType('button')
      .some((button) => button.props['aria-label'] === 'agent.claimDailyCredits Global')
  ).toBe(false);
  await act(async () => {
    await renderer.root.findByProps({ 'aria-label': 'accountCenter.claimAll' }).props.onClick();
  });
  expect(api.claimTraeDailyCredits).toHaveBeenCalledExactlyOnceWith('traecn', 'eligible');
  expect(
    state.traeAccountGroups.traecn.accounts.find((row) => row.id === 'eligible')?.remaining
  ).toBe(120);
  expect(api.refreshTraeAccountQuota).not.toHaveBeenCalled();
  expect(api.switchTraeAccount).not.toHaveBeenCalled();
  expect(api.startTraeLogin).not.toHaveBeenCalled();
});

it('Account Center: a used TRAE device leaves unclaimed gifts disabled and does not request status or rewards', async () => {
  vi.mocked(api.listTraeAccounts).mockResolvedValue([
    { ...traeRow, deviceClaimedAt: Date.now() / 1000 },
    { ...traeRow, id: 'other', name: 'Other', active: false },
  ]);
  await showAccountCenter(['traecn']);
  const other = renderer.root.findByProps({ 'aria-label': 'agent.claimDailyCredits Other' });
  expect(other.props.disabled).toBe(true);
  await act(async () => {
    await renderer.root.findByProps({ 'aria-label': 'accountCenter.claimAll' }).props.onClick();
  });
  expect(api.claimTraeDailyCredits).not.toHaveBeenCalled();
  expect(api.refreshTraeAccountQuota).not.toHaveBeenCalled();
});

it('Account Center: an explicit TRAE reward batch rechecks eligibility after China midnight', async () => {
  vi.setSystemTime(new Date('2026-10-13T15:59:30Z'));
  const beforeMidnight = Date.now() / 1000;
  const account = {
    ...traeRow,
    dailyClaimedAt: beforeMidnight,
    deviceClaimedAt: beforeMidnight,
  };
  vi.mocked(api.listTraeAccounts).mockResolvedValue([account]);
  vi.mocked(api.claimTraeDailyCredits).mockResolvedValue({
    ...account,
    dailyClaimedAt: beforeMidnight + 60,
    deviceClaimedAt: beforeMidnight + 60,
  });
  await showAccountCenter(['traecn']);
  expect(api.claimTraeDailyCredits).not.toHaveBeenCalled();
  vi.setSystemTime(new Date('2026-10-13T16:00:01Z'));
  expect(api.claimTraeDailyCredits).not.toHaveBeenCalled();
  await act(async () => {
    await renderer.root.findByProps({ 'aria-label': 'accountCenter.claimAll' }).props.onClick();
  });
  expect(api.claimTraeDailyCredits).toHaveBeenCalledExactlyOnceWith('traecn', account.id);
  expect(api.refreshTraeAccountQuota).not.toHaveBeenCalled();
  expect(api.startTraeLogin).not.toHaveBeenCalled();
  expect(api.switchTraeAccount).not.toHaveBeenCalled();
});

it.each(['failure', 'success'] as const)(
  'Account Center: a TRAE reward batch rechecks the next row after an asynchronous %s',
  async (outcome) => {
    const first = { ...traeRow, id: 'first', name: 'First' };
    const second = { ...traeRow, id: 'second', name: 'Second', active: false };
    vi.mocked(api.listTraeAccounts).mockResolvedValue([first, second]);
    let resolve!: (account: api.TraeAccount) => void;
    let reject!: (error: Error) => void;
    vi.mocked(api.claimTraeDailyCredits)
      .mockReset()
      .mockReturnValueOnce(
        new Promise<api.TraeAccount>((done, fail) => {
          resolve = done;
          reject = fail;
        })
      )
      .mockResolvedValueOnce({
        ...second,
        dailyClaimedAt: Date.now() / 1000,
        deviceClaimedAt: Date.now() / 1000,
      });
    await showAccountCenter(['traecn']);
    expect(api.claimTraeDailyCredits).not.toHaveBeenCalled();
    let batch!: Promise<void>;
    await act(async () => {
      batch = renderer.root.findByProps({ 'aria-label': 'accountCenter.claimAll' }).props.onClick();
    });
    expect(api.claimTraeDailyCredits).toHaveBeenCalledExactlyOnceWith('traecn', first.id);
    expect(state.traeAccountGroups.traecn.refreshing.has(first.id)).toBe(true);
    await act(async () => {
      if (outcome === 'failure') reject(new Error('accountError.network'));
      else
        resolve({
          ...first,
          dailyClaimedAt: Date.now() / 1000,
          deviceClaimedAt: Date.now() / 1000,
        });
      await batch;
    });
    expect(api.claimTraeDailyCredits).toHaveBeenCalledTimes(outcome === 'failure' ? 2 : 1);
    if (outcome === 'failure') {
      expect(api.claimTraeDailyCredits).toHaveBeenLastCalledWith('traecn', second.id);
      expect(state.applyError).toBe('Claim failed: 1 (TRAE CN · First)');
    }
    expect(state.traeAccountGroups.traecn.refreshing.size).toBe(0);
    expect(api.refreshTraeAccountQuota).not.toHaveBeenCalled();
    expect(api.startTraeLogin).not.toHaveBeenCalled();
    expect(api.switchTraeAccount).not.toHaveBeenCalled();
  }
);

it.each(['kirocrew', 'kirocli'] as const)(
  '%s: passive navigation loads accounts without quota, login, account writes or dialogs',
  async (edition) => {
    const row: api.KiroAccount = {
      id: 'kiro-user',
      edition,
      name: 'Kiro user',
      plan: 'Free',
      active: true,
      quota: [],
    };
    vi.mocked(api.listKiroAccounts).mockResolvedValue([row]);
    await mount(edition);
    expect(state.kiroAccounts.accounts).toEqual([row]);
    expect(state.kiroAccounts.selectedId).toBe(row.id);
    for (const page of ['accounts', 'models', 'apps'] as const) {
      act(() => useNavigationStore.getState().setActivePage(page));
      await tick();
    }
    for (const method of [
      api.startKiroLogin,
      api.refreshKiroAccountQuota,
      api.switchKiroAccount,
      api.deleteKiroAccount,
      api.startTool,
    ])
      expect(method).not.toHaveBeenCalled();
    expect(folderPicker).not.toHaveBeenCalled();
    expect(state.applyError).toBeNull();
  }
);

it.each(['kirocrew', 'kirocli'] as const)(
  '%s: applying an account reuses the existing launch and directory flow',
  async (edition) => {
    const row: api.KiroAccount = {
      id: 'kiro-user',
      edition,
      name: 'Kiro user',
      active: true,
      quota: [],
    };
    vi.mocked(api.listKiroAccounts).mockResolvedValue([row]);
    vi.mocked(api.switchKiroAccount).mockResolvedValue(undefined);
    await mount(edition);
    await act(async () => {
      await state.handleLaunch();
    });
    expect(api.switchKiroAccount).toHaveBeenCalledExactlyOnceWith(edition, row.id);
    if (edition === 'kirocli') {
      expect(folderPicker).toHaveBeenCalledExactlyOnceWith({ directory: true, multiple: false });
      expect(api.startTool).toHaveBeenCalledExactlyOnceWith(
        edition,
        undefined,
        'E:/fixture-project'
      );
    } else {
      expect(folderPicker).not.toHaveBeenCalled();
      expect(api.startTool).toHaveBeenCalledExactlyOnceWith(edition, undefined, undefined);
    }
    expect(state.isLaunching).toBe(false);
    expect(api.restoreToolToOfficial).not.toHaveBeenCalled();
    expect(api.applyModelToTool).not.toHaveBeenCalled();
    expect(api.refreshKiroAccountQuota).not.toHaveBeenCalled();
  }
);

it('Kiro: an explicit switch failure preserves rows and prevents launch', async () => {
  const row: api.KiroAccount = {
    id: 'kiro-user',
    edition: 'kirocrew',
    name: 'Kiro user',
    active: true,
    quota: [],
  };
  vi.mocked(api.listKiroAccounts).mockResolvedValue([row]);
  vi.mocked(api.switchKiroAccount).mockRejectedValueOnce(new Error('accountError.stopClient'));
  await mount('kirocrew');
  await act(async () => {
    await state.handleLaunch();
  });
  expect(state.applyError).toBe('accountError.stopClient');
  expect(state.kiroAccounts.accounts).toEqual([row]);
  expect(api.startTool).not.toHaveBeenCalled();
  expect(folderPicker).not.toHaveBeenCalled();
});
