import { useLayoutEffect } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useManagedAccounts, type AccountClient, type ManagedLogin } from './useManagedAccounts';
import { useNavigationStore } from '../../stores/navigationStore';
import { useRewardClaimStore } from '../../stores/rewardClaimStore';

vi.mock('../../hooks/useI18n', () => {
  const t = (key: string) => key;
  return { useI18n: () => ({ t }) };
});
vi.mock('../../components/ConfirmDialog', () => ({ useConfirm: () => async () => true }));

type Account = { id: string; active?: boolean; quota?: number };
const account: Account = { id: 'saved', active: true };
const clearModel = vi.fn();
const showError = vi.fn();
let client: AccountClient<Account, ManagedLogin>;
let state: ReturnType<typeof useManagedAccounts<Account, ManagedLogin>>;
let renderer: ReactTestRenderer;

function Harness({ enabled = true, navigationKey = 'tool' }) {
  const result = useManagedAccounts(
    'store',
    enabled,
    false,
    clearModel,
    showError,
    client,
    navigationKey
  );
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

async function mount() {
  act(() => {
    renderer = create(<Harness />);
  });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(0);
  });
}

beforeEach(() => {
  useNavigationStore.getState().setActivePage('apps');
  vi.useFakeTimers();
  vi.clearAllMocks();
  client = {
    list: vi.fn().mockResolvedValue([account]),
    start: vi
      .fn()
      .mockImplementation(async () => ({ loginId: 'login', expiresAt: Date.now() / 1000 + 60 })),
    cancel: vi.fn().mockResolvedValue(undefined),
    poll: vi.fn().mockResolvedValue(null),
    open: vi.fn().mockResolvedValue(undefined),
    remove: vi.fn().mockResolvedValue(undefined),
    refresh: vi.fn().mockResolvedValue(account),
    label: (value) => value.id,
  };
});
afterEach(() => {
  act(() => {
    renderer?.unmount();
  });
  vi.useRealTimers();
});

