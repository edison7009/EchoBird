import type { ReactNode } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { ConfigBackupActions, ConfigBackupStatus } from './ConfigBackupActions';
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
  restartRequired: false,
  duplicates: 1,
  skipped: 0,
  deviceBound: 0,
  fileName: 'fixture.ebbackup',
  uiPreferences: {},
};
const storage = { getItem: vi.fn(), setItem: vi.fn() };
beforeEach(() => {
  vi.resetAllMocks();
  mocks.invoke.mockRejectedValue('backupError.passwordRequired');
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
it.each(['', '1', 'ab', 'fixture-password'])(
  'accepts optional backup password %j without retaining it in shared state',
  async (password) => {
    mocks.open.mockResolvedValue('/fixture/destination');
    mocks.invoke.mockResolvedValue(outcome);
    await mount();
    await act(async () => action('backup.action').props.onClick());
    expect(view.root.findAllByType('input')).toHaveLength(1);
    input(0, password);
    await submit();
    expect(mocks.invoke).toHaveBeenCalledExactlyOnceWith('create_config_backup', {
      directory: '/fixture/destination',
      password,
      uiPreferences: {},
    });
    expect(useConfigBackupStore.getState().phase).toBe('idle');
    expect(useConfigBackupStore.getState()).not.toHaveProperty('password');
    expect(view.root.findAllByType('input')).toHaveLength(0);
  }
);
it('restores only after password input, supports retry, and invalidates only saved data', async () => {
  mocks.open.mockResolvedValue('/fixture/source.ebbackup');
  mocks.invoke
    .mockRejectedValueOnce('backupError.passwordRequired')
    .mockRejectedValueOnce('backupError.password')
    .mockResolvedValueOnce({
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
  expect(mocks.invoke).toHaveBeenCalledExactlyOnceWith('restore_config_backup', {
    filePath: '/fixture/source.ebbackup',
    password: '',
  });
  expect(useConfigBackupStore.getState().error).toBeNull();
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

it.each(['backup', 'restore'] as const)(
  'uses the shared dialog layout and visible password inputs for %s',
  async (mode) => {
    mocks.open.mockResolvedValue(mode === 'backup' ? '/fixture-backups' : '/fixture.ebbackup');
    await mount();
    await act(async () =>
      action(mode === 'backup' ? 'backup.action' : 'backup.restore').props.onClick()
    );
    const inputs = view.root.findAllByType('input');
    expect(inputs).toHaveLength(1);
    expect(
      inputs.every((node) => node.props.type === 'text' && node.props.autoComplete === 'off')
    ).toBe(true);
    const dialog = view.root.findByProps({ role: 'dialog' });
    expect(dialog.props.className).toContain('bg-cyber-surface');
    const footerButtons = view.root.findByType('form').findAllByType('button');
    expect(footerButtons).toHaveLength(2);
    expect(footerButtons.every((button) => button.props.className.includes('flex-1'))).toBe(true);
    expect(footerButtons[0].parent!.props.className).toContain('border-t');
    expect(JSON.stringify(view.toJSON())).not.toContain('lucide-eye');
    const close = view.root
      .findAllByType('button')
      .find((button) => button.props['aria-label'] === 'btn.close')!;
    expect(close).toBeDefined();
    await act(async () => close.props.onClick());
    expect(useConfigBackupStore.getState().phase).toBe('idle');
    expect(useConfigBackupStore.getState().challenge).toBeNull();
    expect(mocks.invoke).toHaveBeenCalledTimes(mode === 'restore' ? 1 : 0);
  }
);

it.each([
  ['backup', 'backup.invalidConfiguration'],
  ['restore', 'backup.invalidFile'],
] as const)(
  'distinguishes %s configuration errors from a malformed restore file',
  async (mode, message) => {
    mocks.open.mockResolvedValue(mode === 'backup' ? '/fixture-backups' : '/fixture.ebbackup');
    if (mode === 'restore') mocks.invoke.mockRejectedValueOnce('backupError.passwordRequired');
    mocks.invoke.mockRejectedValueOnce('backupError.format');
    await mount();
    await act(async () =>
      action(mode === 'backup' ? 'backup.action' : 'backup.restore').props.onClick()
    );
    await act(async () => useConfigBackupStore.getState().run('fixture-password'));
    expect(view.root.findByProps({ role: 'alert' }).children).toEqual([message]);
    expect(useConfigBackupStore.getState().phase).toBe('password');
    expect(useConfigBackupStore.getState().challenge?.mode).toBe(mode);
  }
);

it('restores an unencrypted file immediately without displaying a password dialog', async () => {
  mocks.open.mockResolvedValue('/fixture/plain.ebbackup');
  mocks.invoke.mockResolvedValue(outcome);
  await mount();
  await act(async () => action('backup.restore').props.onClick());
  expect(mocks.invoke).toHaveBeenCalledExactlyOnceWith('restore_config_backup', {
    filePath: '/fixture/plain.ebbackup',
    password: '',
  });
  expect(view.root.findAllByProps({ role: 'dialog' })).toHaveLength(0);
  expect(useConfigBackupStore.getState().phase).toBe('idle');
});
it.each([false, true])(
  'reports actual UI setting changes and restart requirement: %s',
  async (changed) => {
    mocks.open.mockResolvedValue('/fixture/plain.ebbackup');
    storage.getItem.mockReturnValue(changed ? 'carbon' : 'none');
    mocks.invoke.mockResolvedValue({
      ...outcome,
      records: 0,
      duplicates: 16,
      uiPreferences: { echobird_background: 'none' },
    });
    await mount();
    await act(async () => action('backup.restore').props.onClick());
    expect(useConfigBackupStore.getState().outcome).toMatchObject({
      records: changed ? 1 : 0,
      duplicates: changed ? 16 : 17,
      restartRequired: changed,
    });
    expect(storage.setItem).toHaveBeenCalledTimes(changed ? 1 : 0);
    await act(async () => view.update(<ConfigBackupStatus />));
    expect(JSON.stringify(view.toJSON()).includes('backup.restartRequired')).toBe(changed);
  }
);
it('does not claim failed UI settings were already present or restored', async () => {
  mocks.open.mockResolvedValue('/fixture/plain.ebbackup');
  storage.getItem.mockReturnValue('carbon');
  storage.setItem.mockImplementation(() => {
    throw new Error('storage unavailable');
  });
  mocks.invoke.mockResolvedValue({
    ...outcome,
    records: 0,
    duplicates: 16,
    uiPreferences: { echobird_background: 'none' },
  });
  await mount();
  await act(async () => action('backup.restore').props.onClick());
  expect(useConfigBackupStore.getState().outcome).toMatchObject({
    records: 0,
    duplicates: 16,
    skipped: 1,
    restartRequired: false,
  });
});
