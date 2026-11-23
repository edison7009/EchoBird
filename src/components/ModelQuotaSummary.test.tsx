import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ModelListCard } from './ModelListCard';
import { ModelCard } from './cards/ModelCard';
import type { ModelUsageData } from '../api/models';

vi.mock('../hooks/useI18n', () => ({ useI18n: () => ({ locale: 'en', t: (key: string) => key }) }));
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

  it('shows monetary balance alongside a plan and keeps the plan amount in its own unit', () => {
    const plan: ModelUsageData = {
      quotas: [
        {
          period: 'monthly',
          percentage: 27.5,
          resetAt: now + 86400_000,
          amounts: { used: 27.5, remaining: 72.5, unit: 'USD' },
        },
        { percentage: 0, resetAt: 0, balance: 25, balanceUnit: 'USD' },
      ],
    };
    act(() => {
      renderer = create(
        <>
          <ModelCard id="test" name="Cloud" type="CLOUD" usageData={plan} />
          <ModelListCard
            model={{ internalId: 'test', name: 'Cloud', baseUrl: '', apiKey: '' }}
            selection={null}
            onSelect={vi.fn()}
            usage={plan}
            t={(key) => key}
          />
        </>
      );
    });
    const text = JSON.stringify(renderer.toJSON());
    expect(text).toContain('model.quota.used 27.5 · model.quota.remaining 72.5 USD');
    expect(text).toContain('1mo 72.5% 1d0h · model.balance 25.00 USD');
  });

  it('preserves the balance unit in a compact model row with a single quota', () => {
    act(() => {
      renderer = create(
        <ModelListCard
          model={{ internalId: 'test', name: 'Cloud', baseUrl: '', apiKey: '' }}
          selection={null}
          onSelect={vi.fn()}
          usage={{ quotas: [{ percentage: 0, resetAt: 0, balance: 12, balanceUnit: 'Credits' }] }}
          t={(key) => key}
        />
      );
    });
    const summary = renderer.root.findByProps({ 'aria-label': 'model.balance 12.00 Credits' });
    expect(summary.children).toContain('model.balance 12.00 Credits');
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
