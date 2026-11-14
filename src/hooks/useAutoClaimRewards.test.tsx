import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { useAutoClaimRewards } from './useAutoClaimRewards';
import { useRewardClaimStore } from '../stores/rewardClaimStore';
import { useToolsStore } from '../stores/toolsStore';
import { useNavigationStore } from '../stores/navigationStore';
import { claimAllSavedRewards } from '../services/accountRewardClaims';

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }));

let view: ReactTestRenderer | undefined;
let rows: Record<string, Record<string, unknown>[]>;
function Harness() {
  useAutoClaimRewards();
  return null;
}
function installed(...ids: string[]) {
  useToolsStore
    .getState()
    .setDetectedTools(ids.map((id) => ({ id, name: id, category: 'Desktop', installed: true })));
}
async function mount() {
  await act(async () => {
    view = create(<Harness />);
  });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(0);
  });
}
const claims = () => mocks.invoke.mock.calls.filter(([command]) => command.startsWith('claim_'));

it('rechecks Doubao daily eligibility after a cached claim without enabling automatic rewards by default', async () => {
  rows.doubao = [{ id: 'doubao', edition: 'doubaowork', dailyReward: { status: 'claimed' } }];
  installed('doubaowork');
  useToolsStore.setState({ hasScanned: true });
  await mount();
  expect(claims()).toHaveLength(0);
  await act(async () => {
    useRewardClaimStore.getState().setEnabled(true);
  });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(0);
  });
  expect(claims()).toEqual([
    ['claim_doubao_daily_credits', { edition: 'doubaowork', accountId: 'doubao' }],
  ]);
  rows.doubao[0].dailyReward = { status: 'unavailable' };
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60 * 60 * 1000);
  });
  expect(claims()).toHaveLength(1);
});

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2026-10-22T06:00:00Z'));
  vi.stubGlobal('localStorage', { getItem: vi.fn(), setItem: vi.fn() });
  vi.clearAllMocks();
  vi.spyOn(console, 'warn').mockImplementation(() => {});
  useRewardClaimStore.setState({ enabled: false, running: false, revision: 0, updatedScope: null });
  useToolsStore.setState({
    detectedTools: [],
    hasScanned: false,
    isScanning: false,
    usingCachedTools: false,
  });
  rows = {
    workbuddy: [
      { id: 'w1', edition: 'workbuddy' },
      { id: 'w2', edition: 'workbuddy' },
    ],
    trae: [{ id: 't1', edition: 'traecn', rewardEnabled: true }],
    minimax: [{ id: 'm1', region: 'cn' }],
    dumate: [{ id: 'd1', enterprise: false }],
    qwenwork: [{ id: 'q1', edition: 'qwenworkcn' }],
    qoder: [{ id: 'o1', edition: 'qoder' }],
    codearts: [{ id: 'c1' }],
  };
  mocks.invoke.mockImplementation(async (command: string, args?: Record<string, string>) => {
    const scope = command.split('_')[1];
    if (command.startsWith('list_')) return rows[scope] ?? [];
    if (command.startsWith('claim_')) {
      const account = rows[scope].find((row) => row.id === args?.accountId)!;
      return { ...account, dailyClaimedAt: Date.now() / 1000 };
    }
    throw new Error(`Unexpected operation ${command}`);
  });
});
afterEach(async () => {
  await act(async () => view?.unmount());
  view = undefined;
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

it('makes no account or claim calls by default, including after navigation and hours of runtime', async () => {
  installed('workbuddy');
  await mount();
  useNavigationStore.getState().setActivePage('accounts');
  await act(async () => {
    await vi.advanceTimersByTimeAsync(3 * 3600_000);
  });
  useNavigationStore.getState().setActivePage('apps');
  expect(mocks.invoke).not.toHaveBeenCalled();
});

it('never includes QwenWork in startup or hourly claims while other gifts still work', async () => {
  installed('qwenworkcn', 'qwenwork', 'workbuddy');
  rows.qwenwork[0].dailyReward = { status: 'CLAIMABLE', checkedAt: Date.now() / 1000 };
  useRewardClaimStore.setState({ enabled: true });
  useToolsStore.setState({ hasScanned: true });
  await mount();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(3600_000);
  });
  expect(claims().map(([command]) => command)).toEqual([
    'claim_workbuddy_daily_credits',
    'claim_workbuddy_daily_credits',
  ]);
  expect(mocks.invoke.mock.calls.some(([command]) => command.includes('qwenwork'))).toBe(false);
});

