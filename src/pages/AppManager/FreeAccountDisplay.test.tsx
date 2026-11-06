import { renderToStaticMarkup } from 'react-dom/server';
import { act, create } from 'react-test-renderer';
import { beforeEach, expect, it, vi } from 'vitest';
import type { AppManagerContextType } from './context';
import { AccountCard } from '../AccountCenter/AccountCenter';
import { accountCenterProviders } from '../AccountCenter/accountCenterData';
import { AntigravityAccountSection } from './AntigravityAccountSection';
import { CodexAccountSection } from './AppManagerComponents';
import { ClaudeCodeAccountSection } from './ClaudeCodeAccountSection';
import { CursorAccountSection } from './CursorAccountSection';
import { GrokAccountSection } from './GrokAccountSection';
import { ManusAccountSection } from './ManusAccountSection';
import { WorkBuddyAccountSection } from './WorkBuddyAccountSection';
import { ZCodeAccountSection } from './ZCodeAccountSection';
import { ClineAccountSection } from './ClineAccountSection';
import { ClientAccountSection } from './ClientAccountSection';
import { DeepSeekAccountSection } from './DeepSeekAccountSection';
import { MinimaxAccountSection } from './MinimaxAccountSection';
import { QoderAccountSection } from './QoderAccountSection';
import { OpenDesignAccountSection } from './OpenDesignAccountSection';
import { KiroAccountSection } from './KiroAccountSection';
import { TraeAccountSection } from './TraeAccountSection';
import { useAccountPrivacyStore } from '../../stores/accountPrivacyStore';
import { en } from '../../i18n/en';
import zhHans from '../../i18n/zh-Hans';

let state: AppManagerContextType;
let missingLabel = 'model.noUsageData';
vi.mock('./context', () => ({ useAppManager: () => state }));
vi.mock('../../hooks/useI18n', () => ({
  useI18n: () => ({
    t: (key: string) => (key === 'model.noUsageData' ? missingLabel : key),
    locale: 'en',
  }),
}));
vi.mock('../../components', () => ({ getModelIcon: () => null }));

