import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { QuotaCountdown } from './QuotaCountdown';

let renderer: ReactTestRenderer;
beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2026-10-02T05:00:00Z'));
});
afterEach(() => {
  act(() => renderer?.unmount());
  vi.useRealTimers();
});

it.each([
  [20 * 86400 + 7 * 3600, '20d7h', '20d7h'],
  [24 * 3600 + 12 * 60, '24h12m', '1d0h'],
  [4 * 3600 + 8 * 60, '4h8m', '4h8m'],
  [15 * 60, '15m', '15m'],
])('formats %s seconds for compact and normal countdowns', async (seconds, compact, normal) => {
  const resetAt = Date.now() / 1000 + Number(seconds);
  act(() => {
    renderer = create(
      <>
        <QuotaCountdown resetAt={resetAt} compact label="Next credit expiry" />
        <QuotaCountdown resetAt={resetAt} />
      </>
    );
  });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(0);
  });
  const spans = renderer.root.findAllByType('span');
  expect(spans[0].props['aria-label']).toBe(`Next credit expiry ${compact}`);
  expect(spans[0].children.filter((child) => typeof child === 'string').join('')).toBe(compact);
  expect(spans[1].children.join('')).toBe(normal);
});

it.each([null, 0, -60])('hides unknown or expired compact countdown (%s)', async (offset) => {
  act(() => {
    renderer = create(
      <QuotaCountdown resetAt={offset == null ? null : Date.now() / 1000 + offset} compact />
    );
  });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(0);
  });
  expect(renderer.toJSON()).toBeNull();
});

it('hides expired compact countdowns while quota rows retain their zero countdown', async () => {
  const resetAt = Date.now() / 1000 + 30;
  act(() => {
    renderer = create(
      <>
        <QuotaCountdown resetAt={resetAt} compact />
        <QuotaCountdown resetAt={resetAt} />
      </>
    );
  });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(0);
  });
  expect(renderer.root.findAllByType('span')).toHaveLength(2);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_000);
  });
  const spans = renderer.root.findAllByType('span');
  expect(spans).toHaveLength(1);
  expect(spans.map((span) => span.children.join(''))).toEqual(['0m']);
});

it.each([null, undefined])(
  'keeps an unknown subscription countdown hidden (%s)',
  async (resetAt) => {
    act(() => {
      renderer = create(<QuotaCountdown resetAt={resetAt} compact />);
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(renderer.toJSON()).toBeNull();
  }
);
