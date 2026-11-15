import { useLayoutEffect, useRef } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import * as api from '../../api/commandcode';
import { startTool } from '../../api/tauri';
import { useCommandCodeAccounts } from '../../pages/AppManager/useCommandCodeAccounts';
import { useApplicationLauncher } from './useApplicationLauncher';

vi.mock('../../api/commandcode', () => ({
  listCommandCodeAccounts: vi.fn(),
  switchCommandCodeAccount: vi.fn(),
  refreshCommandCodeAccount: vi.fn(),
  deleteCommandCodeAccount: vi.fn(),
  startCommandCodeLogin: vi.fn(),
  pollCommandCodeLogin: vi.fn(),
  cancelCommandCodeLogin: vi.fn(),
}));
vi.mock('../../api/tauri', () => ({ startTool: vi.fn() }));
vi.mock('@tauri-apps/plugin-shell', () => ({ open: vi.fn().mockResolvedValue(undefined) }));
vi.mock('../../components/ConfirmDialog', () => ({ useConfirm: () => async () => true }));
vi.mock('../../hooks/useI18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }));

let renderer: ReactTestRenderer;
let accounts: ReturnType<typeof useCommandCodeAccounts>;
let launch: ReturnType<typeof useApplicationLauncher>;
const report = vi.fn();
const saved = [
  { id: 'a', email: 'a@test', active: true },
  { id: 'b', email: 'b@test', active: false },
];
function Harness() {
  const group = useCommandCodeAccounts(true, false, vi.fn(), report);
  const options = {
    selectedTool: 'commandcode',
    detectedTools: [{ id: 'commandcode', installed: true }],
    commandCodeAccounts: group,
    accountNavigation: useRef({ tool: 'commandcode', page: 'apps' }),
    launchGeneration: useRef(0),
    previewLaunch: useRef(false),
    toolModelConfig: {},
    setIsLaunching: vi.fn(),
    setApplyError: report,
    launchAfterApply: true,
    t: (key: string) => key,
  } as unknown as Parameters<typeof useApplicationLauncher>[0];
  const run = useApplicationLauncher(options);
  useLayoutEffect(() => {
    accounts = group;
    launch = run;
  });
  return null;
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(api.listCommandCodeAccounts).mockResolvedValue(saved);
  vi.mocked(startTool).mockResolvedValue(undefined);
});
afterEach(() => {
  act(() => renderer?.unmount());
  vi.useRealTimers();
});

it.each([false, true])(
  'keeps pending CommandCode apply isolated from selection change=%s',
  async (change) => {
    let complete!: (account: (typeof saved)[number]) => void;
    vi.mocked(api.switchCommandCodeAccount).mockImplementation(
      () =>
        new Promise((resolve) => {
          complete = resolve;
        })
    );
    await act(async () => {
      renderer = create(<Harness />);
    });
    act(() => accounts.select('a'));
    let operation!: Promise<void>;
    await act(async () => {
      operation = launch();
    });
    expect(api.switchCommandCodeAccount).toHaveBeenCalledExactlyOnceWith('a');
    expect(startTool).not.toHaveBeenCalled();
    if (change) act(() => accounts.select('b'));
    await act(async () => {
      complete(saved[0]);
      await operation;
    });
    if (change) {
      expect(startTool).not.toHaveBeenCalled();
      expect(accounts.selectedId).toBe('b');
    } else expect(startTool).toHaveBeenCalledExactlyOnceWith('commandcode');
    expect(report).not.toHaveBeenCalled();
  }
);

it('preserves the ten-minute browser-login window and explicit cancellation', async () => {
  vi.useFakeTimers();
  vi.mocked(api.startCommandCodeLogin).mockResolvedValue({
    loginId: 'pending',
    authorizationUrl: 'https://fixture.test/login',
    expiresAt: Math.floor(Date.now() / 1000) + 600,
  });
  vi.mocked(api.pollCommandCodeLogin).mockResolvedValue(null);
  vi.mocked(api.cancelCommandCodeLogin).mockResolvedValue(undefined);
  await act(async () => {
    renderer = create(<Harness />);
  });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(0);
  });
  let login!: Promise<void>;
  await act(async () => {
    login = accounts.add();
  });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(61_000);
  });
  expect(accounts.busy).toBe(true);
  expect(accounts.remainingSeconds).toBeGreaterThan(500);
  expect(api.cancelCommandCodeLogin).not.toHaveBeenCalled();
  act(() => accounts.cancelLogin());
  await act(async () => {
    await vi.advanceTimersByTimeAsync(1_000);
    await login;
  });
  expect(api.cancelCommandCodeLogin).toHaveBeenCalledExactlyOnceWith('pending');
  expect(accounts.busy).toBe(false);
  expect(report).not.toHaveBeenCalled();
});