const sections = {
  codex: () => <CodexAccountSection />,
  claudecode: () => <ClaudeCodeAccountSection />,
  claudedesktop: () => <ClaudeCodeAccountSection desktop />,
  cursor: () => <CursorAccountSection tool="cursor" />,
  grokbot: () => <CursorAccountSection tool="grokbot" />,
  grok: () => <GrokAccountSection />,
  manus: () => <ManusAccountSection />,
  cue: () => <ManusAccountSection tool="cue" />,
  antigravity: () => <AntigravityAccountSection />,
  workbuddy: () => <WorkBuddyAccountSection />,
  zcode: () => <ZCodeAccountSection />,
  cline: () => <ClineAccountSection />,
  kilo: () => <ClientAccountSection provider="kilo" />,
  copilot: () => <ClientAccountSection provider="copilot" />,
  dsh: () => <DeepSeekAccountSection />,
  minimax: () => <MinimaxAccountSection />,
  qoder: () => <QoderAccountSection />,
};
type Tool = keyof typeof sections;
const row = () => ({
  id: 'saved',
  email: 'free@example.test',
  login: 'free@example.test',
  name: 'Free user',
  active: true,
  plan: 'FREE',
  provider: 'bigmodel',
  edition: 'workbuddy',
  remaining: null,
  total: null,
  remainingPercent: null,
  resetAt: null,
  expiresAt: null,
  quotas: [],
  quotaWindows: [],
  balances: null,
  usage: { plan: 'FREE', remainingPercent: null, resetAt: null },
  credits: null,
});
beforeEach(() => {
  useAccountPrivacyStore.setState({ hidden: false });
  missingLabel = 'model.noUsageData';
  const account = row();
  const group = {
    accounts: [account],
    busy: false,
    loading: false,
    remainingSeconds: 0,
    refreshing: new Set(),
    authorizationFailedIds: new Set(),
    add: vi.fn(),
    select: vi.fn(),
    refresh: vi.fn(),
    remove: vi.fn(),
    claimDaily: vi.fn(),
  };
  const emptyGroup = { ...group, accounts: [] };
  state = {
    detectedTools: [],
    isLaunching: false,
    selectedTool: 'workbuddy',
    codexAccounts: [account],
    selectedCodexAccountId: 'saved',
    isAddingCodexAccount: false,
    isLoadingCodexAccounts: false,
    codexOAuthRemainingSeconds: 0,
    refreshingCodexAccountIds: new Set(),
    codexAuthorizationFailedIds: new Set(),
    addCodexAccount: vi.fn(),
    setSelectedCodexAccountId: vi.fn(),
    refreshCodexAccountQuota: vi.fn(),
    deleteCodexAccount: vi.fn(),
    claudeCodeAccounts: group,
    claudeDesktopAccounts: group,
    cursorAccounts: group,
    grokBotAccounts: group,
    grokAccounts: group,
    clineAccounts: group,
    kiloAccounts: group,
    copilotAccounts: group,
    manusAccounts: group,
    cueAccounts: group,
    antigravityAccounts: group,
    workBuddyAccounts: group,
    workBuddyAccountGroups: { workbuddy: group, workbuddyai: group },
    traeAccountGroups: { trae: { ...group, accounts: [] }, traecn: { ...group, accounts: [] } },
    kiroAccounts: { ...group, accounts: [] },
    zcodeAccounts: { ...group, provider: 'bigmodel', setProvider: vi.fn() },
    kimiAccounts: { ...group, region: 'cn', setRegion: vi.fn() },
    openDesignAccounts: { ...group },
    minimaxAccounts: { ...group, region: 'cn', setRegion: vi.fn() },
    qoderAccounts: group,
    qoderAccountGroups: { qoder: group, qodercn: emptyGroup },
    qwenWorkAccountGroups: { qwenwork: emptyGroup, qwenworkcn: emptyGroup },
    deepSeekAccounts: group,
  } as unknown as AppManagerContextType;
});
function markup(tool: Tool) {
  const provider = accountCenterProviders(state, (key) => key, 'en').find((p) => p.id === tool)!;
  return [
    renderToStaticMarkup(sections[tool]()),
    renderToStaticMarkup(<AccountCard provider={provider} account={provider.accounts[0]} />),
  ];
}
const privacySections = {
  ...sections,
  opendesign: () => <OpenDesignAccountSection />,
  kiro: () => <KiroAccountSection />,
  trae: () => <TraeAccountSection />,
};
it.each(Object.keys(privacySections) as (keyof typeof privacySections)[])(
  '%s: follows global privacy without an extra desktop button or passive account actions',
  (tool) => {
    Object.assign(state.codexAccounts[0], {
      quotaPercent: 69,
      usage: { plan: 'FREE', remainingPercent: 69, resetAt: null },
      weekly: { remainingPercent: 69, resetAt: null },
    });
    Object.assign(state.kiroAccounts, { accounts: [{ ...state.codexAccounts[0], quota: [] }] });
    Object.assign(state, { traeAccounts: state.workBuddyAccounts });
    let renderer!: ReturnType<typeof create>;
    act(() => {
      renderer = create(privacySections[tool]());
    });
    try {
      const visible = JSON.stringify(renderer.toJSON());
      expect(visible).toMatch(/free@example.test|Free user/);
      act(() => useAccountPrivacyStore.getState().setHidden(true));
      const hidden = JSON.stringify(renderer.toJSON());
      expect(hidden).not.toContain('free@example.test');
      expect(hidden).not.toContain('Free user');
      expect(hidden).toContain('******');
      expect(hidden).not.toContain('freeModels.showAccountNames');
      expect(hidden).not.toContain('freeModels.hideAccountNames');
      for (const action of [
        state.addCodexAccount,
        state.setSelectedCodexAccountId,
        state.refreshCodexAccountQuota,
        state.deleteCodexAccount,
        state.workBuddyAccounts.add,
        state.workBuddyAccounts.select,
        state.workBuddyAccounts.refresh,
        state.workBuddyAccounts.remove,
        state.workBuddyAccounts.claimDaily,
      ])
        expect(action).not.toHaveBeenCalled();
      act(() => useAccountPrivacyStore.getState().setHidden(false));
      expect(JSON.stringify(renderer.toJSON())).toBe(visible);
      expect(state.codexAccounts[0].email).toBe('free@example.test');
    } finally {
      act(() => renderer.unmount());
      useAccountPrivacyStore.setState({ hidden: false });
    }
  }
);
it.each(Object.keys(sections) as Tool[])(
  '%s: shows Free and the shared missing-usage message in both account views',
  (tool) => {
    if (tool === 'minimax') state.minimaxAccounts.accounts[0].plan = 'M Plan Free';
    for (const html of markup(tool)) {
      if (tool !== 'dsh') expect(html).toContain('>Free<');
      expect(html).toContain('>model.noUsageData<');
      expect(html).not.toContain('>—<');
      expect(html).not.toContain('role="progressbar"');
      expect(html).not.toContain('>0%<');
    }
  }
);
it.each(['Pro', null])(
  'shows the same localized missing-usage status for plan %s in both views',
  (plan) => {
    state.codexAccounts[0].plan = plan ?? undefined;
    state.cursorAccounts.accounts[0].usage!.plan = plan ?? '';
    for (const tool of Object.keys(sections) as Tool[]) {
      const [desktop, card] = markup(tool);
      for (const html of [desktop, card]) {
        expect(html).toContain('model.noUsageData');
        expect(html).not.toContain('role="progressbar"');
        expect(html).not.toContain('>0%<');
      }
      expect(desktop).toContain('grid h-14');
      expect(card).toContain('flex h-44 flex-col gap-2 rounded-card');
    }
  }
);
it.each([en, zhHans])(
  'uses the existing locale translation for every unknown account quota',
  (translations) => {
    missingLabel = translations['model.noUsageData']!;
    for (const tool of Object.keys(sections) as Tool[]) {
      for (const html of markup(tool)) {
        expect(html).toContain(missingLabel);
        expect(html).not.toContain('model.noUsageData');
      }
    }
  }
);
it.each(Object.keys(sections).filter((tool) => tool !== 'grok') as Tool[])(
  '%s: retains genuine zero usage for a Free account',
  (tool) => {
    if (tool === 'minimax') state.minimaxAccounts.accounts[0].plan = 'M Plan Free';
    Object.assign(state.codexAccounts[0], {
      quotaPercent: 0,
      remainingPercent: 0,
      remaining: 0,
      baseRemaining: 0,
      baseTotal: 100,
      fiveHour: { remainingPercent: 0 },
      sevenDay: { remainingPercent: 0 },
      quotas: [
        { name: 'gemini-3', remainingPercent: 0 },
        { name: 'claude', remainingPercent: 0 },
      ],
      usage: { plan: 'FREE', remainingPercent: 0 },
      credits: { total: 0 },
      balanceMicros: 0,
      balance: 0,
      balances: [{ currency: 'USD', amount: 0 }],
      quota: { credits: 0, windows: [] },
    });
    for (const html of markup(tool)) {
      if (tool !== 'dsh') expect(html).toContain('>Free<');
      expect(html).not.toContain('>model.noUsageData<');
      expect(html.replace(/<[^>]*>/g, ' ')).toMatch(/\b0(?:%|\b)/);
    }
  }
);
it.each([null, 'SuperGrok'])(
  'keeps Grok plan %s distinct from Free when no numerical usage is available',
  (plan) => {
    state.grokAccounts.accounts[0].plan = plan;
    for (const html of markup('grok')) {
      expect(html).toContain('>model.noUsageData<');
      expect(html).not.toContain('>Free<');
      if (plan) expect(html).toContain(`>${plan}<`);
    }
  }
);

