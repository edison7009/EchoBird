import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import {
  UsageTooltipCard,
  usageTooltipHeight,
  usageTooltipPosition,
  type UsageTooltipRequest,
  type UsageTooltipSnapshot,
} from './UsageIslandTooltip';
import { UsageIslandTooltipWindow } from './UsageIslandTooltipWindow';
import {
  getUsageIslandTooltip,
  readyUsageIslandTooltip,
  USAGE_ISLAND_TOOLTIP_EVENT,
} from '../../api/usageIsland';

const listeners = vi.hoisted(
  () => new Map<string, (event: { payload: UsageTooltipSnapshot }) => void>()
);
const zoom = vi.hoisted(() => vi.fn(async () => {}));
const unlisten = vi.hoisted(() => vi.fn());
vi.mock('../../api/usageIsland', () => ({
  USAGE_ISLAND_TOOLTIP_EVENT: 'usage-island-tooltip-state',
  getUsageIslandTooltip: vi.fn(async () => null),
  readyUsageIslandTooltip: vi.fn(async () => {}),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(
    async (name: string, handler: (event: { payload: UsageTooltipSnapshot }) => void) => {
      listeners.set(name, handler);
      return unlisten;
    }
  ),
}));
vi.mock('@tauri-apps/api/webview', () => ({ getCurrentWebview: () => ({ setZoom: zoom }) }));
vi.mock('../AppManager/QuotaCountdown', () => ({
  QuotaCountdown: ({ resetAt }: { resetAt: number }) => (
    <span data-reset-at={resetAt}>countdown</span>
  ),
}));
vi.mock('../../hooks/useUsageClock', () => ({ useUsageClock: () => 1_000_000 }));

let view: ReactTestRenderer | undefined;
const request = (name = 'Claude'): UsageTooltipRequest => ({
  item: {
    key: 'account:a',
    kind: 'account',
    name,
    detail: 'hidden identity',
    icon: null,
    percent: 0,
    value: '0%',
    lines: [
      { label: 'Five hour', percent: 0, value: '0%', resetAt: 2000 },
      { label: 'Weekly', percent: null, value: '—', resetAt: null },
    ],
  },
  anchor: { x: 300, y: 400, width: 32, height: 32 },
  width: 276,
  height: 178,
  theme: 'dark',
  zoom: 1,
  resetLabel: 'Quota resets',
});
beforeEach(() => {
  vi.clearAllMocks();
  listeners.clear();
  vi.mocked(getUsageIslandTooltip).mockResolvedValue(null);
});
afterEach(() => {
  act(() => view?.unmount());
  view = undefined;
});

it('renders a known zero allowance as an empty bar and keeps unknown quota unknown', () => {
  const value = request();
  act(() => {
    view = create(<UsageTooltipCard request={value} placement="above" arrowX={80} />);
  });
  const rows = view!.root.findAllByProps({ className: 'usage-island-tip-row' });
  const bars = view!.root.findAllByProps({ className: 'usage-island-tip-track' });
  expect(rows).toHaveLength(2);
  expect(bars).toHaveLength(1);
  expect(bars[0].findByType('span').props.style.width).toBe('0%');
  expect(rows[1].findByType('strong').children).toEqual(['—']);
  expect(view!.root.findByProps({ 'data-reset-at': 2000 })).toBeTruthy();
  expect(view!.root.findAllByType('strong').flatMap((node) => node.children)).toContain('Claude');
  expect(usageTooltipHeight(value.item)).toBe(178);
});

it('shows actual balance buckets without fabricating quota bars or expired reset text', () => {
  const value = request();
  value.item.lines = [
    { label: 'Balance', value: '$12.40', percent: null, resetAt: null },
    { label: 'Daily credits', value: '230', percent: null, resetAt: 900 },
  ];
  act(() => {
    view = create(<UsageTooltipCard request={value} placement="below" arrowX={18} />);
  });
  expect(view!.root.findAllByProps({ className: 'usage-island-tip-track' })).toHaveLength(0);
  expect(view!.root.findAllByProps({ className: 'usage-island-tip-reset' })).toHaveLength(0);
  expect(view!.root.findAllByType('strong').flatMap((node) => node.children)).toEqual([
    'Claude',
    '$12.40',
    '230',
  ]);
  expect(usageTooltipHeight(value.item)).toBe(168);
});

