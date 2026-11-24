import { useLayoutEffect } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import * as api from '../../api/tauri';
import { useTraeAccounts } from './useTraeAccounts';
import { useNavigationStore } from '../../stores/navigationStore';

vi.mock('../../api/tauri', () => ({
  listTraeAccounts: vi.fn(),
  startTraeLogin: vi.fn(),
  pollTraeLogin: vi.fn(),
  cancelTraeLogin: vi.fn().mockResolvedValue(undefined),
  deleteTraeAccount: vi.fn().mockResolvedValue(undefined),
  refreshTraeAccountQuota: vi.fn(),
  claimTraeDailyCredits: vi.fn(),
  openExternal: vi.fn().mockResolvedValue(undefined),
}));
vi.mock('../../hooks/useI18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }));
vi.mock('../../components/ConfirmDialog', () => ({ useConfirm: () => async () => true }));

const cn: api.TraeAccount = {
  id: 'same',
  edition: 'traecn',
  name: 'CN',
  plan: 'Pro',
  active: true,
  remaining: 20,
};
const global: api.TraeAccount = { ...cn, edition: 'trae', name: 'Global' };
const showError = vi.fn();
let state: ReturnType<typeof useTraeAccounts>;
let renderer: ReactTestRenderer;
function Harness({
  edition = 'traecn',
  enabled = true,
}: {
  edition?: api.TraeEdition;
  enabled?: boolean;
}) {
  const result = useTraeAccounts(edition, enabled, showError);
  useLayoutEffect(() => {
    state = result;
  });
  return null;
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}
async function tick() {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(1);
  });
}
async function mount(edition: api.TraeEdition = 'traecn', enabled = true) {
  act(() => {
    renderer = create(<Harness edition={edition} enabled={enabled} />);
  });
  await tick();
}
beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2026-10-12T00:00:00Z'));
  vi.clearAllMocks();
  vi.mocked(api.listTraeAccounts)
    .mockReset()
    .mockImplementation(async (edition) => (edition === 'traecn' ? [cn] : [global]));
  useNavigationStore.getState().setActivePage('apps');
});
afterEach(() => {
  act(() => {
    renderer?.unmount();
  });
  vi.useRealTimers();
});

it.each(['trae', 'traecn'] as const)(
  '%s: loads saved rows and changes selection without network quota or reward calls',
  async (edition) => {
    await mount(edition);
    expect(state.accounts[0].edition).toBe(edition);
    expect(state.selectedId).toBe('same');
    act(() => state.select(null));
    expect(state.selectedId).toBeNull();
    act(() => state.select('same'));
    expect(state.selectedId).toBe('same');
    expect(api.listTraeAccounts).toHaveBeenCalledExactlyOnceWith(edition);
    expect(api.refreshTraeAccountQuota).not.toHaveBeenCalled();
    expect(api.claimTraeDailyCredits).not.toHaveBeenCalled();
    expect(api.startTraeLogin).not.toHaveBeenCalled();
  }
);

it('does not read native rows or perform account operations for a disabled client', async () => {
  await mount('traecn', false);
  await act(async () => {
    await state.add();
    await state.refresh(cn);
    await state.claimDaily(cn);
    await state.remove(cn);
  });
  for (const method of [
    api.listTraeAccounts,
    api.startTraeLogin,
    api.refreshTraeAccountQuota,
    api.claimTraeDailyCredits,
    api.deleteTraeAccount,
  ])
    expect(method).not.toHaveBeenCalled();
});

it('retains each region on a passive read failure after switching regions and pages', async () => {
  await mount();
  act(() => renderer.update(<Harness edition="trae" />));
  await tick();
  expect(state.accounts).toEqual([global]);
  vi.mocked(api.listTraeAccounts).mockRejectedValue(new Error('accountError.read'));
  act(() => renderer.update(<Harness edition="traecn" />));
  expect(state.accounts).toEqual([cn]);
  await tick();
  act(() => useNavigationStore.getState().setActivePage('accounts'));
  await tick();
  expect(state.accounts).toEqual([cn]);
  expect(showError).not.toHaveBeenCalled();
  expect(api.refreshTraeAccountQuota).not.toHaveBeenCalled();
  expect(api.claimTraeDailyCredits).not.toHaveBeenCalled();
});

it('keeps an older region response from replacing the current rows', async () => {
  const old = deferred<api.TraeAccount[]>();
  vi.mocked(api.listTraeAccounts).mockReturnValueOnce(old.promise);
  await mount();
  act(() => renderer.update(<Harness edition="trae" />));
  await tick();
  await act(async () => {
    old.resolve([cn]);
  });
  expect(state.accounts).toEqual([global]);
});

