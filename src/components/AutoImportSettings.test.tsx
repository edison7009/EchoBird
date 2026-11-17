import { act, create } from 'react-test-renderer';
import { beforeEach, expect, it, vi } from 'vitest';
import { AutoImportSettings } from './AutoImportSettings';
import { useAutoImportStore, type ImportProgress } from '../stores/autoImportStore';

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), invalidate: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({
  invoke: mocks.invoke,
  Channel: class {
    onmessage?: (value: ImportProgress) => void;
  },
}));
vi.mock('../stores/modelRepository', () => ({ modelRepository: { invalidate: mocks.invalidate } }));
vi.mock('../hooks/useI18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }));

const progress: ImportProgress = {
  completed: 1,
  total: 8,
  source: '9router',
  models: 2,
  accounts: 1,
  duplicates: 3,
  issues: [
    { source: 'magpie', reason: 'unsupported', count: 1 },
    { source: 'CLIProxyAPI', reason: 'readFailed', count: 1 },
  ],
};

beforeEach(() => {
  vi.clearAllMocks();
  useAutoImportStore.setState({ running: false, progress: null, error: false, revision: 0 });
});

it('does not scan on entry and keeps the running task across unmounts', async () => {
  let resolve!: (value: ImportProgress) => void;
  mocks.invoke.mockReturnValue(
    new Promise<ImportProgress>((done) => {
      resolve = done;
    })
  );
  let view!: ReturnType<typeof create>;
  await act(async () => {
    view = create(<AutoImportSettings />);
  });
  expect(mocks.invoke).not.toHaveBeenCalled();
  expect(JSON.stringify(view.toJSON())).toContain('autoImport.description');
  await act(async () => {
    view.root
      .findAllByType('button')
      .find((button) => button.props.children === 'settings.autoImport')!
      .props.onClick();
  });
  expect(mocks.invoke).toHaveBeenCalledTimes(1);
  expect(mocks.invoke.mock.calls[0][0]).toBe('auto_import');
  expect(JSON.stringify(view.toJSON())).not.toContain('autoImport.description');
  const channel = mocks.invoke.mock.calls[0][1].onProgress;
  act(() => channel.onmessage(progress));
  expect(view.root.findByProps({ role: 'progressbar' }).props['aria-valuenow']).toBe(1 / 8);
  expect(JSON.stringify(view.toJSON())).not.toMatch(/9router|magpie|CLIProxyAPI/);
  await useAutoImportStore.getState().start();
  expect(mocks.invoke).toHaveBeenCalledTimes(1);
  act(() => view.unmount());
  await act(async () => {
    view = create(<AutoImportSettings />);
  });
  expect(view.root.findAllByType('button').every((button) => button.props.disabled)).toBe(true);
  await act(async () => {
    resolve({ ...progress, completed: 8 });
  });
  expect(useAutoImportStore.getState().running).toBe(false);
  expect(mocks.invalidate).toHaveBeenCalledOnce();
  expect(useAutoImportStore.getState().revision).toBe(1);
  expect(JSON.stringify(view.toJSON())).not.toMatch(
    /9router|magpie|CLIProxyAPI|unsupported|readFailed/
  );
  expect(JSON.stringify(view.toJSON())).toContain('autoImport.skipped');
  expect(JSON.stringify(view.toJSON())).not.toContain('autoImport.description');
  expect(view.root.findAllByProps({ role: 'status' })).toHaveLength(1);
  expect(view.root.findAllByProps({ role: 'alert' })).toHaveLength(0);
  act(() => channel.onmessage(progress));
  expect(useAutoImportStore.getState().progress?.completed).toBe(8);
  act(() => view.unmount());
});

it('retains partial results on failure and permits an explicit retry', async () => {
  mocks.invoke.mockImplementation(async (_command, args) => {
    args.onProgress.onmessage(progress);
    throw new Error('synthetic failure');
  });
  await useAutoImportStore.getState().start();
  expect(useAutoImportStore.getState()).toMatchObject({ running: false, error: true, progress });
  mocks.invoke.mockResolvedValue({ ...progress, completed: 8 });
  await useAutoImportStore.getState().start();
  expect(useAutoImportStore.getState().error).toBe(false);
  expect(mocks.invoke).toHaveBeenCalledTimes(2);
});
