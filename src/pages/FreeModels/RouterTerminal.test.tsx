import { useLayoutEffect } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { RouterTerminal } from './RouterTerminal';
import { useTerminalFeed } from './useTerminalFeed';
import { useProtectionStore } from '../../stores/protectionStore';
import type { TerminalSegment } from '../../api/routerTerminal';

const mocks = vi.hoisted(() => ({
  poll: vi.fn(),
  stop: vi.fn(),
  get: vi.fn(),
  save: vi.fn(),
  select: vi.fn(),
  visible: true,
}));
vi.mock('../../api/routerTerminal', () => ({
  pollRouterTerminal: mocks.poll,
  stopRouterTerminal: mocks.stop,
}));
vi.mock('../../api/tauri', () => ({ getSettings: mocks.get, saveSettings: mocks.save }));
vi.mock('../../hooks/useI18n', () => ({ useI18n: () => ({ t: (s: string) => s }) }));
vi.mock('../../hooks/useDocumentVisible', () => ({ useDocumentVisible: () => mocks.visible }));
vi.mock('../../stores/navigationStore', () => ({
  useNavigationStore: (read: (s: object) => unknown) => read({ activePage: 'freeModels' }),
}));
vi.mock('./provider', () => ({
  useFreeModels: () => ({
    routers: [
      { id: 'one', name: 'One' },
      { id: 'two', name: 'Two' },
    ],
    activeRouterId: 'one',
    selectRouter: mocks.select,
    groupPending: false,
  }),
}));
vi.mock('./terminalCanvas', () => ({ animateTerminal: vi.fn() }));

let view: ReactTestRenderer;
beforeEach(() => {
  vi.useFakeTimers();
  vi.clearAllMocks();
  mocks.poll.mockResolvedValue([]);
  mocks.stop.mockResolvedValue(undefined);
  mocks.get.mockResolvedValue({
    sensitiveInfoProtection: true,
    developerMode: false,
    locale: 'ja',
  });
  mocks.save.mockResolvedValue(undefined);
  useProtectionStore.setState({ enabled: null, revision: 0, pending: false, saveError: false });
});
afterEach(async () => {
  if (view) await act(async () => view.unmount());
  vi.useRealTimers();
});

let feed: ReturnType<typeof useTerminalFeed>;
function Probe({ group, active }: { group: string; active: boolean }) {
  const current = useTerminalFeed(group, active);
  useLayoutEffect(() => {
    feed = current;
  });
  return null;
}
const row: TerminalSegment[] = [{ text: 'fixture', protectedText: null, kind: null }];

it('does not subscribe while hidden; subscribes only to the selected group and clears on exit', async () => {
  await act(async () => {
    view = create(<Probe group="one" active={false} />);
  });
  expect(mocks.poll).not.toHaveBeenCalled();
  mocks.poll.mockResolvedValueOnce([row]);
  await act(async () => view.update(<Probe group="one" active />));
  const session = mocks.poll.mock.calls[0][0];
  expect(mocks.poll).toHaveBeenCalledWith(session, 'one');
  expect(feed.pending.current).toEqual([row]);
  expect(feed.realtime).toBe(true);
  await act(async () => view.update(<Probe group="two" active />));
  expect(mocks.stop).toHaveBeenCalledWith(session);
  expect(feed.pending.current).toEqual([]);
  expect(feed.realtime).toBe(false);
  await act(async () => view.update(<Probe group="two" active={false} />));
  const calls = mocks.poll.mock.calls.length;
  await act(async () => {
    await vi.advanceTimersByTimeAsync(5000);
  });
  expect(mocks.poll).toHaveBeenCalledTimes(calls);
  expect(mocks.save).not.toHaveBeenCalled();
});

it('ignores late responses from the previous group and keeps the new queue', async () => {
  let finish!: (rows: TerminalSegment[][]) => void;
  mocks.poll.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      })
  );
  await act(async () => {
    view = create(<Probe group="one" active />);
  });
  const oldSession = mocks.poll.mock.calls[0][0];
  mocks.poll.mockResolvedValueOnce([row]);
  await act(async () => view.update(<Probe group="two" active />));
  await act(async () => finish([[{ ...row[0], text: 'late-secret' }]]));
  expect(feed.pending.current).toEqual([row]);
  expect(mocks.stop).toHaveBeenCalledWith(oldSession);
});

