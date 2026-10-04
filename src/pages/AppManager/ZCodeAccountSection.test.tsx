import React from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ZCodeAccountSection } from './ZCodeAccountSection';
import { AppManagerContext, type AppManagerContextType } from './context';

vi.mock('../../hooks/useI18n', () => ({
  useI18n: () => ({ t: (key: string) => (key === 'agent.zcodeTrial' ? '体验' : key) }),
}));
let renderer: ReactTestRenderer;
afterEach(() => {
  act(() => renderer?.unmount());
  vi.useRealTimers();
});
function fixture(remainingPercent: number | null = 40, busy = false) {
  return {
    isLaunching: false,
    zcodeAccounts: {
      accounts: [
        {
          id: 'bigmodel:one',
          email: 'one@example.test',
          provider: 'bigmodel',
          plan: 'Pro',
          remainingPercent,
          resetAt: null,
        },
      ],
      selectedId: null,
      provider: 'bigmodel',
      setProvider: vi.fn(),
      busy,
      remainingSeconds: 60,
      refreshing: new Set<string>(),
      add: vi.fn(),
      select: vi.fn(),
      refresh: vi.fn(),
      remove: vi.fn(),
    },
  } as unknown as AppManagerContextType;
}
function element(context: AppManagerContextType) {
  return (
    <AppManagerContext.Provider value={context}>
      <ZCodeAccountSection showDivider={false} />
    </AppManagerContext.Provider>
  );
}
describe('ZCode account section', () => {
  it.each(['Trial', 'Free', 'Lite', 'Pro', 'Max', 'Team'])(
    'shows %s once and reserves subscription countdown space only for paid accounts',
    (plan) => {
      vi.useFakeTimers();
      vi.setSystemTime(new Date('2030-01-01T00:00:00Z'));
      const now = Date.now() / 1000;
      const context = fixture();
      Object.assign(context.zcodeAccounts.accounts[0], {
        plan,
        subscriptionEndAt: now + 30 * 86400,
        quotaWindows: [
          { remainingPercent: 74.5, resetAt: now + 5 * 3600 },
          { remainingPercent: 25, resetAt: now + 7 * 86400 },
        ],
      });
      act(() => {
        renderer = create(element(context));
      });
      act(() => {
        vi.advanceTimersByTime(0);
      });
      const markup = JSON.stringify(renderer.toJSON());
      expect(markup.match(new RegExp(`"${plan === 'Trial' ? '体验' : plan}"`, 'g'))).toHaveLength(
        1
      );
      if (plan === 'Trial' || plan === 'Free') {
        expect(markup).not.toContain('30d0h');
        expect(markup).toContain('grid-cols-[16px_minmax(0,1fr)_44px]');
      } else {
        expect(markup).toContain('30d0h');
        expect(markup).toContain('grid-cols-[16px_minmax(0,1fr)_100px]');
      }
      expect(markup).toContain('(5h0m)');
      expect(markup).toContain('(7d0h)');
      expect(markup).toContain('"75","%"');
      expect(markup).toContain('"25","%"');
      expect(markup).not.toMatch(/"title"|tooltip|cursor-/);
      act(() => {
        vi.advanceTimersByTime(60_000);
      });
      expect(JSON.stringify(renderer.toJSON())).toContain('(4h59m)');
    }
  );
  it('keeps the shared pill, typography, quota bar and unknown value', () => {
    act(() => {
      renderer = create(element(fixture()));
    });
    const markup = JSON.stringify(renderer.toJSON());
    expect(markup).toContain('h-12');
    expect(markup).toContain('h-6 w-6');
    expect(markup).toContain('text-[17px] font-bold');
    expect(markup).toContain('text-cyber-text-secondary');
    expect(markup).toContain('"width":"40%"');
    expect(markup).toContain('BigModel · one@example.test');
    expect(markup.match(/"Pro"/g)).toHaveLength(1);
    expect(markup).not.toMatch(/"title"|tooltip|cursor-/);
    act(() => renderer.update(element(fixture(null))));
    expect(JSON.stringify(renderer.toJSON())).toContain('—');
    expect(JSON.stringify(renderer.toJSON())).not.toContain('"children":["0%"]');
    act(() => renderer.update(element(fixture(0))));
    expect(JSON.stringify(renderer.toJSON())).toContain('"children":["0%"]');
  });
  it('toggles provider in both directions without starting login or changing account selection', () => {
    const context = fixture();
    act(() => {
      renderer = create(element(context));
    });
    expect(renderer.root.findAllByType('select')).toHaveLength(0);
    const toggle = renderer.root.findByProps({ 'aria-label': 'agent.zcodeSwitchProvider' });
    expect(toggle.findByType('span').children).toEqual(['BigModel']);
    act(() => toggle.props.onClick());
    expect(context.zcodeAccounts.setProvider).toHaveBeenCalledWith('zai');
    const switched = {
      ...context,
      zcodeAccounts: { ...context.zcodeAccounts, provider: 'zai' as const },
    };
    act(() => renderer.update(element(switched)));
    expect(toggle.findByType('span').children).toEqual(['Z.ai']);
    act(() => toggle.props.onClick());
    expect(context.zcodeAccounts.setProvider).toHaveBeenLastCalledWith('bigmodel');
    expect(context.zcodeAccounts.add).not.toHaveBeenCalled();
    expect(context.zcodeAccounts.select).not.toHaveBeenCalled();
    expect(context.zcodeAccounts.refresh).not.toHaveBeenCalled();
    const login = renderer.root
      .findAllByType('button')
      .filter((button) => button.props['aria-label'] === 'agent.addCurrentAccount');
    expect(login).toHaveLength(1);
    expect(login[0].findAllByType('button')).toHaveLength(1);
    act(() => login[0].props.onClick());
    expect(context.zcodeAccounts.add).toHaveBeenCalledTimes(1);
  });
  it('locks both actions while waiting and isolates nested row actions', () => {
    const context = fixture(40, true);
    act(() => {
      renderer = create(element(context));
    });
    expect(
      renderer.root.findByProps({ 'aria-label': 'agent.zcodeSwitchProvider' }).props.disabled
    ).toBe(true);
    expect(renderer.root.findAllByType('button')[0].props.disabled).toBe(true);
    const refresh = renderer.root
      .findAllByType('button')
      .find((button) => button.props['aria-label']?.startsWith('agent.refreshAccount'))!;
    const stopPropagation = vi.fn();
    act(() => refresh.props.onClick({ stopPropagation }));
    expect(stopPropagation).toHaveBeenCalled();
    expect(context.zcodeAccounts.refresh).toHaveBeenCalledTimes(1);
    expect(context.zcodeAccounts.select).not.toHaveBeenCalled();
    const row = renderer.root.findByProps({ role: 'radio' });
    act(() =>
      row.props.onKeyDown({
        key: 'Enter',
        target: refresh,
        currentTarget: row,
        preventDefault: vi.fn(),
      })
    );
    expect(context.zcodeAccounts.select).not.toHaveBeenCalled();
    act(() =>
      row.props.onKeyDown({ key: ' ', target: row, currentTarget: row, preventDefault: vi.fn() })
    );
    expect(context.zcodeAccounts.select).toHaveBeenCalledWith('bigmodel:one');
  });
});
