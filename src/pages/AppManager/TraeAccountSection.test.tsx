import { renderToStaticMarkup } from 'react-dom/server';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { describe, expect, it, vi } from 'vitest';
import type { TraeAccount } from '../../api/tauri';
import type { AppManagerContextType } from './context';
import { TraeAccountSection } from './TraeAccountSection';
import type { useTraeAccounts } from './useTraeAccounts';
import { traeClaimedToday } from './traeDailyCredits';
import { enableAccountPlugins } from '../../test/accountPlugins';
import { usePluginsStore } from '../../stores/pluginsStore';
import { useNavigationStore } from '../../stores/navigationStore';
import { openAccountPlugins } from './openAccountPlugins';

vi.mock('./openAccountPlugins', () => ({
  openAccountPlugins: vi.fn().mockResolvedValue(undefined),
}));

vi.mock('../../hooks/useI18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }));
let context: AppManagerContextType;
vi.mock('./context', () => ({ useAppManager: () => context }));
const saved: TraeAccount = {
  id: 'saved',
  edition: 'traecn',
  name: 'Saved',
  plan: 'Pro',
  active: true,
  remaining: 20,
};
function fixture(
  accounts: TraeAccount[] = [saved],
  overrides: Partial<ReturnType<typeof useTraeAccounts>> = {}
) {
  const group: ReturnType<typeof useTraeAccounts> = {
    accounts,
    selectedId: 'saved',
    busy: false,
    loading: false,
    loaded: true,
    readFailed: false,
    mutationRevision: 0,
    remainingSeconds: 60,
    refreshing: new Set(),
    authorizationFailedIds: new Set(),
    select: vi.fn(),
    add: vi.fn().mockResolvedValue(undefined),
    refresh: vi.fn().mockResolvedValue(undefined),
    remove: vi.fn().mockResolvedValue(undefined),
    claimDaily: vi.fn().mockResolvedValue(undefined),
    cancelLogin: vi.fn(),
    reload: vi.fn().mockResolvedValue(accounts),
    login: null,
    submitting: false,
    loginError: null,
    completeLogin: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  };
  const edition = accounts[0]?.edition ?? 'traecn';
  context = {
    selectedTool: edition,
    traeAccounts: group,
    traeAccountGroups: { trae: { ...group, accounts: [] }, traecn: group },
  } as unknown as AppManagerContextType;
  const content = <TraeAccountSection />;
  return { group, content, html: () => renderToStaticMarkup(content) };
}

describe('TRAE account rows', () => {
  it.each(['trae', 'traecn'] as const)(
    '%s opens the Hub without a module and keeps login when installed',
    async (edition) => {
      const { content, group } = fixture([{ ...saved, edition }]);
      usePluginsStore.setState({ installed: [] });
      useNavigationStore.setState({ developerMode: true });
      vi.mocked(openAccountPlugins).mockClear();
      let renderer: ReactTestRenderer;
      act(() => {
        renderer = create(content);
      });
      try {
        const login = () =>
          renderer.root
            .findAllByType('button')
            .find((node) =>
              node
                .findAllByType('span')
                .some((span) => span.props.children === 'plugins.category.account')
            )!;
        await act(async () => login().props.onClick());
        expect(openAccountPlugins).toHaveBeenCalledOnce();
        expect(group.add).not.toHaveBeenCalled();
        act(() => enableAccountPlugins());
        await act(async () => login().props.onClick());
        expect(group.add).toHaveBeenCalledOnce();
        expect(openAccountPlugins).toHaveBeenCalledOnce();
      } finally {
        act(() => renderer!.unmount());
        usePluginsStore.setState(usePluginsStore.getInitialState(), true);
        useNavigationStore.setState(useNavigationStore.getInitialState(), true);
      }
    }
  );
  it.each(['Free', 'Lite', 'Pro', 'Pro+', 'Ultra', 'Express'])(
    'shows plan %s once using shared row actions',
    (plan) => {
      const html = fixture([{ ...saved, plan }]).html();
      expect(html.match(new RegExp(`>${plan.replace('+', '\\+')}</span>`, 'g'))).toHaveLength(1);
      expect(html).toContain('grid h-12');
      expect(html).toContain('aria-checked="true"');
      expect(html).toContain('agent.refreshAccount Saved');
      expect(html).toContain('btn.delete Saved');
      expect(html).not.toMatch(/title=|tooltip|cursor-/);
    }
  );

  it('keeps an uncategorized balance compact without treating Work credits as rewards', () => {
    const single = fixture().html();
    expect(single).toContain('>20</span>');
    expect(single).not.toContain('agent.generalCredits');
    const workOnly = fixture([{ ...saved, remaining: null, workRemaining: 0 }]).html();
    expect(workOnly).not.toContain('agent.rewardCredits');
    expect(workOnly).not.toContain('agent.generalCredits');
    expect(workOnly).toContain('model.noUsageData');
    expect(fixture([{ ...saved, plan: 'Free', remaining: null }]).html()).toContain(
      'model.noUsageData'
    );
  });

  it('uses the shared credit row for confirmed base, purchased and daily reward balances', () => {
    const unclaimed = fixture([
      {
        ...saved,
        baseRemaining: 75,
        baseTotal: 100,
        rewardRemaining: 20,
        rewardTotal: 50,
        addonRemaining: 12,
        rewardEnabled: true,
      },
    ]).html();
    expect(unclaimed).toMatch(/aria-label="agent.baseCredits"><span[^>]*>75<\/span>/);
    expect(unclaimed).toContain('aria-label="agent.purchasedCredits">12</span>');
    expect(unclaimed).toContain('>agent.dailyCreditsUnclaimed</span>');
    expect(unclaimed).toMatch(
      /aria-label="agent.rewardCredits"><span[^>]*>.*?lucide-gift.*?<\/svg>20<\/span>/
    );
    expect(unclaimed.match(/lucide-gift/g)).toHaveLength(2);
    const claimed = fixture([
      {
        ...saved,
        baseRemaining: 75,
        baseTotal: 100,
        rewardRemaining: 0,
        rewardTotal: 50,
        rewardEnabled: true,
        dailyClaimedAt: Date.now() / 1000,
      },
    ]).html();
    expect(claimed).not.toContain('agent.dailyCreditsUnclaimed');
    expect(claimed).toContain('aria-label="agent.rewardCredits"');
    expect(claimed.match(/lucide-gift/g)).toHaveLength(1);
    expect(claimed).toContain('0</span>');
    expect(fixture([{ ...saved, rewardEnabled: null }]).html()).not.toContain(
      'agent.dailyCreditsUnclaimed'
    );
  });

  it('shows a reward-only CN balance once without a fabricated base balance', () => {
    const html = fixture([
      {
        ...saved,
        plan: 'Free',
        remaining: 4600,
        total: 4600,
        baseRemaining: 0,
        baseTotal: 0,
        rewardRemaining: 4600,
        rewardTotal: 4600,
        rewardEnabled: true,
        dailyClaimedAt: Date.now() / 1000,
      },
    ]).html();
    expect(html).toContain('aria-label="agent.rewardCredits"');
    expect(html.match(/4600/g)).toHaveLength(1);
    expect(html).not.toContain('agent.baseCredits');
    expect(html).not.toContain('>—</span>');
    expect(html.match(/lucide-gift/g)).toHaveLength(1);
    expect(html).toContain('aria-label="agent.dailyCreditsClaimed Saved"');
  });

  it('shows the confirmed international usage currency without labeling it as credits', () => {
    const html = fixture([
      { ...saved, edition: 'trae', remaining: 1, total: 1, currency: 'USD' },
    ]).html();
    expect(html).toContain('aria-label="accountCenter.quota">$1</span>');
    expect(html).not.toContain('agent.credits');
    expect(
      fixture([{ ...saved, edition: 'trae', remaining: null, currency: 'USD' }]).html()
    ).toContain('model.noUsageData');
  });

  it('keeps subscription expiry beside the plan and each balance deadline with its own amount', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-10-13T04:00:00Z'));
    const now = Date.now() / 1000;
    try {
      for (const edition of ['traecn', 'trae'] as const) {
        const account = {
          ...saved,
          edition,
          remaining: 400,
          total: 500,
          expiresAt: now + 5 * 86400,
          baseRemaining: 75,
          baseTotal: 100,
          baseResetAt: now + 3 * 86400,
          rewardRemaining: 25,
          rewardTotal: 100,
          rewardNextExpiryAt: now + 86400,
          subscriptionEndAt: now + 30 * 86400,
        };
        let renderer!: ReactTestRenderer;
        act(() => {
          renderer = create(fixture([account]).content);
        });
        await act(async () => {
          await vi.advanceTimersByTimeAsync(0);
        });
        expect(
          renderer.root.findByProps({
            'aria-label': `${edition === 'traecn' ? 'agent.baseCreditsReset 3d0h' : 'accountCenter.reset 5d0h'}`,
          })
        ).toBeDefined();
        expect(
          renderer.root.findAllByProps({
            'aria-label': 'agent.baseCreditsReset 30d0h',
          })
        ).toHaveLength(0);
        const planRow = renderer.root
          .findAllByType('span')
          .find((node) => node.children.includes('Pro'))!;
        expect(
          planRow.findAllByProps({
            'aria-label': 'accountCenter.subscription 30d0h',
          })
        ).toHaveLength(1);
        expect(
          renderer.root.findAllByProps({
            'aria-label': 'agent.rewardCreditsNextExpiry 24h0m',
          })
        ).toHaveLength(edition === 'traecn' ? 1 : 0);
        expect(
          renderer.root
            .findAllByType('span')
            .filter((node) => node.children.includes('agent.rewardCreditsNextExpiry'))
        ).toHaveLength(0);
        act(() => renderer.unmount());
      }
    } finally {
      vi.useRealTimers();
    }
  });

  it('borrows only the unused CN action width and does not reserve expired subscription space', () => {
    const subscriptionEndAt = Date.now() / 1000 + 30 * 86400;
    const cn = fixture([{ ...saved, subscriptionEndAt }]).html();
    expect(cn).toContain('w-[calc(100%+40px)] flex-shrink-0');
    const global = fixture([{ ...saved, edition: 'trae', subscriptionEndAt }]).html();
    expect(global).not.toContain('w-[calc(100%+40px)]');
    for (const edition of ['traecn', 'trae'] as const) {
      const expired = fixture([
        { ...saved, edition, subscriptionEndAt: Date.now() / 1000 - 1 },
      ]).html();
      expect(expired).not.toContain('grid-cols-[minmax(0,1fr)_100px]');
      expect(expired).not.toContain('w-[calc(100%+40px)]');
    }
  });

  it('shows one earliest date when base and reward expire together and keeps the full accessible label', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-10-13T04:00:00Z'));
    const expiry = Date.now() / 1000 + 3 * 86400;
    let renderer: ReactTestRenderer | undefined;
    try {
      act(() => {
        renderer = create(
          fixture([
            {
              ...saved,
              baseRemaining: 75,
              baseTotal: 100,
              baseResetAt: expiry,
              rewardRemaining: 25,
              rewardTotal: 100,
              rewardNextExpiryAt: expiry,
              rewardEnabled: true,
            },
          ]).content
        );
      });
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      expect(
        renderer!.root.findAllByProps({ 'aria-label': 'agent.baseCreditsReset 3d0h' })
      ).toHaveLength(0);
      expect(
        renderer!.root.findAllByProps({ 'aria-label': 'agent.rewardCreditsNextExpiry 3d0h' })
      ).toHaveLength(1);
      expect(
        renderer!.root
          .findAllByProps({ 'aria-label': 'agent.rewardCredits' })[0]
          .findAllByType('span')
          .flatMap((node) => node.children.filter((child) => typeof child === 'string'))
      ).toContain('25');
    } finally {
      act(() => renderer?.unmount());
      vi.useRealTimers();
    }
  });

  it('keeps an unclaimed gift disabled for a device-used day, without marking this account claimed', () => {
    const html = fixture([{ ...saved, deviceClaimedAt: Date.now() / 1000 }]).html();
    expect(html).toMatch(/aria-label="agent.claimDailyCredits Saved" disabled=""/);
    expect(html).toContain('lucide-gift');
    expect(html).not.toContain('agent.dailyCreditsClaimed');
    expect(html).not.toContain('lucide-check');
  });

  it.each(['expired cache', 'minute tick'])(
    'hides expired subscription/base/reward dates while retaining balances without account actions (%s)',
    async (scenario) => {
      vi.useFakeTimers();
      vi.setSystemTime(new Date('2026-10-13T04:00:00Z'));
      const now = Date.now() / 1000;
      const future = scenario === 'minute tick';
      const { group, content } = fixture([
        {
          ...saved,
          baseRemaining: 75,
          baseTotal: 100,
          baseResetAt: now + (future ? 15 : -15),
          rewardRemaining: 25,
          rewardTotal: 100,
          rewardNextExpiryAt: now + (future ? 30 : -30),
          subscriptionEndAt: now + (future ? 45 : -45),
          rewardEnabled: true,
        },
      ]);
      let renderer: ReactTestRenderer | undefined;
      try {
        act(() => {
          renderer = create(content);
        });
        await act(async () => {
          await vi.advanceTimersByTimeAsync(0);
        });
        expect(
          renderer!.root.findAllByProps({
            'aria-label': 'agent.rewardCreditsNextExpiry 1m',
          })
        ).toHaveLength(future ? 1 : 0);
        if (future) {
          expect(
            renderer!.root.findAllByProps({ 'aria-label': 'agent.baseCreditsReset 1m' })
          ).toHaveLength(1);
          await act(async () => {
            await vi.advanceTimersByTimeAsync(60_000);
          });
        }
        const spans = renderer!.root.findAllByType('span');
        const text = spans.flatMap((node) =>
          node.children.filter((child) => typeof child === 'string')
        );
        expect(text).toContain('75');
        expect(text).toContain('25');
        expect(text).not.toContain('agent.rewardCreditsNextExpiry');
        expect(
          spans.filter((node) => node.props['aria-label']?.startsWith('accountCenter.subscription'))
        ).toHaveLength(0);
        expect(
          spans.filter((node) => node.props['aria-label']?.startsWith('agent.baseCreditsReset'))
        ).toHaveLength(0);
        expect(
          spans.filter((node) =>
            node.props['aria-label']?.startsWith('agent.rewardCreditsNextExpiry ')
          )
        ).toHaveLength(0);
        expect(spans.some((node) => node.props.className?.includes('min-w-min flex-auto'))).toBe(
          false
        );
        expect(renderer!.root.findByProps({ role: 'radio' }).props.className).not.toContain(
          '_100px]'
        );
        expect(group.add).not.toHaveBeenCalled();
        expect(group.refresh).not.toHaveBeenCalled();
        expect(group.claimDaily).not.toHaveBeenCalled();
        expect(group.select).not.toHaveBeenCalled();
      } finally {
        act(() => renderer?.unmount());
        vi.useRealTimers();
      }
    }
  );

  it('shows the shared check only for an account that claimed today', () => {
    const html = fixture([{ ...saved, dailyClaimedAt: Date.now() / 1000 }]).html();
    expect(html).toContain('aria-label="agent.dailyCreditsClaimed Saved" disabled=""');
    expect(html).toContain('lucide-check');
  });

  it('disables every CN reward on a used device while leaving account claim identity separate', () => {
    const html = fixture([
      { ...saved, id: 'first', name: 'First', deviceClaimedAt: Date.now() / 1000 },
      { ...saved, id: 'second', name: 'Second' },
    ]).html();
    expect(html).toMatch(/aria-label="agent.claimDailyCredits Second" disabled=""/);
    expect(html).not.toContain('agent.dailyCreditsClaimed Second');
    expect(fixture([{ ...saved, rewardEnabled: false }]).html()).toMatch(
      /aria-label="agent.claimDailyCredits Saved" disabled=""/
    );
  });

  it('hides rewards for the international client and uses its existing native icon', () => {
    const html = fixture([{ ...saved, edition: 'trae' }]).html();
    expect(html).toContain('/icons/tools/trae.svg');
    expect(html).not.toContain('agent.claimDailyCredits');
    expect(html).not.toContain('agent.dailyCreditsClaimed');
  });

  it('shows shared login waiting/cancel controls and pending actions without extra controls', () => {
    const pending = fixture([saved], { busy: true, refreshing: new Set([saved.id]) }).html();
    expect(pending).toContain('agent.waitingForBrowser');
    expect(pending).toContain('aria-label="btn.cancel"');
    expect(pending).toMatch(/aria-label="agent.refreshAccount Saved" disabled=""/);
    expect(pending).toMatch(/aria-label="agent.claimDailyCredits Saved" disabled=""/);
  });

  it('isolates reward/refresh/delete clicks and nested keyboard events from selection', async () => {
    const { group, content } = fixture();
    let renderer!: ReactTestRenderer;
    act(() => {
      renderer = create(content);
    });
    const radio = renderer.root.findByProps({ role: 'radio' });
    const stopPropagation = vi.fn();
    for (const label of [
      'agent.claimDailyCredits Saved',
      'agent.refreshAccount Saved',
      'btn.delete Saved',
    ]) {
      const button = renderer.root.findByProps({ 'aria-label': label });
      await act(async () => button.props.onClick({ stopPropagation }));
      for (const key of ['Enter', ' ']) {
        const preventDefault = vi.fn();
        act(() =>
          radio.props.onKeyDown({ key, target: button, currentTarget: radio, preventDefault })
        );
        expect(preventDefault).not.toHaveBeenCalled();
      }
    }
    expect(group.select).not.toHaveBeenCalled();
    expect(stopPropagation).toHaveBeenCalledTimes(3);
    expect(group.claimDaily).toHaveBeenCalledExactlyOnceWith(saved);
    expect(group.refresh).toHaveBeenCalledExactlyOnceWith(saved);
    expect(group.remove).toHaveBeenCalledExactlyOnceWith(saved);
    for (const key of ['Enter', ' ']) {
      act(() =>
        radio.props.onKeyDown({ key, target: radio, currentTarget: radio, preventDefault: vi.fn() })
      );
    }
    expect(group.select).toHaveBeenCalledTimes(2);
    act(() => renderer.unmount());
  });

  it('resets account/device daily status at the China midnight boundary', () => {
    const midnight = Date.parse('2026-10-11T16:00:00Z');
    expect(traeClaimedToday((midnight - 1000) / 1000, midnight + 1000)).toBe(false);
    expect(traeClaimedToday(midnight / 1000, midnight + 1000)).toBe(true);
    expect(traeClaimedToday(null, midnight)).toBe(false);
  });

  it('restores the gift across China midnight for an account without expiry dates without automatic requests', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-10-13T15:59:30Z'));
    const now = Date.now() / 1000;
    const { group, content } = fixture([
      {
        ...saved,
        rewardEnabled: true,
        dailyClaimedAt: now,
        deviceClaimedAt: now,
      },
    ]);
    let renderer: ReactTestRenderer | undefined;
    try {
      act(() => {
        renderer = create(content);
      });
      expect(
        renderer!.root.findByProps({
          'aria-label': 'agent.dailyCreditsClaimed Saved',
        }).props.disabled
      ).toBe(true);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(60_000);
      });
      const gift = renderer!.root.findByProps({ 'aria-label': 'agent.claimDailyCredits Saved' });
      expect(gift.props.disabled).toBe(false);
      expect(group.add).not.toHaveBeenCalled();
      expect(group.refresh).not.toHaveBeenCalled();
      expect(group.claimDaily).not.toHaveBeenCalled();
      expect(group.select).not.toHaveBeenCalled();
      await act(async () => gift.props.onClick({ stopPropagation: vi.fn() }));
      expect(group.claimDaily).toHaveBeenCalledExactlyOnceWith(group.accounts[0]);
    } finally {
      act(() => renderer?.unmount());
      vi.useRealTimers();
    }
  });
});
