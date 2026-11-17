import type { ReactNode } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { ConfigBackupActions } from './ConfigBackupActions';
import { useConfigBackupStore, type BackupOutcome } from '../stores/configBackupStore';
import { useAutoImportStore } from '../stores/autoImportStore';
const mocks = vi.hoisted(() => ({ open: vi.fn(), invoke: vi.fn(), invalidate: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke, Channel: class {} }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: mocks.open }));
vi.mock('react-dom', () => ({ createPortal: (node: ReactNode) => node }));
vi.mock('../stores/modelRepository', () => ({ modelRepository: { invalidate: mocks.invalidate } }));
vi.mock('../hooks/useI18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }));
let view: ReactTestRenderer;
const outcome: BackupOutcome = {
  records: 4,
  duplicates: 1,
  skipped: 0,
  deviceBound: 0,
  fileName: 'fixture.ebbackup',
  uiPreferences: {},
};
const storage = { getItem: vi.fn(), setItem: vi.fn() };
beforeEach(() => {
  vi.clearAllMocks();
  vi.stubGlobal('document', { activeElement: null, body: {} });
  vi.stubGlobal('HTMLElement', class {});
  vi.stubGlobal('localStorage', storage);
  storage.getItem.mockReturnValue(null);
  useAutoImportStore.setState({ running: false, progress: null, error: false, revision: 0 });
  useConfigBackupStore.setState({
    phase: 'idle',
    challenge: null,
    outcome: null,
    error: null,
    mode: null,
    revision: 0,
  });
});
afterEach(() => {
  if (view) act(() => view.unmount());
  vi.unstubAllGlobals();
});
function action(label: string) {
  return view.root.findAllByType('button').find((button) => button.props.children === label)!;
}
async function mount() {
  await act(async () => {
    view = create(<ConfigBackupActions />);
  });
}
function input(index: number, value: string) {
  act(() => view.root.findAllByType('input')[index].props.onChange({ target: { value } }));
}
async function submit() {
  await act(async () => view.root.findByType('form').props.onSubmit({ preventDefault: vi.fn() }));
}
it('opens no dialogs on entry and leaves cancellation without a backup or error', async () => {
  await mount();
  expect(mocks.open).not.toHaveBeenCalled();
  expect(mocks.invoke).not.toHaveBeenCalled();
  mocks.open.mockResolvedValue(null);
  await act(async () => action('backup.action').props.onClick());
  expect(mocks.open).toHaveBeenCalledWith(
    expect.objectContaining({ directory: true, multiple: false })
  );
  expect(mocks.invoke).not.toHaveBeenCalled();
  expect(useConfigBackupStore.getState().error).toBeNull();
});
it('requires matching backup passwords and never retains the password in shared state', async () => {
  mocks.open.mockResolvedValue('/fixture/destination');
  mocks.invoke.mockResolvedValue(outcome);
  await mount();
  await act(async () => action('backup.action').props.onClick());
  input(0, 'fixture-password');
  input(1, 'different-password');
  await submit();
  expect(mocks.invoke).not.toHaveBeenCalled();
  expect(view.root.findByProps({ role: 'alert' }).props.children).toBe('backup.passwordMismatch');
  input(1, 'fixture-password');
  await submit();
  expect(mocks.invoke).toHaveBeenCalledExactlyOnceWith('create_config_backup', {
    directory: '/fixture/destination',
    password: 'fixture-password',
    uiPreferences: {},
  });
  expect(useConfigBackupStore.getState().phase).toBe('idle');
  expect(JSON.stringify(useConfigBackupStore.getState())).not.toContain('fixture-password');
});
it('restores only after password input, supports retry, and invalidates only saved data', async () => {
  mocks.open.mockResolvedValue('/fixture/source.ebbackup');
  mocks.invoke.mockRejectedValueOnce('backupError.password').mockResolvedValueOnce({
    ...outcome,
    uiPreferences: { echobird_background: 'none', unrelated: 'not restored' },
  });
  await mount();
  await act(async () => action('backup.restore').props.onClick());
  expect(mocks.open).toHaveBeenCalledWith(
    expect.objectContaining({
      directory: false,
      filters: [{ name: 'EchoBird', extensions: ['ebbackup'] }],
    })
  );
  expect(mocks.invoke).not.toHaveBeenCalled();
  input(0, 'wrong-password');
  await submit();
  expect(useConfigBackupStore.getState().phase).toBe('password');
  expect(mocks.invalidate).not.toHaveBeenCalled();
  expect(storage.setItem).not.toHaveBeenCalled();
  input(0, 'fixture-password');
  await submit();
  expect(mocks.invoke).toHaveBeenLastCalledWith('restore_config_backup', {
    filePath: '/fixture/source.ebbackup',
    password: 'fixture-password',
  });
  expect(mocks.invalidate).toHaveBeenCalledOnce();
  expect(useAutoImportStore.getState().revision).toBe(1);
  expect(storage.setItem).toHaveBeenCalledExactlyOnceWith('echobird_background', 'none');
});
it('ignores a late picker result after leaving settings and blocks operations during import', async () => {
  let resolve!: (value: string) => void;
  mocks.open.mockReturnValue(
    new Promise<string>((done) => {
      resolve = done;
    })
  );
  await mount();
  let choosing!: Promise<void>;
  act(() => {
    choosing = useConfigBackupStore.getState().choose('backup', 'title');
  });
  act(() => view.unmount());
  await act(async () => {
    resolve('/fixture/late');
    await choosing;
  });
  expect(useConfigBackupStore.getState().challenge).toBeNull();
  expect(mocks.invoke).not.toHaveBeenCalled();
  useAutoImportStore.setState({ running: true });
  await useConfigBackupStore.getState().choose('restore', 'title');
  expect(mocks.open).toHaveBeenCalledTimes(1);
});
