import { act, create } from 'react-test-renderer';
import { describe, expect, it, vi } from 'vitest';
import { AntigravityAccountSection } from './AntigravityAccountSection';
import { AppManagerContext, type AppManagerContextType } from './context';
import { QuotaCountdown } from './QuotaCountdown';

describe.each(['antigravity', 'antigravitydesktop'] as const)('%s account controls', (tool) => {
  const fixture = (
    refreshing = false,
    resetAt: number | null = null,
    claudeResetAt: number | null = resetAt
  ) => {
    const select = vi.fn();
    const refresh = vi.fn();
    const remove = vi.fn();
    const account = {
      id: 'one',
      email: 'one@example.test',
      active: true,
      plan: 'Ultra',
      quotas: [
        { name: 'gemini-3', remainingPercent: 0, resetAt },
        { name: 'gemini-3-fast', remainingPercent: 40, resetAt: resetAt && resetAt + 3600 },
        { name: 'claude-sonnet', remainingPercent: 75, resetAt: claudeResetAt },
      ],
    };
    const context = {
      selectedTool: tool,
      isLaunching: false,
      antigravityAccounts: {
        accounts: [account],
        selectedId: account.id,
        select,
        refresh,
        remove,
        refreshing: new Set(refreshing ? [account.id] : []),
        authorizationFailedIds: new Set(),
        add: vi.fn(),
        busy: false,
        remainingSeconds: 0,
      },
    } as unknown as AppManagerContextType;
    return {
      select,
      refresh,
      remove,
      element: (
        <AppManagerContext.Provider value={context}>
          <AntigravityAccountSection />
        </AppManagerContext.Provider>
      ),
    };
  };

  it('shows the plan once and the lowest real quota in each model family', () => {
    const renderer = create(fixture(true).element);
    const markup = JSON.stringify(renderer.toJSON());
    expect(markup).toContain(`/icons/tools/${tool}.png`);
    expect(markup.match(/Ultra/g)).toHaveLength(1);
    expect(markup).toContain('"Gemini ",{"type":"span"');
    expect(markup).toContain('"0%"');
    expect(markup).toContain('"Claude ",{"type":"span"');
    expect(markup).toContain('"75%"');
    expect(
      renderer.root.findByProps({ role: 'radio' }).findAllByType('button')[0].props.disabled
    ).toBe(true);
    act(() => renderer.unmount());
  });

  it('keeps refresh and delete actions separate from account selection', () => {
    const { element, select, refresh, remove } = fixture();
    const renderer = create(element);
    const row = renderer.root.findByProps({ role: 'radio' });
    act(() =>
      row
        .findAllByType('button')
        .forEach((button) => button.props.onClick({ stopPropagation: vi.fn() }))
    );
    expect(refresh).toHaveBeenCalledTimes(1);
    expect(remove).toHaveBeenCalledTimes(1);
    expect(select).not.toHaveBeenCalled();
    act(() => renderer.unmount());
  });

  it('uses each model family’s limiting quota reset time without assuming a plan window', () => {
    const resetAt = 1_800_000_000;
    const renderer = create(fixture(false, resetAt, resetAt + 7200).element);
    const countdowns = renderer.root.findAllByType(QuotaCountdown);
    expect(countdowns.map((countdown) => countdown.props.resetAt)).toEqual([
      resetAt,
      resetAt + 7200,
    ]);
    expect(countdowns.every((countdown) => countdown.props.compact && countdown.props.small)).toBe(
      true
    );
    const quotaLabels = renderer.root.findAllByType('span').map((span) =>
      span.children
        .filter((child) => typeof child === 'string')
        .join('')
        .trim()
    );
    expect(quotaLabels).toContain('G');
    expect(quotaLabels).toContain('C');
    act(() => renderer.unmount());
  });

  it('shows one countdown when Gemini and Claude share a reset time', () => {
    const resetAt = 1_800_000_000;
    const renderer = create(fixture(false, resetAt).element);
    expect(renderer.root.findAllByType(QuotaCountdown).map((timer) => timer.props.resetAt)).toEqual(
      [resetAt]
    );
    expect(renderer.root.findByType(QuotaCountdown).props.compact).toBe(true);
    act(() => renderer.unmount());
  });
});