it.each(['leave', 'timeout', 'cancel'] as const)(
  'cancels login on %s and ignores a late account',
  async (reason) => {
    await mount();
    const poll = deferred<api.TraeAccount | null>();
    vi.mocked(api.startTraeLogin).mockResolvedValue({
      loginId: 'login',
      verificationUri: 'https://www.trae.cn/login',
      expiresAt: Date.now() / 1000 + 120,
    });
    vi.mocked(api.pollTraeLogin).mockReturnValueOnce(poll.promise);
    let adding!: Promise<void>;
    await act(async () => {
      adding = state.add();
    });
    expect(state.remainingSeconds).toBe(60);
    expect(api.openExternal).toHaveBeenCalledExactlyOnceWith('https://www.trae.cn/login');
    if (reason === 'leave') {
      act(() => renderer.update(<Harness edition="trae" />));
      await tick();
    }
    if (reason === 'timeout')
      await act(async () => {
        await vi.advanceTimersByTimeAsync(60_000);
      });
    if (reason === 'cancel') act(() => state.cancelLogin());
    await act(async () => {
      poll.resolve({ ...cn, id: 'late' });
      await adding;
    });
    expect(api.cancelTraeLogin).toHaveBeenCalledWith('login');
    expect(state.accounts.some((row) => row.id === 'late')).toBe(false);
    expect(state.busy).toBe(false);
    if (reason === 'timeout')
      expect(showError).toHaveBeenCalledExactlyOnceWith('accountError.expired');
    else expect(showError).not.toHaveBeenCalled();
    expect(api.refreshTraeAccountQuota).not.toHaveBeenCalled();
  }
);

it('cancels a login whose start arrives after leaving without opening the browser', async () => {
  await mount();
  const login = deferred<api.TraeLogin>();
  vi.mocked(api.startTraeLogin).mockReturnValueOnce(login.promise);
  let adding!: Promise<void>;
  act(() => {
    adding = state.add();
  });
  act(() => renderer.update(<Harness edition="trae" />));
  await act(async () => {
    login.resolve({
      loginId: 'late-start',
      verificationUri: 'https://www.trae.cn/login',
      expiresAt: Date.now() / 1000 + 60,
    });
    await adding;
  });
  expect(api.cancelTraeLogin).toHaveBeenCalledWith('late-start');
  expect(api.openExternal).not.toHaveBeenCalled();
});

it('saves a successful login without refreshing usage or claiming rewards', async () => {
  await mount();
  const added = { ...cn, id: 'new', active: false };
  vi.mocked(api.startTraeLogin).mockResolvedValue({
    loginId: 'new-login',
    verificationUri: 'https://www.trae.cn/login',
    expiresAt: Date.now() / 1000 + 60,
  });
  vi.mocked(api.pollTraeLogin).mockResolvedValueOnce(added);
  vi.mocked(api.listTraeAccounts).mockResolvedValueOnce([cn, added]);
  await act(async () => {
    await state.add();
  });
  expect(state.accounts).toEqual([cn, added]);
  expect(state.selectedId).toBe('new');
  expect(api.refreshTraeAccountQuota).not.toHaveBeenCalled();
  expect(api.claimTraeDailyCredits).not.toHaveBeenCalled();
});

it('refreshes only explicitly and preserves selection while pending and after an error', async () => {
  await mount();
  const request = deferred<api.TraeAccount>();
  vi.mocked(api.refreshTraeAccountQuota).mockReturnValueOnce(request.promise);
  let refreshing!: Promise<unknown>;
  act(() => {
    refreshing = state.refresh(cn);
  });
  expect(state.refreshing.has(cn.id)).toBe(true);
  await act(async () => {
    await state.refresh(cn);
  });
  expect(api.refreshTraeAccountQuota).toHaveBeenCalledExactlyOnceWith('traecn', cn.id);
  await act(async () => {
    request.resolve({ ...cn, remaining: 0 });
    await refreshing;
  });
  expect(state.accounts[0].remaining).toBe(0);
  expect(state.selectedId).toBe(cn.id);
  vi.mocked(api.refreshTraeAccountQuota).mockRejectedValueOnce(new Error('accountError.network'));
  await act(async () => {
    await state.refresh(state.accounts[0]);
  });
  expect(showError).toHaveBeenCalledExactlyOnceWith('accountError.network');
  expect(state.accounts[0].remaining).toBe(0);
});

