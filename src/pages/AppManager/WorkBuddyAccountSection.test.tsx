import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { WorkBuddyAccountSection } from './WorkBuddyAccountSection';
import { AppManagerContext, type AppManagerContextType } from './context';
import type { WorkBuddyAccount } from '../../api/tauri';
import { I18nContext } from '../../hooks/i18nContext';
import { loadLocale, translate } from '../../i18n';

function renderAccount(overrides: Partial<WorkBuddyAccount> = {}, locale?: string, busy = false) {
  const account: WorkBuddyAccount = {
    id: 'test',
    name: 'test@example.com',
    edition: 'workbuddy',
    plan: 'Free',
    remaining: 600,
    total: 600,
    baseRemaining: 500,
    baseTotal: 500,
    baseResetAt: 1900000000,
    rewardRemaining: 100,
    rewardTotal: 100,
    addonRemaining: 0,
    dailyClaimedAt: Math.floor(Date.now() / 1000),
    expiresAt: 1900000000,
    active: false,
    ...overrides,
  };
  const context = {
    selectedTool: account.edition,
    workBuddyAccounts: {
      accounts: [account],
      selectedId: 'test',
      busy,
      remainingSeconds: 100,
      refreshing: new Set(),
      authorizationFailedIds: new Set(),
      select: () => {},
      add: async () => {},
      refresh: async () => {},
      claimDaily: async () => {},
      remove: async () => {},
    },
  } as unknown as AppManagerContextType;
  return renderToStaticMarkup(
    <I18nContext.Provider
      value={{
        locale: locale || 'en',
        setLocale: () => {},
        t: (key) => (locale ? translate(key, locale) : key),
      }}
    >
      <AppManagerContext.Provider value={context}>
        <WorkBuddyAccountSection showDivider={false} />
      </AppManagerContext.Provider>
    </I18nContext.Provider>
  );
}

describe('WorkBuddy account card', () => {
  it.each(['体验版', '标准版', '高级版', '旗舰版', 'Free', 'Pro', 'Team', null])(
    'shows the account tier %s above the actions',
    (plan) => {
      const markup = renderAccount({ plan });
      const planIndex = markup.indexOf(`>${plan || '—'}</span>`);
      expect(planIndex).toBeGreaterThan(-1);
      expect(planIndex).toBeLessThan(markup.indexOf('aria-label="agent.baseCredits"'));
      expect(markup.indexOf('aria-label="agent.baseCredits"')).toBeLessThan(
        markup.indexOf('aria-label="agent.dailyCreditsClaimed test@example.com"')
      );
    }
  );
  it.each(['workbuddy', 'workbuddyai'] as const)(
    'keeps the existing compact actions for %s',
    (edition) => {
      const markup = renderAccount({ edition });
      expect(markup).toContain(`/icons/tools/${edition}.png`);
      expect(markup).toContain('test@example.com');
      expect(markup).toContain('agent.baseCredits');
      expect(markup).toContain('>500</span>');
      expect(markup).toContain('agent.rewardCredits');
      expect(markup.includes('agent.dailyCreditsClaimed')).toBe(edition === 'workbuddy');
      expect(markup).toContain('aria-checked="true"');
      expect(markup).toContain('grid h-14');
      expect(markup).not.toContain('title=');
      expect(markup).not.toContain('tooltip');
      expect(markup).not.toContain('cursor-');
    }
  );
  it('shows purchased credits only when positive, without a stray dash', () => {
    expect(renderAccount({ addonRemaining: 20 })).toContain('>20</span>');
    for (const value of [0, null]) {
      const markup = renderAccount({ addonRemaining: value });
      expect(markup).not.toContain('agent.purchasedCredits');
    }
  });
  it('shows base, purchased, and all reward balances after today’s claim', async () => {
    await loadLocale('zh-Hans');
    const markup = renderAccount({ remaining: 1600, addonRemaining: 1000 }, 'zh-Hans');
    expect(markup).not.toContain('总1600');
    expect(markup).not.toContain('500/500');
    expect(markup).toContain('>500</span>');
    expect(markup).toContain('>1000</span>');
    expect(markup).toContain('奖励积分');
    expect(renderAccount({ rewardRemaining: 0 }, 'zh-Hans')).toContain('奖励积分');
    expect(renderAccount({ rewardRemaining: 0 }, 'zh-Hans')).toContain('>0</span>');
  });
  it('uses unclaimed in place of the reward balance until today’s claim', async () => {
    await loadLocale('zh-Hans');
    const markup = renderAccount({ dailyClaimedAt: null, addonRemaining: 1000 }, 'zh-Hans');
    expect(markup).toContain('>未领取</span>');
    expect(markup).not.toContain('奖励积分');
    expect(markup).toContain('>1000</span>');
  });
  it('preserves unknown base and hides unknown reward details', () => {
    const markup = renderAccount({ remaining: null, rewardRemaining: null, baseRemaining: null });
    expect(markup).toContain('agent.baseCredits');
    expect(markup).toContain('model.noUsageData');
    expect(markup).not.toContain('agent.rewardCredits');
  });
  it.each([
    ['zh-Hans', '未领取'],
    ['zh-Hant', '未領取'],
    ['en', 'Unclaimed'],
    ['ja', '未受取'],
  ])('labels unclaimed credits in %s', async (locale, unclaimed) => {
    await loadLocale(locale);
    const markup = renderAccount({ addonRemaining: 20, dailyClaimedAt: null }, locale);
    expect(markup).toContain('>20</span>');
    expect(markup).toContain(`>${unclaimed}</span>`);
  });
  it('uses the existing authorization waiting state', () => {
    const markup = renderAccount({ edition: 'workbuddyai' }, undefined, true);
    expect(markup).toContain('aria-label="btn.cancel"');
    expect(markup).toContain('agent.waitingForBrowser');
    expect(markup).not.toContain('agent.addCurrentAccount');
  });
});
