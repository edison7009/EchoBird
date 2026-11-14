import { useLayoutEffect } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import * as api from '../../api/tauri';
import { useCursorAccounts } from './useCursorAccounts';

vi.mock('../../api/tauri', () => ({
  listGrokBotAccounts: vi.fn(),
  startGrokBotLogin: vi.fn(),
  pollGrokBotLogin: vi.fn(),
  cancelGrokBotLogin: vi.fn().mockResolvedValue(undefined),
  listCursorAccounts: vi.fn(),
  startCursorLogin: vi.fn(),
  pollCursorLogin: vi.fn(),
  cancelCursorLogin: vi.fn().mockResolvedValue(undefined),
  openExternal: vi.fn().mockResolvedValue(undefined),
  switchGrokBotAccount: vi.fn(),
  switchCursorAccount: vi.fn(),
  deleteCursorAccount: vi.fn().mockResolvedValue(undefined),
  deleteGrokBotAccount: vi.fn().mockResolvedValue(undefined),
  refreshCursorAccount: vi.fn(),
  refreshGrokBotAccount: vi.fn(),
}));
vi.mock('../../hooks/useI18n', () => {
  const t = (key: string) => key;
  return { useI18n: () => ({ t }) };
});
vi.mock('../../components/ConfirmDialog', () => ({ useConfirm: () => async () => true }));
describe.each(['grokbot', 'cursor', 'cursorcli'] as const)('%s account lifecycle', (tool) => {
  const client =
    tool === 'grokbot'
      ? {
          list: api.listGrokBotAccounts,
          start: api.startGrokBotLogin,
          poll: api.pollGrokBotLogin,
          cancel: api.cancelGrokBotLogin,
          remove: api.deleteGrokBotAccount,
          switch: api.switchGrokBotAccount,
          refresh: api.refreshGrokBotAccount,
        }
      : {
          list: api.listCursorAccounts,
          start: api.startCursorLogin,
          poll: api.pollCursorLogin,
          cancel: api.cancelCursorLogin,
          remove: api.deleteCursorAccount,
          switch: api.switchCursorAccount,
          refresh: api.refreshCursorAccount,
        };
  const clearModel = vi.fn();
  const showError = vi.fn();
  const usage: api.CursorUsage = { plan: 'Pro', remainingPercent: 75, resetAt: null };
  const account: api.GrokBotAccount = { id: 'one', email: 'one@example.test', active: true, usage };
  let state: ReturnType<typeof useCursorAccounts>;
  let renderer: ReactTestRenderer;
  function Harness({ enabled = true }: { enabled?: boolean }) {
    const result = useCursorAccounts(tool, enabled, clearModel, showError);
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
      await vi.runOnlyPendingTimersAsync();
    });
  }
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    vi.mocked(client.list).mockResolvedValue([account]);
    vi.mocked(client.start).mockResolvedValue({
      loginId: 'login',
      verificationUri: 'https://cursor.com/loginDeepControl',
      expiresAt: Date.now() / 1000 + 60,
    });
    vi.mocked(client.poll).mockResolvedValue(account);
    vi.mocked(client.refresh).mockReset().mockResolvedValue(usage);
  });
  afterEach(() => {
    act(() => {
      renderer?.unmount();
    });
    vi.useRealTimers();
  });

  it('loads the current account and saves browser login without switching the client', async () => {
    await mount();
    expect(state.selectedId).toBe('one');
    await act(async () => {
      await state.add();
    });
    expect(api.openExternal).toHaveBeenCalledWith('https://cursor.com/loginDeepControl');
    expect(clearModel).toHaveBeenCalled();
    expect(client.switch).not.toHaveBeenCalled();
    expect(state.busy).toBe(false);
    if (tool === 'cursorcli') {
      expect(api.listCursorAccounts).toHaveBeenCalledWith('cursorcli');
      expect(api.startCursorLogin).toHaveBeenCalledWith('cursorcli');
      expect(api.pollCursorLogin).toHaveBeenCalledWith('login', 'cursorcli');
    } else if (tool === 'cursor') {
      expect(api.listCursorAccounts).toHaveBeenCalledWith();
      expect(api.startCursorLogin).toHaveBeenCalledWith();
      expect(api.pollCursorLogin).toHaveBeenCalledWith('login');
    }
  });
  it('cancels late initialization after leaving the tool without opening a browser', async () => {
    await mount();
    const waiting = deferred<api.GrokBotLogin>();
    vi.mocked(client.start).mockReturnValue(waiting.promise);
    let operation!: Promise<void>;
    act(() => {
      operation = state.add();
    });
    act(() => {
      renderer.update(<Harness enabled={false} />);
    });
    await act(async () => {
      waiting.resolve({
        loginId: 'late',
        verificationUri: 'https://cursor.com/loginDeepControl',
        expiresAt: Date.now() / 1000 + 60,
      });
      await operation;
    });
    expect(client.cancel).toHaveBeenCalledWith('late');
    expect(api.openExternal).not.toHaveBeenCalled();
  });
  it('keeps an explicit selection made during browser login and prevents duplicate login', async () => {
    await mount();
    const waiting = deferred<api.GrokBotAccount | null>();
    vi.mocked(client.poll).mockReturnValue(waiting.promise);
    let operation!: Promise<void>;
    await act(async () => {
      operation = state.add();
    });
    expect(state.busy).toBe(true);
    expect(state.remainingSeconds).toBe(60);
    await act(async () => {
      await state.add();
      state.select(null);
    });
    expect(client.start).toHaveBeenCalledTimes(1);
    await act(async () => {
      waiting.resolve(account);
      await operation;
    });
    expect(state.selectedId).toBeNull();
    expect(clearModel).not.toHaveBeenCalled();
  });
  it('cancels at 60 seconds even with a poll in flight and ignores its late result', async () => {
    await mount();
    const waiting = deferred<api.GrokBotAccount | null>();
    vi.mocked(client.poll).mockReturnValue(waiting.promise);
    let operation!: Promise<void>;
    await act(async () => {
      operation = state.add();
      state.select(null);
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    expect(state.busy).toBe(false);
    expect(client.cancel).toHaveBeenCalledWith('login');
    expect(vi.getTimerCount()).toBe(0);
    expect(showError).toHaveBeenCalledWith('accountError.expired');
    const calls = vi.mocked(client.list).mock.calls.length;
    await act(async () => {
      waiting.resolve(account);
      await operation;
    });
    expect(client.list).toHaveBeenCalledTimes(calls);
    expect(state.selectedId).toBeNull();
  });
  it('deletes only the saved entry and clears its selection', async () => {
    await mount();
    await act(async () => {
      await state.remove(account);
    });
    expect(client.remove).toHaveBeenCalledWith('one');
    expect(state.accounts).toEqual([]);
    expect(state.selectedId).toBeNull();
    expect(client.switch).not.toHaveBeenCalled();
  });
  it('does not fetch usage on import, list reload, login, or returning to the tool', async () => {
    vi.mocked(client.list).mockResolvedValue([{ ...account, usage: null }]);
    await mount();
    await act(async () => {
      await state.reload();
    });
    await act(async () => {
      await state.add();
    });
    act(() => {
      renderer.update(<Harness enabled={false} />);
    });
    act(() => {
      renderer.update(<Harness />);
    });
    await act(async () => {
      await vi.runOnlyPendingTimersAsync();
    });
    expect(client.refresh).not.toHaveBeenCalled();
    expect(state.accounts[0].usage).toBeNull();
    await act(async () => {
      await state.refresh(state.accounts[0]);
    });
    expect(client.refresh).toHaveBeenCalledTimes(1);
    expect(state.accounts[0].usage).toEqual(usage);
  });
  it('keeps cached accounts without a popup when returning to unreadable client data', async () => {
    await mount();
    vi.mocked(client.list).mockRejectedValueOnce(new Error('accountError.read'));
    act(() => renderer.update(<Harness enabled={false} />));
    act(() => renderer.update(<Harness />));
    await act(async () => {
      await vi.runOnlyPendingTimersAsync();
    });
    expect(state.accounts).toEqual([account]);
    expect(showError).not.toHaveBeenCalled();
    expect(client.refresh).not.toHaveBeenCalled();
  });
  it('refreshes once while pending without changing selection or switching the client', async () => {
    await mount();
    const waiting = deferred<api.CursorUsage>();
    vi.mocked(client.refresh).mockReturnValue(waiting.promise);
    let operation!: Promise<unknown>;
    act(() => {
      state.select(null);
      operation = state.refresh(account);
    });
    expect(state.refreshing.has('one')).toBe(true);
    await act(async () => {
      await state.refresh(account);
    });
    expect(client.refresh).toHaveBeenCalledTimes(1);
    await act(async () => {
      waiting.resolve({ ...usage, remainingPercent: 0 });
      await operation;
    });
    expect(state.refreshing.size).toBe(0);
    expect(state.accounts[0].usage?.remainingPercent).toBe(0);
    expect(state.selectedId).toBeNull();
    expect(client.switch).not.toHaveBeenCalled();
    expect(clearModel).not.toHaveBeenCalled();
  });
  it('ignores refresh results after leaving the tool', async () => {
    await mount();
    const waiting = deferred<api.CursorUsage>();
    vi.mocked(client.refresh).mockReturnValue(waiting.promise);
    let operation!: Promise<unknown>;
    act(() => {
      operation = state.refresh(account);
    });
    act(() => {
      renderer.update(<Harness enabled={false} />);
    });
    await act(async () => {
      waiting.resolve({ ...usage, plan: 'Late result' });
      await operation;
    });
    expect(state.accounts[0].usage?.plan).toBe('Pro');
  });
  it('preserves known usage and releases pending state when refresh fails', async () => {
    await mount();
    vi.mocked(client.refresh).mockRejectedValue(new Error('accountError.network'));
    await act(async () => {
      await state.refresh(account);
    });
    expect(state.accounts[0].usage).toEqual(usage);
    expect(state.refreshing.size).toBe(0);
    expect(showError).toHaveBeenCalledWith('accountError.network');
  });
});
