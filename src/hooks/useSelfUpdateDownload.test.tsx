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
  isUpdateDownloaded: vi.fn(),
  installDownloadedUpdate: vi.fn(),
  downloadUpdate: vi.fn(),
}));
vi.mock('../components/Toast', () => ({ useToast: () => ({ showToast: toast }) }));
vi.mock('./useI18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }));
let view: ReactTestRenderer;
let state: ReturnType<typeof useSelfUpdateDownload>;
let notify: Parameters<typeof api.onSelfUpdateProgress>[0];
let stop: ReturnType<typeof vi.fn<() => void>>;
function Harness({ version = '6.2.1' }: { version?: string } = {}) {
  const value = useSelfUpdateDownload(version);
  useLayoutEffect(() => {
    state = value;
  });
  return <span>{value.progress?.percent ?? 'idle'}</span>;
}
beforeEach(() => {
  vi.clearAllMocks();
  useNavigationStore.setState(useNavigationStore.getInitialState());
  vi.mocked(fetchLatestManifest).mockResolvedValue(null);
  vi.mocked(api.isUpdateDownloaded).mockResolvedValue(false);
  vi.mocked(api.installDownloadedUpdate).mockResolvedValue(true);
  stop = vi.fn();
  vi.mocked(api.onSelfUpdateProgress).mockImplementation(async (callback) => {
    notify = callback;
    return stop;
  });
  vi.mocked(api.downloadUpdate).mockResolvedValue(false);
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
  expect(api.isUpdateDownloaded).toHaveBeenCalledExactlyOnceWith('6.2.1');
  expect(api.installDownloadedUpdate).not.toHaveBeenCalled();
  expect(fetchLatestManifest).not.toHaveBeenCalled();
  expect(api.downloadUpdate).not.toHaveBeenCalled();
  expect(toast).not.toHaveBeenCalled();
});
it('downloads explicitly once, keeps progress through rerenders and releases listener on completion', async () => {
  let finish!: () => void;
  vi.mocked(api.downloadUpdate).mockReturnValueOnce(
    new Promise<boolean>((resolve) => {
      finish = () => resolve(false);
    })
  );
  await mount();
  await act(async () => {
    void state.activate('6.2.1');
    void state.activate('6.2.1');
  });
  expect(api.downloadUpdate).toHaveBeenCalledExactlyOnceWith('6.2.1');
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
  vi.mocked(api.downloadUpdate).mockReturnValueOnce(
    new Promise<boolean>((resolve) => {
      finishRetry = () => resolve(false);
    })
  );
  await act(async () => {
    void state.activate('6.2.1');
  });
  act(() => oldNotify({ status: 'downloading', percent: 99 }));
  expect(state.progress?.percent).toBe(0);
  await act(async () => finishRetry());
});
it('shows failure and permits explicit retry without opening a browser', async () => {
  vi.mocked(api.downloadUpdate).mockRejectedValueOnce(new Error('offline'));
  await mount();
  await act(async () => state.activate('6.2.1'));
  expect(toast).toHaveBeenCalledExactlyOnceWith('error', 'updates.failed');
  expect(state.progress).toBeNull();
  await act(async () => state.activate('6.2.1'));
  expect(api.downloadUpdate).toHaveBeenCalledTimes(2);
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
    void state.activate('6.2.1');
  });
  act(() => view.unmount());
  await act(async () => finish(stop));
  expect(stop).toHaveBeenCalledOnce();
  expect(api.downloadUpdate).not.toHaveBeenCalled();
});

it('releases an active subscription only once when unmounted during download', async () => {
  let finish!: () => void;
  vi.mocked(api.downloadUpdate).mockReturnValueOnce(
    new Promise<boolean>((resolve) => {
      finish = () => resolve(false);
    })
  );
  await mount();
  await act(async () => {
    void state.activate('6.2.1');
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
  await act(async () => state.activate('6.2.1'));
  expect(api.downloadUpdate).toHaveBeenCalledExactlyOnceWith('6.2.2');
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
    void state.activate('6.2.1');
  });
  act(() => view.unmount());
  await act(async () => finish({ version: '6.2.2' }));
  expect(api.downloadUpdate).not.toHaveBeenCalled();
  expect(useNavigationStore.getState().updateAvailable).toBeNull();
  expect(stop).toHaveBeenCalledOnce();
});

