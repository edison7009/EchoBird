import { useLayoutEffect } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import * as api from '../../api/tauri';
import { useDeepSeekAccounts } from './useDeepSeekAccounts';

vi.mock('../../api/tauri', () => ({
  listDeepSeekAccounts: vi.fn(),
  startDeepSeekLogin: vi.fn(),
  pollDeepSeekLogin: vi.fn(),
  cancelDeepSeekLogin: vi.fn().mockResolvedValue(undefined),
  openExternal: vi.fn().mockResolvedValue(undefined),
  refreshDeepSeekAccountQuota: vi.fn(),
  deleteDeepSeekAccount: vi.fn().mockResolvedValue(undefined),
}));
vi.mock('../../hooks/useI18n', () => {
  const t = (key: string) => key;
  return { useI18n: () => ({ t, locale: 'en' }) };
});
vi.mock('../../components/ConfirmDialog', () => ({ useConfirm: () => async () => true }));
const clearModel = vi.fn();
const showError = vi.fn();
const account: api.DeepSeekAccount = { id: 'one', name: 'One', balances: null, active: true };
let state: ReturnType<typeof useDeepSeekAccounts>;
let renderer: ReactTestRenderer;
function Harness({ enabled = true, hasModel = false }: { enabled?: boolean; hasModel?: boolean }) {
  const result = useDeepSeekAccounts(enabled, hasModel, clearModel, showError);
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
async function mount(hasModel = false) {
  act(() => {
    renderer = create(<Harness hasModel={hasModel} />);
  });
  await act(async () => {
    await vi.runOnlyPendingTimersAsync();
  });
}
beforeEach(() => {
  vi.clearAllMocks();
  vi.useFakeTimers();
  vi.mocked(api.listDeepSeekAccounts).mockResolvedValue([account]);
  vi.mocked(api.refreshDeepSeekAccountQuota).mockResolvedValue({
    ...account,
    balances: [{ currency: 'CNY', amount: 10 }],
  });
  vi.mocked(api.startDeepSeekLogin).mockResolvedValue({
    loginId: 'login',
    verificationUri: 'https://platform.deepseek.com/dsh/authorize',
    expiresAt: Date.now() / 1000 + 600,
  });
  vi.mocked(api.pollDeepSeekLogin).mockResolvedValue(account);
});
afterEach(() => {
  act(() => {
    renderer?.unmount();
  });
  vi.useRealTimers();
});

describe('DeepSeek account lifecycle', () => {
  it('adds an account through browser login and clears a previously selected model', async () => {
    await mount(true);
    await act(async () => {
      await state.add();
    });
    expect(api.openExternal).toHaveBeenCalledWith('https://platform.deepseek.com/dsh/authorize');
    expect(clearModel).toHaveBeenCalled();
    expect(state.accounts[0].balances).toBeNull();
    expect(api.refreshDeepSeekAccountQuota).not.toHaveBeenCalled();
    expect(state.busy).toBe(false);
    await act(async () => {
      await state.refresh(state.accounts[0]);
    });
    expect(state.accounts[0].balances?.[0].amount).toBe(10);
  });
  it('cancels a late login initialization after leaving the tool', async () => {
    await mount();
    const waiting = deferred<api.DeepSeekLogin>();
    vi.mocked(api.startDeepSeekLogin).mockReturnValue(waiting.promise);
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
        verificationUri: 'https://platform.deepseek.com/dsh/authorize',
        expiresAt: Date.now() / 1000 + 600,
      });
      await operation;
    });
    expect(api.cancelDeepSeekLogin).toHaveBeenCalledWith('late');
    expect(api.openExternal).not.toHaveBeenCalled();
  });
  it('does not override a model selection made while authorization is pending', async () => {
    await mount();
    const waiting = deferred<api.DeepSeekAccount | null>();
    vi.mocked(api.pollDeepSeekLogin).mockReturnValue(waiting.promise);
    let operation!: Promise<void>;
    await act(async () => {
      operation = state.add();
    });
    act(() => {
      state.select(null);
    });
    await act(async () => {
      waiting.resolve(account);
      await operation;
    });
    expect(clearModel).not.toHaveBeenCalled();
    expect(state.selectedId).toBeNull();
  });
  it('does not resurrect an account deleted during a quota refresh', async () => {
    await mount();
    const waiting = deferred<api.DeepSeekAccount>();
    vi.mocked(api.refreshDeepSeekAccountQuota).mockReturnValue(waiting.promise);
    let refresh!: Promise<unknown>;
    act(() => {
      refresh = state.refresh(account);
    });
    await act(async () => {
      await state.remove(account);
    });
    await act(async () => {
      waiting.resolve({ ...account, balances: [] });
      await refresh;
    });
    expect(state.accounts).toEqual([]);
    expect(state.selectedId).toBeNull();
  });
});
