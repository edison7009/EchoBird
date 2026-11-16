import { useLayoutEffect } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import * as api from '../../api/tauri';
import { useNavigationStore } from '../../stores/navigationStore';
import { useClaudeDesktopAccounts } from './useClaudeDesktopAccounts';

vi.mock('../../api/tauri', () => ({
  listClaudeDesktopAccounts: vi.fn(),
  startClaudeDesktopLogin: vi.fn(),
  resetClaudeDesktopLogin: vi.fn(),
  pollClaudeDesktopLogin: vi.fn(),
  cancelClaudeDesktopLogin: vi.fn(),
  refreshClaudeDesktopAccount: vi.fn(),
  warmUpClaudeDesktopAccount: vi.fn(),
  deleteClaudeDesktopAccount: vi.fn(),
  startTool: vi.fn(),
}));
vi.mock('../../hooks/useI18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }));
const confirm = vi.hoisted(() => vi.fn());
vi.mock('../../components/ConfirmDialog', () => ({ useConfirm: () => confirm }));

const saved: api.ClaudeCodeAccount = {
  id: 'saved',
  email: 'saved@example.test',
  active: true,
  plan: 'Pro',
};
const captured = { ...saved, id: 'new', email: 'new@example.test' };
const clearModel = vi.fn();
const showError = vi.fn();
let renderer: ReactTestRenderer;
let state: ReturnType<typeof useClaudeDesktopAccounts>;
function Harness({ enabled = true, hasModel = false } = {}) {
  const result = useClaudeDesktopAccounts(enabled, hasModel, clearModel, showError);
  useLayoutEffect(() => {
    state = result;
  });
  return null;
}
async function tick(ms = 1) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
}
async function mount(enabled = true) {
  act(() => {
    renderer = create(<Harness enabled={enabled} />);
  });
  await tick();
}
beforeEach(() => {
  vi.clearAllMocks();
  vi.useFakeTimers();
  useNavigationStore.setState({ activePage: 'apps' });
  vi.mocked(api.resetClaudeDesktopLogin).mockResolvedValue(undefined);
  vi.mocked(api.startTool).mockResolvedValue(undefined);
  confirm.mockResolvedValue(true);
  vi.mocked(api.listClaudeDesktopAccounts).mockResolvedValue([saved]);
  vi.mocked(api.startClaudeDesktopLogin).mockResolvedValue({
    loginId: 'login',
    expiresAt: Date.now() / 1000 + 60,
  });
  vi.mocked(api.pollClaudeDesktopLogin).mockResolvedValue({
    account: null,
    awaitingClientExit: false,
    expiresAt: null,
  });
  vi.mocked(api.cancelClaudeDesktopLogin).mockResolvedValue(undefined);
});
afterEach(() => {
  act(() => renderer?.unmount());
  vi.useRealTimers();
});