it('shows the downloaded release before launch and ignores duplicate confirmation clicks', async () => {
  const manifest = { version: '6.2.1', releaseNotes: 'Downloaded release notes' };
  vi.mocked(fetchLatestManifest).mockResolvedValueOnce(manifest);
  vi.mocked(api.downloadUpdate).mockResolvedValueOnce(true);
  await mount();
  await act(async () => state.activate('6.2.1'));
  expect(state.ready).toBe(true);
  expect(api.installDownloadedUpdate).not.toHaveBeenCalled();

  let finish!: (opened: boolean) => void;
  vi.mocked(api.installDownloadedUpdate).mockReturnValueOnce(
    new Promise((resolve) => {
      finish = resolve;
    })
  );
  await act(async () => state.activate('6.2.1'));
  expect(state.confirmation).toEqual({ version: '6.2.1', manifest });
  expect(state.progress).toBeNull();
  expect(api.installDownloadedUpdate).not.toHaveBeenCalled();

  act(() => state.cancelConfirmation());
  expect(state.confirmation).toBeNull();
  expect(state.ready).toBe(true);
  expect(api.installDownloadedUpdate).not.toHaveBeenCalled();

  await act(async () => state.activate('6.2.1'));
  await act(async () => {
    void state.confirmUpdate();
    void state.confirmUpdate();
    state.cancelConfirmation();
  });
  expect(state.progress?.status).toBe('launching');
  expect(state.confirmation?.version).toBe('6.2.1');
  expect(api.installDownloadedUpdate).toHaveBeenCalledExactlyOnceWith('6.2.1');
  await act(async () => finish(true));
  expect(state.confirmation).toBeNull();
  expect(api.downloadUpdate).toHaveBeenCalledExactlyOnceWith('6.2.1');
  expect(state.ready).toBe(true);
});

it('offers the cached package after reopening and returns to download if it disappears', async () => {
  vi.mocked(api.isUpdateDownloaded).mockResolvedValueOnce(true);
  vi.mocked(api.installDownloadedUpdate).mockResolvedValueOnce(false);
  await mount();
  expect(state.ready).toBe(true);
  expect(api.downloadUpdate).not.toHaveBeenCalled();
  await act(async () => state.activate('6.2.1'));
  expect(state.confirmation).toEqual({ version: '6.2.1' });
  expect(api.installDownloadedUpdate).not.toHaveBeenCalled();
  await act(async () => state.confirmUpdate());
  expect(state.ready).toBe(false);
  expect(state.confirmation).toBeNull();
  expect(api.installDownloadedUpdate).toHaveBeenCalledExactlyOnceWith('6.2.1');
  expect(api.downloadUpdate).not.toHaveBeenCalled();
});

it('does not offer a previously downloaded version for a new update', async () => {
  vi.mocked(api.isUpdateDownloaded).mockResolvedValueOnce(true);
  await mount();
  expect(state.ready).toBe(true);
  await act(async () => view.update(<Harness version="6.2.2" />));
  expect(state.ready).toBe(false);
  expect(api.isUpdateDownloaded).toHaveBeenLastCalledWith('6.2.2');
});

it('checks the new version when the previous download finishes', async () => {
  let finish!: (ready: boolean) => void;
  vi.mocked(api.downloadUpdate).mockReturnValueOnce(
    new Promise<boolean>((resolve) => {
      finish = resolve;
    })
  );
  await mount();
  await act(async () => {
    void state.activate('6.2.1');
  });
  await act(async () => view.update(<Harness version="6.2.2" />));
  expect(api.isUpdateDownloaded).toHaveBeenCalledExactlyOnceWith('6.2.1');

  vi.mocked(api.isUpdateDownloaded).mockResolvedValueOnce(true);
  await act(async () => finish(true));
  expect(api.isUpdateDownloaded).toHaveBeenLastCalledWith('6.2.2');
  expect(state.ready).toBe(true);
  await act(async () => state.activate('6.2.2'));
  expect(api.installDownloadedUpdate).not.toHaveBeenCalled();
  await act(async () => state.confirmUpdate());
  expect(api.installDownloadedUpdate).toHaveBeenCalledExactlyOnceWith('6.2.2');
});

it('confirms the displayed version even if a newer package becomes available', async () => {
  vi.mocked(api.isUpdateDownloaded).mockResolvedValue(true);
  await mount();
  await act(async () => state.activate('6.2.1'));
  await act(async () => view.update(<Harness version="6.2.2" />));
  expect(state.confirmation?.version).toBe('6.2.1');

  vi.mocked(api.installDownloadedUpdate).mockRejectedValueOnce(new Error('could not open'));
  await act(async () => state.confirmUpdate());
  expect(toast).toHaveBeenCalledExactlyOnceWith('error', 'updates.failed');
  expect(state.confirmation?.version).toBe('6.2.1');
  expect(state.progress).toBeNull();

  await act(async () => state.confirmUpdate());
  expect(vi.mocked(api.installDownloadedUpdate).mock.calls).toEqual([['6.2.1'], ['6.2.1']]);
  expect(state.ready).toBe(true);
  expect(state.confirmation).toBeNull();
});
