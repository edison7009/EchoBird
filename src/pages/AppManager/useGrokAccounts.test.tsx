import { useLayoutEffect } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import * as api from '../../api/tauri';
import { useGrokAccounts } from './useGrokAccounts';

vi.mock('../../api/tauri', () => ({
  listGrokAccounts: vi.fn(),
  startGrokLogin: vi.fn(),
  pollGrokLogin: vi.fn(),
  cancelGrokLogin: vi.fn().mockResolvedValue(undefined),
  switchGrokAccount: vi.fn(),
  deleteGrokAccount: vi.fn().mockResolvedValue(undefined),
  refreshGrokAccount: vi.fn(),
  listManusAccounts: vi.fn(),
  startManusLogin: vi.fn(),
  pollManusLogin: vi.fn(),
  cancelManusLogin: vi.fn(),
  switchManusAccount: vi.fn(),
  deleteManusAccount: vi.fn(),
  refreshManusAccount: vi.fn(),
  listCueAccounts: vi.fn(),
  startCueLogin: vi.fn(),
  pollCueLogin: vi.fn(),
  cancelCueLogin: vi.fn(),
  switchCueAccount: vi.fn(),
  deleteCueAccount: vi.fn(),
  refreshCueAccount: vi.fn(),
  openExternal: vi.fn().mockResolvedValue(undefined),
}));
vi.mock('../../hooks/useI18n', () => {
  const t = (key: string) => key;
  return { useI18n: () => ({ t }) };
});
vi.mock('../../components/ConfirmDialog', () => ({ useConfirm: () => async () => true }));

