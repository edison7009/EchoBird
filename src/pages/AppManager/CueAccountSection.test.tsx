import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ManusAccountSection } from './ManusAccountSection';
import { AppManagerContext, type AppManagerContextType } from './context';
import { I18nContext } from '../../hooks/i18nContext';
import { en } from '../../i18n/en';

describe('Manus and Cue subscription countdown', () => {
  let renderer: ReactTestRenderer | undefined;
  afterEach(() => {
    act(() => renderer?.unmount());
    vi.useRealTimers();
  });
  it.each([
    ['cue', 'plus', true],
    ['cue', 'pro', true],
    ['cue', 'max', true],
    ['cue', 'free', true],
    ['cue', 'plus', false],
    ['manus', 'pro', true],
    ['manus', 'free', true],
    ['manus', 'Free', true],
    ['manus', 'pro', false],
  ] as const)(
    'shows %s %s subscription expiry only when provided (%s), preserving provider quota',
    (tool, plan, hasExpiry) => {
      vi.useFakeTimers();
      vi.setSystemTime(new Date('2030-01-01T00:00:00Z'));
      const now = Date.now() / 1000;
      const group = {
        accounts: [
          {
            id: tool,
            email: `${tool}@example.test`,
            plan,
            active: true,
            credits: { total: 1300, free: 1000, refresh: 300, nextRefreshAt: now + 3600 },
            subscriptionEndAt: hasExpiry ? now + 30 * 86400 : null,
            ...(tool === 'cue'
              ? { weekly: { remainingPercent: 80, resetAt: now + 3 * 3600 } }
              : {}),
          },
        ],
        busy: false,
        remainingSeconds: 0,
        refreshing: new Set(),
        authorizationFailedIds: new Set(),
        add: vi.fn(),
        select: vi.fn(),
        refresh: vi.fn(),
        remove: vi.fn(),
      };
      const context = { [`${tool}Accounts`]: group } as unknown as AppManagerContextType;
      act(() => {
        renderer = create(
          <I18nContext.Provider value={{ locale: 'en', setLocale: () => {}, t: (key) => en[key] }}>
            <AppManagerContext.Provider value={context}>
              <ManusAccountSection tool={tool} />
            </AppManagerContext.Provider>
          </I18nContext.Provider>
        );
      });
      act(() => {
        vi.advanceTimersByTime(0);
      });
      const markup = JSON.stringify(renderer!.toJSON());
      const displayed = hasExpiry && plan.toLowerCase() !== 'free';
      expect(markup.includes('30d0h')).toBe(displayed);
      expect(markup).toContain('auto-rows-[16px] content-center');
      expect(markup).not.toContain('grid-cols-[minmax(0,1fr)_100px]');
      if (tool === 'cue') {
        expect(markup).toContain('3h0m');
        expect(markup).toContain('80%');
      } else {
        expect(markup).toContain(`1300 ${en['agent.credits']}`);
        expect(markup).not.toContain('progressbar');
        expect(markup).not.toContain('1h0m');
      }
      expect(markup).toContain('h-12');
      expect(markup).not.toMatch(/"title"|tooltip|cursor-/);
      act(() => {
        vi.advanceTimersByTime(60_000);
      });
      const updated = JSON.stringify(renderer!.toJSON());
      if (tool === 'cue') expect(updated).toContain('2h59m');
      if (displayed) expect(updated).toContain('29d23h');
      expect(group.refresh).not.toHaveBeenCalled();
      expect(group.add).not.toHaveBeenCalled();
      expect(group.select).not.toHaveBeenCalled();
    }
  );
});