describe('shared account request boundaries', () => {
  it('updates a visible saved account after background claiming with only a local list read', async () => {
    await mount();
    const selected = state.selectedId;
    vi.mocked(client.list).mockResolvedValue([{ ...account, quota: 123 }]);
    await act(async () => useRewardClaimStore.getState().updated('another-provider'));
    expect(client.list).toHaveBeenCalledTimes(1);
    await act(async () => useRewardClaimStore.getState().updated('store'));
    expect(state.accounts).toEqual([{ ...account, quota: 123 }]);
    expect(state.selectedId).toBe(selected);
    expect(client.list).toHaveBeenCalledTimes(2);
    expect(client.refresh).not.toHaveBeenCalled();
    expect(client.start).not.toHaveBeenCalled();
    expect(showError).not.toHaveBeenCalled();
    await act(async () => renderer.update(<Harness enabled={false} />));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
      useRewardClaimStore.getState().updated('store');
    });
    expect(client.list).toHaveBeenCalledTimes(2);
  });

  it('honors a longer browser deadline while keeping the backend expiry authoritative', async () => {
    client.loginTimeoutSeconds = 600;
    const poll = deferred<Account | null>();
    vi.mocked(client.start).mockResolvedValue({
      loginId: 'browser',
      expiresAt: Date.now() / 1000 + 600,
    });
    vi.mocked(client.poll!).mockReturnValue(poll.promise);
    await mount();
    let adding!: Promise<void>;
    await act(async () => {
      adding = state.add();
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    expect(state.busy).toBe(true);
    expect(state.remainingSeconds).toBe(540);
    expect(client.cancel).not.toHaveBeenCalled();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(540_000);
    });
    expect(state.busy).toBe(false);
    expect(client.cancel).toHaveBeenCalledWith('browser');
    expect(showError).toHaveBeenCalledWith('accountError.expired');
    await act(async () => {
      poll.resolve({ id: 'late-browser-account' });
      await adding;
    });
    expect(state.accounts).toEqual([account]);
  });

  it('uses the shared unavailable error for reward failures and treats already claimed results as success', async () => {
    await mount();
    const claim = vi.fn().mockRejectedValueOnce('accountError.unavailable');
    await act(async () => {
      await state.refresh(account, claim);
    });
    expect(showError).toHaveBeenCalledExactlyOnceWith('accountError.unavailable');
    expect(state.accounts).toEqual([account]);
    expect(state.selectedId).toBe(account.id);
    expect(state.authorizationFailedIds.size).toBe(0);
    expect(state.refreshing.size).toBe(0);

    showError.mockClear();
    claim.mockResolvedValueOnce({ ...account, quota: 42 });
    await act(async () => {
      await state.refresh(account, claim);
    });
    expect(showError).not.toHaveBeenCalled();
    expect(state.accounts).toEqual([{ ...account, quota: 42 }]);
    expect(state.selectedId).toBe(account.id);
    expect(client.refresh).not.toHaveBeenCalled();
    expect(client.start).not.toHaveBeenCalled();
  });

  it('marks each navigation pending before its deferred list starts and retains rows after failure', async () => {
    act(() => {
      renderer = create(<Harness />);
    });
    expect(state.loading).toBe(true);
    expect(state.loaded).toBe(false);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(state.loading).toBe(false);
    expect(state.loaded).toBe(true);
    expect(state.readFailed).toBe(false);

    act(() => useNavigationStore.getState().setActivePage('accounts'));
    expect(state.loading).toBe(true);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(state.loading).toBe(false);
    act(() => useNavigationStore.getState().setActivePage('models'));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    vi.mocked(client.list).mockRejectedValueOnce(new Error('unreadable local accounts'));
    act(() => useNavigationStore.getState().setActivePage('accounts'));
    expect(state.loading).toBe(true);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(state.loading).toBe(false);
    expect(state.loaded).toBe(true);
    expect(state.readFailed).toBe(true);
    expect(state.accounts).toEqual([account]);
    expect(client.refresh).not.toHaveBeenCalled();
    expect(client.start).not.toHaveBeenCalled();
    expect(clearModel).not.toHaveBeenCalled();
    expect(showError).not.toHaveBeenCalled();
  });

  it('distinguishes a failed first read from a successful empty list', async () => {
    vi.mocked(client.list).mockRejectedValueOnce(new Error('unreadable local accounts'));
    await mount();
    expect(state.loading).toBe(false);
    expect(state.loaded).toBe(false);
    expect(state.readFailed).toBe(true);
    vi.mocked(client.list).mockResolvedValueOnce([]);
    await act(async () => {
      await state.reload();
    });
    expect(state.loaded).toBe(true);
    expect(state.readFailed).toBe(false);
    expect(state.accounts).toEqual([]);
  });

  it('does not mark a newly added account as a complete list after failed local reads', async () => {
    vi.mocked(client.list).mockRejectedValue(new Error('unreadable local accounts'));
    const added = { id: 'new-account' };
    vi.mocked(client.poll!).mockResolvedValue(added);
    await mount();
    await act(async () => {
      await state.add();
    });
    expect(state.accounts).toEqual([added]);
    expect(state.loaded).toBe(false);
    expect(state.readFailed).toBe(true);
    expect(state.mutationRevision).toBe(1);
    await act(async () => {
      await state.remove(added);
    });
    expect(state.accounts).toEqual([]);
    expect(state.loaded).toBe(false);
    expect(state.mutationRevision).toBe(2);
    vi.mocked(client.list).mockResolvedValueOnce([]);
    await act(async () => {
      await state.reload();
    });
    expect(state.loaded).toBe(true);
  });

  it('cancels immediately, preserves saved accounts and isolates a restarted login from old results and deadlines', async () => {
    const first = deferred<Account | null>();
    const second = deferred<Account | null>();
    vi.mocked(client.start)
      .mockResolvedValueOnce({ loginId: 'first', expiresAt: Date.now() / 1000 + 60 })
      .mockImplementationOnce(async () => ({
        loginId: 'second',
        expiresAt: Date.now() / 1000 + 60,
      }));
    vi.mocked(client.poll!).mockImplementation((id) =>
      id === 'first' ? first.promise : second.promise
    );
    await mount();
    let firstTask!: Promise<void>;
    await act(async () => {
      firstTask = state.add();
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(10_000);
      state.cancelLogin();
    });
    expect(client.cancel).toHaveBeenCalledExactlyOnceWith('first');
    expect(state.busy).toBe(false);
    expect(state.remainingSeconds).toBe(0);
    expect(state.accounts).toEqual([account]);
    expect(state.selectedId).toBe(account.id);
    expect(clearModel).not.toHaveBeenCalled();
    let secondTask!: Promise<void>;
    await act(async () => {
      secondTask = state.add();
      first.resolve({ id: 'cancelled-account' });
      await firstTask;
    });
    expect(state.busy).toBe(true);
    expect(state.remainingSeconds).toBe(60);
    expect(state.accounts).toEqual([account]);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(55_000);
    });
    expect(state.busy).toBe(true);
    expect(state.remainingSeconds).toBe(5);
    expect(showError).not.toHaveBeenCalled();
    const added = { id: 'new-account' };
    vi.mocked(client.list).mockResolvedValue([account, added]);
    await act(async () => {
      second.resolve(added);
      await secondTask;
    });
    expect(state.busy).toBe(false);
    expect(state.accounts).toEqual([account, added]);
    expect(state.selectedId).toBe(added.id);
    expect(client.start).toHaveBeenCalledTimes(2);
    expect(client.refresh).not.toHaveBeenCalled();
    expect(client.remove).not.toHaveBeenCalled();
  });

  it('cancels a late login initialization without opening its browser or resetting the newer attempt', async () => {
    const initialization = deferred<ManagedLogin>();
    const poll = deferred<Account | null>();
    vi.mocked(client.start)
      .mockReturnValueOnce(initialization.promise)
      .mockResolvedValueOnce({ loginId: 'new', expiresAt: Date.now() / 1000 + 60 });
    vi.mocked(client.poll!).mockReturnValue(poll.promise);
    await mount();
    let oldTask!: Promise<void>;
    act(() => {
      oldTask = state.add();
      state.cancelLogin();
    });
    expect(state.busy).toBe(false);
    let newTask!: Promise<void>;
    await act(async () => {
      newTask = state.add();
      initialization.resolve({ loginId: 'old', expiresAt: Date.now() / 1000 + 60 });
      await oldTask;
    });
    expect(client.cancel).toHaveBeenCalledExactlyOnceWith('old');
    expect(client.open).toHaveBeenCalledTimes(1);
    expect(client.poll).toHaveBeenCalledExactlyOnceWith('new', expect.any(Function));
    expect(state.busy).toBe(true);
    expect(state.login?.loginId).toBe('new');
    expect(state.remainingSeconds).toBe(60);
    await act(async () => {
      state.cancelLogin();
      poll.resolve({ id: 'also-cancelled' });
      await newTask;
    });
    expect(state.accounts).toEqual([account]);
    expect(client.list).toHaveBeenCalledTimes(1);
    expect(showError).not.toHaveBeenCalled();
    expect(clearModel).not.toHaveBeenCalled();
  });

  it('collects raw refresh errors per request without suppressing other individual errors', async () => {
    const rows = [account, { id: 'other', quota: 80 }];
    vi.mocked(client.list).mockResolvedValue(rows);
    const collect = vi.fn();
    const authError = new Error('accountError.network|HTTP 401 Unauthorized');
    vi.mocked(client.refresh).mockRejectedValueOnce(authError);
    await mount();
    await act(async () => {
      await state.refresh(account, undefined, collect);
    });
    expect(collect).toHaveBeenCalledExactlyOnceWith(authError);
    expect(showError).not.toHaveBeenCalled();
    expect([...state.authorizationFailedIds]).toEqual(['saved']);
    expect(state.accounts).toEqual(rows);
    vi.mocked(client.refresh).mockRejectedValueOnce('accountError.network');
    await act(async () => {
      await state.refresh(rows[1]);
    });
    expect(showError).toHaveBeenCalledExactlyOnceWith('accountError.network');
    expect(collect).toHaveBeenCalledTimes(1);
    expect(state.authorizationFailedIds.has('other')).toBe(false);
    expect(state.accounts).toEqual(rows);
    await act(async () => {
      await state.refresh(account, undefined, collect);
    });
    expect(state.authorizationFailedIds.size).toBe(0);
    expect(collect).toHaveBeenCalledTimes(1);
    expect(client.start).not.toHaveBeenCalled();
    expect(client.remove).not.toHaveBeenCalled();
  });

  it('retains per-account authorization failures across passive navigation and clears on a successful retry', async () => {
    const rows = [account, { id: 'other', quota: 80 }];
    vi.mocked(client.list).mockResolvedValue(rows);
    vi.mocked(client.refresh).mockRejectedValueOnce('accountError.loginRequired');
    await mount();
    await act(async () => {
      await state.refresh(account);
    });
    expect([...state.authorizationFailedIds]).toEqual(['saved']);
    expect(state.accounts).toEqual(rows);
    expect(state.selectedId).toBe('saved');
    act(() => useNavigationStore.getState().setActivePage('accounts'));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect([...state.authorizationFailedIds]).toEqual(['saved']);
    expect(client.refresh).toHaveBeenCalledTimes(1);
    expect(client.start).not.toHaveBeenCalled();
    expect(client.remove).not.toHaveBeenCalled();
    expect(clearModel).not.toHaveBeenCalled();
    expect(showError).toHaveBeenCalledTimes(1);
    vi.mocked(client.refresh).mockRejectedValueOnce('accountError.network');
    await act(async () => {
      await state.refresh(account);
    });
    expect([...state.authorizationFailedIds]).toEqual(['saved']);
    const retry = deferred<Account>();
    vi.mocked(client.refresh).mockReturnValueOnce(retry.promise);
    let task!: Promise<unknown>;
    act(() => {
      task = state.refresh(account);
    });
    expect(state.authorizationFailedIds.has('saved')).toBe(true);
    expect(state.refreshing.has('saved')).toBe(true);
    await act(async () => {
      retry.resolve({ ...account, quota: 42 });
      await task;
    });
    expect(state.authorizationFailedIds.size).toBe(0);
    expect(state.accounts).toEqual([{ ...account, quota: 42 }, rows[1]]);
    expect(state.refreshing.size).toBe(0);
  });

  it.each([
    ['login', true],
    ['login', false],
    ['remove', true],
    ['remove', false],
  ] as const)(
    'clears failed authorization after %s and ignores an older quota result (failure=%s)',
    async (action, failure) => {
      vi.mocked(client.refresh).mockRejectedValueOnce('accountError.loginRequired');
      await mount();
      await act(async () => {
        await state.refresh(account);
      });
      const late = deferred<void>();
      vi.mocked(client.refresh).mockImplementationOnce(async () => {
        await late.promise;
        if (failure) throw new Error('accountError.loginRequired');
        return { ...account, quota: 1 };
      });
      let task!: Promise<unknown>;
      act(() => {
        task = state.refresh(account);
      });
      if (action === 'login') {
        const updated = { ...account, quota: 80 };
        vi.mocked(client.poll!).mockResolvedValueOnce(updated);
        vi.mocked(client.list).mockResolvedValue([updated]);
        await act(async () => {
          await state.add();
        });
      } else {
        await act(async () => {
          await state.remove(account);
        });
      }
      expect(state.authorizationFailedIds.size).toBe(0);
      expect(state.refreshing.size).toBe(0);
      await act(async () => {
        late.resolve();
        expect(await task).toBeUndefined();
      });
      expect(state.authorizationFailedIds.size).toBe(0);
      expect(showError).toHaveBeenCalledTimes(1);
      expect(state.accounts).toEqual(action === 'login' ? [{ ...account, quota: 80 }] : []);
    }
  );

  it('ignores an authorization failure returned after leaving and re-entering the page', async () => {
    await mount();
    const late = deferred<void>();
    vi.mocked(client.refresh).mockImplementationOnce(async () => {
      await late.promise;
      throw new Error('accountError.loginRequired');
    });
    let task!: Promise<unknown>;
    act(() => {
      task = state.refresh(account);
    });
    act(() => useNavigationStore.getState().setActivePage('models'));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    act(() => useNavigationStore.getState().setActivePage('apps'));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    await act(async () => {
      late.resolve();
      await task;
    });
    expect(state.authorizationFailedIds.size).toBe(0);
    expect(showError).not.toHaveBeenCalled();
    expect(state.accounts).toEqual([account]);
  });

  it.each(['refresh', 'remove'] as const)(
    'does not overwrite an explicit %s with an older account list',
    async (action) => {
      await mount();
      const stale = deferred<Account[]>();
      vi.mocked(client.list).mockReturnValueOnce(stale.promise);
      vi.mocked(client.refresh).mockResolvedValue({ ...account, quota: 42 });
      let reloading!: Promise<Account[] | undefined>;
      act(() => {
        reloading = state.reload();
      });
      await act(async () => {
        await state[action](account);
      });
      await act(async () => {
        stale.resolve([account]);
        await reloading;
      });
      expect(state.accounts).toEqual(action === 'remove' ? [] : [{ ...account, quota: 42 }]);
      expect(state.loading).toBe(false);
    }
  );

  it('keeps the latest list when concurrent reloads finish out of order', async () => {
    await mount();
    const stale = deferred<Account[]>();
    vi.mocked(client.list)
      .mockReturnValueOnce(stale.promise)
      .mockResolvedValueOnce([{ ...account, quota: 9 }]);
    let earlier!: Promise<Account[] | undefined>;
    act(() => {
      earlier = state.reload();
    });
    await act(async () => {
      await state.reload();
    });
    await act(async () => {
      stale.resolve([account]);
      await earlier;
    });
    expect(state.accounts).toEqual([{ ...account, quota: 9 }]);
  });

  it('finishes a successful browser login before a slow list reload can expire it', async () => {
    await mount();
    const list = deferred<Account[]>();
    vi.mocked(client.list).mockReturnValueOnce(list.promise);
    vi.mocked(client.poll!).mockResolvedValueOnce({ id: 'new-account' });
    let adding!: Promise<void>;
    await act(async () => {
      adding = state.add();
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    expect(state.busy).toBe(false);
    expect(state.accounts.map((a) => a.id)).toContain('new-account');
    expect(showError).not.toHaveBeenCalled();
    await act(async () => {
      await state.remove(account);
    });
    await act(async () => {
      list.resolve([account, { id: 'new-account' }]);
      await adding;
    });
    expect(state.accounts).toEqual([{ id: 'new-account' }]);
  });

  it('times out slow initialization and cancels its late result without touching a newer attempt', async () => {
    await mount();
    const lateStart = deferred<ManagedLogin>();
    const newPoll = deferred<Account | null>();
    vi.mocked(client.start).mockReturnValueOnce(lateStart.promise);
    vi.mocked(client.poll!).mockReturnValue(newPoll.promise);
    let oldOperation!: Promise<void>;
    let newOperation!: Promise<void>;
    act(() => {
      oldOperation = state.add();
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    expect(state.busy).toBe(false);
    expect(showError).toHaveBeenCalledWith('accountError.expired');
    await act(async () => {
      newOperation = state.add();
    });
    await act(async () => {
      lateStart.resolve({ loginId: 'late', expiresAt: Date.now() / 1000 + 60 });
      await oldOperation;
    });
    expect(client.cancel).toHaveBeenCalledWith('late');
    expect(state.busy).toBe(true);
    expect(state.login?.loginId).toBe('login');
    expect(client.open).toHaveBeenCalledTimes(1);
    await act(async () => {
      newPoll.resolve(account);
      await newOperation;
    });
    expect(state.busy).toBe(false);
  });

  it('does not discard an independent quota refresh when login expires', async () => {
    await mount();
    const quota = deferred<Account>();
    const poll = deferred<Account | null>();
    vi.mocked(client.refresh).mockReturnValue(quota.promise);
    vi.mocked(client.poll!).mockReturnValue(poll.promise);
    let refreshing!: Promise<unknown>;
    let adding!: Promise<void>;
    await act(async () => {
      refreshing = state.refresh(account);
      adding = state.add();
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    await act(async () => {
      quota.resolve({ ...account, quota: 12 });
      poll.resolve({ id: 'late-account' });
      await Promise.all([refreshing, adding]);
    });
    expect(state.accounts).toEqual([{ ...account, quota: 12 }]);
    expect(state.refreshing.size).toBe(0);
    expect(clearModel).not.toHaveBeenCalled();
  });

  it('closes a successful manual login even if reloading saved accounts fails', async () => {
    client.complete = vi.fn().mockResolvedValue({ id: 'new-account' });
    await mount();
    await act(async () => {
      await state.add();
    });
    vi.mocked(client.list).mockRejectedValue(new Error('accountError.read'));
    await act(async () => {
      await state.completeLogin(' code ');
    });
    expect(client.complete).toHaveBeenCalledWith('login', 'code');
    expect(state.login).toBeNull();
    expect(state.busy).toBe(false);
    expect(state.loginError).toBeNull();
    expect(state.accounts.map((a) => a.id)).toContain('new-account');
    expect(state.selectedId).toBe('new-account');
    expect(showError).toHaveBeenCalledWith('accountError.read');
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    expect(showError).toHaveBeenCalledTimes(1);
  });

  it('keeps manual authorization errors retryable and ignores completion after leaving', async () => {
    client.complete = vi.fn().mockRejectedValueOnce(new Error('accountError.auth'));
    await mount();
    await act(async () => {
      await state.add();
      await state.completeLogin('bad');
    });
    expect(state.loginError).toBe('accountError.auth');
    expect(state.login?.loginId).toBe('login');
    const completion = deferred<Account>();
    vi.mocked(client.complete).mockReturnValue(completion.promise);
    let submitting!: Promise<void>;
    await act(async () => {
      submitting = state.completeLogin('good');
    });
    act(() => {
      renderer.update(<Harness enabled={false} />);
    });
    await act(async () => {
      completion.resolve({ id: 'late-account' });
      await submitting;
    });
    expect(client.cancel).toHaveBeenCalledWith('login');
    expect(client.list).toHaveBeenCalledTimes(1);
    expect(clearModel).not.toHaveBeenCalled();
    expect(state.accounts).toEqual([account]);
  });

  it('preserves cached rows on navigation failure without quota, login or dialogs', async () => {
    await mount();
    vi.mocked(client.list).mockRejectedValue(new Error('accountError.read'));
    act(() => {
      renderer.update(<Harness navigationKey="other-tool-sharing-store" />);
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(state.accounts).toEqual([account]);
    expect(client.start).not.toHaveBeenCalled();
    expect(client.refresh).not.toHaveBeenCalled();
    expect(client.remove).not.toHaveBeenCalled();
    expect(client.open).not.toHaveBeenCalled();
    expect(showError).not.toHaveBeenCalled();
  });
});
