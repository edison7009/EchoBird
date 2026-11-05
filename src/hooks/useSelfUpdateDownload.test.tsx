import { useLayoutEffect } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import * as api from '../api/tauri';
import { useSelfUpdateDownload } from './useSelfUpdateDownload';
import { fetchLatestManifest } from '../services/updateManifest';
import { useNavigationStore } from '../stores/navigationStore';
vi.mock('../services/updateManifest', async (original) => ({
  ...(await original<typeof import('../services/updateManifest')>()),
  fetchLatestManifest: vi.fn(),
}));
const toast = vi.hoisted(() => vi.fn());
vi.mock('../api/tauri', () => ({
  onSelfUpdateProgress: vi.fn(),
  downloadAndInstallUpdate: vi.fn(),
}));
vi.mock('../components/Toast', () => ({ useToast: () => ({ showToast: toast }) }));
vi.mock('./useI18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }));
let view: ReactTestRenderer;
let state: ReturnType<typeof useSelfUpdateDownload>;
let notify: Parameters<typeof api.onSelfUpdateProgress>[0];
let stop: ReturnType<typeof vi.fn<() => void>>;
function Harness() {
  const value = useSelfUpdateDownload();
  useLayoutEffect(() => {
    state = value;
  });
  return <span>{value.progress?.percent ?? 'idle'}</span>;
}
beforeEach(() => {
  vi.clearAllMocks();
  useNavigationStore.setState(useNavigationStore.getInitialState());
  vi.mocked(fetchLatestManifest).mockResolvedValue(null);
  stop = vi.fn();
  vi.mocked(api.onSelfUpdateProgress).mockImplementation(async (callback) => {
    notify = callback;
    return stop;
  });
  vi.mocked(api.downloadAndInstallUpdate).mockResolvedValue(undefined);
});
afterEach(() => act(() => view?.unmount()));
const mount = async () => {
  await act(async () => {
    view = create(<Harness />);
  });
};
it('does not subscribe, download or show feedback during passive renders', async () => {
  await mount();
  await act(async () => view.update(<Harness />));
  expect(api.onSelfUpdateProgress).not.toHaveBeenCalled();
  expect(fetchLatestManifest).not.toHaveBeenCalled();
  expect(api.downloadAndInstallUpdate).not.toHaveBeenCalled();
  expect(toast).not.toHaveBeenCalled();
});
it('downloads explicitly once, keeps progress through rerenders and releases listener on completion', async () => {
  let finish!: () => void;
  vi.mocked(api.downloadAndInstallUpdate).mockReturnValueOnce(
    new Promise<void>((resolve) => {
      finish = resolve;
    })
  );
  await mount();
  await act(async () => {
    void state.download('6.2.1');
    void state.download('6.2.1');
  });
  expect(api.downloadAndInstallUpdate).toHaveBeenCalledExactlyOnceWith('6.2.1');
  act(() => notify({ status: 'downloading', percent: 42 }));
  await act(async () => view.update(<Harness />));
  expect(state.progress?.percent).toBe(42);
  await act(async () => finish());
  expect(state.progress).toBeNull();
  expect(stop).toHaveBeenCalledOnce();
  const oldNotify = notify;
  act(() => oldNotify({ status: 'downloading', percent: 99 }));
  expect(state.progress).toBeNull();
  let finishRetry!: () => void;
  vi.mocked(api.downloadAndInstallUpdate).mockReturnValueOnce(
    new Promise<void>((resolve) => {
      finishRetry = resolve;
    })
  );
  await act(async () => {
    void state.download('6.2.1');
  });
  act(() => oldNotify({ status: 'downloading', percent: 99 }));
  expect(state.progress?.percent).toBe(0);
  await act(async () => finishRetry());
});
it('shows failure and permits explicit retry without opening a browser', async () => {
  vi.mocked(api.downloadAndInstallUpdate).mockRejectedValueOnce(new Error('offline'));
  await mount();
  await act(async () => state.download('6.2.1'));
  expect(toast).toHaveBeenCalledExactlyOnceWith('error', 'updates.failed');
  expect(state.progress).toBeNull();
  await act(async () => state.download('6.2.1'));
  expect(api.downloadAndInstallUpdate).toHaveBeenCalledTimes(2);
});
it('does not start a download when listener registration completes after unmount', async () => {
  let finish!: (stop: () => void) => void;
  vi.mocked(api.onSelfUpdateProgress).mockReturnValueOnce(
    new Promise((resolve) => {
      finish = resolve;
    })
  );
  await mount();
  act(() => {
    void state.download('6.2.1');
  });
  act(() => view.unmount());
  await act(async () => finish(stop));
  expect(stop).toHaveBeenCalledOnce();
  expect(api.downloadAndInstallUpdate).not.toHaveBeenCalled();
});

it('releases an active subscription only once when unmounted during download', async () => {
  let finish!: () => void;
  vi.mocked(api.downloadAndInstallUpdate).mockReturnValueOnce(
    new Promise<void>((resolve) => {
      finish = resolve;
    })
  );
  await mount();
  await act(async () => {
    void state.download('6.2.1');
  });
  act(() => view.unmount());
  expect(stop).toHaveBeenCalledOnce();
  await act(async () => {
    notify({ status: 'downloaded', percent: 100 });
    finish();
  });
  expect(stop).toHaveBeenCalledOnce();
  expect(toast).not.toHaveBeenCalled();
});

it('refreshes a stale version notification only after an explicit click', async () => {
  useNavigationStore.getState().setUpdateAvailable('6.2.1');
  vi.mocked(fetchLatestManifest).mockResolvedValueOnce({ version: '6.2.2' });
  await mount();
  expect(fetchLatestManifest).not.toHaveBeenCalled();
  await act(async () => state.download('6.2.1'));
  expect(api.downloadAndInstallUpdate).toHaveBeenCalledExactlyOnceWith('6.2.2');
  expect(useNavigationStore.getState().updateAvailable).toBe('6.2.2');
});
it('ignores a version response after unmount without starting a download', async () => {
  let finish!: (value: { version: string }) => void;
  vi.mocked(fetchLatestManifest).mockReturnValueOnce(
    new Promise((resolve) => {
      finish = resolve;
    })
  );
  await mount();
  await act(async () => {
    void state.download('6.2.1');
  });
  act(() => view.unmount());
  await act(async () => finish({ version: '6.2.2' }));
  expect(api.downloadAndInstallUpdate).not.toHaveBeenCalled();
  expect(useNavigationStore.getState().updateAvailable).toBeNull();
  expect(stop).toHaveBeenCalledOnce();
});
