import { act, create } from 'react-test-renderer';
import { describe, expect, it, vi } from 'vitest';
import { CursorAccountSection } from './CursorAccountSection';
import { AppManagerContext, type AppManagerContextType } from './context';
import type { CursorUsage } from '../../api/tauri';
import { QuotaCountdown } from './QuotaCountdown';

describe.each(['grokbot', 'cursor', 'cursorcli'] as const)('%s account controls', (tool) => {
  function fixture(busy = false, usage?: CursorUsage, refreshing = false) {
    const select = vi.fn();
    const remove = vi.fn();
    const refresh = vi.fn();
    const cancelLogin = vi.fn();
    const context = {
      isLaunching: false,
      [tool === 'grokbot' ? 'grokBotAccounts' : 'cursorAccounts']: {
        accounts: [{ id: 'one', email: 'one@example.test', active: true, usage }],
        selectedId: 'one',
        select,
        remove,
        refresh,
        refreshing: new Set(refreshing ? ['one'] : []),
        authorizationFailedIds: new Set(),
        add: vi.fn(),
        cancelLogin,
        busy,
        remainingSeconds: 60,
      },
    } as unknown as AppManagerContextType;
    return {
      select,
      remove,
      refresh,
      cancelLogin,
      element: (
        <AppManagerContext.Provider value={context}>
          <CursorAccountSection tool={tool} />
        </AppManagerContext.Provider>
      ),
    };
  }

  it('shows unknown quota and plan while keeping the real refresh action available', () => {
    const renderer = create(fixture().element);
    const markup = JSON.stringify(renderer.toJSON());
    expect(markup).toContain(
      tool === 'grokbot' ? '/icons/tools/grokbot.png' : '/icons/tools/cursor.svg'
    );
    expect(renderer.root.findByProps({ role: 'radio' }).props['aria-checked']).toBe(true);
    expect(
      renderer.root.findAllByType('span').filter((node) => node.children.includes('—'))
    ).toHaveLength(1);
    expect(renderer.root.findAllByProps({ role: 'progressbar' })).toHaveLength(0);
    expect(markup).toContain('model.noUsageData');
    expect(markup).toContain('agent.refreshAccount');
    expect(markup).not.toContain('title=');
    expect(markup).not.toContain('cursor-');
    const waiting = fixture(true);
    act(() => renderer.update(waiting.element));
    const cancelButton = renderer.root.findByProps({ 'aria-label': 'btn.cancel' });
    expect(cancelButton.props.disabled).not.toBe(true);
    act(() => cancelButton.props.onClick({ stopPropagation: vi.fn() }));
    expect(waiting.cancelLogin).toHaveBeenCalledTimes(1);
    act(() => {
      renderer.unmount();
    });
  });
  it('isolates refresh/delete clicks and nested keyboard actions from row selection', () => {
    const { element, select, remove, refresh } = fixture();
    const renderer = create(element);
    const row = renderer.root.findByProps({ role: 'radio' });
    const stopPropagation = vi.fn();
    act(() => {
      row.findAllByType('button').forEach((button) => button.props.onClick({ stopPropagation }));
    });
    expect(stopPropagation).toHaveBeenCalled();
    expect(remove).toHaveBeenCalledTimes(1);
    expect(refresh).toHaveBeenCalledTimes(1);
    expect(select).not.toHaveBeenCalled();
    const target = {};
    act(() => {
      row.props.onKeyDown({ key: 'Enter', target, currentTarget: {}, preventDefault: vi.fn() });
    });
    expect(select).not.toHaveBeenCalled();
    for (const key of ['Enter', ' '])
      act(() => {
        row.props.onKeyDown({ key, target, currentTarget: target, preventDefault: vi.fn() });
      });
    expect(select).toHaveBeenCalledTimes(2);
    act(() => {
      renderer.unmount();
    });
  });
  it('shows the plan once, preserves a real zero, and disables refresh while pending', () => {
    const renderer = create(
      fixture(false, { plan: 'Ultra', remainingPercent: 0, resetAt: null }, true).element
    );
    const markup = JSON.stringify(renderer.toJSON());
    expect(markup.match(/Ultra/g)).toHaveLength(1);
    expect(markup).toContain('0%');
    expect(renderer.root.findByProps({ role: 'progressbar' }).props['aria-valuenow']).toBe(0);
    expect(
      renderer.root.findByProps({ role: 'radio' }).findAllByType('button')[0].props.disabled
    ).toBe(true);
    act(() => renderer.unmount());
  });
  it('renders a single remaining quota as a progress bar, percentage, and reset countdown', () => {
    const renderer = create(
      fixture(false, { plan: 'Pro', remainingPercent: 75, resetAt: 1800000000 }).element
    );
    const progress = renderer.root.findByProps({ role: 'progressbar' });
    expect(progress.props['aria-valuenow']).toBe(75);
    expect(progress.find((node) => node.props.style?.width).props.style.width).toBe('75%');
    expect(JSON.stringify(renderer.toJSON())).toContain('75%');
    expect(renderer.root.findByType(QuotaCountdown).props.resetAt).toBe(1800000000);
    act(() => renderer.unmount());
  });
  it('hides a generic heading saved by an older client until the user refreshes', () => {
    const renderer = create(
      fixture(false, { plan: 'Grok Bot Plan', remainingPercent: 0, resetAt: null }).element
    );
    const markup = JSON.stringify(renderer.toJSON());
    expect(markup).not.toContain('Grok Bot Plan');
    expect(markup).toContain('—');
    expect(markup).toContain('0%');
    expect(markup).toContain('agent.refreshAccount');
    act(() => renderer.unmount());
  });
});