it('resumes demo on a read failure and recovers on the next read', async () => {
  mocks.poll.mockResolvedValueOnce([row]).mockRejectedValueOnce(new Error('fixture'));
  await act(async () => {
    view = create(<Probe group="one" active />);
  });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(600);
  });
  expect(feed.error).toBe(true);
  expect(feed.pending.current).toEqual([]);
  expect(feed.realtime).toBe(false);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(600);
  });
  expect(feed.error).toBe(false);
});

it('uses existing group selection without writes; protection saves merge the latest settings and publish to both controls', async () => {
  await act(async () => {
    view = create(<RouterTerminal />);
  });
  expect(mocks.save).not.toHaveBeenCalled();
  await act(async () => view.root.findByProps({ 'aria-label': 'Two' }).props.onClick());
  expect(mocks.select).toHaveBeenCalledWith('two');
  const toggle = () => view.root.findByProps({ role: 'switch' });
  await act(async () => toggle().props.onClick());
  expect(mocks.save).toHaveBeenCalledExactlyOnceWith({
    sensitiveInfoProtection: false,
  });
  expect(useProtectionStore.getState().enabled).toBe(false);
  await act(async () => useProtectionStore.getState().publish(true));
  expect(toggle().props['aria-checked']).toBe(true);
});

it('keeps the saved protection value and reports explicit save failures', async () => {
  await act(async () => {
    view = create(<RouterTerminal />);
  });
  mocks.save.mockRejectedValueOnce(new Error('fixture'));
  await act(async () => view.root.findByProps({ role: 'switch' }).props.onClick());
  expect(useProtectionStore.getState().enabled).toBe(true);
  expect(view.root.findByProps({ role: 'alert' }).children).toContain(
    'settings.sensitiveInfoProtectionError'
  );
});

it('shares pending protection across group remounts and prevents duplicate saves', async () => {
  await act(async () => {
    view = create(<RouterTerminal />);
  });
  let finish!: () => void;
  mocks.save.mockImplementationOnce(
    () =>
      new Promise<void>((resolve) => {
        finish = resolve;
      })
  );
  await act(async () => view.root.findByProps({ role: 'switch' }).props.onClick());
  await act(async () => view.update(<RouterTerminal key="new-group" />));
  expect(view.root.findByProps({ role: 'switch' }).props.disabled).toBe(true);
  await act(async () => view.root.findByProps({ role: 'switch' }).props.onClick());
  expect(mocks.save).toHaveBeenCalledOnce();
  await act(async () => finish());
  expect(view.root.findByProps({ role: 'switch' }).props['aria-checked']).toBe(false);
});

it('resumes demo on the first idle poll and accepts fresh real input without remounting', async () => {
  await act(async () => {
    view = create(<Probe group="one" active />);
  });
  expect(feed.realtime).toBe(false);
  mocks.poll.mockResolvedValueOnce([row]);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(600);
  });
  expect(feed.realtime).toBe(true);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(600);
  });
  expect(feed.realtime).toBe(false);
  expect(feed.pending.current).toEqual([]);
  mocks.poll.mockResolvedValueOnce([row]);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(600);
  });
  expect(feed.realtime).toBe(true);
  await act(async () => view.update(<Probe group="two" active />));
  expect(feed.realtime).toBe(false);
});

it('restores the demo notice as soon as real input stops', async () => {
  await act(async () => {
    view = create(<RouterTerminal />);
  });
  expect(view.root.findByProps({ role: 'status' }).children).toContain(
    'freeModels.terminal.demoNotice'
  );
  mocks.poll.mockResolvedValueOnce([row]);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(600);
  });
  expect(view.root.findAllByProps({ role: 'status' })).toHaveLength(0);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(600);
  });
  expect(view.root.findByProps({ role: 'status' }).children).toContain(
    'freeModels.terminal.demoNotice'
  );
  expect(mocks.save).not.toHaveBeenCalled();
});

it('replaces undisplayed input with the latest poll instead of replaying the backlog', async () => {
  mocks.poll.mockResolvedValueOnce(Array.from({ length: 64 }, () => row));
  await act(async () => {
    view = create(<Probe group="one" active />);
  });
  const latest = [{ ...row[0], text: 'latest-request' }];
  mocks.poll.mockResolvedValueOnce([latest]);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(600);
  });
  expect(feed.pending.current).toEqual([latest]);
  expect(feed.realtime).toBe(true);
});