it('loads installed accounts passively and never loads native state when disabled', async () => {
  await mount(false);
  expect(api.listClaudeDesktopAccounts).not.toHaveBeenCalled();
  act(() => renderer.update(<Harness />));
  await tick();
  expect(state.accounts).toEqual([saved]);
  expect(api.listClaudeDesktopAccounts).toHaveBeenCalledTimes(1);
  expect(api.startClaudeDesktopLogin).not.toHaveBeenCalled();
  expect(api.resetClaudeDesktopLogin).not.toHaveBeenCalled();
  expect(api.refreshClaudeDesktopAccount).not.toHaveBeenCalled();
  expect(api.deleteClaudeDesktopAccount).not.toHaveBeenCalled();
});
it('keeps model and account selection exclusive without applying native changes', async () => {
  await mount();
  act(() => state.select('saved'));
  expect(clearModel).toHaveBeenCalled();
  act(() => renderer.update(<Harness hasModel />));
  await tick();
  expect(state.selectedId).toBeNull();
  expect(api.startClaudeDesktopLogin).not.toHaveBeenCalled();
  expect(api.refreshClaudeDesktopAccount).not.toHaveBeenCalled();
});
it('uses one browser deadline, saves once and does not auto-refresh quota', async () => {
  await mount();
  let task!: Promise<void>;
  act(() => {
    task = state.add();
  });
  await tick(50_000);
  const deadline = Date.now() / 1000 + 60;
  vi.mocked(api.pollClaudeDesktopLogin).mockResolvedValue({
    account: null,
    awaitingClientExit: true,
    expiresAt: deadline,
  });
  await tick(2000);
  expect(state.busy).toBe(true);
  expect(state.remainingSeconds).toBeLessThanOrEqual(8);
  await tick(2000);
  expect(state.busy).toBe(true);
  vi.mocked(api.listClaudeDesktopAccounts).mockResolvedValue([saved, captured]);
  vi.mocked(api.pollClaudeDesktopLogin).mockResolvedValue({
    account: captured,
    awaitingClientExit: false,
    expiresAt: null,
  });
  await tick(2000);
  await act(async () => {
    await task;
  });
  expect(state.busy).toBe(false);
  expect(state.accounts.map((a) => a.id)).toEqual(['saved', 'new']);
  expect(api.startClaudeDesktopLogin).toHaveBeenCalledTimes(1);
  expect(api.refreshClaudeDesktopAccount).not.toHaveBeenCalled();
  expect(showError).not.toHaveBeenCalled();
});
it('cancels on timeout and never imports a late account after leaving the tool', async () => {
  await mount();
  let task!: Promise<void>;
  act(() => {
    task = state.add();
  });
  await tick(60_000);
  await act(async () => {
    await task;
  });
  expect(showError).toHaveBeenCalledWith('accountError.expired');
  expect(api.cancelClaudeDesktopLogin).toHaveBeenCalledWith('login');
  vi.mocked(api.startClaudeDesktopLogin).mockResolvedValue({
    loginId: 'next',
    expiresAt: Date.now() / 1000 + 60,
  });
  let resolve!: (value: api.ClaudeDesktopLoginPoll) => void;
  vi.mocked(api.pollClaudeDesktopLogin).mockReturnValue(
    new Promise((done) => {
      resolve = done;
    })
  );
  act(() => {
    task = state.add();
  });
  await tick();
  act(() => renderer.update(<Harness enabled={false} />));
  await tick();
  await act(async () => {
    resolve({ account: captured, awaitingClientExit: true, expiresAt: Date.now() / 1000 + 60 });
    await task;
  });
  expect(state.accounts).toEqual([saved]);
  expect(api.cancelClaudeDesktopLogin).toHaveBeenCalledWith('next');
});

it('saves a snapshot within the shared 60-second deadline and never launches the client itself', async () => {
  await mount();
  vi.mocked(api.listClaudeDesktopAccounts).mockResolvedValue([saved, captured]);
  vi.mocked(api.pollClaudeDesktopLogin).mockResolvedValue({
    account: captured,
    awaitingClientExit: false,
    expiresAt: null,
  });
  let task!: Promise<void>;
  act(() => {
    task = state.add();
  });
  await tick();
  expect(state.remainingSeconds).toBeLessThanOrEqual(60);
  await tick(2000);
  await act(async () => {
    await task;
  });
  expect(state.accounts.map((a) => a.id)).toEqual(['saved', 'new']);
  // A snapshot only reads Claude Desktop's sign-in; the page starts nothing.
  expect(api.startTool).not.toHaveBeenCalled();
  expect(api.resetClaudeDesktopLogin).not.toHaveBeenCalled();
  expect(api.refreshClaudeDesktopAccount).not.toHaveBeenCalled();
  expect(showError).not.toHaveBeenCalled();
});

it('warns before a snapshot and starts nothing unless the user confirms', async () => {
  await mount();
  confirm.mockResolvedValue(false);
  await act(async () => {
    await state.add();
  });
  expect(confirm).toHaveBeenCalledWith({
    type: 'warning',
    title: 'agent.accountSnapshotNoticeTitle',
    message: 'agent.accountSnapshotHint',
    preserveLineBreaks: true,
  });
  expect(api.startClaudeDesktopLogin).not.toHaveBeenCalled();
  expect(state.busy).toBe(false);
  confirm.mockResolvedValue(true);
  let task!: Promise<void>;
  act(() => {
    task = state.add();
  });
  await tick();
  expect(api.startClaudeDesktopLogin).toHaveBeenCalledTimes(1);
  act(() => state.cancelLogin());
  await tick(2000);
  await act(async () => {
    await task;
  });
});