it('waits for a current scan before using cached tools for automatic rewards', async () => {
  installed('workbuddy');
  useToolsStore.setState({ usingCachedTools: true });
  useRewardClaimStore.setState({ enabled: true });
  await mount();
  expect(mocks.invoke).not.toHaveBeenCalled();
  await act(async () => installed('workbuddy'));
  await act(async () => {
    await vi.advanceTimersByTimeAsync(0);
  });
  expect(claims()).toHaveLength(2);
});

it('starts after installation detection and keeps an hourly schedule across pages', async () => {
  useRewardClaimStore.setState({ enabled: true });
  await mount();
  expect(mocks.invoke).not.toHaveBeenCalled();
  await act(async () => installed('workbuddy'));
  await act(async () => {
    await vi.advanceTimersByTimeAsync(0);
  });
  expect(claims()).toHaveLength(2);
  expect(useRewardClaimStore.getState().revision).toBe(2);
  await act(async () => {
    useNavigationStore.getState().setActivePage('mother');
    await vi.advanceTimersByTimeAsync(3599_999);
  });
  expect(claims()).toHaveLength(2);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(1);
  });
  expect(claims()).toHaveLength(2);
  expect(
    mocks.invoke.mock.calls.filter(([command]) => command === 'list_workbuddy_accounts')
  ).toHaveLength(2);
  await act(async () => useRewardClaimStore.getState().setEnabled(false));
  await act(async () => {
    await vi.advanceTimersByTimeAsync(2 * 3600_000);
  });
  expect(claims()).toHaveLength(2);
});

it('reuses only installed tools and their existing claim endpoints and eligibility rules', async () => {
  installed(
    'workbuddy',
    'workbuddyai',
    'traecn',
    'trae',
    'minimaxdesktop',
    'dumate',
    'qwenworkcn',
    'qwenwork',
    'qoder',
    'codearts'
  );
  rows.workbuddy[0].dailyClaimedAt = Date.now() / 1000;
  rows.dumate[0].enterprise = true;
  rows.qwenwork[0].dailyReward = {
    checkedAt: Date.now() / 1000,
    nextClaimAt: Date.now() / 1000 + 86400,
    status: 'CLAIMED_TODAY',
  };
  rows.qoder[0].dailyReward = {
    checkedAt: Date.now() / 1000,
    resetAt: Date.now() / 1000 + 86400,
    status: 'unavailable',
  };
  await claimAllSavedRewards(() => true);
  expect(claims().map(([command]) => command)).toEqual([
    'claim_workbuddy_daily_credits',
    'claim_trae_daily_credits',
    'claim_minimax_daily_credits',
    'claim_codearts_credits',
  ]);
  expect(mocks.invoke.mock.calls.filter(([command]) => command.startsWith('list_'))).toHaveLength(
    6
  );
  expect(mocks.invoke.mock.calls).not.toContainEqual([
    'list_workbuddy_accounts',
    { edition: 'workbuddyai' },
  ]);
  expect(mocks.invoke.mock.calls).not.toContainEqual([
    'list_qwenwork_accounts',
    { edition: 'qwenwork' },
  ]);
  expect(useRewardClaimStore.getState().running).toBe(false);
});

