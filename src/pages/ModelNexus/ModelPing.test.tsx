import { useLayoutEffect } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import * as api from '../../api/tauri';
import { ModelCard } from '../../components/cards/ModelCard';
import { ModelNexusProvider } from './provider';
import { useModelNexus } from './context';
import { useNavigationStore } from '../../stores/navigationStore';
import { OnFirstVisit } from '../../components/OnFirstVisit';

vi.mock('../../hooks/useI18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }));
vi.mock('../../components/Toast', () => ({ useToast: () => ({ showToast: vi.fn() }) }));
vi.mock('../../components/ConfirmDialog', () => ({ useConfirm: () => vi.fn() }));
vi.mock('../../components/LatencyIndicator', () => ({ LatencyIndicator: () => null }));
vi.mock('../../api/tauri', () => ({
  getModels: vi.fn(),
  pingModel: vi.fn(),
  finishConnectionTestBatch: vi.fn().mockResolvedValue(undefined),
  queryModelUsage: vi.fn(),
  updateModel: vi.fn(),
}));

let renderer: ReactTestRenderer | undefined;
let context: ReturnType<typeof useModelNexus>;
function Harness() {
  const state = useModelNexus();
  useLayoutEffect(() => {
    context = state;
  });
  return (
    <ModelCard id="fixture" name="Fixture" type="cloud" latency={state.modelLatencies.fixture} />
  );
}
beforeEach(() => {
  useNavigationStore.setState({ activePage: 'models' });
  vi.mocked(api.getModels).mockResolvedValue([
    { internalId: 'fixture', name: 'Fixture', baseUrl: 'https://fixture.test/v1', apiKey: '' },
  ]);
});

it('continues queued model speed tests while another page is shown and retains results on return', async () => {
  const replies: ((result: Awaited<ReturnType<typeof api.pingModel>>) => void)[] = [];
  vi.mocked(api.getModels).mockResolvedValue([
    { internalId: 'fixture', name: 'Fixture', baseUrl: 'https://fixture.test/v1', apiKey: '' },
    { internalId: 'second', name: 'Second', baseUrl: 'https://second.test/v1', apiKey: '' },
  ]);
  vi.mocked(api.pingModel).mockImplementation(
    () => new Promise((resolve) => replies.push(resolve))
  );
  function ModelPage() {
    const active = useNavigationStore((state) => state.activePage === 'models');
    return (
      <div hidden={!active}>
        <OnFirstVisit active={active}>
          <Harness />
        </OnFirstVisit>
      </div>
    );
  }
  await act(async () => {
    renderer = create(
      <ModelNexusProvider>
        <ModelPage />
      </ModelNexusProvider>
    );
  });
  expect(api.pingModel).not.toHaveBeenCalled();
  let round!: Promise<void>;
  await act(async () => {
    round = context.pingAllModels();
  });
  const batch = vi.mocked(api.pingModel).mock.calls[0][1];
  await act(async () => useNavigationStore.getState().setActivePage('freeModels'));
  expect(context.isTesting).toBe(true);
  expect(api.finishConnectionTestBatch).not.toHaveBeenCalled();
  await act(async () => {
    replies[0]({ success: true, latency: 123, url: 'https://fixture.test' });
  });
  expect(api.pingModel).toHaveBeenNthCalledWith(2, 'second', batch);
  await act(async () => useNavigationStore.getState().setActivePage('models'));
  expect(context.pingingModelIds).toEqual(new Set(['second']));
  expect(context.modelLatencies.fixture).toBe(123);
  await act(async () => context.pingAllModels());
  expect(api.pingModel).toHaveBeenCalledTimes(2);
  await act(async () => useNavigationStore.getState().setActivePage('apps'));
  await act(async () => {
    replies[1]({ success: false, latency: -1, error: 'timeout', url: 'https://second.test' });
    await round;
  });
  await act(async () => useNavigationStore.getState().setActivePage('models'));
  expect(context.modelLatencies).toEqual({ fixture: 123, second: -2 });
  expect(context.isTesting).toBe(false);
  expect(context.pingingModelIds.size).toBe(0);
  expect(api.pingModel).toHaveBeenCalledTimes(2);
  expect(api.finishConnectionTestBatch).toHaveBeenCalledExactlyOnceWith(batch);
  expect(api.queryModelUsage).not.toHaveBeenCalled();
  expect(api.updateModel).not.toHaveBeenCalled();
});
afterEach(() => {
  act(() => renderer?.unmount());
  vi.clearAllMocks();
  vi.useRealTimers();
});

it('does not rerender the model tree on a timer while a request is pending and retains off-page results', async () => {
  vi.useFakeTimers();
  let renders = 0;
  function ModelTree() {
    useModelNexus();
    renders++;
    return <Harness />;
  }
  let finish!: (value: Awaited<ReturnType<typeof api.pingModel>>) => void;
  vi.mocked(api.pingModel).mockReturnValueOnce(new Promise((resolve) => (finish = resolve)));
  await act(async () => {
    renderer = create(
      <ModelNexusProvider>
        <ModelTree />
      </ModelNexusProvider>
    );
  });
  let request!: Promise<void>;
  act(() => {
    request = context.pingAllModels();
  });
  const waiting = renders;
  await act(async () => {
    vi.advanceTimersByTime(2000);
  });
  expect(renders).toBe(waiting);
  await act(async () => useNavigationStore.getState().setActivePage('apps'));
  const hidden = renders;
  await act(async () => {
    vi.advanceTimersByTime(1000);
  });
  expect(renders).toBe(hidden);
  expect(context.isTesting).toBe(true);
  expect(api.pingModel).toHaveBeenCalledOnce();
  await act(async () => {
    finish({ success: true, latency: 10, url: 'https://fixture.test' });
    await request;
  });
  await act(async () => useNavigationStore.getState().setActivePage('models'));
  expect(context.isTesting).toBe(false);
  expect(context.modelLatencies.fixture).toBe(10);
  expect(vi.getTimerCount()).toBe(0);
  expect(api.queryModelUsage).not.toHaveBeenCalled();
  expect(api.updateModel).not.toHaveBeenCalled();
});

it.each([
  [{ success: true, latency: 123 }, '123ms', 'text-green-500'],
  [{ success: true, latency: 1000 }, '1000ms', 'text-red-500'],
  [{ success: true, latency: 1001 }, 'model.timedOut', 'text-red-500'],
  [{ success: false, latency: -1, error: 'timeout' }, 'model.timedOut', 'text-red-500'],
  [{ success: false, latency: -1, error: 'connection refused' }, 'Error', 'text-red-500'],
])('renders connection results distinctly: %j', async (result, label, color) => {
  vi.mocked(api.pingModel).mockResolvedValue({ url: 'https://fixture.test', ...result });
  await act(async () => {
    renderer = create(
      <ModelNexusProvider>
        <Harness />
      </ModelNexusProvider>
    );
  });
  expect(api.pingModel).not.toHaveBeenCalled();
  await act(async () => context.pingAllModels());
  const value = renderer!.root
    .findAllByType('span')
    .find((span) => span.children.join('') === label);
  expect(value).toBeDefined();
  expect(value!.props.className).toContain(color);
  expect(api.pingModel).toHaveBeenCalledExactlyOnceWith('fixture', expect.any(String));
  expect(api.finishConnectionTestBatch).toHaveBeenCalledExactlyOnceWith(
    vi.mocked(api.pingModel).mock.calls[0][1]
  );
  expect(api.queryModelUsage).not.toHaveBeenCalled();
  expect(api.updateModel).not.toHaveBeenCalled();
});

it('uses one round for all rows and starts a fresh round on the next click', async () => {
  vi.mocked(api.getModels).mockResolvedValue([
    { internalId: 'fixture', name: 'Fixture', baseUrl: 'https://fixture.test/v1', apiKey: '' },
    { internalId: 'second', name: 'Second', baseUrl: 'https://fixture.test/anthropic', apiKey: '' },
  ]);
  vi.mocked(api.pingModel).mockResolvedValue({
    url: 'https://fixture.test',
    success: true,
    latency: 220,
  });
  await act(async () => {
    renderer = create(
      <ModelNexusProvider>
        <Harness />
      </ModelNexusProvider>
    );
  });
  expect(api.pingModel).not.toHaveBeenCalled();
  expect(api.finishConnectionTestBatch).not.toHaveBeenCalled();
  await act(async () => context.pingAllModels());
  const batch = vi.mocked(api.pingModel).mock.calls[0][1];
  expect(batch).toEqual(expect.any(String));
  expect(api.pingModel).toHaveBeenNthCalledWith(2, 'second', batch);
  expect(api.finishConnectionTestBatch).toHaveBeenCalledExactlyOnceWith(batch);
  expect(context.modelLatencies).toEqual({ fixture: 220, second: 220 });
  expect(context.pingingModelIds.size).toBe(0);
  expect(context.isTesting).toBe(false);
  await act(async () => context.pingAllModels());
  expect(api.pingModel).toHaveBeenCalledTimes(4);
  expect(vi.mocked(api.pingModel).mock.calls[2][1]).not.toBe(batch);
  expect(api.finishConnectionTestBatch).toHaveBeenCalledTimes(2);
  expect(api.queryModelUsage).not.toHaveBeenCalled();
  expect(api.updateModel).not.toHaveBeenCalled();
});