it('resets local sign-in independently and opens Claude without capturing an account', async () => {
  await mount();
  vi.mocked(api.listClaudeDesktopAccounts).mockResolvedValue([{ ...saved, active: false }]);
  await act(async () => {
    await state.addNext();
  });
  expect(confirm).not.toHaveBeenCalled();
  expect(api.resetClaudeDesktopLogin).toHaveBeenCalledExactlyOnceWith();
  expect(api.startClaudeDesktopLogin).not.toHaveBeenCalled();
  expect(api.pollClaudeDesktopLogin).not.toHaveBeenCalled();
  expect(api.refreshClaudeDesktopAccount).not.toHaveBeenCalled();
  expect(api.deleteClaudeDesktopAccount).not.toHaveBeenCalled();
  expect(api.startTool).toHaveBeenCalledExactlyOnceWith('claudedesktop');
  expect(state.accounts).toEqual([{ ...saved, active: false }]);
  expect(state.selectedId).toBeNull();
  expect(clearModel).toHaveBeenCalledTimes(1);
  expect(state.busy).toBe(false);
  expect(showError).not.toHaveBeenCalled();
});

it('keeps independent pending state and blocks duplicate reset and snapshot actions', async () => {
  await mount();
  let finish!: () => void;
  vi.mocked(api.resetClaudeDesktopLogin).mockReturnValue(
    new Promise((resolve) => {
      finish = resolve;
    })
  );
  let task!: Promise<void>;
  act(() => {
    task = state.addNext();
  });
  expect(state.resetting).toBe(true);
  expect(state.busy).toBe(true);
  expect(state.snapshotBusy).toBe(false);
  await act(async () => {
    await state.addNext();
    await state.add();
  });
  expect(api.resetClaudeDesktopLogin).toHaveBeenCalledTimes(1);
  expect(confirm).not.toHaveBeenCalled();
  await act(async () => {
    finish();
    await task;
  });
  expect(state.resetting).toBe(false);
  expect(api.startTool).toHaveBeenCalledTimes(1);
});

it('blocks reset while a snapshot confirmation or capture is pending', async () => {
  await mount();
  let finish!: (value: boolean) => void;
  confirm.mockReturnValue(
    new Promise((resolve) => {
      finish = resolve;
    })
  );
  let task!: Promise<void>;
  act(() => {
    task = state.add();
  });
  await act(async () => {
    await state.addNext();
  });
  expect(api.resetClaudeDesktopLogin).not.toHaveBeenCalled();
  await act(async () => {
    finish(true);
  });
  await tick();
  expect(state.snapshotBusy).toBe(true);
  await act(async () => {
    await state.addNext();
  });
  expect(api.resetClaudeDesktopLogin).not.toHaveBeenCalled();
  act(() => state.cancelLogin());
  await tick(2000);
  await task;
});

it('preserves saved rows and leaves Claude closed when reset fails', async () => {
  await mount();
  vi.mocked(api.resetClaudeDesktopLogin).mockRejectedValue(new Error('accountError.write'));
  await act(async () => {
    await state.addNext();
  });
  expect(showError).toHaveBeenCalledTimes(1);
  expect(state.accounts).toEqual([saved]);
  expect(state.resetting).toBe(false);
  expect(api.startTool).not.toHaveBeenCalled();
  expect(api.startClaudeDesktopLogin).not.toHaveBeenCalled();
});

it('reports an explicit launch failure without falling back to snapshot capture', async () => {
  await mount();
  vi.mocked(api.startTool).mockRejectedValue(new Error('launch failed'));
  await act(async () => {
    await state.addNext();
  });
  expect(showError).toHaveBeenCalledTimes(1);
  expect(state.accounts).toEqual([saved]);
  expect(state.busy).toBe(false);
  expect(api.startClaudeDesktopLogin).not.toHaveBeenCalled();
});

it.each(['tool', 'page', 'unmount'] as const)(
  'suppresses late reset launch after leaving the %s',
  async (leave) => {
    await mount();
    let finish!: () => void;
    vi.mocked(api.resetClaudeDesktopLogin).mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      })
    );
    let task!: Promise<void>;
    act(() => {
      task = state.addNext();
    });
    act(() => {
      if (leave === 'tool') renderer.update(<Harness enabled={false} />);
      if (leave === 'page') useNavigationStore.setState({ activePage: 'models' });
      if (leave === 'unmount') renderer.unmount();
    });
    await act(async () => {
      finish();
      await task;
    });
    expect(api.startTool).not.toHaveBeenCalled();
    expect(showError).not.toHaveBeenCalled();
    expect(clearModel).not.toHaveBeenCalled();
  }
);

