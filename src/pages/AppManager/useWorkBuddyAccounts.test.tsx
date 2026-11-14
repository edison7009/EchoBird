import { useLayoutEffect } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import * as api from '../../api/tauri';
import { useWorkBuddyAccounts } from './useWorkBuddyAccounts';
import { WorkBuddyAccountSection } from './WorkBuddyAccountSection';
import { AppManagerContext, type AppManagerContextType } from './context';

vi.mock('../../api/tauri', () => ({
  listWorkBuddyAccounts: vi.fn(),
  refreshWorkBuddyAccountQuota: vi.fn(),
  claimWorkBuddyDailyCredits: vi.fn(),
  startWorkBuddyLogin: vi.fn(),
  pollWorkBuddyLogin: vi.fn(),
  cancelWorkBuddyLogin: vi.fn().mockResolvedValue(undefined),
  openExternal: vi.fn(),
  deleteWorkBuddyAccount: vi.fn().mockResolvedValue(undefined),
}));
vi.mock('../../hooks/useI18n', () => {
  const t = (key: string) => key;
  return { useI18n: () => ({ t }) };
});
vi.mock('../../components/ConfirmDialog', () => ({
  useConfirm: () => async () => true,
}));

const clearModel = vi.fn();
const showError = vi.fn();
const cn: api.WorkBuddyAccount = {
  id: 'cn',
  name: 'Domestic',
  edition: 'workbuddy',
  plan: 'Free',
  remaining: 10,
  total: 100,
  expiresAt: null,
  active: true,
};
const ai: api.WorkBuddyAccount = { ...cn, id: 'ai', name: 'International', edition: 'workbuddyai' };
let state: ReturnType<typeof useWorkBuddyAccounts>;
let renderer: ReactTestRenderer;
function Harness({
  edition,
  hasModel = false,
}: {
  edition: api.WorkBuddyEdition | null;
  hasModel?: boolean;
}) {
  const result = useWorkBuddyAccounts(edition, hasModel, clearModel, showError);
  useLayoutEffect(() => {
    state = result;
  });
  return null;
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
async function mount(edition: api.WorkBuddyEdition = 'workbuddy') {
  act(() => {
    renderer = create(<Harness edition={edition} />);
  });
  await act(async () => {
    await vi.runOnlyPendingTimersAsync();
  });
}
beforeEach(() => {
  vi.clearAllMocks();
  vi.useFakeTimers();
  vi.mocked(api.listWorkBuddyAccounts)
    .mockReset()
    .mockImplementation(async (edition) => (edition === 'workbuddy' ? [cn] : [ai]));
});
afterEach(() => {
  act(() => {
    renderer?.unmount();
  });
  vi.useRealTimers();
});

describe('WorkBuddy account interactions', () => {
  it('loads locally and selects accounts without refreshing quota, with exclusive model selection', async () => {
    await mount();
    expect(state.selectedId).toBe(cn.id);
    act(() => {
      state.select(cn.id);
    });
    expect(clearModel).toHaveBeenCalledWith('workbuddy');
    act(() => {
      renderer.update(<Harness edition="workbuddy" hasModel />);
    });
    expect(state.selectedId).toBeNull();
    expect(api.refreshWorkBuddyAccountQuota).not.toHaveBeenCalled();
    expect(api.claimWorkBuddyDailyCredits).not.toHaveBeenCalled();
    expect(api.listWorkBuddyAccounts).toHaveBeenCalledTimes(1);
  });

  it('keeps each edition cached and restores its selection without waiting for reload', async () => {
    await mount();
    act(() => {
      renderer.update(<Harness edition="workbuddyai" />);
    });
    await act(async () => {
      await vi.runOnlyPendingTimersAsync();
    });
    expect(state.accounts).toEqual([ai]);
    expect(state.selectedId).toBe(ai.id);
    act(() => {
      renderer.update(<Harness edition="workbuddy" />);
    });
    expect(state.accounts).toEqual([cn]);
    expect(state.selectedId).toBe(cn.id);
    expect(api.refreshWorkBuddyAccountQuota).not.toHaveBeenCalled();
    expect(api.claimWorkBuddyDailyCredits).not.toHaveBeenCalled();
  });

  it('ignores a late list response after changing editions', async () => {
    const list = deferred<api.WorkBuddyAccount[]>();
    vi.mocked(api.listWorkBuddyAccounts).mockImplementationOnce(() => list.promise);
    await mount();
    act(() => {
      renderer.update(<Harness edition="workbuddyai" />);
    });
    await act(async () => {
      await vi.runOnlyPendingTimersAsync();
    });
    await act(async () => {
      list.resolve([cn]);
    });
    expect(state.accounts).toEqual([ai]);
    expect(state.selectedId).toBe(ai.id);
  });

  it('cancels a login that starts after the user leaves its edition', async () => {
    await mount();
    const login = deferred<api.WorkBuddyLogin>();
    vi.mocked(api.startWorkBuddyLogin).mockReturnValueOnce(login.promise);
    let adding!: Promise<void>;
    act(() => {
      adding = state.add();
    });
    expect(state.remainingSeconds).toBe(60);
    act(() => {
      renderer.update(<Harness edition="workbuddyai" />);
    });
    await act(async () => {
      login.resolve({
        loginId: 'pending',
        verificationUri: 'https://www.codebuddy.cn/login',
        expiresAt: Date.now() / 1000 + 60,
      });
      await adding;
      await vi.runOnlyPendingTimersAsync();
    });
    expect(api.cancelWorkBuddyLogin).toHaveBeenCalledWith('pending');
    expect(api.openExternal).not.toHaveBeenCalled();
    expect(state.busy).toBe(false);
  });

  it('refreshes manually and does not resurrect an account deleted while refresh was pending', async () => {
    await mount();
    const quota = deferred<api.WorkBuddyAccount>();
    vi.mocked(api.refreshWorkBuddyAccountQuota).mockReturnValueOnce(quota.promise);
    let refreshing!: Promise<unknown>;
    act(() => {
      refreshing = state.refresh(cn);
    });
    expect(state.refreshing.has(cn.id)).toBe(true);
    vi.mocked(api.listWorkBuddyAccounts).mockResolvedValueOnce([]);
    await act(async () => {
      await state.remove(cn);
    });
    await act(async () => {
      quota.resolve({ ...cn, remaining: 20 });
      await refreshing;
    });
    expect(api.refreshWorkBuddyAccountQuota).toHaveBeenCalledTimes(1);
    expect(state.accounts).toEqual([]);
    expect(state.selectedId).toBeNull();
    expect(state.refreshing.size).toBe(0);
  });

  it('does not intercept keyboard events from nested action buttons', () => {
    const select = vi.fn();
    const claimDaily = vi.fn();
    const context = {
      selectedTool: 'workbuddy',
      workBuddyAccounts: {
        accounts: [cn],
        selectedId: null,
        refreshing: new Set(),
        authorizationFailedIds: new Set(),
        select,
        claimDaily,
      },
    } as unknown as AppManagerContextType;
    act(() => {
      renderer = create(
        <AppManagerContext.Provider value={context}>
          <WorkBuddyAccountSection />
        </AppManagerContext.Provider>
      );
    });
    const row = renderer.root.findByProps({ role: 'radio' });
    const currentTarget = {};
    const preventDefault = vi.fn();
    row.props.onKeyDown({ key: 'Enter', target: {}, currentTarget, preventDefault });
    expect(select).not.toHaveBeenCalled();
    expect(preventDefault).not.toHaveBeenCalled();
    const claim = renderer.root.findByProps({ 'aria-label': 'agent.claimDailyCredits Domestic' });
    const stopPropagation = vi.fn();
    claim.props.onClick({ stopPropagation });
    expect(stopPropagation).toHaveBeenCalledOnce();
    expect(claimDaily).toHaveBeenCalledWith(cn);
    expect(select).not.toHaveBeenCalled();
    for (const key of ['Enter', ' ']) {
      row.props.onKeyDown({ key, target: claim, currentTarget, preventDefault });
    }
    expect(select).not.toHaveBeenCalled();
    row.props.onKeyDown({ key: 'Enter', target: currentTarget, currentTarget, preventDefault });
    expect(select).toHaveBeenCalledWith(cn.id);
  });

  it('claims explicitly, shares the refresh lock, and updates balances without selecting', async () => {
    await mount();
    const result = deferred<api.WorkBuddyAccount>();
    vi.mocked(api.claimWorkBuddyDailyCredits).mockReturnValueOnce(result.promise);
    let claiming!: Promise<unknown>;
    act(() => {
      claiming = state.claimDaily(cn);
    });
    expect(state.refreshing.has(cn.id)).toBe(true);
    await act(async () => {
      await state.claimDaily(cn);
      await state.refresh(cn);
    });
    expect(api.claimWorkBuddyDailyCredits).toHaveBeenCalledExactlyOnceWith('workbuddy', cn.id);
    expect(api.refreshWorkBuddyAccountQuota).not.toHaveBeenCalled();
    await act(async () => {
      result.resolve({
        ...cn,
        remaining: 30,
        rewardRemaining: 20,
        addonRemaining: 5,
        dailyClaimedAt: Date.now() / 1000,
      });
      await claiming;
    });
    expect(state.accounts[0].remaining).toBe(30);
    expect(state.accounts[0].rewardRemaining).toBe(20);
    expect(state.accounts[0].addonRemaining).toBe(5);
    expect(state.refreshing.size).toBe(0);
    expect(clearModel).not.toHaveBeenCalled();
    expect(showError).not.toHaveBeenCalled();
  });

  it('does not claim during a pending refresh or for an international/disabled tool', async () => {
    await mount();
    const result = deferred<api.WorkBuddyAccount>();
    vi.mocked(api.refreshWorkBuddyAccountQuota).mockReturnValueOnce(result.promise);
    let pending!: Promise<unknown>;
    act(() => {
      pending = state.refresh(cn);
    });
    await act(async () => {
      await state.claimDaily(cn);
    });
    await act(async () => {
      result.resolve(cn);
      await pending;
    });
    act(() => {
      renderer.update(<Harness edition="workbuddyai" />);
    });
    await act(async () => {
      await vi.runOnlyPendingTimersAsync();
      await state.claimDaily(ai);
    });
    act(() => {
      renderer.update(<Harness edition={null} />);
    });
    await act(async () => {
      await state.claimDaily(cn);
    });
    expect(api.claimWorkBuddyDailyCredits).not.toHaveBeenCalled();
  });

  it('preserves cached balances and reports an explicit claim failure', async () => {
    await mount();
    vi.mocked(api.claimWorkBuddyDailyCredits).mockRejectedValueOnce('accountError.claim');
    await act(async () => {
      await state.claimDaily(cn);
    });
    expect(state.accounts).toEqual([cn]);
    expect(state.refreshing.size).toBe(0);
    expect(showError).toHaveBeenCalledWith('accountError.claim');
  });

  it.each([false, true])(
    'ignores late claim success/failure after leaving (failure=%s)',
    async (fail) => {
      await mount();
      const result = deferred<api.WorkBuddyAccount>();
      vi.mocked(api.claimWorkBuddyDailyCredits).mockImplementationOnce(async () => {
        const account = await result.promise;
        if (fail) throw new Error('accountError.claim');
        return account;
      });
      let claiming!: Promise<unknown>;
      act(() => {
        claiming = state.claimDaily(cn);
      });
      act(() => {
        renderer.update(<Harness edition="workbuddyai" />);
      });
      await act(async () => {
        await vi.runOnlyPendingTimersAsync();
        result.resolve({ ...cn, remaining: 50 });
        await claiming;
      });
      expect(state.accounts).toEqual([ai]);
      expect(showError).not.toHaveBeenCalled();
    }
  );

  it('does not resurrect a deleted account after a pending claim', async () => {
    await mount();
    const result = deferred<api.WorkBuddyAccount>();
    vi.mocked(api.claimWorkBuddyDailyCredits).mockReturnValueOnce(result.promise);
    let claiming!: Promise<unknown>;
    act(() => {
      claiming = state.claimDaily(cn);
    });
    await act(async () => {
      await state.remove(cn);
    });
    await act(async () => {
      result.resolve({ ...cn, remaining: 40 });
      await claiming;
    });
    expect(state.accounts).toEqual([]);
    expect(state.selectedId).toBeNull();
  });

  it('disables claim during a request and resets the claimed icon on the next China day', async () => {
    vi.setSystemTime(new Date('2026-10-02T15:59:30Z'));
    const context = {
      selectedTool: 'workbuddy',
      workBuddyAccounts: {
        accounts: [{ ...cn, dailyClaimedAt: Date.now() / 1000 }],
        refreshing: new Set([cn.id]),
        authorizationFailedIds: new Set(),
      },
    } as unknown as AppManagerContextType;
    act(() => {
      renderer = create(
        <AppManagerContext.Provider value={context}>
          <WorkBuddyAccountSection />
        </AppManagerContext.Provider>
      );
    });
    expect(
      renderer.root.findByProps({ 'aria-label': 'agent.dailyCreditsClaimed Domestic' }).props
        .disabled
    ).toBe(true);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    expect(
      renderer.root.findByProps({ 'aria-label': 'agent.claimDailyCredits Domestic' }).props.disabled
    ).toBe(true);
    context.workBuddyAccounts.refreshing.clear();
    act(() => {
      renderer.update(
        <AppManagerContext.Provider value={{ ...context }}>
          <WorkBuddyAccountSection />
        </AppManagerContext.Provider>
      );
    });
    expect(
      renderer.root.findByProps({ 'aria-label': 'agent.claimDailyCredits Domestic' }).props.disabled
    ).toBe(false);
  });
});