it.each([
  { x: 1, y: 1, width: 32, height: 32 },
  { x: 780, y: 560, width: 32, height: 32 },
  { x: 400, y: 400, width: 32, height: 32 },
])('keeps preview cards on screen and points the arrow at the anchor near $x,$y', (anchor) => {
  const result = usageTooltipPosition(anchor, 276, 206, { width: 820, height: 600 });
  expect(result!.left).toBeGreaterThanOrEqual(12);
  expect(result!.left + 276).toBeLessThanOrEqual(808);
  expect(result!.top).toBeGreaterThanOrEqual(12);
  expect(result!.top + 206).toBeLessThanOrEqual(588);
  expect(result!.placement).toBe(anchor.y === 1 ? 'below' : 'above');
  expect(result!.arrowX).toBeGreaterThanOrEqual(18);
  expect(result!.arrowX).toBeLessThanOrEqual(258);
});

it('ignores an old initial snapshot and older events after a newer hover arrives', async () => {
  let resolve!: (value: UsageTooltipSnapshot | null) => void;
  vi.mocked(getUsageIslandTooltip).mockReturnValue(
    new Promise((done) => {
      resolve = done;
    })
  );
  await act(async () => {
    view = create(<UsageIslandTooltipWindow />);
  });
  const snapshot = (name: string, revision: number, scale = 1): UsageTooltipSnapshot => ({
    request: { ...request(name), zoom: scale },
    revision,
    placement: 'above',
    arrowX: 100,
  });
  await act(async () =>
    listeners.get(USAGE_ISLAND_TOOLTIP_EVENT)!({ payload: snapshot('Newest', 3, 1.5) })
  );
  expect(zoom).toHaveBeenLastCalledWith(1.5);
  expect(readyUsageIslandTooltip).toHaveBeenLastCalledWith(3);
  await act(async () => resolve(snapshot('Stale initial', 1)));
  await act(async () =>
    listeners.get(USAGE_ISLAND_TOOLTIP_EVENT)!({ payload: snapshot('Stale event', 2) })
  );
  expect(view!.root.findAllByType('strong').flatMap((node) => node.children)).toContain('Newest');
  expect(JSON.stringify(view!.toJSON())).not.toContain('Stale');
  await act(async () => view!.unmount());
  view = undefined;
  expect(unlisten).toHaveBeenCalledOnce();
});

it.each(['above', 'below', 'left', 'right'] as const)(
  'places dial details outside the entire disk toward %s',
  (direction) => {
    const disk = { x: 400, y: 300, width: 216, height: 216 };
    const anchors = {
      above: { x: 492, y: 310, width: 32, height: 32 },
      below: { x: 492, y: 474, width: 32, height: 32 },
      left: { x: 410, y: 392, width: 32, height: 32 },
      right: { x: 574, y: 392, width: 32, height: 32 },
    };
    const result = usageTooltipPosition(
      anchors[direction],
      276,
      206,
      { width: 1400, height: 1000 },
      disk,
      direction
    );
    expect(result!.placement).toBe(direction);
    expect(
      result!.left + 276 <= disk.x ||
        result!.left >= disk.x + disk.width ||
        result!.top + 206 <= disk.y ||
        result!.top >= disk.y + disk.height
    ).toBe(true);
  }
);

it('chooses another outside edge when a dial is against the top of the display', () => {
  const disk = { x: 400, y: 12, width: 216, height: 216 };
  const result = usageTooltipPosition(
    { x: 492, y: 22, width: 32, height: 32 },
    276,
    206,
    { width: 1400, height: 1000 },
    disk,
    'above'
  );
  expect(result!.placement).toBe('below');
  expect(result!.top).toBeGreaterThan(disk.y + disk.height);
});

it('hides dial details when no outside edge fits the viewport', () => {
  expect(
    usageTooltipPosition(
      { x: 142, y: 60, width: 32, height: 32 },
      276,
      206,
      { width: 320, height: 320 },
      { x: 52, y: 52, width: 216, height: 216 },
      'above'
    )
  ).toBeNull();
});
