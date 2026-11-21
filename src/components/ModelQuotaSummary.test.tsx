import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ModelListCard } from './ModelListCard';
import { ModelCard } from './cards/ModelCard';
import type { ModelUsageData } from '../api/models';

vi.mock('../hooks/useI18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }));
vi.mock('./ConfirmDialog', () => ({ useConfirm: () => async () => true }));

describe('multi-window model quotas', () => {
  let renderer: ReactTestRenderer;
  const now = Date.UTC(2027, 0, 1);
  const usage: ModelUsageData = {
    quotas: [
      { period: 'fiveHour', percentage: 99, resetAt: now + 248 * 60_000 },
      { period: 'weekly', percentage: 20, resetAt: now + 3 * 86400_000 },
      { period: 'monthly', percentage: 80, resetAt: 0 },
    ],
  };
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(now);
  });
  afterEach(() => {
    act(() => renderer?.unmount());
    vi.useRealTimers();
  });

  it('ticks the compact countdown locally and isolates refresh from selection', () => {
    const onSelect = vi.fn();
    const onRefresh = vi.fn();
    act(() => {
      renderer = create(
        <ModelListCard
          model={{ internalId: 'test', name: 'MiniMax', baseUrl: '', apiKey: '' }}
          selection={null}
          onSelect={onSelect}
          usage={usage}
          onRefreshUsage={onRefresh}
          t={(key) => key}
        />
      );
    });
    expect(JSON.stringify(renderer.toJSON())).toContain('1% 4h8m(80%)');
    act(() => {
      vi.advanceTimersByTime(60_000);
    });
    expect(JSON.stringify(renderer.toJSON())).toContain('1% 4h7m(80%)');
    expect(onRefresh).not.toHaveBeenCalled();
    const stopPropagation = vi.fn();
    act(() =>
      renderer.root
        .findByProps({ 'aria-label': 'btn.refreshUsage' })
        .props.onClick({ stopPropagation })
    );
    expect(stopPropagation).toHaveBeenCalledOnce();
    expect(onRefresh).toHaveBeenCalledExactlyOnceWith('test');
    expect(onSelect).not.toHaveBeenCalled();
    act(() => renderer.unmount());
    expect(vi.getTimerCount()).toBe(0);
  });

  it('heads the card with the quota that runs out first and labels every bar', () => {
    const onRefresh = vi.fn();
    act(() => {
      renderer = create(
        <ModelCard
          id="test"
          name="OpenCode Go"
          type="CLOUD"
          usageData={usage}
          onRefresh={onRefresh}
          isRefreshingUsage
        />
      );
    });
    const bars = renderer.root.findAllByProps({ role: 'progressbar' });
    expect(bars.map((bar) => bar.props['aria-label'])).toEqual([
      'model.quota.fiveHour',
      'model.quota.weekly',
      'model.quota.monthly',
    ]);
    expect(bars.map((bar) => bar.props['aria-valuenow'])).toEqual([1, 80, 20]);
    const refresh = renderer.root.findByProps({ 'aria-label': 'btn.refresh' });
    expect(refresh.props.disabled).toBe(true);
    act(() => refresh.props.onClick({ stopPropagation: vi.fn() }));
    expect(onRefresh).not.toHaveBeenCalled();
    const text = JSON.stringify(renderer.toJSON());
    // The five-hour quota is the headline, with its countdown; the others sit beside it.
    expect(text).toContain('1%');
    expect(text).toContain('4h8m');
    expect(text).toContain('80%');
    expect(text).toContain('20%');
  });
});
