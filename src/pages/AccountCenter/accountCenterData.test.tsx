import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { describe, expect, it, vi } from 'vitest';
import { AppManagerContext, type AppManagerContextType } from '../AppManager/context';
import { useNavigationStore } from '../../stores/navigationStore';
import { usePluginsStore } from '../../stores/pluginsStore';
import { installedAccountPlugins } from '../../test/accountPlugins';
import type { useClaudeCodeAccounts } from '../AppManager/useClaudeCodeAccounts';
import { useAccountPrivacyStore } from '../../stores/accountPrivacyStore';
import { useRewardClaimStore } from '../../stores/rewardClaimStore';
import { AccountSectionRow } from '../AppManager/AccountSectionPrimitives';
import { OpenDesignAccountSection } from '../AppManager/OpenDesignAccountSection';
import { AccountCard, AccountCenterTitleActions } from './AccountCenter';
import { accountCenterProviders } from './accountCenterData';
import type { QwenWorkAccount } from '../../api/qwenwork';
import type { DuMateAccount } from '../../api/dumate';
import type { DoubaoAccount } from '../../api/doubao';

vi.mock('../../hooks/useI18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }));
const openAccountPlugins = vi.hoisted(() => vi.fn());
vi.mock('../AppManager/openAccountPlugins', () => ({ openAccountPlugins }));

function context() {
  const group = () => ({
    accounts: [],
    busy: false,
    loading: false,
    remainingSeconds: 0,
    refreshing: new Set<string>(),
    authorizationFailedIds: new Set<string>(),
    add: vi.fn(),
    refresh: vi.fn(),
    claimDaily: vi.fn(),
    remove: vi.fn(),
  });
  return {
    detectedTools: [],
    setApplyError: vi.fn(),
    codexAccounts: [],
    isAddingCodexAccount: false,
    isLoadingCodexAccounts: false,
    codexOAuthRemainingSeconds: 0,
    refreshingCodexAccountIds: new Set<string>(),
    codexAuthorizationFailedIds: new Set<string>(),
    addCodexAccount: vi.fn(),
    refreshCodexAccountQuota: vi.fn(),
    deleteCodexAccount: vi.fn(),
    claudeCodeAccounts: group(),
    claudeDesktopAccounts: group(),
    deepSeekAccounts: group(),
    workBuddyAccountGroups: { workbuddy: group(), workbuddyai: group() },
    traeAccountGroups: { trae: group(), traecn: group() },
    kiroAccounts: group(),
    antigravityAccounts: group(),
    cursorAccounts: group(),
    zcodeAccounts: group(),
    kimiAccounts: { ...group(), region: 'cn', setRegion: vi.fn() },
    openDesignAccounts: group(),
    minimaxAccounts: group(),
    qoderAccountGroups: { qoder: group(), qodercn: group() },
    qwenWorkAccountGroups: { qwenwork: group(), qwenworkcn: group() },
    dumateAccounts: group(),
    doubaoAccountGroups: { doubaowork: group(), dola: group() },
    grokAccounts: group(),
    kiloAccounts: group(),
    clineAccounts: group(),
    copilotAccounts: group(),
    grokBotAccounts: group(),
    manusAccounts: group(),
    cueAccounts: group(),
  } as unknown as AppManagerContextType;
}

const providers = (state: AppManagerContextType) =>
  accountCenterProviders(state, (key) => key, 'en');

describe('Account Center display data', () => {
  it.each([
    'claudecode',
    'claudedesktop',
    'doubaowork',
    'dola',
    'workbuddy',
    'workbuddyai',
  ] as const)('%s redirects account add to the complete plugin list', (id) => {
    const state = context();
    openAccountPlugins.mockClear();
    openAccountPlugins.mockResolvedValue(undefined);
    state.detectedTools = [{ id, name: id, category: 'Desktop', installed: true }];
    const entry = providers(state).find((provider) => provider.id === id)!;

    entry.add();

    expect(openAccountPlugins).toHaveBeenCalledWith();
  });

  it.each([false, true])(
    'shares one Cursor group when CLI is installed and editor installed=%s',
    (editorInstalled) => {
      const state = context();
      state.detectedTools = [
        {
          id: 'cursorcli',
          name: 'Cursor CLI',
          category: 'CLI Code',
          installed: true,
          noModelConfig: true,
        },
        {
          id: 'cursor',
          name: 'Cursor',
          category: 'Desktop',
          installed: editorInstalled,
          noModelConfig: true,
        },
      ];
      const groups = providers(state).filter((entry) => entry.id === 'cursor');
      expect(groups).toHaveLength(1);
      expect(groups[0].installed).toBe(true);
      expect(groups[0].icon).toBe('/icons/tools/cursor.svg');
      expect(state.cursorAccounts.add).not.toHaveBeenCalled();
      expect(state.cursorAccounts.refresh).not.toHaveBeenCalled();
    }
  );
  it('keeps overlapping Doubao windows separate from bonus credits and uses real plan names', () => {
    const state = context();
    const row: DoubaoAccount = {
      id: 'doubao',
      edition: 'doubaowork',
      name: 'Fixture',
      email: null,
      active: false,
      plan: '体验套餐',
      expiresAt: null,
      remaining: null,
      total: null,
      quota: {
        windows: [
          {
            kind: '5h',
            label: null,
            remaining: 735,
            total: 735,
            usedPercent: 0,
            resetAt: null,
            unlimited: false,
          },
          {
            kind: '7d',
            label: null,
            remaining: null,
            total: null,
            usedPercent: 20,
            resetAt: 1800000000,
            unlimited: false,
          },
        ],
        bonus: { remaining: 200, total: 200, usedPercent: 0, expiresAt: 1800100000 },
      },
      dailyReward: { status: 'available', scene: 'fixture', nextClaimAt: null },
    };
    state.doubaoAccountGroups.doubaowork.accounts = [row];
    const entry = providers(state).find((p) => p.id === 'doubaowork')!.accounts[0];
    expect(entry.plan).toBe('体验套餐');
    expect(entry.metrics).toMatchObject([
      { label: '5h', value: '735', percent: 100, resetAt: null },
      { label: '7d', value: '80%', percent: 80, resetAt: 1800000000 },
      { label: 'agent.rewardCredits', value: '200', percent: 100, resetAt: 1800100000 },
    ]);
    expect(entry.subscriptionEndAt).toBeNull();
    expect(state.doubaoAccountGroups.doubaowork.refresh).not.toHaveBeenCalled();
    expect(state.doubaoAccountGroups.doubaowork.claimDaily).not.toHaveBeenCalled();
  });
  it.each(['doubaowork', 'dola'] as const)(
    '%s shows only actual plan and quota without rewards or passive refresh',
    (edition) => {
      const state = context();
      const row: DoubaoAccount = {
        id: edition,
        edition,
        name: 'Fixture',
        email: null,
        active: false,
        plan: null,
        expiresAt: null,
        remaining: null,
        total: null,
      };
      state.doubaoAccountGroups[edition].accounts = [row];
      let entry = providers(state).find((p) => p.id === edition)!.accounts[0];
      expect(entry.plan).toBeNull();
      expect(entry.metrics).toEqual([
        { label: 'agent.credits', value: '—', resetAt: undefined, percent: undefined },
      ]);
      expect(entry.claim).toBeUndefined();
      row.plan = 'Pro';
      row.remaining = 40;
      row.total = 100;
      entry = providers(state).find((p) => p.id === edition)!.accounts[0];
      expect(entry.plan).toBe('Pro');
      expect(entry.metrics[0]).toMatchObject({ value: '40', percent: 40 });
      expect(state.doubaoAccountGroups[edition].refresh).not.toHaveBeenCalled();
    }
  );
  it('DuMate uses real package and reward capacities without refreshing during rendering', async () => {
    const state = context();
    const row: DuMateAccount = {
      id: 'dumate',
      name: 'Fixture',
      enterprise: false,
      plan: 'pro',
      expiresAt: null,
      active: false,
      remaining: 1350,
      total: 1600,
      dailyReward: null,
      creditBuckets: [
        {
          name: 'plan_pro',
          source: 'subscription',
          reward: false,
          remaining: 900,
          total: 1000,
          expiresAt: null,
        },
        {
          name: 'plan_point',
          source: 'purchase',
          reward: false,
          remaining: 50,
          total: 100,
          expiresAt: null,
        },
        {
          name: 'grant_point',
          source: 'login_bonus',
          reward: true,
          remaining: 400,
          total: 500,
          expiresAt: null,
        },
      ],
    };
    state.dumateAccounts.accounts = [row];
    const account = providers(state).find((p) => p.id === 'dumate')!.accounts[0];
    expect(account.plan).toBe('Pro');
    expect(account.metrics).toMatchObject([
      { label: 'agent.baseCredits', value: '950', percent: (950 / 1100) * 100, showProgress: true },
      { label: 'agent.rewardCredits', value: '400', percent: 80, showProgress: true },
    ]);
    expect(state.dumateAccounts.refresh).not.toHaveBeenCalled();
    expect(state.dumateAccounts.claimDaily).not.toHaveBeenCalled();
    await account.claim!();
    expect(state.dumateAccounts.claimDaily).toHaveBeenCalledWith(row, undefined);
    row.enterprise = true;
    expect(providers(state).find((p) => p.id === 'dumate')!.accounts[0].claim).toBeUndefined();
  });
  it.each([null, 0, 100])(
    'colors exhausted DuMate credits red even when their capacity is %s',
    (total) => {
      const state = context();
      const row: DuMateAccount = {
        id: 'dumate-empty',
        name: 'Fixture',
        enterprise: false,
        plan: 'free',
        expiresAt: null,
        active: false,
        remaining: 100,
        total: null,
        dailyReward: null,
        creditBuckets: [
          {
            name: 'package',
            source: 'subscription',
            reward: false,
            remaining: 0,
            total,
            expiresAt: null,
          },
          {
            name: 'reward',
            source: 'login_bonus',
            reward: true,
            remaining: 100,
            total: null,
            expiresAt: null,
          },
        ],
      };
      state.dumateAccounts.accounts = [row];
      let provider = providers(state).find((p) => p.id === 'dumate')!;
      let html = renderToStaticMarkup(
        <AccountCard provider={provider} account={provider.accounts[0]} />
      );
      expect(html).toMatch(/class="[^"]*text-cyber-error[^"]*"[^>]*>0<\/span>/);
      expect(html).toContain('border-cyber-error/20');
      expect(html).toMatch(/class="[^"]*text-green-500[^"]*"[^>]*>100<\/span>/);
      expect(html).toContain('border-green-500');
      expect(html.match(/role="progressbar"/g) ?? []).toHaveLength(total ? 1 : 0);

      row.remaining = null;
      row.creditBuckets = [];
      provider = providers(state).find((p) => p.id === 'dumate')!;
      html = renderToStaticMarkup(
        <AccountCard provider={provider} account={provider.accounts[0]} />
      );
      expect(html).not.toContain('text-cyber-error');
      expect(html).not.toContain('border-cyber-error');
      expect(html).not.toContain('role="progressbar"');
      expect(state.dumateAccounts.refresh).not.toHaveBeenCalled();
      expect(state.dumateAccounts.claimDaily).not.toHaveBeenCalled();
    }
  );
  it.each(['qwenworkcn', 'qwenwork'] as const)(
    'shows %s identity and edition-specific credit rows without adding organization pools or passive requests',
    (edition) => {
      const state = context();
      state.qwenWorkAccountGroups[edition].accounts = [
        {
          id: edition,
          edition,
          name: 'Fixture',
          email: null,
          organization: 'Fixture team',
          plan: 'enterprise_premium',
          planName: 'Enterprise Premium',
          expiresAt: null,
          active: false,
          remaining: 80,
          total: 100,
          creditBuckets: [
            { name: 'Member', remaining: 80, total: 100, expiresAt: null },
            { name: 'Organization', remaining: 15, total: null, expiresAt: null },
          ],
          dailyReward: null,
        },
      ];
      const account = providers(state).find((provider) => provider.id === edition)!.accounts[0];
      expect(account.identity).toBe('Fixture · Fixture team');
      expect(account.detail).toBeUndefined();
      const provider = providers(state).find((provider) => provider.id === edition)!;
      const previouslyHidden = useAccountPrivacyStore.getState().hidden;
      let card!: ReactTestRenderer;
      try {
        act(() => {
          useAccountPrivacyStore.setState({ hidden: false });
          card = create(<AccountCard provider={provider} account={account} />);
        });
        expect(JSON.stringify(card.toJSON())).toContain('Fixture team');
        act(() => {
          useAccountPrivacyStore.setState({ hidden: true });
        });
        expect(JSON.stringify(card.toJSON())).not.toContain('Fixture team');
      } finally {
        act(() => {
          card?.unmount();
          useAccountPrivacyStore.setState({ hidden: previouslyHidden });
        });
      }
      expect(account.plan).toBe(
        edition === 'qwenwork' ? 'Enterprise Premium' : 'agent.qwenworkEnterprisePremium'
      );
      expect(account.metrics.map((metric) => metric.value)).toEqual(
        edition === 'qwenwork' ? ['80'] : ['80', '—']
      );
      expect(account.claim).toBeUndefined();
      expect(state.qwenWorkAccountGroups[edition].refresh).not.toHaveBeenCalled();
      state.qwenWorkAccountGroups[edition].accounts[0].remaining = null;
      state.qwenWorkAccountGroups[edition].accounts[0].total = null;
      state.qwenWorkAccountGroups[edition].accounts[0].creditBuckets = [];
      expect(
        providers(state).find((provider) => provider.id === edition)!.accounts[0].metrics[0].value
      ).toBe('—');
    }
  );
  it.each(['qwenworkcn', 'qwenwork'] as const)(
    'shows %s package and reward balances without overlap or unsupported reward controls',
    (edition) => {
      const state = context();
      state.qwenWorkAccountGroups[edition].accounts = [
        {
          id: edition,
          edition,
          name: 'Fixture',
          email: null,
          organization: null,
          plan: 'free',
          planName: 'Free',
          expiresAt: null,
          active: false,
          remaining: 2099.7501,
          total: null,
          creditBuckets: [
            { name: 'Daily', remaining: 99.7501, total: 100, expiresAt: null },
            { name: 'Monthly', remaining: 0, total: null, expiresAt: null },
            { name: 'Long-term', remaining: 2000, total: null, expiresAt: null },
          ],
          dailyReward: null,
        },
      ];
      state.qwenWorkAccountGroups[edition].accounts[0].dailyReward = {
        status: 'CLAIMABLE',
        rewardCredits: 200,
        checkedAt: Date.now() / 1000,
        nextClaimAt: Date.now() / 1000 + 3600,
        expiresAt: null,
      };
      const provider = providers(state).find((provider) => provider.id === edition)!;
      const account = provider.accounts[0];
      expect(account.metrics).toHaveLength(edition === 'qwenwork' ? 1 : 2);
      expect(account.metrics[0]).toMatchObject({
        label: 'agent.baseCredits',
        value: edition === 'qwenwork' ? '2,099.75' : '2,000',
        percent: undefined,
        showProgress: true,
        balanceState: 'available',
      });
      if (edition === 'qwenworkcn')
        expect(account.metrics[1]).toMatchObject({
          label: 'agent.rewardCredits',
          value: '99.75',
          percent: 99.7501,
          showProgress: true,
        });
      const html = renderToStaticMarkup(<AccountCard provider={provider} account={account} />);
      expect(html).not.toContain('Monthly');
      expect(html).not.toContain('Long-term');
      expect(html).toContain('border-green-500');
      if (edition === 'qwenworkcn') expect(html).toContain('role="progressbar"');
      else {
        expect(html).not.toContain('text-2xl');
        expect(html).toContain('>agent.baseCredits</span>');
        expect(html).not.toContain('agent.rewardCredits');
        expect(html).not.toContain('agent.claimDailyCredits');
        expect(html).not.toContain('aria-valuenow=');
        expect(account.plan).toBe('Free');
      }
      expect(account.claim).toBeUndefined();
      expect(html).not.toContain('agent.claimDailyCredits');
      expect(html).not.toContain('agent.dailyCreditsUnclaimed');
      expect(state.qwenWorkAccountGroups[edition].refresh).not.toHaveBeenCalled();
      state.qwenWorkAccountGroups[edition].accounts[0].creditBuckets[0].total = null;
      const updated = providers(state).find((p) => p.id === edition)!.accounts[0];
      if (edition === 'qwenworkcn') {
        expect(updated.metrics[1].percent).toBeUndefined();
        const withoutCapacity = renderToStaticMarkup(
          <AccountCard provider={provider} account={updated} />
        );
        expect(withoutCapacity).toContain('border-green-500');
        expect(withoutCapacity).not.toContain('aria-valuenow=');
      }
    }
  );
  it('keeps zero and unknown QwenWork balances distinct, and uses real limits for real progress', () => {
    const state = context();
    const row: QwenWorkAccount = {
      id: 'credits',
      edition: 'qwenworkcn' as const,
      name: 'Fixture',
      email: null,
      organization: null,
      plan: 'free',
      planName: 'Free',
      expiresAt: null,
      active: false,
      remaining: 0,
      total: null,
      creditBuckets: [
        { name: 'Daily', remaining: 0, total: null, expiresAt: null },
        { name: 'Monthly', remaining: 0, total: null, expiresAt: null },
        { name: 'Long-term', remaining: 0, total: null, expiresAt: null },
      ],
      dailyReward: null,
    };
    state.qwenWorkAccountGroups.qwenworkcn.accounts = [row];
    let provider = providers(state).find((p) => p.id === 'qwenworkcn')!;
    expect(provider.accounts[0].metrics.map((metric) => metric.value)).toEqual(['0', '0']);
    expect(provider.accounts[0].metrics.map((metric) => metric.balanceState)).toEqual([
      'empty',
      'empty',
    ]);
    let html = renderToStaticMarkup(
      <AccountCard provider={provider} account={provider.accounts[0]} />
    );
    expect(html).not.toContain('border-green-500');
    expect(html).toContain('text-cyber-error');
    expect(html).toContain('border-cyber-error/20');
    expect(html).not.toContain('aria-valuenow=');
    row.remaining = 2099.75;
    row.creditBuckets = [
      { name: 'Daily', remaining: 99.75, total: 100, expiresAt: null },
      { name: 'Monthly', remaining: 0, total: 0, expiresAt: null },
      { name: 'Long-term', remaining: 2000, total: 2000, expiresAt: null },
    ];
    provider = providers(state).find((p) => p.id === 'qwenworkcn')!;
    expect(provider.accounts[0].metrics.map((metric) => metric.percent)).toEqual([100, 99.75]);
    html = renderToStaticMarkup(<AccountCard provider={provider} account={provider.accounts[0]} />);
    expect(html.match(/role="progressbar"/g)).toHaveLength(2);
    row.remaining = null;
    row.creditBuckets = [];
    provider = providers(state).find((p) => p.id === 'qwenworkcn')!;
    expect(provider.accounts[0].metrics.map((metric) => metric.value)).toEqual(['—', '—']);
  });
  it('shows a Kimi Free account without returned quota as no usage data', () => {
    const state = context();
    state.kimiAccounts.accounts = [
      {
        id: 'kimi-free',
        email: 'free@example.test',
        region: 'cn',
        active: false,
        plan: 'Free',
        quotaWindows: [],
        wallet: null,
      },
    ];
    const provider = providers(state).find((p) => p.id === 'kimi')!;
    const account = provider.accounts[0];
    expect(account.plan).toBe('Free');
    const markup = renderToStaticMarkup(<AccountCard provider={provider} account={account} />);
    expect(markup).toContain('model.noUsageData');
    expect(markup).not.toContain('role="progressbar"');
    expect(markup).not.toContain('>0%<');
  });

  it.each(['Free', 'Plus', 'Pro', 'Max', 'Ultra'])(
    'keeps Kimi %s and real windows separate from the booster wallet',
    (plan) => {
      const state = context();
      state.kimiAccounts.accounts = [
        {
          id: 'kimi',
          email: 'kimi@example.test',
          region: 'global',
          active: false,
          plan,
          quotaWindows: [
            { name: 'limit_5h', remainingPercent: 80, resetAt: 2_000_000_000 },
            { name: 'limit_month_code', remainingPercent: 45 },
          ],
          wallet: { remaining: 13.5, total: 20, currency: 'CNY' },
        },
      ];
      const account = providers(state).find((p) => p.id === 'kimi')!.accounts[0];
      expect(account.detail).toBe('kimi.ai');
      expect(account.plan).toBe(plan);
      expect(account.metrics).toMatchObject([
        { label: 'agent.kimi5h', value: '80%', resetAt: 2_000_000_000 },
        { label: 'agent.kimiMonthCode', value: '45%' },
        { label: 'model.balance', percent: 67.5 },
      ]);
      expect(account.metrics[2].value).toContain('13.50');
      state.kimiAccounts.accounts = [
        { ...state.kimiAccounts.accounts[0], plan: null, quotaWindows: [], wallet: null },
      ];
      const unknown = providers(state).find((p) => p.id === 'kimi')!.accounts[0];
      expect(unknown.plan).toBeNull();
      expect(unknown.metrics).toMatchObject([{ value: '—' }]);
    }
  );
  it('shows Open Design vendor windows separately and keeps a missing balance unknown', () => {
    const state = context();
    const account = {
      id: 'design',
      email: 'designer@example.test',
      active: false,
      plan: 'go',
      quotaWindows: [
        { label: '5h', remainingPercent: 75, resetAt: 2_000_000_000 },
        { label: '7d', remainingPercent: 60, resetAt: 2_000_100_000 },
      ],
      balanceUsd: null,
    };
    state.openDesignAccounts.accounts = [account];
    const find = () =>
      providers(state).find((provider) => provider.id === 'opendesign')!.accounts[0];
    expect(find().metrics).toMatchObject([
      { label: '5h', value: '75%', percent: 75 },
      { label: '7d', value: '60%', percent: 60 },
    ]);
    expect(find().identity).toBe(account.email);
    expect(find().plan).toBe('go');
    state.openDesignAccounts.accounts = [{ ...account, quotaWindows: [] }];
    expect(find().metrics).toEqual([
      { label: 'model.balance', value: '—', resetAt: undefined, percent: undefined },
    ]);
    state.openDesignAccounts.accounts = [{ ...account, quotaWindows: [], balanceUsd: 0 }];
    expect(find().metrics[0].value).toBe('$0');
  });
  it.each([
    ['free', 'Free'],
    ['go', 'Go'],
    ['plus', 'Plus'],
    ['pro', 'Pro'],
    ['max', 'Max'],
    ['team', 'Team'],
  ])('shows Open Design %s in both account views', (plan, label) => {
    const state = context();
    state.openDesignAccounts.accounts = [
      {
        id: 'design',
        email: 'designer@example.test',
        active: false,
        plan,
        quotaWindows: [],
        balanceUsd: null,
        subscriptionEndAt: null,
      },
    ];
    const provider = providers(state).find((p) => p.id === 'opendesign')!;
    const card = renderToStaticMarkup(
      <AccountCard provider={provider} account={provider.accounts[0]} />
    );
    let sidebarRenderer!: ReactTestRenderer;
    act(() => {
      sidebarRenderer = create(
        <AppManagerContext.Provider value={state}>
          <OpenDesignAccountSection />
        </AppManagerContext.Provider>
      );
    });
    const sidebar = JSON.stringify(sidebarRenderer.toJSON());
    act(() => sidebarRenderer.unmount());
    expect(card).toContain(`>${label}<`);
    expect(sidebar).toContain(`"${label}"`);
    expect(card).toContain('model.noUsageData');
    expect(sidebar).not.toContain('0%');
    expect(state.openDesignAccounts.refresh).not.toHaveBeenCalled();
  });
  it('keeps an unqueried Open Design plan unknown', () => {
    const state = context();
    state.openDesignAccounts.accounts = [
      {
        id: 'design',
        email: 'designer@example.test',
        active: false,
        plan: null,
        quotaWindows: [],
        balanceUsd: null,
        subscriptionEndAt: null,
      },
    ];
    const provider = providers(state).find((p) => p.id === 'opendesign')!;
    expect(provider.accounts[0].plan).toBeNull();
    let sidebarRenderer!: ReactTestRenderer;
    act(() => {
      sidebarRenderer = create(
        <AppManagerContext.Provider value={state}>
          <OpenDesignAccountSection />
        </AppManagerContext.Provider>
      );
    });
    const sidebar = JSON.stringify(sidebarRenderer.toJSON());
    act(() => sidebarRenderer.unmount());
    expect(sidebar).toContain('—');
    expect(sidebar).not.toContain('"Free"');
    expect(state.openDesignAccounts.refresh).not.toHaveBeenCalled();
  });
  it('hides the identity and action labels while preserving quota, plan and real action targets', () => {
    const state = context();
    const provider = {
      ...providers(state).find((entry) => entry.id === 'codex')!,
      installed: true,
    };
    const account = {
      id: 'private-account',
      identity: 'private@example.test',
      plan: 'Pro 100',
      refreshing: false,
      authorizationFailed: false,
      metrics: [{ label: '7d', value: '69%', percent: 69 }],
      refresh: vi.fn().mockResolvedValue(undefined),
      remove: vi.fn(),
      claim: vi.fn().mockResolvedValue(undefined),
    };
    let renderer: ReactTestRenderer | undefined;
    try {
      act(() => {
        renderer = create(
          <AppManagerContext.Provider value={state}>
            <AccountCenterTitleActions />
            <AccountCard provider={provider} account={account} />
            <AccountSectionRow
              email={account.identity}
              selected={false}
              plan={account.plan}
              onSelect={vi.fn()}
              onDelete={account.remove}
            />
          </AppManagerContext.Provider>
        );
      });
      expect(renderer!.root.findByType('h3').children).toEqual([account.identity]);
      act(() =>
        renderer!.root.findByProps({ 'aria-label': 'freeModels.hideAccountNames' }).props.onClick()
      );
      const hidden = JSON.stringify(renderer!.toJSON());
      expect(hidden).not.toContain(account.identity);
      expect(renderer!.root.findByType('h3').children).toEqual(['******']);
      expect(hidden).toContain('Pro 100');
      expect(hidden).toContain('69%');
      expect(account.identity).toBe('private@example.test');
      expect(account.refresh).not.toHaveBeenCalled();
      expect(account.claim).not.toHaveBeenCalled();
      expect(account.remove).not.toHaveBeenCalled();
      act(() =>
        renderer!.root
          .findByProps({ 'aria-label': 'agent.refreshAccount ******' })
          .props.onClick({ stopPropagation: vi.fn() })
      );
      expect(account.refresh).toHaveBeenCalledOnce();
      act(() =>
        renderer!.root.findByProps({ 'aria-label': 'freeModels.showAccountNames' }).props.onClick()
      );
      expect(renderer!.root.findByType('h3').children).toEqual([account.identity]);
      expect(account.refresh).toHaveBeenCalledOnce();
    } finally {
      act(() => renderer?.unmount());
      useAccountPrivacyStore.setState({ hidden: false });
    }
  });

  it('keeps the identity and plan in the same grid row when two unknown quotas become available', () => {
    const state = context();
    state.claudeCodeAccounts.accounts = [
      { id: 'saved', email: 'saved@example.test', plan: 'Max', active: true },
    ];
    const provider = providers(state).find((entry) => entry.id === 'claudecode')!;
    const account = provider.accounts[0];
    const renderer = create(<AccountCard provider={provider} account={account} />);
    const gridClass = 'grid min-h-0 flex-1 grid-rows-[30%_35%_35%] items-center';
    try {
      const unknownGrid = renderer.root.findByProps({ className: gridClass });
      const identity = unknownGrid.children[0];
      expect(renderer.root.findByType('article').props.className).toContain('flex h-44');
      expect(renderer.root.findByProps({ role: 'status' }).props.className).toContain(
        'row-span-2 flex h-full items-center justify-center'
      );
      expect(renderer.root.findAllByProps({ role: 'progressbar' })).toHaveLength(0);
      act(() =>
        renderer.update(
          <AccountCard
            provider={provider}
            account={{
              ...account,
              metrics: [
                { label: '5h', value: '0%', percent: 0 },
                { label: '7d', value: '80%', percent: 80 },
              ],
            }}
          />
        )
      );
      const knownGrid = renderer.root.findByProps({ className: gridClass });
      expect(knownGrid.children[0]).toBe(identity);
      expect(renderer.root.findByType('h3').children).toEqual(['saved@example.test']);
      expect(JSON.stringify(renderer.toJSON())).toContain('Max');
      expect(renderer.root.findByType('article').props.className).toContain('flex h-44');
      expect(renderer.root.findAllByProps({ role: 'status' })).toHaveLength(0);
      expect(renderer.root.findAllByProps({ role: 'progressbar' })).toHaveLength(2);
      expect(renderer.root.findAllByProps({ role: 'progressbar' })[0].props['aria-valuenow']).toBe(
        0
      );
    } finally {
      act(() => renderer.unmount());
    }
  });

  it.each([
    [undefined, '—'],
    [null, '—'],
    [0, '$0.00'],
    [12_340_000, '$12.34'],
  ] as const)(
    'shows Cline micro-dollar balance %s without inventing quota',
    (balanceMicros, value) => {
      const state = context();
      state.detectedTools = [
        { id: 'cline', name: 'Cline CLI', category: 'CLI Code', installed: false },
        { id: 'clinedesktop', name: 'Cline Desktop', category: 'Desktop', installed: true },
      ];
      state.clineAccounts.accounts = [
        {
          id: 'saved-cline',
          email: 'cline@example.test',
          active: true,
          plan: 'ClinePass',
          balanceMicros,
          subscriptionEndAt: 1900000000,
        },
      ];
      const matching = providers(state).filter((provider) => provider.id === 'cline');
      expect(matching).toHaveLength(1);
      const provider = matching[0];
      const account = provider.accounts[0];
      expect(provider.installed).toBe(true);
      expect(account.metrics).toEqual([{ label: 'model.balance', value }]);
      expect(account.plan).toBe('ClinePass');
      expect(account.subscriptionEndAt).toBe(1900000000);
      const markup = renderToStaticMarkup(<AccountCard provider={provider} account={account} />);
      expect(markup).toContain(balanceMicros == null ? 'model.noUsageData' : value);
      expect(markup).toContain('flex h-44 flex-col gap-2 rounded-card');
      if (balanceMicros == null) expect(markup).not.toContain('model.balance —');
      expect(markup).not.toContain('role="progressbar"');
      expect(markup).not.toContain('agent.claimDailyCredits');
      expect(state.clineAccounts.accounts[0].balanceMicros).toBe(balanceMicros);
      expect(state.clineAccounts.refresh).not.toHaveBeenCalled();
      expect(state.clineAccounts.add).not.toHaveBeenCalled();
    }
  );

  it('surfaces the Cline device confirmation code through the existing waiting state', () => {
    const state = context();
    state.clineAccounts.busy = true;
    state.clineAccounts.login = {
      loginId: 'login',
      userCode: 'ABCD-EFGH',
      verificationUri: 'https://auth.example.test/device',
      expiresAt: 1900000000,
    };
    const provider = providers(state).find((entry) => entry.id === 'cline')!;
    expect(provider.waitingLabel).toBe('ABCD-EFGH · agent.waitingForBrowser');
    expect(state.clineAccounts.add).not.toHaveBeenCalled();
  });

  it.each(['Free', 'Go', 'Explore', 'Build', 'M Plan Future', 'custom plan'])(
    'displays MiniMax %s without altering the saved raw plan or credit metric',
    (tier) => {
      const state = context();
      const plan = ['M Plan Future', 'custom plan'].includes(tier) ? tier : `M Plan ${tier}`;
      state.minimaxAccounts.accounts = [
        {
          id: 'saved',
          region: 'cn',
          email: 'saved@example.test',
          active: false,
          plan,
          quota: { credits: 42, windows: [] },
        },
      ];
      const provider = providers(state).find((p) => p.id === 'minimax')!;
      expect(provider.accounts[0].plan).toBe(tier);
      expect(
        renderToStaticMarkup(<AccountCard provider={provider} account={provider.accounts[0]} />)
      ).toContain(`>${tier}</span>`);
      expect(provider.accounts[0].metrics[0].value).toBe('42');
      expect(state.minimaxAccounts.accounts[0].plan).toBe(plan);
      expect(state.minimaxAccounts.refresh).not.toHaveBeenCalled();
      expect(state.minimaxAccounts.claimDaily).not.toHaveBeenCalled();
    }
  );

  it('expires a MiniMax gift cache locally without displaying a fabricated daily reset', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-10-13T12:13:14Z'));
    const state = context();
    state.detectedTools = [
      { id: 'minimaxdesktop', name: 'MiniMax Desktop', category: 'Desktop', installed: true },
    ];
    state.minimaxAccounts.accounts = [
      {
        id: 'cached',
        region: 'global',
        email: 'cached@example.test',
        active: false,
        plan: 'M Plan Explore',
        quota: { credits: 42, windows: [] },
        dailyReward: { status: 'claimed', checkedAt: Date.now() / 1000 },
      },
    ];
    const provider = providers(state).find((p) => p.id === 'minimax')!;
    let renderer: ReactTestRenderer | undefined;
    try {
      act(() => {
        renderer = create(<AccountCard provider={provider} account={provider.accounts[0]} />);
      });
      expect(
        renderer!.root.findByProps({
          'aria-label': 'agent.dailyCreditsClaimed cached@example.test',
        }).props.disabled
      ).toBe(true);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(300_000);
      });
      expect(
        renderer!.root.findByProps({ 'aria-label': 'agent.claimDailyCredits cached@example.test' })
          .props.disabled
      ).toBe(false);
      expect(JSON.stringify(renderer!.toJSON())).not.toContain('accountCenter.reset');
      expect(state.minimaxAccounts.accounts[0].dailyReward?.status).toBe('claimed');
      expect(state.minimaxAccounts.accounts[0].quota?.credits).toBe(42);
      expect(state.minimaxAccounts.refresh).not.toHaveBeenCalled();
      expect(state.minimaxAccounts.claimDaily).not.toHaveBeenCalled();
    } finally {
      act(() => renderer?.unmount());
      vi.useRealTimers();
    }
  });

  it('uses Qoder campaign status rather than the WorkBuddy midnight boundary', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-10-13T01:59:30Z'));
    const state = context();
    const now = Date.now() / 1000;
    state.detectedTools = [
      { id: 'qodercn', name: 'Qoder CN', category: 'Desktop', installed: true },
    ];
    state.qoderAccountGroups.qodercn.accounts = [
      {
        id: 'daily',
        edition: 'qodercn',
        name: 'Daily',
        email: null,
        plan: 'personal_standard',
        active: false,
        remaining: 100,
        total: 100,
        expiresAt: null,
        quotaWindows: [],
        dailyReward: { status: 'claimed', checkedAt: now, resetAt: now + 30 },
      },
    ];
    const provider = providers(state).find((p) => p.id === 'qodercn')!;
    let renderer: ReactTestRenderer | undefined;
    try {
      act(() => {
        renderer = create(<AccountCard provider={provider} account={provider.accounts[0]} />);
      });
      expect(
        renderer!.root.findByProps({ 'aria-label': 'agent.dailyCreditsClaimed Daily' }).props
          .disabled
      ).toBe(true);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(60_000);
      });
      expect(
        renderer!.root.findByProps({ 'aria-label': 'agent.claimDailyCredits Daily' }).props.disabled
      ).toBe(false);
      expect(state.qoderAccountGroups.qodercn.claimDaily).not.toHaveBeenCalled();
      expect(state.qoderAccountGroups.qodercn.refresh).not.toHaveBeenCalled();
      expect(state.qoderAccountGroups.qodercn.accounts[0].dailyReward?.status).toBe('claimed');
    } finally {
      act(() => renderer?.unmount());
      vi.useRealTimers();
    }
  });

  it('corrects cached Qoder trial plans and permanent dates while preserving real bucket zeros', () => {
    const state = context();
    state.qoderAccountGroups.qodercn.accounts = [
      {
        id: 'cached',
        edition: 'qodercn',
        name: 'saved account',
        email: null,
        plan: 'personal_standard',
        active: false,
        remaining: 300,
        total: 300,
        expiresAt: 253402214400,
        quotaWindows: [
          { name: 'Credits', remaining: 0, total: 0 },
          { name: 'Add-on', remaining: 300, total: 300 },
        ],
      },
    ];
    const provider = providers(state).find((p) => p.id === 'qodercn')!;
    const account = provider.accounts[0];
    expect(account.plan).toBe('agent.qoderTrial');
    expect(account.subscriptionEndAt).toBeUndefined();
    expect(account.metrics).toMatchObject([
      { label: 'agent.baseCredits', value: '0', percent: undefined, resetAt: undefined },
      { label: 'agent.additionalCredits', value: '300', percent: 100, resetAt: undefined },
    ]);
    const html = renderToStaticMarkup(<AccountCard provider={provider} account={account} />);
    expect(html).toContain('>Agent.qoderTrial<');
    expect(html).not.toContain('personal_standard');
    expect(html).not.toContain('accountCenter.subscription');
    expect(state.qoderAccountGroups.qodercn.accounts[0].expiresAt).toBe(253402214400);
  });

  it('keeps Qoder subscription expiry separate from each quota bucket date', () => {
    const state = context();
    const future = Math.floor(Date.now() / 1000) + 86400;
    state.qoderAccountGroups.qoder.accounts = [
      {
        id: 'paid',
        edition: 'qoder',
        name: 'paid account',
        email: null,
        plan: 'personal_professional',
        active: false,
        remaining: 50,
        total: 100,
        expiresAt: future + 86400,
        quotaWindows: [
          { name: 'Credits', remaining: 0, total: 10, expiresAt: future - 86401 },
          { name: 'Add-on', remaining: 50, total: 100, expiresAt: future },
          { name: 'Dedicated', remaining: null, total: null, expiresAt: 253402214400 },
        ],
      },
    ];
    const account = providers(state).find((p) => p.id === 'qoder')!.accounts[0];
    expect(account.plan).toBe('Pro');
    expect(account.subscriptionEndAt).toBe(future + 86400);
    expect(account.metrics).toMatchObject([
      { value: '0', resetAt: undefined },
      { value: '50', resetAt: future },
      { label: 'agent.personalResourceCredits', value: '—', resetAt: undefined },
    ]);
  });

  it('displays MiniMax credits or quota windows and separates Qoder editions without inventing quota', () => {
    const state = context();
    state.minimaxAccounts.accounts = [
      {
        id: 'cn',
        email: 'same@example.test',
        region: 'cn',
        active: false,
        quota: { credits: 42, windows: [] },
      },
      {
        id: 'global',
        email: 'same@example.test',
        region: 'global',
        active: false,
        plan: 'Pro',
        quota: {
          windows: [
            { name: '5h', usedPercent: 20, resetsAt: 2000000000 },
            { name: '7d', usedPercent: 50 },
          ],
        },
      },
    ];
    for (const edition of ['qoder', 'qodercn'] as const) {
      state.qoderAccountGroups[edition].accounts = [
        {
          id: 'same',
          edition,
          name: edition,
          email: null,
          plan: null,
          active: false,
          remaining: null,
          total: null,
          expiresAt: null,
          quotaWindows: [],
        },
      ];
    }
    const result = providers(state);
    const mini = result.find((provider) => provider.id === 'minimax')!;
    expect(mini.accounts[0].detail).toBe('agent.minimaxRegionCn');
    expect(mini.accounts[0].metrics).toEqual([
      { label: 'agent.credits', value: '42', resetAt: undefined, percent: undefined },
    ]);
    const html = renderToStaticMarkup(<AccountCard provider={mini} account={mini.accounts[0]} />);
    expect(html).toContain('text-2xl');
    expect(mini.accounts[1].detail).toBe('agent.minimaxRegionGlobal');
    expect(mini.accounts[1].metrics).toMatchObject([
      { label: '5h', value: '80%', percent: 80, resetAt: 2000000000 },
      { label: '7d', value: '50%', percent: 50 },
    ]);
    for (const edition of ['qoder', 'qodercn']) {
      const group = result.find((provider) => provider.id === edition)!;
      expect(group.accounts[0].identity).toBe(edition);
      expect(group.accounts[0].metrics[0]).toMatchObject({ value: '—', percent: undefined });
    }
  });
  it('separates MiniMax purchased and gifted credits with server expiry and keeps MPlan windows independent', () => {
    const state = context();
    const expiresAt = Math.floor(Date.now() / 1000) + 15 * 86400;
    state.minimaxAccounts.accounts = [
      {
        id: 'credits',
        email: 'saved@example.test',
        region: 'cn',
        active: false,
        quota: {
          credits: 1500,
          purchasedCredits: 1100,
          grantedCredits: 400,
          grantedExpiresAt: expiresAt,
          windows: [{ name: '5h', usedPercent: 20, resetsAt: expiresAt + 1 }],
        },
      },
    ];
    const provider = providers(state).find((p) => p.id === 'minimax')!;
    expect(provider.accounts[0].metrics).toMatchObject([
      { label: '5h', value: '80%', resetAt: expiresAt + 1 },
      { label: 'agent.minimaxPurchasedCredits', value: '1,100', resetAt: undefined },
      { label: 'agent.grantedCredits', value: '400', resetAt: expiresAt },
    ]);
    const html = renderToStaticMarkup(
      <AccountCard provider={provider} account={provider.accounts[0]} />
    );
    expect(html).toContain('agent.minimaxPurchasedCredits');
    expect(html).toContain('agent.grantedCredits');
    expect(html).not.toContain('1,500');
    expect(state.minimaxAccounts.refresh).not.toHaveBeenCalled();
    expect(state.minimaxAccounts.claimDaily).not.toHaveBeenCalled();
    expect(state.minimaxAccounts.accounts[0].quota?.credits).toBe(1500);
  });

  it('does not fabricate a MiniMax missing category or expiry', () => {
    const state = context();
    state.minimaxAccounts.accounts = [
      {
        id: 'credits',
        email: 'saved@example.test',
        region: 'global',
        active: false,
        quota: { credits: 400, grantedCredits: 400, windows: [] },
      },
    ];
    expect(providers(state).find((p) => p.id === 'minimax')!.accounts[0].metrics).toMatchObject([
      { label: 'agent.minimaxPurchasedCredits', value: '—', resetAt: undefined },
      { label: 'agent.grantedCredits', value: '400', resetAt: undefined },
    ]);
  });

  it('renders MiniMax category progress from actual grants without adding another total row', () => {
    const state = context();
    state.minimaxAccounts.accounts = [
      {
        id: 'progress',
        email: 'saved@example.test',
        region: 'cn',
        active: false,
        plan: 'Free',
        quota: {
          credits: 420,
          purchasedCredits: 20,
          purchasedTotal: 100,
          grantedCredits: 400,
          grantedTotal: 400,
          windows: [],
        },
      },
    ];
    const provider = providers(state).find((p) => p.id === 'minimax')!;
    expect(provider.accounts[0].metrics).toMatchObject([
      { label: 'agent.minimaxPurchasedCredits', value: '20', percent: 20, showProgress: true },
      { label: 'agent.grantedCredits', value: '400', percent: 100, showProgress: true },
    ]);
    const html = renderToStaticMarkup(
      <AccountCard provider={provider} account={provider.accounts[0]} />
    );
    expect(html.match(/role="progressbar"/g)).toHaveLength(2);
    expect(html).toContain('aria-valuenow="20"');
    expect(html).toContain('aria-valuenow="100"');
    expect(html).toContain('border-t-[6px]');
    expect(html).toContain('flex h-44');
    expect(html).not.toContain('420');
    expect(state.minimaxAccounts.refresh).not.toHaveBeenCalled();
    expect(state.minimaxAccounts.claimDaily).not.toHaveBeenCalled();
  });

  it('keeps MiniMax exhausted credits at zero progress and missing denominators indeterminate', () => {
    const state = context();
    state.minimaxAccounts.accounts = [
      {
        id: 'progress',
        email: 'saved@example.test',
        region: 'cn',
        active: false,
        quota: {
          credits: 400,
          purchasedCredits: 0,
          purchasedTotal: 100,
          grantedCredits: 400,
          windows: [],
        },
      },
    ];
    const provider = providers(state).find((p) => p.id === 'minimax')!;
    expect(provider.accounts[0].metrics).toMatchObject([
      { value: '0', percent: 0, showProgress: true },
      { value: '400', percent: undefined, showProgress: true },
    ]);
    const html = renderToStaticMarkup(
      <AccountCard provider={provider} account={provider.accounts[0]} />
    );
    expect(html.match(/role="progressbar"/g)).toHaveLength(1);
    expect(html).toContain('aria-valuenow="0"');
    expect(html).not.toContain('aria-valuenow="100"');
    expect(html).toContain('border-cyber-border');
  });

  it('retains both MPlan windows and both credit bars within the fixed MiniMax card layout', () => {
    const state = context();
    state.minimaxAccounts.accounts = [
      {
        id: 'paid',
        email: 'saved@example.test',
        region: 'global',
        active: false,
        plan: 'M Plan Go',
        quota: {
          credits: 500,
          purchasedCredits: 100,
          purchasedTotal: 200,
          grantedCredits: 400,
          grantedTotal: 400,
          windows: [
            { name: '5 hours', usedPercent: 25 },
            { name: '7 days', usedPercent: 60 },
          ],
        },
      },
    ];
    const provider = providers(state).find((p) => p.id === 'minimax')!;
    expect(provider.accounts[0].metrics).toMatchObject([
      { label: '5 hours', percent: 75 },
      { label: '7 days', percent: 40 },
      { label: 'agent.minimaxPurchasedCredits', percent: 50 },
      { label: 'agent.grantedCredits', percent: 100 },
    ]);
    const html = renderToStaticMarkup(
      <AccountCard provider={provider} account={provider.accounts[0]} />
    );
    expect(html.match(/role="progressbar"/g)).toHaveLength(4);
    expect(html).toContain('flex h-44');
    expect(html).toContain('justify-center gap-0');
    expect(html.match(/flex w-full flex-shrink-0 flex-col gap-0/g)).toHaveLength(4);
    expect(html).toContain('border-t-[6px]');
    expect(state.minimaxAccounts.refresh).not.toHaveBeenCalled();
  });
  it('keeps Kilo balance and Copilot request quota distinct without fabricating missing values', () => {
    const state = context();
    state.kiloAccounts.accounts = [
      { id: 'k', email: 'kilo@example.test', active: false, plan: null, balance: null },
    ];
    state.copilotAccounts.accounts = [
      {
        id: 'c',
        login: 'github-user',
        host: 'github.com',
        active: false,
        plan: null,
        remaining: null,
        total: null,
        resetAt: null,
      },
    ];
    const kilo = providers(state).find((p) => p.id === 'kilo')!;
    const copilot = providers(state).find((p) => p.id === 'copilot')!;
    expect(kilo.accounts[0].metrics).toEqual([{ label: 'model.balance', value: '—' }]);
    expect(copilot.accounts[0].metrics[0]).toMatchObject({ value: '—', percent: undefined });
    const markup = renderToStaticMarkup(<AccountCard provider={kilo} account={kilo.accounts[0]} />);
    expect(markup).toContain('model.noUsageData');
    expect(markup).not.toContain('model.balance —');
    state.kiloAccounts.accounts[0].balance = 12.34;
    state.copilotAccounts.accounts[0] = {
      ...state.copilotAccounts.accounts[0],
      remaining: 30,
      total: 100,
      resetAt: 1794132901,
    };
    expect(providers(state).find((p) => p.id === 'kilo')!.accounts[0].metrics[0].value).toBe(
      '$12.34'
    );
    expect(providers(state).find((p) => p.id === 'copilot')!.accounts[0].metrics[0]).toMatchObject({
      value: '30',
      percent: 30,
      resetAt: 1794132901,
    });
  });
  it('shows one Kiro family provider when both clients are installed', () => {
    const state = context();
    state.detectedTools = [
      { id: 'kirocrew', name: 'KiroCrew', category: 'Desktop', installed: true },
      { id: 'kirocli', name: 'Kiro CLI', category: 'CLI Code', installed: true },
    ];
    state.kiroAccounts.accounts = [
      { id: 'saved', edition: 'kirocrew', name: 'user@example.test', active: true, quota: [] },
    ];
    const family = providers(state).filter((provider) => provider.id.startsWith('kiro'));
    expect(family).toHaveLength(1);
    expect(family[0]).toMatchObject({ id: 'kiro', name: 'Kiro', installed: true });
    expect(family[0].accounts).toHaveLength(1);
    expect(family[0].accounts[0].identity).toBe('user@example.test');
    family[0].add();
    expect(state.kiroAccounts.add).toHaveBeenCalledOnce();
  });

  it.each(['kirocrew', 'kirocli'] as const)(
    '%s: maps real credit buckets and dates without rewards actions or subscription guesses',
    (edition) => {
      const state = context();
      state.detectedTools = [
        {
          id: edition,
          name: edition,
          category: edition === 'kirocli' ? 'CLI Code' : 'Desktop',
          installed: true,
        },
      ];
      state.kiroAccounts.accounts = [
        {
          id: 'saved',
          edition,
          name: 'Kiro user',
          plan: 'Pro',
          active: true,
          quota: [
            { label: 'credits', remaining: 78.5, total: 100, resetAt: 1793491200 },
            { label: 'trial', remaining: 300, total: 500, resetAt: 1792406534 },
          ],
        },
      ];
      const provider = providers(state).find((provider) => provider.id === 'kiro')!;
      const account = provider.accounts[0];
      expect(account.plan).toBe('Pro');
      expect(account.subscriptionEndAt).toBeUndefined();
      expect(account.claim).toBeUndefined();
      expect(account.metrics).toEqual([
        { label: 'agent.baseCredits', value: '78.5', percent: 78.5, resetAt: 1793491200 },
        { label: 'agent.rewardCredits', value: '300', percent: 60, resetAt: 1792406534 },
      ]);
      const html = renderToStaticMarkup(<AccountCard provider={provider} account={account} />);
      expect(html).not.toContain('agent.claimDailyCredits');
      expect(html.match(/role="progressbar"/g)).toHaveLength(2);
    }
  );

  it('Kiro: missing usage remains unknown rather than zero', () => {
    const state = context();
    state.kiroAccounts.accounts = [
      { id: 'saved', edition: 'kirocli', name: 'Kiro user', active: false, quota: [] },
    ];
    expect(
      providers(state).find((provider) => provider.id === 'kiro')!.accounts[0].metrics
    ).toEqual([{ label: 'agent.credits', value: '—', percent: undefined, resetAt: undefined }]);
  });

  it('shares the manual claim batch lock with background claiming, including across page changes', async () => {
    const state = context();
    const account = {
      id: 'saved',
      edition: 'workbuddy' as const,
      name: 'Fixture',
      active: false,
      plan: 'Free',
      remaining: null,
      total: null,
      expiresAt: null,
    };
    state.detectedTools = [
      { id: 'workbuddy', name: 'WorkBuddy', category: 'Desktop', installed: true },
    ];
    state.workBuddyAccountGroups.workbuddy.accounts = [account];
    let finish!: () => void;
    const claim = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        })
    );
    state.workBuddyAccountGroups.workbuddy.claimDaily = claim;
    useNavigationStore.getState().setActivePage('accounts');
    let renderer: ReactTestRenderer | undefined;
    try {
      act(() => {
        renderer = create(
          <AppManagerContext.Provider value={state}>
            <AccountCenterTitleActions />
          </AppManagerContext.Provider>
        );
      });
      const button = () => renderer!.root.findByProps({ 'aria-label': 'accountCenter.claimAll' });
      act(() => {
        useRewardClaimStore.getState().tryStart();
      });
      expect(button().props.disabled).toBe(true);
      await act(async () => {
        void button().props.onClick();
      });
      expect(claim).not.toHaveBeenCalled();
      act(() => useRewardClaimStore.getState().finish());
      await act(async () => {
        void button().props.onClick();
      });
      expect(claim).toHaveBeenCalledOnce();
      expect(useRewardClaimStore.getState().tryStart()).toBe(false);
      act(() => useNavigationStore.getState().setActivePage('models'));
      expect(useRewardClaimStore.getState().running).toBe(true);
      await act(async () => finish());
      expect(useRewardClaimStore.getState().running).toBe(false);
    } finally {
      act(() => renderer?.unmount());
      useNavigationStore.getState().setActivePage('apps');
      useRewardClaimStore.getState().finish();
    }
  });

  it.each(['device claimed', 'no device claim', 'ignored response'] as const)(
    'uses the actual TRAE claim result before a render snapshot updates (%s)',
    async (result) => {
      const state = context();
      const first = {
        id: 'first',
        edition: 'traecn' as const,
        name: 'First',
        plan: 'Free',
        active: true,
      };
      const second = { ...first, id: 'second', name: 'Second', active: false };
      state.detectedTools = [
        { id: 'traecn', name: 'TRAE CN', category: 'Desktop', installed: true },
      ];
      state.traeAccountGroups.traecn.accounts = [first, second];
      const claim = vi.fn().mockResolvedValueOnce(
        result === 'ignored response'
          ? undefined
          : {
              ...first,
              dailyClaimedAt: Date.now() / 1000,
              deviceClaimedAt: result === 'device claimed' ? Date.now() / 1000 : undefined,
            }
      );
      state.traeAccountGroups.traecn.claimDaily = claim;
      useNavigationStore.getState().setActivePage('accounts');
      let renderer: ReactTestRenderer | undefined;
      try {
        act(() => {
          renderer = create(
            <AppManagerContext.Provider value={state}>
              <AccountCenterTitleActions />
            </AppManagerContext.Provider>
          );
        });
        expect(claim).not.toHaveBeenCalled();
        await act(async () => {
          await renderer!.root
            .findByProps({ 'aria-label': 'accountCenter.claimAll' })
            .props.onClick();
        });
        expect(claim).toHaveBeenCalledTimes(result === 'device claimed' ? 1 : 2);
        expect(claim).toHaveBeenNthCalledWith(1, first, expect.any(Function));
        if (result !== 'device claimed')
          expect(claim).toHaveBeenNthCalledWith(2, second, expect.any(Function));
        expect(state.traeAccountGroups.traecn.accounts).toEqual([first, second]);
        expect(state.traeAccountGroups.traecn.refresh).not.toHaveBeenCalled();
      } finally {
        act(() => renderer?.unmount());
        useNavigationStore.getState().setActivePage('apps');
      }
    }
  );

  it.each(['pro', 'free', 'Free'])(
    'keeps Manus %s subscription expiry separate from its credit balance and refresh time',
    (plan) => {
      const state = context();
      state.manusAccounts.accounts = [
        {
          id: 'manus',
          email: 'manus@example.test',
          active: true,
          plan,
          credits: { total: 1300, free: 1000, refresh: 300, nextRefreshAt: 1791561600 },
          subscriptionEndAt: 1794132901,
        },
      ];
      const account = providers(state).find((p) => p.id === 'manus')!.accounts[0];
      expect(account.subscriptionEndAt).toBe(
        plan.toLowerCase() === 'free' ? undefined : 1794132901
      );
      expect(account.metrics).toEqual([{ label: 'agent.credits', value: '1,300' }]);
      state.manusAccounts.accounts = [
        { ...state.manusAccounts.accounts[0], subscriptionEndAt: null },
      ];
      const provider = providers(state).find((p) => p.id === 'manus')!;
      expect(provider.accounts[0].subscriptionEndAt).toBeUndefined();
      expect(
        renderToStaticMarkup(<AccountCard provider={provider} account={provider.accounts[0]} />)
      ).not.toContain('aria-label="accountCenter.subscription"');
    }
  );

  it('shows Cue weekly reset separately from Manus balance and additional purchased credits', () => {
    const state = context();
    const credits = { total: 24000, free: null, refresh: null, nextRefreshAt: null };
    state.cueAccounts.accounts = [
      {
        id: 'cue',
        email: 'cue@example.test',
        active: true,
        plan: 'plus',
        credits,
        weekly: { remainingPercent: 80, resetAt: 1792145701 },
      },
    ];
    state.manusAccounts.accounts = [
      { id: 'manus', email: 'manus@example.test', active: true, plan: 'free', credits },
    ];
    const groups = providers(state);
    expect(groups.find((p) => p.id === 'cue')!.accounts[0].metrics).toEqual([
      {
        label: '7d',
        value: '80%',
        percent: 80,
        resetAt: 1792145701,
        showProgress: true,
      },
    ]);
    expect(groups.find((p) => p.id === 'manus')!.accounts[0].metrics).toEqual([
      {
        label: 'agent.credits',
        value: '24,000',
      },
    ]);
  });
  it.each(['plus', 'pro', 'max', 'free'])(
    'keeps Cue %s subscription expiry distinct from weekly quota and hides missing expiry',
    (plan) => {
      const state = context();
      state.cueAccounts.accounts = [
        {
          id: 'cue',
          email: 'cue@example.test',
          active: true,
          plan,
          credits: null,
          subscriptionEndAt: 1794132901,
          weekly: { remainingPercent: 80, resetAt: 1792145701 },
        },
      ];
      const account = providers(state).find((p) => p.id === 'cue')!.accounts[0];
      expect(account.subscriptionEndAt).toBe(plan === 'free' ? undefined : 1794132901);
      expect(account.metrics[0]).toMatchObject({ label: '7d', percent: 80, resetAt: 1792145701 });
      state.cueAccounts.accounts = [{ ...state.cueAccounts.accounts[0], subscriptionEndAt: null }];
      const provider = providers(state).find((p) => p.id === 'cue')!;
      expect(provider.accounts[0].subscriptionEndAt).toBeUndefined();
      expect(
        renderToStaticMarkup(<AccountCard provider={provider} account={provider.accounts[0]} />)
      ).not.toContain('aria-label="accountCenter.subscription"');
    }
  );
  it('keeps Cue browser waiting and balance independent of Manus', () => {
    const state = context();
    state.cueAccounts.busy = true;
    expect(providers(state).find((p) => p.id === 'cue')?.waitingLabel).toBeUndefined();
    state.cueAccounts.awaitingClientExit = true;
    state.cueAccounts.accounts = [
      {
        id: 'cue',
        email: 'cue@example.test',
        active: true,
        plan: 'Free',
        credits: { total: 0, free: null, refresh: null, nextRefreshAt: null },
      },
    ];
    const groups = providers(state);
    expect(groups.find((p) => p.id === 'cue')).toMatchObject({
      waitingLabel: undefined,
      accounts: [{ plan: 'Free', metrics: [{ value: '0' }] }],
    });
    expect(groups.find((p) => p.id === 'manus')).toMatchObject({
      waitingLabel: undefined,
      accounts: [],
    });
  });
  it('labels Claude desktop as account snapshots instead of browser sign-in', () => {
    const state = context();
    state.claudeDesktopAccounts.busy = true;
    const groups = providers(state);
    expect(groups.find((provider) => provider.id === 'claudedesktop')).toMatchObject({
      addLabel: 'agent.accountSnapshot',
      waitingLabel: 'agent.accountSnapshotSaving',
      busy: true,
    });
    expect(
      groups
        .filter((provider) => provider.id !== 'claudedesktop')
        .every((provider) => provider.addLabel === undefined)
    ).toBe(true);
  });
  it('keeps Manus on shared browser waiting even if an older backend reports native progress', () => {
    const state = context();
    const signIn = (provider: { id: string }) => provider.id !== 'claudedesktop';
    expect(
      providers(state)
        .filter(signIn)
        .every((provider) => provider.waitingLabel === undefined)
    ).toBe(true);
    state.manusAccounts.awaitingClientExit = true;
    state.manusAccounts.busy = true;
    state.manusAccounts.remainingSeconds = 30;
    const groups = providers(state);
    expect(groups.find((provider) => provider.id === 'manus')).toMatchObject({
      waitingLabel: undefined,
      busy: true,
      remainingSeconds: 30,
    });
    expect(
      groups
        .filter((provider) => provider.id !== 'manus' && signIn(provider))
        .every((provider) => provider.waitingLabel === undefined)
    ).toBe(true);
  });

  it('replaces failed account quotas and balances with a short status while keeping identity and actions', () => {
    const state = context();
    state.codexAccounts = [
      {
        id: 'same',
        email: 'failed@example.test',
        plan: 'Plus',
        active: false,
        quotaWindows: [
          { label: '5h', remainingPercent: 75, resetAt: 1900000000 },
          { label: '7d', remainingPercent: 80 },
        ],
      },
    ];
    state.codexAuthorizationFailedIds.add('same');
    state.deepSeekAccounts.accounts = [
      {
        id: 'same',
        name: 'Balance account',
        active: false,
        balances: [{ currency: 'CNY', amount: 12.5 }],
      },
    ];
    const groups = providers(state);
    const codex = groups[0];
    const balance = groups.find((p) => p.id === 'dsh')!;
    expect(codex.accounts[0].authorizationFailed).toBe(true);
    expect(balance.accounts[0].authorizationFailed).toBe(false);
    state.deepSeekAccounts.authorizationFailedIds.add('same');
    for (const provider of providers(state).filter((p) => ['codex', 'dsh'].includes(p.id))) {
      const account = provider.accounts[0];
      const markup = renderToStaticMarkup(<AccountCard provider={provider} account={account} />);
      expect(markup).toContain('role="status"');
      expect(markup).toContain('>accountCenter.authFailed<');
      expect(markup).toContain(account.identity);
      expect(markup).toContain(`agent.refreshAccount ${account.identity}`);
      expect(markup).toContain(`btn.delete ${account.identity}`);
      expect(markup).not.toContain('role="progressbar"');
      expect(markup).not.toContain('accountCenter.reset');
      expect(markup).not.toContain('>75%<');
      expect(markup).not.toContain('>5h<');
      expect(markup).not.toContain('12.50');
      if (account.plan) expect(markup).toContain(`>${account.plan}<`);
    }
  });

  it('reauthorizes a failed card through the same provider add flow and locks while pending', () => {
    const state = context();
    state.codexAccounts = [
      {
        id: 'failed',
        email: 'failed@example.test',
        plan: 'Pro',
        active: false,
        quotaWindows: [],
      },
    ];
    state.codexAuthorizationFailedIds.add('failed');
    const add = vi.fn();
    const base = providers(state).find((provider) => provider.id === 'codex')!;
    let renderer: ReactTestRenderer | undefined;
    try {
      act(() => {
        renderer = create(
          <AccountCard provider={{ ...base, installed: true, add }} account={base.accounts[0]} />
        );
      });
      const button = renderer!.root.findByProps({
        'aria-label': 'accountCenter.reauthorize ChatGPT / Codex',
      });
      expect(button.props.disabled).not.toBe(true);
      act(() => button.props.onClick());
      expect(add).toHaveBeenCalledOnce();

      act(() => {
        renderer!.update(
          <AccountCard
            provider={{
              ...base,
              installed: true,
              add,
              busy: true,
              remainingSeconds: 42,
              waitingLabel: 'agent.waitingForBrowser {seconds}',
            }}
            account={base.accounts[0]}
          />
        );
      });
      const pending = renderer!.root.findByProps({
        'aria-label': 'accountCenter.reauthorize ChatGPT / Codex',
      });
      expect(pending.props.disabled).toBe(true);
      expect(JSON.stringify(renderer!.toJSON())).toContain('agent.waitingForBrowser 42');
    } finally {
      act(() => renderer?.unmount());
    }
  });

  it('shows real balances without making up percentages and preserves unknown vs zero', () => {
    const state = context();
    state.deepSeekAccounts.accounts = [
      {
        id: 'balance',
        name: 'balance',
        active: false,
        balances: [{ currency: 'USD', amount: 12.5 }],
      },
      { id: 'unknown', name: 'unknown', active: false, balances: null },
      { id: 'zero', name: 'zero', active: false, balances: [] },
    ];
    const provider = providers(state).find((p) => p.id === 'dsh')!;
    expect(provider.accounts.map((a) => a.metrics[0].value)).toEqual(['$12.50', '—', '0']);
    for (const account of provider.accounts) {
      expect(account.metrics[0].percent).toBeUndefined();
      expect(
        renderToStaticMarkup(<AccountCard provider={provider} account={account} />)
      ).not.toContain('role="progressbar"');
    }
  });

  it('keeps zero quota real and unknown quota unknown, with the plan shown once', () => {
    const state = context();
    state.codexAccounts = [
      {
        id: 'known',
        email: 'known@example.test',
        plan: 'Plus',
        active: true,
        quotaWindows: [{ label: '5h', remainingPercent: 0 }],
      },
      { id: 'unknown', email: 'unknown@example.test', active: false },
    ];
    const provider = providers(state)[0];
    const known = renderToStaticMarkup(
      <AccountCard provider={provider} account={provider.accounts[0]} />
    );
    const unknown = renderToStaticMarkup(
      <AccountCard provider={provider} account={provider.accounts[1]} />
    );
    expect(known).toContain('aria-valuenow="0"');
    expect(known).toContain('0%');
    expect(known.split('Plus')).toHaveLength(2);
    expect(unknown).toContain('model.noUsageData');
    expect(unknown).not.toContain('role="progressbar"');
    expect(unknown).not.toContain('0%');
  });

  it('uses separate WorkBuddy balances and only derives a percentage from a real limit', () => {
    const state = context();
    state.workBuddyAccountGroups.workbuddy.accounts = [
      {
        id: 'cn',
        name: 'CN',
        edition: 'workbuddy',
        active: false,
        plan: null,
        remaining: 50,
        total: 100,
        baseRemaining: 25,
        baseTotal: 100,
        baseResetAt: 123,
        rewardRemaining: 0,
        rewardTotal: null,
        addonRemaining: 12,
        expiresAt: null,
      },
    ];
    state.workBuddyAccountGroups.workbuddyai.accounts = [
      {
        id: 'global',
        name: 'Global',
        edition: 'workbuddyai',
        active: false,
        plan: null,
        remaining: null,
        total: null,
        expiresAt: null,
      },
    ];
    const groups = providers(state);
    const cn = groups.find((p) => p.id === 'workbuddy')!.accounts[0];
    const global = groups.find((p) => p.id === 'workbuddyai')!.accounts[0];
    expect(cn.metrics.map((m) => [m.value, m.percent])).toEqual([
      ['25', 25],
      ['0', undefined],
      ['12', undefined],
    ]);
    expect(cn.metrics[0].resetAt).toBe(123);
    expect(global.metrics[0]).toMatchObject({ value: '—', percent: undefined });
  });

  it('keeps regional TRAE plans and uncategorized balances separate, including unknown Free balances', () => {
    const state = context();
    state.traeAccountGroups.trae.accounts = [
      { id: 'same', edition: 'trae', name: 'Global', plan: 'Free', active: false, remaining: null },
    ];
    state.traeAccountGroups.traecn.accounts = [
      {
        id: 'same',
        edition: 'traecn',
        name: 'CN',
        plan: 'Pro+',
        active: true,
        remaining: 25,
        workRemaining: 0,
        subscriptionEndAt: 1900000000,
      },
    ];
    const groups = providers(state);
    const global = groups.find((p) => p.id === 'trae')!;
    const cn = groups.find((p) => p.id === 'traecn')!;
    expect(global.accounts[0].metrics).toEqual([{ label: 'accountCenter.quota', value: '—' }]);
    expect(global.accounts[0].claim).toBeUndefined();
    expect(cn.accounts[0].metrics).toEqual([{ label: 'agent.credits', value: '25' }]);
    expect(cn.accounts[0].subscriptionEndAt).toBe(1900000000);
    const html = renderToStaticMarkup(<AccountCard provider={cn} account={cn.accounts[0]} />);
    expect(html.split('Pro+')).toHaveLength(2);
    expect(html).not.toContain('role="progressbar"');
    const unknown = renderToStaticMarkup(
      <AccountCard provider={global} account={global.accounts[0]} />
    );
    expect(unknown).not.toContain('text-2xl');
    expect(unknown).toContain('items-center text-center');
    expect(unknown).not.toContain('>—</span>');
    expect(unknown).toContain('model.noUsageData');
  });

  it('shows WorkBuddy reward expiry after unclaimed independently of the package reset', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-10-22T00:00:00Z'));
    const now = Date.now() / 1000;
    const state = context();
    state.workBuddyAccountGroups.workbuddy.accounts = [
      {
        id: 'reward',
        name: 'Reward',
        edition: 'workbuddy',
        plan: '体验版',
        active: true,
        remaining: 600,
        total: 600,
        baseRemaining: 500,
        baseTotal: 500,
        baseResetAt: now + 10 * 86400,
        rewardRemaining: 100,
        rewardTotal: 100,
        rewardNextExpiryAt: now + 3600,
        expiresAt: null,
      },
    ];
    const provider = providers(state).find((item) => item.id === 'workbuddy')!;
    const account = provider.accounts[0];
    expect(account.metrics[1]).toMatchObject({
      resetAt: now + 3600,
      resetLabel: 'agent.rewardCreditsNextExpiry',
    });
    let renderer!: ReactTestRenderer;
    try {
      act(() => {
        renderer = create(
          <AccountCard provider={provider} account={account} dailyClaimed={false} />
        );
      });
      expect(
        renderer.root.findByProps({ 'aria-label': 'agent.rewardCreditsNextExpiry 1h0m' })
      ).toBeDefined();
      const rendered = JSON.stringify(renderer.toJSON());
      expect(rendered.indexOf('agent.dailyCreditsUnclaimed')).toBeGreaterThan(-1);
      expect(rendered.indexOf('agent.dailyCreditsUnclaimed')).toBeLessThan(
        rendered.indexOf('agent.rewardCreditsNextExpiry 1h0m')
      );
      act(() => {
        renderer.update(<AccountCard provider={provider} account={account} dailyClaimed />);
      });
      expect(JSON.stringify(renderer.toJSON())).not.toContain('agent.dailyCreditsUnclaimed');
      expect(
        renderer.root.findByProps({ 'aria-label': 'agent.rewardCreditsNextExpiry 1h0m' })
      ).toBeDefined();
      await act(async () => {
        await vi.advanceTimersByTimeAsync(3600_000);
      });
      expect(JSON.stringify(renderer.toJSON())).not.toContain('agent.rewardCreditsNextExpiry');
    } finally {
      if (renderer) act(() => renderer.unmount());
      vi.useRealTimers();
    }
    expect(state.workBuddyAccountGroups.workbuddy.refresh).not.toHaveBeenCalled();
    expect(state.workBuddyAccountGroups.workbuddy.claimDaily).not.toHaveBeenCalled();
  });

  it('reuses base/reward progress and daily claim text only from confirmed CN balances', () => {
    const state = context();
    state.traeAccountGroups.traecn.accounts = [
      {
        id: 'cn',
        edition: 'traecn',
        name: 'CN',
        plan: 'Pro',
        active: true,
        remaining: 95,
        total: 150,
        baseRemaining: 75,
        baseTotal: 100,
        baseResetAt: 1900000000,
        rewardRemaining: 20,
        rewardTotal: 50,
        rewardEnabled: true,
      },
    ];
    let provider = providers(state).find((p) => p.id === 'traecn')!;
    expect(provider.accounts[0].metrics).toEqual([
      { label: 'agent.baseCredits', value: '75', percent: 75, resetAt: 1900000000 },
      { label: 'agent.rewardCredits', value: '20', percent: 40, resetAt: undefined },
    ]);
    const html = renderToStaticMarkup(
      <AccountCard provider={provider} account={provider.accounts[0]} />
    );
    expect(html.match(/role="progressbar"/g)).toHaveLength(2);
    expect(html).toContain('>agent.dailyCreditsUnclaimed</span>');
    expect(html).not.toContain('workbuddy-account-pill');
    state.traeAccountGroups.traecn.accounts[0].dailyClaimedAt = Date.now() / 1000;
    provider = providers(state).find((p) => p.id === 'traecn')!;
    expect(
      renderToStaticMarkup(<AccountCard provider={provider} account={provider.accounts[0]} />)
    ).not.toContain('agent.dailyCreditsUnclaimed');
    state.traeAccountGroups.traecn.accounts[0].dailyClaimedAt = null;
    state.traeAccountGroups.traecn.accounts[0].rewardEnabled = null;
    provider = providers(state).find((p) => p.id === 'traecn')!;
    expect(
      renderToStaticMarkup(<AccountCard provider={provider} account={provider.accounts[0]} />)
    ).not.toContain('agent.dailyCreditsUnclaimed');
  });

  it('shows one international credit progress row and keeps its deadline separate from subscription expiry', () => {
    const state = context();
    state.traeAccountGroups.trae.accounts = [
      {
        id: 'global',
        edition: 'trae',
        name: 'Global',
        plan: 'Lite',
        active: true,
        remaining: 4,
        total: 8,
        currency: 'USD',
        expiresAt: 1900000000,
        subscriptionEndAt: 1900100000,
        baseRemaining: 3,
        baseTotal: 10,
        rewardRemaining: 1,
        rewardTotal: 10,
      },
    ];
    const provider = providers(state).find((p) => p.id === 'trae')!;
    const account = provider.accounts[0];
    expect(account.metrics).toEqual([
      { label: 'accountCenter.quota', value: '$4', percent: 50, resetAt: 1900000000 },
    ]);
    expect(account.subscriptionEndAt).toBe(1900100000);
    const html = renderToStaticMarkup(<AccountCard provider={provider} account={account} />);
    expect(html.match(/role="progressbar"/g)).toHaveLength(1);
    expect(html).not.toContain('agent.claimDailyCredits');
    expect(html).not.toContain('agent.dailyCreditsUnclaimed');
  });

  it('shows a CN reward-only account without creating a base quota or deadline', () => {
    const state = context();
    state.traeAccountGroups.traecn.accounts = [
      {
        id: 'cn',
        edition: 'traecn',
        name: 'CN',
        plan: 'Free',
        active: true,
        remaining: 4600,
        total: 4600,
        baseRemaining: null,
        baseTotal: null,
        baseResetAt: null,
        rewardRemaining: 4600,
        rewardTotal: 4600,
      },
    ];
    const provider = providers(state).find((p) => p.id === 'traecn')!;
    const account = provider.accounts[0];
    expect(account.metrics).toEqual([
      { label: 'agent.rewardCredits', value: '4,600', percent: 100, resetAt: undefined },
    ]);
    const html = renderToStaticMarkup(<AccountCard provider={provider} account={account} />);
    expect(html.match(/role="progressbar"/g)).toHaveLength(1);
    expect(html).not.toContain('agent.baseCredits');
    expect(html).not.toContain('accountCenter.reset');
  });

  it.each(['expired cache', 'minute tick'])(
    'hides expired earliest-expiry text locally while keeping the CN credit value and progress (%s)',
    async (scenario) => {
      vi.useFakeTimers();
      vi.setSystemTime(new Date('2026-10-13T04:00:00Z'));
      const now = Date.now() / 1000;
      const future = scenario === 'minute tick';
      const state = context();
      state.traeAccountGroups.traecn.accounts = [
        {
          id: 'cn',
          edition: 'traecn',
          name: 'CN',
          plan: 'Free',
          active: true,
          baseRemaining: 0,
          baseTotal: 0,
          rewardRemaining: 4600,
          rewardTotal: 4600,
          rewardNextExpiryAt: now + (future ? 30 : -30),
        },
      ];
      const provider = providers(state).find((p) => p.id === 'traecn')!;
      let renderer: ReactTestRenderer | undefined;
      try {
        act(() => {
          renderer = create(<AccountCard provider={provider} account={provider.accounts[0]} />);
        });
        await act(async () => {
          await vi.advanceTimersByTimeAsync(0);
        });
        const earliestLabels = () =>
          renderer!.root
            .findAllByType('span')
            .filter((node) => node.children.includes('agent.rewardCreditsNextExpiry'));
        expect(earliestLabels()).toHaveLength(0);
        if (future) {
          expect(
            renderer!.root.findAllByProps({ 'aria-label': 'agent.rewardCreditsNextExpiry 1m' })
          ).toHaveLength(1);
          await act(async () => {
            await vi.advanceTimersByTimeAsync(60_000);
          });
        }
        expect(earliestLabels()).toHaveLength(0);
        expect(
          renderer!.root.findAllByProps({ 'aria-label': 'agent.rewardCreditsNextExpiry 1m' })
        ).toHaveLength(0);
        expect(
          renderer!.root.findAllByType('span').some((node) => node.children.includes('4,600'))
        ).toBe(true);
        expect(renderer!.root.findAllByProps({ role: 'progressbar' })).toHaveLength(2);
        expect(state.traeAccountGroups.traecn.refresh).not.toHaveBeenCalled();
        expect(state.traeAccountGroups.traecn.add).not.toHaveBeenCalled();
        expect(state.traeAccountGroups.traecn.remove).not.toHaveBeenCalled();
      } finally {
        act(() => renderer?.unmount());
        vi.useRealTimers();
      }
    }
  );

  it('shows a confirmed empty CN base bucket and the real earliest reward expiry without changing other providers', () => {
    const state = context();
    state.traeAccountGroups.traecn.accounts = [
      {
        id: 'cn',
        edition: 'traecn',
        name: 'CN',
        plan: 'Free',
        active: true,
        baseRemaining: 0,
        baseTotal: 0,
        rewardRemaining: 4600,
        rewardTotal: 4600,
        rewardNextExpiryAt: 1900000000,
      },
    ];
    state.workBuddyAccountGroups.workbuddy.accounts = [
      {
        id: 'buddy',
        edition: 'workbuddy',
        name: 'Buddy',
        plan: 'Free',
        active: true,
        remaining: 20,
        total: 20,
        expiresAt: null,
        baseRemaining: 0,
        baseTotal: 0,
        rewardRemaining: 20,
        rewardTotal: 20,
      },
    ];
    const groups = providers(state);
    const cn = groups.find((p) => p.id === 'traecn')!;
    expect(cn.accounts[0].metrics).toEqual([
      { label: 'agent.baseCredits', value: '0', percent: 0, resetAt: undefined },
      {
        label: 'agent.rewardCredits',
        value: '4,600',
        percent: 100,
        resetAt: 1900000000,
        resetLabel: 'agent.rewardCreditsNextExpiry',
      },
    ]);
    expect(
      groups.find((p) => p.id === 'workbuddy')!.accounts[0].metrics[0].percent
    ).toBeUndefined();
    const html = renderToStaticMarkup(<AccountCard provider={cn} account={cn.accounts[0]} />);
    expect(html.match(/role="progressbar"/g)).toHaveLength(2);
    expect(html).toContain('aria-valuenow="0"');
    expect(html).not.toContain('>agent.rewardCreditsNextExpiry</span>');
  });

  it('preserves missing TRAE credit limits and zero balances without fabricated progress', () => {
    const state = context();
    state.traeAccountGroups.traecn.accounts = [
      {
        id: 'cn',
        edition: 'traecn',
        name: 'CN',
        plan: 'Free',
        active: false,
        baseRemaining: 0,
        baseTotal: 100,
        rewardRemaining: 20,
        rewardTotal: null,
      },
    ];
    const provider = providers(state).find((p) => p.id === 'traecn')!;
    expect(provider.accounts[0].metrics).toMatchObject([
      { label: 'agent.baseCredits', value: '0', percent: 0 },
      { label: 'agent.rewardCredits', value: '20', percent: undefined },
    ]);
    const html = renderToStaticMarkup(
      <AccountCard provider={provider} account={provider.accounts[0]} />
    );
    expect(html.match(/role="progressbar"/g)).toHaveLength(1);
    expect(html).toContain('aria-valuenow="0"');
    expect(html).not.toContain('accountCenter.reset');
  });

  it('disables CN gifts on a device-used day without displaying another account as claimed', () => {
    const state = context();
    state.detectedTools = [{ id: 'traecn', name: 'TRAE CN', category: 'Desktop', installed: true }];
    state.traeAccountGroups.traecn.accounts = [
      {
        id: 'first',
        edition: 'traecn',
        name: 'First',
        plan: 'Pro',
        active: true,
        remaining: 100,
        dailyClaimedAt: Date.now() / 1000,
        deviceClaimedAt: Date.now() / 1000,
      },
      {
        id: 'second',
        edition: 'traecn',
        name: 'Second',
        plan: 'Free',
        active: false,
        remaining: 0,
      },
    ];
    const provider = providers(state).find((p) => p.id === 'traecn')!;
    expect(provider.accounts.map((account) => account.claimDisabled)).toEqual([true, true]);
    const first = renderToStaticMarkup(
      <AccountCard provider={provider} account={provider.accounts[0]} />
    );
    const second = renderToStaticMarkup(
      <AccountCard provider={provider} account={provider.accounts[1]} />
    );
    expect(first).toContain('agent.dailyCreditsClaimed First');
    expect(first).toContain('lucide-check');
    expect(second).toMatch(/aria-label="agent.claimDailyCredits Second" disabled=""/);
    expect(second).toContain('lucide-gift');
    expect(second).not.toContain('agent.dailyCreditsClaimed');
  });

  it('keeps the Antigravity minimum-family reset paired with its percentage', () => {
    const state = context();
    state.antigravityAccounts.accounts = [
      {
        id: 'a',
        email: 'a@example.test',
        active: false,
        plan: null,
        quotas: [
          { name: 'gemini-fast', remainingPercent: 90, resetAt: 10 },
          { name: 'gemini-pro', remainingPercent: 20, resetAt: 30 },
          { name: 'claude', remainingPercent: 80, resetAt: 40 },
        ],
      },
    ];
    const account = providers(state).find((p) => p.id === 'antigravity')!.accounts[0];
    expect(account.metrics).toMatchObject([
      { label: 'Gemini', percent: 20, resetAt: 30 },
      { label: 'Claude', percent: 80, resetAt: 40 },
    ]);
  });

  it.each(['claudecode', 'claudedesktop'])(
    'keeps both %s windows unknown until each has its own quota data',
    (id) => {
      const state = context();
      const group = id === 'claudecode' ? state.claudeCodeAccounts : state.claudeDesktopAccounts;
      const row = { id: 'claude', email: 'claude@example.test', plan: 'Max', active: false };
      group.accounts = [row];
      const provider = providers(state).find((p) => p.id === id)!;
      expect(provider.accounts[0].subscriptionEndAt).toBeUndefined();
      expect(provider.accounts[0].metrics).toMatchObject([
        { label: '5h', value: '—', percent: undefined, resetAt: undefined },
        { label: '7d', value: '—', percent: undefined, resetAt: undefined },
      ]);
      const markup = renderToStaticMarkup(
        <AccountCard provider={provider} account={provider.accounts[0]} />
      );
      expect(markup).toContain('model.noUsageData');
      expect(markup).not.toContain('>5h<');
      expect(markup).not.toContain('>7d<');
      expect(markup).not.toContain('role="progressbar"');
      expect(markup).not.toContain('>0%<');
      group.accounts = [{ ...row, fiveHour: { remainingPercent: 0, resetAt: 123 } }];
      expect(providers(state).find((p) => p.id === id)!.accounts[0].metrics).toMatchObject([
        { label: '5h', value: '0%', percent: 0, resetAt: 123 },
        { label: '7d', value: '—', percent: undefined, resetAt: undefined },
      ]);
    }
  );

  it.each(['claudecode', 'claudedesktop'])(
    'offers %s warm-up only when the plugin declares it, never for Free',
    async (id) => {
      const state = context();
      const group = (id === 'claudecode'
        ? state.claudeCodeAccounts
        : state.claudeDesktopAccounts) as unknown as ReturnType<typeof useClaudeCodeAccounts>;
      group.warmUp = vi.fn().mockResolvedValue(undefined);
      const row = { id: 'idle', email: 'idle@example.test', plan: 'Max', active: false };
      group.accounts = [row];
      const account = () => providers(state).find((p) => p.id === id)!.accounts[0];
      const { installed } = usePluginsStore.getState();
      const { developerMode } = useNavigationStore.getState();
      useNavigationStore.setState({ developerMode: true });
      usePluginsStore.setState({ installed: installedAccountPlugins() });
      expect(account().warmUp).toBeUndefined();
      usePluginsStore.setState({
        installed: installedAccountPlugins().map((plugin) => ({
          ...plugin,
          manifest: {
            ...plugin.manifest,
            echobird: {
              ...plugin.manifest.echobird,
              contributes: {
                ...plugin.manifest.echobird.contributes,
                providers: plugin.manifest.echobird.contributes.providers?.map((provider) => ({
                  ...provider,
                  operations: [...(provider.operations ?? []), 'account.warmup'],
                })),
              },
            },
          },
        })),
      });
      try {
        const onError = vi.fn();
        await account().warmUp!(onError);
        expect(group.warmUp).toHaveBeenCalledWith(row, onError);
        const provider = providers(state).find((p) => p.id === id)!;
        expect(
          renderToStaticMarkup(<AccountCard provider={provider} account={account()} />)
        ).toContain('aria-label="agent.warmUpAccount idle@example.test"');
        group.accounts = [{ ...row, plan: 'Free' }];
        expect(account().warmUp).toBeUndefined();
      } finally {
        usePluginsStore.setState({ installed });
        useNavigationStore.setState({ developerMode });
      }
    }
  );

  it.each(['claudecode', 'claudedesktop'])(
    'shows the shared missing-usage status for Free %s without a subscription placeholder',
    (id) => {
      const state = context();
      const group = id === 'claudecode' ? state.claudeCodeAccounts : state.claudeDesktopAccounts;
      const row = { id: 'free', email: 'free@example.test', plan: 'Free', active: false };
      group.accounts = [row];
      const provider = providers(state).find((p) => p.id === id)!;
      const account = provider.accounts[0];
      expect(account.subscriptionEndAt).toBeUndefined();
      expect(account.metrics).toEqual([]);
      const markup = renderToStaticMarkup(<AccountCard provider={provider} account={account} />);
      expect(markup).toContain('>Free<');
      expect(markup).toContain('>model.noUsageData<');
      expect(markup).not.toContain('aria-label="accountCenter.subscription"');
      expect(markup).not.toContain('role="progressbar"');
      expect(markup).not.toContain('>—<');
      expect(markup).not.toContain('>0%<');

      group.accounts = [{ ...row, fiveHour: { remainingPercent: 0, resetAt: 123 } }];
      const withUsage = providers(state).find((p) => p.id === id)!.accounts[0];
      expect(withUsage.metrics).toMatchObject([
        { label: '5h', value: '0%', percent: 0, resetAt: 123 },
        { label: '7d', value: '—', percent: undefined },
      ]);
      const knownMarkup = renderToStaticMarkup(
        <AccountCard provider={provider} account={withUsage} />
      );
      expect(knownMarkup).toContain('aria-valuenow="0"');
      expect(knownMarkup).toContain('>model.noUsageData<');
    }
  );

  it('keeps Plus short/weekly quotas and subscription expiry as three independent deadlines', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-10-07T04:00:00Z'));
    const now = Date.now() / 1000;
    const state = context();
    state.codexAccounts = [
      {
        id: 'plus',
        email: 'plus@example.test',
        plan: 'Plus',
        active: true,
        subscriptionEndAt: now + 30 * 86400,
        quotaWindows: [
          { label: '7d', remainingPercent: 40, resetAt: now + 4 * 86400 },
          { label: '5h', remainingPercent: 75, resetAt: now + 3 * 3600 },
        ],
      },
    ];
    const provider = providers(state)[0];
    const account = provider.accounts[0];
    expect(account.metrics).toMatchObject([
      { label: '5h', percent: 75, resetAt: now + 3 * 3600 },
      { label: '7d', percent: 40, resetAt: now + 4 * 86400 },
    ]);
    expect(account.subscriptionEndAt).toBe(now + 30 * 86400);
    let renderer: ReactTestRenderer | undefined;
    try {
      act(() => {
        renderer = create(<AccountCard provider={provider} account={account} />);
      });
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      expect(renderer!.root.findAllByProps({ role: 'progressbar' })).toHaveLength(2);
      const times = renderer!.root.findAllByType('span').filter((node) => node.props['aria-label']);
      expect(times.map((node) => node.props['aria-label']).sort()).toEqual([
        'accountCenter.reset 3h0m',
        'accountCenter.reset 4d0h',
        'accountCenter.subscription 30d0h',
      ]);
      const identityRow = renderer!.root.findByType('h3').parent!;
      const metadata = identityRow.findAllByType('span')[0];
      expect(
        metadata
          .findAllByType('span')
          .flatMap((node) => node.children.filter((child) => child === 'Plus' || child === '30d0h'))
      ).toEqual(['30d0h', 'Plus']);
      expect(identityRow.findAllByType('svg')).toHaveLength(0);
    } finally {
      act(() => renderer?.unmount());
      vi.useRealTimers();
    }
  });

  it('shows only the weekly quota for Pro and preserves a Free 30-day quota as usage', () => {
    const state = context();
    state.codexAccounts = [
      {
        id: 'paid',
        email: 'paid@example.test',
        plan: 'prolite',
        active: true,
        quotaWindows: [{ label: '7d', remainingPercent: 59, resetAt: 123 }],
      },
      {
        id: 'free',
        email: 'free@example.test',
        plan: 'free',
        active: false,
        quotaWindows: [{ label: '30d', remainingPercent: 100, resetAt: 456 }],
      },
    ];
    const [paid, free] = providers(state)[0].accounts;
    expect(paid.plan).toBe('Pro 100');
    expect(paid.metrics).toMatchObject([{ label: '7d', value: '59%', percent: 59, resetAt: 123 }]);
    expect(paid.subscriptionEndAt).toBeNull();
    expect(free.metrics).toMatchObject([{ label: '30d', percent: 100, resetAt: 456 }]);
    expect(free.subscriptionEndAt).toBeUndefined();
  });

  it.each(['prolite', 'pro', 'promax'])(
    'shows one weekly row for %s even with cached short-window data',
    (plan) => {
      const state = context();
      state.codexAccounts = [
        {
          id: 'pro',
          email: 'pro@example.test',
          plan,
          active: false,
          quotaWindows: [
            { label: '5h', remainingPercent: 0, resetAt: 456 },
            { label: '7d', remainingPercent: 59, resetAt: 123 },
          ],
        },
      ];
      const provider = providers(state)[0];
      const account = provider.accounts[0];
      expect(account.metrics).toHaveLength(1);
      expect(account.metrics[0]).toMatchObject({
        label: '7d',
        value: '59%',
        percent: 59,
        resetAt: 123,
      });
      const markup = renderToStaticMarkup(<AccountCard provider={provider} account={account} />);
      expect(markup).not.toContain('>5h<');
      expect(markup).not.toContain('aria-label="5h pro@example.test"');
      expect(markup).toContain('aria-label="7d pro@example.test"');
      expect(markup).toContain('aria-valuenow="59"');
    }
  );

  it.each(['Plus', 'Business', undefined])(
    'includes a missing short-window row only for Plus (%s)',
    (plan) => {
      const state = context();
      state.codexAccounts = [
        {
          id: 'other',
          email: 'other@example.test',
          plan,
          active: false,
          quotaWindows: [{ label: '7d', remainingPercent: 59, resetAt: 123 }],
        },
      ];
      const metrics = providers(state)[0].accounts[0].metrics;
      expect(metrics.map((metric) => metric.label)).toEqual(
        plan === 'Plus' ? ['5h', '7d'] : ['7d']
      );
      if (plan === 'Plus')
        expect(metrics[0]).toMatchObject({
          label: '5h',
          value: '—',
          percent: undefined,
          resetAt: undefined,
        });
    }
  );

  it('keeps Pro weekly usage separate from cached short usage and leaves absent data unknown', () => {
    const state = context();
    state.codexAccounts = [
      {
        id: 'explicit',
        email: 'explicit@example.test',
        plan: 'pro',
        active: false,
        quotaWindows: [
          { label: '5h', remainingPercent: 0, resetAt: 123 },
          { label: '7d', remainingPercent: 59, resetAt: 456 },
        ],
      },
      { id: 'missing', email: 'missing@example.test', plan: 'pro', active: false },
    ];
    const [explicit, missing] = providers(state)[0].accounts;
    expect(explicit.metrics).toHaveLength(1);
    expect(explicit.metrics[0]).toMatchObject({
      label: '7d',
      value: '59%',
      percent: 59,
      resetAt: 456,
    });
    expect(missing.metrics).toHaveLength(1);
    expect(missing.metrics[0]).toMatchObject({
      value: '—',
      percent: undefined,
      resetAt: undefined,
    });
  });

  it('preserves ZCode paid subscription expiry separately from quota reset, with no trial subscription', () => {
    const state = context();
    state.zcodeAccounts.accounts = ['Pro', 'Trial'].map((plan) => ({
      id: plan,
      email: `${plan}@example.test`,
      provider: 'bigmodel',
      active: false,
      plan,
      subscriptionEndAt: 999,
      remainingPercent: 50,
      resetAt: 100,
    }));
    const [paid, trial] = providers(state).find((p) => p.id === 'zcode')!.accounts;
    expect(paid.subscriptionEndAt).toBe(999);
    expect(paid.metrics[0]).toMatchObject({ percent: 50, resetAt: 100 });
    expect(trial.subscriptionEndAt).toBeUndefined();
  });

  it('identifies each ZCode region without repeating it beside the plan or inventing an expiry', () => {
    const state = context();
    state.zcodeAccounts.accounts = [
      {
        id: 'cn',
        email: 'cn@example.test',
        provider: 'bigmodel',
        active: false,
        plan: null,
        remainingPercent: null,
        resetAt: null,
      },
      {
        id: 'global',
        email: 'global@example.test',
        provider: 'zai',
        active: false,
        plan: 'Trial',
        remainingPercent: null,
        resetAt: null,
      },
      {
        id: 'paid',
        email: 'paid@example.test',
        provider: 'zai',
        active: false,
        plan: 'ZCode Pro',
        remainingPercent: null,
        resetAt: null,
      },
    ];
    const provider = providers(state).find((p) => p.id === 'zcode')!;
    const [cn, global, paid] = provider.accounts;
    expect(cn.subscriptionEndAt).toBeUndefined();
    expect(global.subscriptionEndAt).toBeUndefined();
    expect(paid.subscriptionEndAt).toBeNull();
    expect(paid.plan).toBe('Pro');
    for (const account of provider.accounts) {
      const html = renderToStaticMarkup(<AccountCard provider={provider} account={account} />);
      expect(html).toContain(`aria-label="ZCode · ${account.detail} ${account.identity}"`);
      expect(html.split(`>${account.detail}<`)).toHaveLength(1);
      expect(html).not.toContain('aria-label="accountCenter.subscription"');
    }
    expect(global.plan).toBe('agent.zcodeTrial');
  });
  it('shows a confirmed empty ZCode plan without inventing a subscription expiry or quota', () => {
    const state = context();
    state.zcodeAccounts.accounts = [
      {
        id: 'none',
        email: 'none@example.test',
        provider: 'bigmodel',
        active: false,
        plan: 'No Plan',
        remainingPercent: null,
        resetAt: null,
      },
    ];
    const provider = providers(state).find((p) => p.id === 'zcode')!;
    const account = provider.accounts[0];
    expect(account.plan).toBe('agent.zcodeNoPlan');
    expect(account.subscriptionEndAt).toBeUndefined();
    expect(account.metrics[0]).toMatchObject({ value: '—', percent: null });
    const html = renderToStaticMarkup(<AccountCard provider={provider} account={account} />);
    expect(html).toContain('Agent.zcodeNoPlan');
    expect(html).not.toContain('aria-label="accountCenter.subscription"');
    expect(html).not.toContain('0%');
    expect(state.zcodeAccounts.refresh).not.toHaveBeenCalled();
  });
});