it('stops the remaining queue after opt-out while allowing the in-flight result to save', async () => {
  installed('workbuddy', 'codearts');
  useRewardClaimStore.setState({ enabled: true });
  let finish!: (value: unknown) => void;
  const original = mocks.invoke.getMockImplementation()!;
  mocks.invoke.mockImplementation((command, args) =>
    command === 'claim_workbuddy_daily_credits'
      ? new Promise((resolve) => {
          finish = resolve;
        })
      : original(command, args)
  );
  await mount();
  expect(claims()).toHaveLength(1);
  await act(async () => {
    useRewardClaimStore.getState().setEnabled(false);
    finish(rows.workbuddy[0]);
  });
  expect(claims()).toHaveLength(1);
  expect(mocks.invoke.mock.calls.some(([command]) => command === 'list_codearts_accounts')).toBe(
    false
  );
  expect(useRewardClaimStore.getState().running).toBe(false);
});

it('does not overlap a manual batch and waits for startup scanning and the current batch to finish', async () => {
  installed('codearts');
  useToolsStore.setState({ isScanning: true });
  useRewardClaimStore.setState({ enabled: true });
  expect(useRewardClaimStore.getState().tryStart()).toBe(true);
  await mount();
  expect(mocks.invoke).not.toHaveBeenCalled();
  await act(async () => useToolsStore.setState({ isScanning: false }));
  expect(mocks.invoke).not.toHaveBeenCalled();
  await act(async () => useRewardClaimStore.getState().finish());
  expect(claims()).toHaveLength(1);
  expect(useRewardClaimStore.getState().running).toBe(false);
});

it('keeps one sequential batch while a slow claim spans navigation and hourly ticks', async () => {
  installed('workbuddy');
  useRewardClaimStore.setState({ enabled: true });
  let finish!: (value: unknown) => void;
  const original = mocks.invoke.getMockImplementation()!;
  mocks.invoke.mockImplementation((command, args) =>
    command === 'claim_workbuddy_daily_credits' && args.accountId === 'w1'
      ? new Promise((resolve) => {
          finish = resolve;
        })
      : original(command, args)
  );
  await mount();
  expect(claims()).toHaveLength(1);
  expect(useRewardClaimStore.getState().tryStart()).toBe(false);
  await act(async () => {
    useNavigationStore.getState().setActivePage('models');
    await vi.advanceTimersByTimeAsync(2 * 3600_000);
  });
  expect(claims()).toHaveLength(1);
  await act(async () => finish(rows.workbuddy[0]));
  expect(claims()).toHaveLength(2);
  expect(useRewardClaimStore.getState().running).toBe(false);
});

it('keeps TRAE device-level deduplication and skips the next account after a device claim', async () => {
  installed('traecn');
  rows.trae.push({ ...rows.trae[0], id: 't2' });
  const original = mocks.invoke.getMockImplementation()!;
  mocks.invoke.mockImplementation((command, args) =>
    command === 'claim_trae_daily_credits'
      ? Promise.resolve({
          ...rows.trae[0],
          dailyClaimedAt: Date.now() / 1000,
          deviceClaimedAt: Date.now() / 1000,
        })
      : original(command, args)
  );
  await claimAllSavedRewards(() => true);
  expect(claims()).toHaveLength(1);
});

it('continues after one account or provider fails without opening login, quota refresh or apply dialogs', async () => {
  installed('workbuddy', 'traecn', 'codearts');
  const original = mocks.invoke.getMockImplementation()!;
  mocks.invoke.mockImplementation((command, args) => {
    if (
      command === 'list_trae_accounts' ||
      (command === 'claim_workbuddy_daily_credits' && args?.accountId === 'w1')
    )
      return Promise.reject(new Error('fixture failure'));
    return original(command, args);
  });
  await claimAllSavedRewards(() => true);
  expect(claims()).toHaveLength(3);
  expect(console.warn).toHaveBeenCalledTimes(2);
  expect(mocks.invoke.mock.calls.every(([command]) => /^(list_|claim_)/.test(command))).toBe(true);
  expect(useRewardClaimStore.getState().running).toBe(false);
});