describe('Grok Build account lifecycle', () => {
  const account: api.GrokAccount = {
    id: 'one',
    email: 'one@example.test',
    plan: 'SuperGrok',
    active: true,
  };
  const clearModel = vi.fn();
  const showError = vi.fn();
  let state: ReturnType<typeof useGrokAccounts>;
  let renderer: ReactTestRenderer;
  function Harness({
    enabled = true,
    hasModel = false,
    tool = 'grok',
  }: { enabled?: boolean; hasModel?: boolean; tool?: 'grok' | 'manus' | 'cue' } = {}) {
    const result = useGrokAccounts(enabled, hasModel, clearModel, showError, tool);
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
  async function mount(tool: 'grok' | 'manus' | 'cue' = 'grok') {
    act(() => {
      renderer = create(<Harness tool={tool} />);
    });
    await act(async () => {
      await vi.runOnlyPendingTimersAsync();
    });
  }
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    vi.mocked(api.listGrokAccounts).mockResolvedValue([account]);
    vi.mocked(api.startGrokLogin).mockResolvedValue({
      loginId: 'login',
      expiresAt: Date.now() / 1000 + 60,
    });
    vi.mocked(api.pollGrokLogin).mockResolvedValue(account);
    vi.mocked(api.refreshGrokAccount).mockResolvedValue(account);
    vi.mocked(api.cancelManusLogin).mockResolvedValue(undefined);
    vi.mocked(api.listCueAccounts).mockResolvedValue([]);
    vi.mocked(api.cancelCueLogin).mockResolvedValue(undefined);
    vi.mocked(api.startCueLogin).mockResolvedValue({
      loginId: 'cue-login',
      expiresAt: Date.now() / 1000 + 60,
    });
    vi.mocked(api.startManusLogin).mockResolvedValue({
      loginId: 'manus-login',
      expiresAt: Date.now() / 1000 + 60,
      verificationUri: 'https://manus.im/login?from=desktop&type=signIn&nonce=fixture',
    });
    vi.mocked(api.openExternal).mockResolvedValue(undefined);
  });
  afterEach(() => {
    act(() => renderer?.unmount());
    vi.useRealTimers();
  });

  it('propagates a failed switch so the caller cannot continue launching', async () => {
    await mount();
    vi.mocked(api.switchGrokAccount).mockRejectedValue(new Error('accountError.write'));
    await expect(state.switchAccount()).rejects.toThrow('accountError.write');
  });
  it('refreshes only the requested tier and preserves it when a later refresh fails', async () => {
    const unknown = { ...account, plan: null };
    const other = { ...account, id: 'two', email: 'two@example.test', active: false };
    vi.mocked(api.listGrokAccounts).mockResolvedValue([unknown, other]);
    await mount();
    expect(api.refreshGrokAccount).not.toHaveBeenCalled();
    const selected = state.selectedId;
    vi.mocked(api.refreshGrokAccount).mockResolvedValueOnce({ ...unknown, plan: 'Free' });
    await act(async () => {
      await state.refresh(unknown);
    });
    expect(api.refreshGrokAccount).toHaveBeenCalledWith(unknown.id);
    expect(state.accounts).toEqual([{ ...unknown, plan: 'Free' }, other]);
    expect(state.selectedId).toBe(selected);
    expect(api.switchGrokAccount).not.toHaveBeenCalled();
    vi.mocked(api.refreshGrokAccount).mockRejectedValueOnce(new Error('accountError.network'));
    await act(async () => {
      await state.refresh(state.accounts[0]);
    });
    expect(state.accounts).toEqual([{ ...unknown, plan: 'Free' }, other]);
    expect(showError).toHaveBeenCalledWith('accountError.network');
  });
  it('keeps cached accounts without a dialog when returning to unreadable local state', async () => {
    await mount();
    act(() => renderer.update(<Harness enabled={false} />));
    vi.mocked(api.listGrokAccounts).mockRejectedValueOnce(new Error('accountError.read'));
    act(() => renderer.update(<Harness />));
    await act(async () => {
      await vi.runOnlyPendingTimersAsync();
    });
    expect(state.accounts).toEqual([account]);
    expect(showError).not.toHaveBeenCalled();
    expect(api.refreshGrokAccount).not.toHaveBeenCalled();

    vi.mocked(api.refreshGrokAccount).mockRejectedValueOnce(new Error('accountError.network'));
    await act(async () => {
      await state.refresh(account);
    });
    expect(showError).toHaveBeenCalledWith('accountError.network');
  });
  it('cancels a login that finishes starting after leaving the tool', async () => {
    await mount();
    const waiting = deferred<api.GrokLogin>();
    vi.mocked(api.startGrokLogin).mockReturnValue(waiting.promise);
    let operation!: Promise<void>;
    act(() => {
      operation = state.add();
      renderer.update(<Harness enabled={false} />);
    });
    await act(async () => {
      waiting.resolve({ loginId: 'late', expiresAt: Date.now() / 1000 + 60 });
      await operation;
    });
    expect(api.cancelGrokLogin).toHaveBeenCalledWith('late');
    expect(api.pollGrokLogin).not.toHaveBeenCalled();
  });
  it('times out an in-flight poll and ignores its late result', async () => {
    await mount();
    const waiting = deferred<api.GrokAccount | null>();
    vi.mocked(api.pollGrokLogin).mockReturnValue(waiting.promise);
    let operation!: Promise<void>;
    await act(async () => {
      operation = state.add();
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    expect(state.busy).toBe(false);
    expect(api.cancelGrokLogin).toHaveBeenCalledWith('login');
    expect(showError).toHaveBeenCalledWith('accountError.expired');
    const calls = vi.mocked(api.listGrokAccounts).mock.calls.length;
    await act(async () => {
      waiting.resolve(account);
      await operation;
    });
    expect(api.listGrokAccounts).toHaveBeenCalledTimes(calls);
    expect(clearModel).not.toHaveBeenCalled();
  });
  it('keeps a model selected while login is pending and prevents duplicate login', async () => {
    await mount();
    const waiting = deferred<api.GrokAccount | null>();
    vi.mocked(api.pollGrokLogin).mockReturnValue(waiting.promise);
    let operation!: Promise<void>;
    await act(async () => {
      operation = state.add();
      await state.add();
      state.select(null);
    });
    expect(api.startGrokLogin).toHaveBeenCalledTimes(1);
    await act(async () => {
      waiting.resolve(account);
      await operation;
    });
    expect(state.selectedId).toBeNull();
    expect(clearModel).not.toHaveBeenCalled();
  });
  it('ignores a refresh response after leaving and prevents duplicate refreshes', async () => {
    await mount();
    const waiting = deferred<api.GrokAccount>();
    vi.mocked(api.refreshGrokAccount).mockReturnValue(waiting.promise);
    let operation!: Promise<unknown>;
    act(() => {
      operation = state.refresh(account);
    });
    await act(async () => {
      await state.refresh(account);
    });
    expect(api.refreshGrokAccount).toHaveBeenCalledTimes(1);
    act(() => renderer.update(<Harness enabled={false} />));
    await act(async () => {
      waiting.resolve({ ...account, plan: 'Late plan' });
      await operation;
    });
    expect(state.accounts[0].plan).toBe('SuperGrok');
  });
  it('uses the built-in browser with saved Manus accounts and saves only the verified result', async () => {
    const manus = { ...account, credits: null };
    vi.mocked(api.listManusAccounts).mockResolvedValue([manus]);
    vi.mocked(api.startManusLogin).mockResolvedValue({
      loginId: 'manus-login',
      verificationUri: 'https://manus.im/login?from=desktop&type=signIn&nonce=fixture',
      expiresAt: Date.now() / 1000 + 60,
    });
    const waiting = deferred<api.ManusLoginPoll>();
    vi.mocked(api.pollManusLogin).mockReturnValue(waiting.promise);
    await mount('manus');
    expect(api.startManusLogin).not.toHaveBeenCalled();
    expect(api.refreshManusAccount).not.toHaveBeenCalled();
    const selectedBeforeLogin = state.selectedId;
    let operation!: Promise<void>;
    await act(async () => {
      operation = state.add();
    });
    expect(api.openExternal).not.toHaveBeenCalled();
    expect(api.pollManusLogin).toHaveBeenCalledWith('manus-login');
    expect(state.busy).toBe(true);
    expect(state.selectedId).toBe(selectedBeforeLogin);
    await act(async () => {
      waiting.resolve({ account: manus, awaitingClientExit: false, expiresAt: null });
      await operation;
    });
    expect(state.selectedId).toBe(manus.id);
    expect(api.cancelManusLogin).toHaveBeenCalledWith('manus-login');
    expect(api.switchManusAccount).not.toHaveBeenCalled();
    expect(api.refreshManusAccount).not.toHaveBeenCalled();
    vi.mocked(api.refreshManusAccount).mockResolvedValue({
      ...manus,
      credits: { total: 1300, free: 1000, refresh: 300, nextRefreshAt: null },
    });
    await act(async () => {
      await state.refresh(manus);
    });
    expect(api.refreshManusAccount).toHaveBeenCalledWith(manus.id);
    expect(state.accounts[0]).toMatchObject({ credits: { total: 1300 } });
  });
  it('allows ten minutes for isolated Manus login and cancels a late result', async () => {
    const manus = { ...account, credits: null };
    const waiting = deferred<api.ManusLoginPoll>();
    vi.mocked(api.listManusAccounts).mockResolvedValue([manus]);
    vi.mocked(api.startManusLogin).mockResolvedValue({
      loginId: 'manus-login',
      verificationUri: 'https://manus.im/login?from=desktop&type=signIn&nonce=fixture',
      expiresAt: Date.now() / 1000 + 600,
    });
    vi.mocked(api.pollManusLogin).mockReturnValue(waiting.promise);
    await mount('manus');
    let operation!: Promise<void>;
    await act(async () => {
      operation = state.add();
    });
    expect(api.openExternal).not.toHaveBeenCalled();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    expect(state.busy).toBe(true);
    expect(api.cancelManusLogin).not.toHaveBeenCalled();
    expect(state.remainingSeconds).toBe(540);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(540_000);
    });
    expect(api.cancelManusLogin).toHaveBeenCalledWith('manus-login');
    expect(api.refreshManusAccount).not.toHaveBeenCalled();
    await act(async () => {
      waiting.resolve({
        account: null,
        awaitingClientExit: true,
        expiresAt: Date.now() / 1000 + 60,
      });
      await operation;
    });
    expect(state.accounts).toEqual([manus]);
    expect(state.busy).toBe(false);
    expect(state.awaitingClientExit).toBe(false);
  });

  it.each([
    { exit: 'leave', stage: 'browser' },
    { exit: 'switch', stage: 'browser' },
    { exit: 'leave', stage: 'client-exit' },
    { exit: 'switch', stage: 'client-exit' },
  ])(
    'cancels Manus login on $exit during $stage and ignores a late callback',
    async ({ exit, stage }) => {
      const manus = { ...account, credits: null };
      vi.mocked(api.listManusAccounts).mockResolvedValue([manus]);
      vi.mocked(api.startManusLogin).mockResolvedValue({
        loginId: 'manus-login',
        verificationUri: 'https://manus.im/login?from=desktop&type=signIn&nonce=fixture',
        expiresAt: Date.now() / 1000 + 60,
      });
      const waiting = deferred<api.ManusLoginPoll>();
      vi.mocked(api.pollManusLogin).mockReturnValue(waiting.promise);
      if (stage === 'client-exit')
        vi.mocked(api.pollManusLogin).mockResolvedValueOnce({
          account: null,
          awaitingClientExit: true,
          expiresAt: Date.now() / 1000 + 60,
        });
      await mount('manus');
      let operation!: Promise<void>;
      await act(async () => {
        operation = state.add();
      });
      if (stage === 'client-exit') {
        await act(async () => {
          await vi.advanceTimersByTimeAsync(1000);
        });
        expect(state.awaitingClientExit).toBe(false);
      }
      act(() => {
        renderer.update(
          exit === 'leave' ? <Harness tool="manus" enabled={false} /> : <Harness tool="grok" />
        );
      });
      expect(api.cancelManusLogin).toHaveBeenCalledWith('manus-login');
      const listCalls = vi.mocked(api.listManusAccounts).mock.calls.length;
      await act(async () => {
        waiting.resolve({
          account: { ...manus, id: 'late' },
          awaitingClientExit: true,
          expiresAt: Date.now() / 1000 + 60,
        });
        await operation;
      });
      expect(api.listManusAccounts).toHaveBeenCalledTimes(listCalls);
      expect(state.accounts.some((row) => row.id === 'late')).toBe(false);
      expect(api.switchManusAccount).not.toHaveBeenCalled();
      expect(api.refreshManusAccount).not.toHaveBeenCalled();
      expect(clearModel).not.toHaveBeenCalled();
      expect(state.awaitingClientExit).toBe(false);
    }
  );

  it('reports failed account validation and cancels without changing Manus accounts', async () => {
    const manus = { ...account, credits: null };
    vi.mocked(api.listManusAccounts).mockResolvedValue([manus]);
    vi.mocked(api.startManusLogin).mockResolvedValue({
      loginId: 'manus-login',
      verificationUri: 'https://manus.im/login?from=desktop&type=signIn&nonce=fixture',
      expiresAt: Date.now() / 1000 + 60,
    });
    vi.mocked(api.pollManusLogin).mockRejectedValueOnce(new Error('accountError.authResponse'));
    await mount('manus');
    await act(async () => state.add());
    expect(showError).toHaveBeenCalledWith('accountError.authResponse');
    expect(api.cancelManusLogin).toHaveBeenCalledWith('manus-login');
    expect(api.pollManusLogin).toHaveBeenCalledWith('manus-login');
    expect(state.accounts).toEqual([manus]);
    expect(state.busy).toBe(false);
    expect(api.switchManusAccount).not.toHaveBeenCalled();
  });

  it('ignores legacy native-exit hints, continues polling and cancels the current browser login', async () => {
    vi.mocked(api.listManusAccounts).mockResolvedValue([]);
    vi.mocked(api.pollManusLogin).mockResolvedValue({
      account: null,
      awaitingClientExit: true,
      expiresAt: Date.now() / 1000 + 60,
    });
    await mount('manus');
    let operation!: Promise<void>;
    await act(async () => {
      operation = state.add();
    });
    expect(state.awaitingClientExit).toBe(false);
    expect(state.busy).toBe(true);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(api.pollManusLogin).toHaveBeenCalledTimes(2);
    await act(async () => state.cancelLogin());
    expect(state.awaitingClientExit).toBe(false);
    expect(api.cancelManusLogin).toHaveBeenCalledWith('manus-login');
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
      await operation;
    });
    expect(api.switchManusAccount).not.toHaveBeenCalled();
    expect(api.refreshManusAccount).not.toHaveBeenCalled();
  });

  it('keeps the newer Manus browser attempt when an earlier login finishes starting late', async () => {
    vi.mocked(api.listManusAccounts).mockResolvedValue([]);
    const earlier = deferred<api.ManusLogin>();
    vi.mocked(api.startManusLogin).mockReturnValueOnce(earlier.promise);
    vi.mocked(api.pollManusLogin).mockResolvedValue({
      account: null,
      awaitingClientExit: true,
      expiresAt: Date.now() / 1000 + 60,
    });
    await mount('manus');
    let oldOperation!: Promise<void>;
    await act(async () => {
      oldOperation = state.add();
    });
    act(() => renderer.update(<Harness tool="manus" enabled={false} />));
    act(() => renderer.update(<Harness tool="manus" />));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    let newOperation!: Promise<void>;
    await act(async () => {
      newOperation = state.add();
    });
    expect(state.awaitingClientExit).toBe(false);
    await act(async () => {
      earlier.resolve({
        loginId: 'old-login',
        expiresAt: Date.now() / 1000 + 60,
        verificationUri: 'https://manus.im/login?nonce=old',
      });
      await oldOperation;
    });
    expect(api.cancelManusLogin).toHaveBeenCalledWith('old-login');
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(state.awaitingClientExit).toBe(false);
    expect(state.login?.loginId).toBe('manus-login');
    expect(api.openExternal).not.toHaveBeenCalled();
    await act(async () => state.cancelLogin());
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
      await newOperation;
    });
  });

  it.each(['success', 'timeout'] as const)(
    'keeps a single 60-second Manus browser deadline: %s',
    async (outcome) => {
      const manus = { ...account, credits: null };
      vi.mocked(api.listManusAccounts).mockResolvedValue([]);
      const initial = deferred<api.ManusLoginPoll>();
      const result = deferred<api.ManusLoginPoll>();
      vi.mocked(api.pollManusLogin)
        .mockReturnValueOnce(initial.promise)
        .mockReturnValue(result.promise);
      await mount('manus');
      let operation!: Promise<void>;
      await act(async () => {
        operation = state.add();
        await vi.advanceTimersByTimeAsync(45_000);
        initial.resolve({
          account: null,
          awaitingClientExit: true,
          expiresAt: Date.now() / 1000 + 60,
        });
      });
      expect(state.remainingSeconds).toBe(15);
      expect(state.awaitingClientExit).toBe(false);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(14_000);
      });
      expect(state.busy).toBe(true);
      expect(state.remainingSeconds).toBe(1);
      if (outcome === 'success') {
        vi.mocked(api.listManusAccounts).mockResolvedValue([manus]);
        await act(async () => {
          result.resolve({ account: manus, awaitingClientExit: false, expiresAt: null });
          await operation;
        });
        expect(state.accounts).toEqual([manus]);
        expect(showError).not.toHaveBeenCalled();
      } else {
        await act(async () => {
          await vi.advanceTimersByTimeAsync(1000);
        });
        expect(state.busy).toBe(false);
        expect(showError).toHaveBeenCalledWith('accountError.expired');
        expect(api.cancelManusLogin).toHaveBeenCalledWith('manus-login');
        await act(async () => {
          result.resolve({ account: manus, awaitingClientExit: false, expiresAt: null });
          await operation;
        });
        expect(state.accounts).toEqual([]);
        expect(api.listManusAccounts).toHaveBeenCalledTimes(1);
        expect(clearModel).not.toHaveBeenCalled();
      }
      expect(state.busy).toBe(false);
      expect(api.refreshManusAccount).not.toHaveBeenCalled();
      expect(api.switchManusAccount).not.toHaveBeenCalled();
      expect(api.openExternal).not.toHaveBeenCalled();
    }
  );

  it('does not extend the browser deadline on repeated legacy progress hints', async () => {
    vi.mocked(api.listManusAccounts).mockResolvedValue([]);
    const browser = deferred<api.ManusLoginPoll>();
    vi.mocked(api.pollManusLogin)
      .mockReturnValueOnce(browser.promise)
      .mockImplementation(async () => ({
        account: null,
        awaitingClientExit: true,
        expiresAt: Date.now() / 1000 + 60,
      }));
    await mount('manus');
    let operation!: Promise<void>;
    await act(async () => {
      operation = state.add();
      await vi.advanceTimersByTimeAsync(45_000);
      browser.resolve({
        account: null,
        awaitingClientExit: true,
        expiresAt: Date.now() / 1000 + 60,
      });
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(14_000);
    });
    expect(state.remainingSeconds).toBe(1);
    expect(state.busy).toBe(true);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
      await operation;
    });
    expect(state.busy).toBe(false);
    expect(api.cancelManusLogin).toHaveBeenCalledTimes(1);
    expect(showError).toHaveBeenCalledWith('accountError.expired');
  });

  it('can finish Manus login when the system browser opener is unavailable', async () => {
    vi.mocked(api.listManusAccounts).mockResolvedValue([]);
    vi.mocked(api.openExternal).mockRejectedValueOnce(new Error('accountError.failed'));
    vi.mocked(api.pollManusLogin).mockResolvedValue({
      account: { ...account, credits: null },
      awaitingClientExit: false,
      expiresAt: null,
    });
    await mount('manus');
    await act(async () => state.add());
    expect(api.openExternal).not.toHaveBeenCalled();
    expect(showError).not.toHaveBeenCalled();
    expect(api.cancelManusLogin).toHaveBeenCalledWith('manus-login');
    expect(api.pollManusLogin).toHaveBeenCalledWith('manus-login');
    expect(api.switchManusAccount).not.toHaveBeenCalled();
    expect(state.busy).toBe(false);
  });

  it('reports native login initialization failure without opening a browser or polling', async () => {
    vi.mocked(api.listManusAccounts).mockResolvedValue([]);
    vi.mocked(api.startManusLogin).mockRejectedValueOnce(new Error('accountError.failed'));
    await mount('manus');
    await act(async () => state.add());
    expect(showError).toHaveBeenCalledWith('accountError.failed');
    expect(state.busy).toBe(false);
    expect(api.openExternal).not.toHaveBeenCalled();
    expect(api.pollManusLogin).not.toHaveBeenCalled();
    expect(api.switchManusAccount).not.toHaveBeenCalled();
  });

  it('captures Cue through its isolated browser with one deadline and keeps Manus untouched', async () => {
    const cue = {
      ...account,
      credits: { total: 1300, free: null, refresh: null, nextRefreshAt: null },
    };
    const first = deferred<api.ManusLoginPoll>();
    const exit = deferred<api.ManusLoginPoll>();
    vi.mocked(api.pollCueLogin).mockReturnValueOnce(first.promise).mockReturnValue(exit.promise);
    await mount('cue');
    let operation!: Promise<void>;
    await act(async () => {
      operation = state.add();
      await vi.advanceTimersByTimeAsync(45_000);
      first.resolve({ account: null, awaitingClientExit: true, expiresAt: Date.now() / 1000 + 60 });
    });
    expect(state.awaitingClientExit).toBe(false);
    expect(state.remainingSeconds).toBe(15);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });
    expect(state.busy).toBe(true);
    vi.mocked(api.listCueAccounts).mockResolvedValue([cue]);
    await act(async () => {
      exit.resolve({ account: cue, awaitingClientExit: false, expiresAt: null });
      await operation;
    });
    expect(state.accounts).toEqual([cue]);
    expect(state.busy).toBe(false);
    expect(api.openExternal).not.toHaveBeenCalled();
    expect(api.startManusLogin).not.toHaveBeenCalled();
    expect(api.pollManusLogin).not.toHaveBeenCalled();
    expect(api.refreshCueAccount).not.toHaveBeenCalled();
    expect(api.switchCueAccount).not.toHaveBeenCalled();
  });

  it('updates Manus subscription and credits only on explicit refresh and preserves them on failure', async () => {
    const manus: api.ManusAccount = {
      ...account,
      plan: 'free',
      credits: { total: 1300, free: 1000, refresh: 300, nextRefreshAt: null },
    };
    vi.mocked(api.listManusAccounts).mockResolvedValue([manus]);
    await mount('manus');
    expect(api.refreshManusAccount).not.toHaveBeenCalled();
    const updated = {
      ...manus,
      plan: 'pro',
      subscriptionEndAt: 1794132901,
      credits: { total: 8000, free: 1000, refresh: 300, nextRefreshAt: 1791561600 },
    };
    vi.mocked(api.refreshManusAccount).mockResolvedValueOnce(updated);
    const selected = state.selectedId;
    await act(async () => {
      await state.refresh(manus);
    });
    expect(state.accounts).toEqual([updated]);
    expect(state.selectedId).toBe(selected);
    expect(api.refreshManusAccount).toHaveBeenCalledWith(manus.id);
    expect(api.refreshCueAccount).not.toHaveBeenCalled();
    expect(api.switchManusAccount).not.toHaveBeenCalled();
    expect(api.startManusLogin).not.toHaveBeenCalled();
    expect(api.openExternal).not.toHaveBeenCalled();
    vi.mocked(api.refreshManusAccount).mockRejectedValueOnce(
      new Error('accountError.loginRequired')
    );
    await act(async () => {
      await state.refresh(state.accounts[0]);
    });
    expect(state.accounts).toEqual([updated]);
    expect(state.refreshing.size).toBe(0);
    expect(showError).toHaveBeenCalledWith('accountError.loginRequired');
  });

  it('updates Cue weekly usage only on explicit refresh and preserves the cached summary on failure', async () => {
    const cue: api.CueAccount = { ...account, plan: 'plus', credits: null };
    vi.mocked(api.listCueAccounts).mockResolvedValue([cue]);
    await mount('cue');
    expect(api.refreshCueAccount).not.toHaveBeenCalled();
    const updated = { ...cue, weekly: { remainingPercent: 80, resetAt: 1792145701 } };
    vi.mocked(api.refreshCueAccount).mockResolvedValueOnce(updated);
    const selected = state.selectedId;
    await act(async () => {
      await state.refresh(cue);
    });
    expect(state.accounts).toEqual([updated]);
    expect(state.selectedId).toBe(selected);
    expect(api.refreshCueAccount).toHaveBeenCalledWith(cue.id);
    expect(api.refreshManusAccount).not.toHaveBeenCalled();
    expect(api.switchCueAccount).not.toHaveBeenCalled();
    vi.mocked(api.refreshCueAccount).mockRejectedValueOnce(new Error('accountError.network'));
    await act(async () => {
      await state.refresh(state.accounts[0]);
    });
    expect(state.accounts).toEqual([updated]);
    expect(state.refreshing.size).toBe(0);
    expect(showError).toHaveBeenCalledWith('accountError.network');
  });

  it.each(['leave', 'timeout'] as const)(
    'ignores a late Cue login result after %s',
    async (end) => {
      const waiting = deferred<api.ManusLoginPoll>();
      vi.mocked(api.pollCueLogin).mockReturnValue(waiting.promise);
      await mount('cue');
      let operation!: Promise<void>;
      await act(async () => {
        operation = state.add();
      });
      if (end === 'leave') {
        act(() => renderer.update(<Harness tool="cue" enabled={false} />));
      } else {
        await act(async () => {
          await vi.advanceTimersByTimeAsync(60_000);
        });
      }
      await act(async () => {
        waiting.resolve({
          account: { ...account, credits: null },
          awaitingClientExit: false,
          expiresAt: null,
        });
        await operation;
      });
      expect(api.cancelCueLogin).toHaveBeenCalledWith('cue-login');
      expect(state.accounts).toEqual([]);
      expect(api.listCueAccounts).toHaveBeenCalledTimes(1);
      expect(api.openExternal).not.toHaveBeenCalled();
      expect(api.switchCueAccount).not.toHaveBeenCalled();
    }
  );
});
