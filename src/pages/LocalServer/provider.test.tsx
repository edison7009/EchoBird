import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { LocalServerProvider } from './provider';
import * as api from '../../api/tauri';
import { useNavigationStore } from '../../stores/navigationStore';
import { useLocalServerStatusStore } from '../../stores/localServerStatusStore';
const visibility = vi.hoisted(() => ({ visible: true }));
vi.mock('../../hooks/useDocumentVisible', () => ({ useDocumentVisible: () => visibility.visible }));
vi.mock('../../api/tauri', () => ({
  getLlmServerInfo: vi.fn(),
  getModelsDirs: vi.fn(),
  scanGgufFiles: vi.fn(),
  scanHfModels: vi.fn(),
}));
let view: ReactTestRenderer;
beforeEach(() => {
  vi.useFakeTimers();
  vi.clearAllMocks();
  visibility.visible = true;
  vi.stubGlobal('window', Object.assign(new EventTarget(), { dispatchEvent: vi.fn() }));
  useNavigationStore.setState({ activePage: 'apps' });
  useLocalServerStatusStore.setState({ running: false });
  vi.mocked(api.getLlmServerInfo).mockResolvedValue({ running: false } as Awaited<
    ReturnType<typeof api.getLlmServerInfo>
  >);
  vi.mocked(api.getModelsDirs).mockResolvedValue(['/models']);
  vi.mocked(api.scanGgufFiles).mockResolvedValue([]);
});
afterEach(() => {
  act(() => view?.unmount());
  vi.useRealTimers();
  vi.unstubAllGlobals();
});
const mount = async () => {
  await act(async () => {
    view = create(
      <LocalServerProvider>
        <span />
      </LocalServerProvider>
    );
  });
};
it('uses one status observer on the desktop and defers disk scanning until first local visit', async () => {
  await mount();
  expect(api.getModelsDirs).not.toHaveBeenCalled();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_000);
  });
  expect(api.getLlmServerInfo).toHaveBeenCalledTimes(31);
  await act(async () => useNavigationStore.getState().setActivePage('localLlm'));
  expect(api.scanGgufFiles).toHaveBeenCalledOnce();
  await act(async () => useNavigationStore.getState().setActivePage('apps'));
  await act(async () => useNavigationStore.getState().setActivePage('localLlm'));
  expect(api.scanGgufFiles).toHaveBeenCalledOnce();
});
it('does not overlap slow requests and ignores a response after hiding', async () => {
  let resolve!: (value: Awaited<ReturnType<typeof api.getLlmServerInfo>>) => void;
  vi.mocked(api.getLlmServerInfo).mockReturnValue(
    new Promise((done) => {
      resolve = done;
    })
  );
  await mount();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(10_000);
  });
  expect(api.getLlmServerInfo).toHaveBeenCalledOnce();
  visibility.visible = false;
  await act(async () =>
    view.update(
      <LocalServerProvider>
        <span />
      </LocalServerProvider>
    )
  );
  await act(async () => {
    resolve({ running: true } as Awaited<ReturnType<typeof api.getLlmServerInfo>>);
  });
  expect(useLocalServerStatusStore.getState().running).toBe(false);
  expect(vi.getTimerCount()).toBe(0);
  vi.mocked(api.getLlmServerInfo).mockResolvedValue({ running: true } as Awaited<
    ReturnType<typeof api.getLlmServerInfo>
  >);
  visibility.visible = true;
  await act(async () =>
    view.update(
      <LocalServerProvider>
        <span />
      </LocalServerProvider>
    )
  );
  expect(api.getLlmServerInfo).toHaveBeenCalledTimes(2);
  expect(useLocalServerStatusStore.getState().running).toBe(true);
});

it('does not let an old poll undo an explicit start or stop', async () => {
  let resolve!: (value: Awaited<ReturnType<typeof api.getLlmServerInfo>>) => void;
  vi.mocked(api.getLlmServerInfo).mockReturnValue(
    new Promise((done) => {
      resolve = done;
    })
  );
  await mount();
  act(() => useLocalServerStatusStore.getState().setRunning(true));
  await act(async () =>
    resolve({ running: false } as Awaited<ReturnType<typeof api.getLlmServerInfo>>)
  );
  expect(useLocalServerStatusStore.getState().running).toBe(true);
});

it('performs no status polling or disk scans while disabled, including after a previous visit', async () => {
  const render = (enabled: boolean) => (
    <LocalServerProvider enabled={enabled}>
      <span />
    </LocalServerProvider>
  );
  await act(async () => {
    view = create(render(false));
  });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_000);
  });
  expect(api.getLlmServerInfo).not.toHaveBeenCalled();
  expect(api.getModelsDirs).not.toHaveBeenCalled();
  expect(vi.getTimerCount()).toBe(0);
  await act(async () => {
    view.update(render(true));
  });
  expect(api.getLlmServerInfo).toHaveBeenCalledOnce();
  await act(async () => useNavigationStore.getState().setActivePage('localLlm'));
  expect(api.scanGgufFiles).toHaveBeenCalledOnce();
  await act(async () => {
    view.update(render(false));
  });
  const reads = vi.mocked(api.getLlmServerInfo).mock.calls.length;
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_000);
  });
  expect(api.getLlmServerInfo).toHaveBeenCalledTimes(reads);
  expect(vi.getTimerCount()).toBe(0);
  await act(async () => useNavigationStore.getState().setActivePage('apps'));
  await act(async () => {
    view.update(render(true));
  });
  expect(api.scanGgufFiles).toHaveBeenCalledOnce();
});

it('does not scan another directory or publish a late status after disabling during pending work', async () => {
  useNavigationStore.setState({ activePage: 'localLlm' });
  vi.mocked(api.getModelsDirs).mockResolvedValue(['/first', '/second']);
  let finishScan!: (files: []) => void;
  let finishStatus!: (info: Awaited<ReturnType<typeof api.getLlmServerInfo>>) => void;
  vi.mocked(api.scanGgufFiles).mockReturnValueOnce(
    new Promise((resolve) => {
      finishScan = resolve;
    })
  );
  vi.mocked(api.getLlmServerInfo).mockReturnValueOnce(
    new Promise((resolve) => {
      finishStatus = resolve;
    })
  );
  await mount();
  expect(api.scanGgufFiles).toHaveBeenCalledExactlyOnceWith('/first');
  await act(async () => {
    view.update(
      <LocalServerProvider enabled={false}>
        <span />
      </LocalServerProvider>
    );
  });
  await act(async () => {
    finishScan([]);
    finishStatus({ running: true } as Awaited<ReturnType<typeof api.getLlmServerInfo>>);
  });
  expect(api.scanGgufFiles).toHaveBeenCalledOnce();
  expect(useLocalServerStatusStore.getState().running).toBe(false);
  expect(vi.getTimerCount()).toBe(0);
});