it('prevents repeated TRAE claims for the same row and checks the next row independently', async () => {
  const second = { ...cn, id: 'second', active: false };
  vi.mocked(api.listTraeAccounts).mockResolvedValueOnce([cn, second]);
  await mount();
  const request = deferred<api.TraeAccount>();
  vi.mocked(api.claimTraeDailyCredits).mockReturnValueOnce(request.promise);
  let claiming!: Promise<unknown>;
  act(() => {
    claiming = state.claimDaily(cn);
  });
  await act(async () => {
    await state.claimDaily(cn);
  });
  expect(api.claimTraeDailyCredits).toHaveBeenCalledExactlyOnceWith('traecn', cn.id);
  await act(async () => {
    request.resolve({
      ...cn,
      dailyClaimedAt: Date.now() / 1000,
      deviceClaimedAt: Date.now() / 1000,
      remaining: 120,
    });
    expect(await claiming).toMatchObject({ remaining: 120, deviceClaimedAt: Date.now() / 1000 });
  });
  await act(async () => {
    await state.claimDaily(state.accounts[0]);
  });
  expect(api.claimTraeDailyCredits).toHaveBeenCalledTimes(1);
  vi.mocked(api.claimTraeDailyCredits).mockResolvedValueOnce({
    ...second,
    deviceClaimedAt: Date.now() / 1000,
  });
  await act(async () => {
    await state.claimDaily(second);
  });
  expect(api.claimTraeDailyCredits).toHaveBeenNthCalledWith(2, 'traecn', second.id);
  expect(state.accounts[1].dailyClaimedAt).toBeUndefined();
  await act(async () => {
    await state.claimDaily(second);
  });
  expect(api.claimTraeDailyCredits).toHaveBeenCalledTimes(2);
  expect(state.accounts[0].remaining).toBe(120);
  expect(state.selectedId).toBe(cn.id);
});

it('allows a fresh claim after leaving and returning while an older claim response is pending', async () => {
  await mount();
  const old = deferred<api.TraeAccount>();
  const current = deferred<api.TraeAccount>();
  vi.mocked(api.claimTraeDailyCredits)
    .mockReturnValueOnce(old.promise)
    .mockReturnValueOnce(current.promise);
  let oldClaim!: Promise<unknown>;
  act(() => {
    oldClaim = state.claimDaily(cn);
  });
  act(() => renderer.update(<Harness enabled={false} />));
  await tick();
  act(() => renderer.update(<Harness />));
  await tick();
  expect(state.refreshing.size).toBe(0);
  let newClaim!: Promise<unknown>;
  act(() => {
    newClaim = state.claimDaily(cn);
  });
  expect(api.claimTraeDailyCredits).toHaveBeenCalledTimes(2);
  await act(async () => {
    old.resolve({ ...cn, remaining: 999, deviceClaimedAt: Date.now() / 1000 });
    expect(await oldClaim).toBeUndefined();
  });
  expect(state.accounts[0].remaining).toBe(20);
  expect(state.refreshing.has(cn.id)).toBe(true);
  await act(async () => {
    current.resolve({
      ...cn,
      remaining: 120,
      dailyClaimedAt: Date.now() / 1000,
      deviceClaimedAt: Date.now() / 1000,
    });
    expect(await newClaim).toMatchObject({ remaining: 120, deviceClaimedAt: Date.now() / 1000 });
  });
  expect(state.accounts[0].remaining).toBe(120);
  expect(state.refreshing.size).toBe(0);
  expect(showError).not.toHaveBeenCalled();
});

it.each(['global', 'wrong-region', 'claimed', 'device', 'disabled'] as const)(
  'does not claim for %s',
  async (reason) => {
    const account = {
      ...cn,
      ...(reason === 'claimed' ? { dailyClaimedAt: Date.now() / 1000 } : {}),
      ...(reason === 'device' ? { deviceClaimedAt: Date.now() / 1000 } : {}),
      ...(reason === 'disabled' ? { rewardEnabled: false } : {}),
    };
    vi.mocked(api.listTraeAccounts).mockResolvedValueOnce([reason === 'global' ? global : account]);
    await mount(reason === 'global' ? 'trae' : 'traecn');
    await act(async () => {
      await state.claimDaily(reason === 'global' || reason === 'wrong-region' ? global : account);
    });
    expect(api.claimTraeDailyCredits).not.toHaveBeenCalled();
  }
);

it('preserves an explicit claim failure and prevents a late refresh from resurrecting a deleted row', async () => {
  await mount();
  vi.mocked(api.claimTraeDailyCredits).mockRejectedValueOnce(new Error('accountError.network'));
  await act(async () => {
    expect(await state.claimDaily(cn)).toBeUndefined();
  });
  expect(showError).toHaveBeenCalledExactlyOnceWith('accountError.network');
  const request = deferred<api.TraeAccount>();
  vi.mocked(api.refreshTraeAccountQuota).mockReturnValueOnce(request.promise);
  let refreshing!: Promise<unknown>;
  act(() => {
    refreshing = state.refresh(cn);
  });
  await act(async () => {
    await state.remove(cn);
    request.resolve({ ...cn, remaining: 100 });
    expect(await refreshing).toBeUndefined();
  });
  expect(api.deleteTraeAccount).toHaveBeenCalledExactlyOnceWith('traecn', cn.id);
  expect(state.accounts).toEqual([]);
});