it.each(['free', 'plus', 'pro', 'max'])(
  'Cue: displays the actual %s tier and weekly quota in both account views',
  (plan) => {
    state.cueAccounts = {
      ...state.cueAccounts,
      accounts: [
        {
          ...row(),
          plan,
          credits: { total: 20000, free: null, refresh: null, nextRefreshAt: null },
          weekly: { remainingPercent: 0, resetAt: 1900000000 },
        },
      ],
    };
    for (const html of markup('cue')) {
      expect(html).toContain(`>${plan[0].toUpperCase()}${plan.slice(1)}<`);
      expect(html).toContain('>0%<');
      expect(html).toContain('aria-label="7d free@example.test"');
      expect(html).not.toContain('>model.noUsageData<');
      expect(html).not.toContain('20000');
      expect(html).not.toContain('title=');
      expect(html).not.toContain('cursor-');
    }
    expect(state.cueAccounts.refresh).not.toHaveBeenCalled();
    expect(state.cueAccounts.add).not.toHaveBeenCalled();
    expect(markup('manus')[0]).toContain('>model.noUsageData<');
  }
);

it('Cue: does not fabricate weekly usage for an inactive Free window', () => {
  state.cueAccounts = {
    ...state.cueAccounts,
    accounts: [{ ...row(), weekly: { remainingPercent: null, resetAt: null } }],
  };
  for (const html of markup('cue')) {
    expect(html).toContain('>Free<');
    expect(html).toContain('>model.noUsageData<');
    expect(html).not.toContain('role="progressbar"');
    expect(html).not.toContain('>0%<');
  }
});
