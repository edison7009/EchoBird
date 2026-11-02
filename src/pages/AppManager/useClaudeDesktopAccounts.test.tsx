import { useLayoutEffect } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import * as api from '../../api/tauri';
import { useClaudeDesktopAccounts } from './useClaudeDesktopAccounts';

vi.mock('../../api/tauri', () => ({
  listClaudeDesktopAccounts: vi.fn(),
  startClaudeDesktopLogin: vi.fn(),
  pollClaudeDesktopLogin: vi.fn(),
  cancelClaudeDesktopLogin: vi.fn(),
  refreshClaudeDesktopAccount: vi.fn(),
  deleteClaudeDesktopAccount: vi.fn(),
  startTool: vi.fn(),
}));
vi.mock('../../hooks/useI18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }));
vi.mock('../../components/ConfirmDialog', () => ({ useConfirm: () => async () => true }));

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
  expect(api.refreshClaudeDesktopAccount).not.toHaveBeenCalled();
  expect(showError).not.toHaveBeenCalled();
});