it('never resets or snapshots a disabled client', async () => {
  await mount(false);
  await act(async () => {
    await state.addNext();
    await state.add();
  });
  expect(api.resetClaudeDesktopLogin).not.toHaveBeenCalled();
  expect(api.startClaudeDesktopLogin).not.toHaveBeenCalled();
  expect(confirm).not.toHaveBeenCalled();
});

it('allows an immediate snapshot retry after cancellation while ignoring the old response', async () => {
  await mount();
  let finish!: (value: api.ClaudeDesktopLoginPoll) => void;
  vi.mocked(api.pollClaudeDesktopLogin).mockReturnValueOnce(
    new Promise((resolve) => {
      finish = resolve;
    })
  );
  let previous!: Promise<void>;
  act(() => {
    previous = state.add();
  });
  await tick();
  act(() => state.cancelLogin());
  vi.mocked(api.pollClaudeDesktopLogin).mockResolvedValue({
    account: captured,
    awaitingClientExit: false,
    expiresAt: null,
  });
  vi.mocked(api.listClaudeDesktopAccounts).mockResolvedValue([saved, captured]);
  let retry!: Promise<void>;
  act(() => {
    retry = state.add();
  });
  await tick(2000);
  await act(async () => {
    await retry;
  });
  expect(api.startClaudeDesktopLogin).toHaveBeenCalledTimes(2);
  expect(state.accounts).toEqual([saved, captured]);
  await act(async () => {
    finish({ account: saved, awaitingClientExit: false, expiresAt: null });
    await previous;
  });
  expect(state.accounts).toEqual([saved, captured]);
  expect(api.resetClaudeDesktopLogin).not.toHaveBeenCalled();
});

it('resets a signed-out client while preserving an empty saved-account list', async () => {
  vi.mocked(api.listClaudeDesktopAccounts).mockResolvedValue([]);
  await mount();
  await act(async () => {
    await state.addNext();
  });
  expect(api.resetClaudeDesktopLogin).toHaveBeenCalledTimes(1);
  expect(api.startTool).toHaveBeenCalledTimes(1);
  expect(state.accounts).toEqual([]);
  expect(api.startClaudeDesktopLogin).not.toHaveBeenCalled();
});

it('warms up only when asked, without selecting and with the refresh pending state', async () => {
  await mount();
  expect(api.warmUpClaudeDesktopAccount).not.toHaveBeenCalled();
  expect(api.refreshClaudeDesktopAccount).not.toHaveBeenCalled();
  const warmed = { ...saved, fiveHour: { remainingPercent: 100, resetAt: 1791561600 } };
  let finish!: (account: api.ClaudeCodeAccount) => void;
  vi.mocked(api.warmUpClaudeDesktopAccount).mockReturnValue(
    new Promise((resolve) => {
      finish = resolve;
    })
  );
  const selected = state.selectedId;
  void state.warmUp(saved);
  await tick();
  expect(state.refreshing.has('saved')).toBe(true);
  void state.warmUp(saved);
  await tick();
  expect(api.warmUpClaudeDesktopAccount).toHaveBeenCalledTimes(1);
  await act(async () => finish(warmed));
  expect(state.refreshing.has('saved')).toBe(false);
  expect(state.accounts[0].fiveHour?.resetAt).toBe(1791561600);
  expect(state.selectedId).toBe(selected);
  expect(api.refreshClaudeDesktopAccount).not.toHaveBeenCalled();
  expect(clearModel).not.toHaveBeenCalled();
});

it('shows an explicit warm-up failure and keeps the row', async () => {
  await mount();
  vi.mocked(api.warmUpClaudeDesktopAccount).mockRejectedValue(
    new Error('accountError.claudeCodeRequired')
  );
  await act(async () => {
    await state.warmUp(saved);
  });
  expect(showError).toHaveBeenCalledTimes(1);
  expect(state.accounts).toEqual([saved]);
  expect(state.authorizationFailedIds.has('saved')).toBe(false);
});
