import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { AppManagerContext, type AppManagerContextType } from './context';
import { CodexAccountSection } from './AppManagerComponents';
import { AccountCard } from '../AccountCenter/AccountCenter';
import type { AccountCardData, AccountProvider } from '../AccountCenter/accountCenterData';

const actions = vi.hoisted(() => ({ invoke: vi.fn(), refresh: vi.fn() }));
vi.hoisted(() => vi.stubGlobal('__APP_EDITION__', 'full'));
vi.mock('@tauri-apps/api/core', () => ({ invoke: actions.invoke }));
vi.mock('../../components', () => ({ getModelIcon: () => null }));
vi.mock('../../hooks/useI18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }));

let view: ReactTestRenderer | undefined;
beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2026-10-22T06:00:00Z'));
  vi.clearAllMocks();
});
afterEach(() => {
  act(() => view?.unmount());
  view = undefined;
  vi.useRealTimers();
});

function fixture(page: 'center' | 'desktop', subscriptionEndAt?: number | null) {
  const account: AccountCardData = {
    id: 'saved',
    identity: 'fixture@example.test',
    plan: 'Pro 100',
    subscriptionEndAt,
    metrics: [{ label: '7d', value: '69%', percent: 69 }],
    refreshing: false,
    authorizationFailed: false,
    refresh: actions.refresh,
    remove: vi.fn(),
  };
  const provider: AccountProvider = {
    id: 'codex',
    name: 'ChatGPT / Codex',
    icon: '/icons/tools/codex.svg',
    installed: true,
    busy: false,
    loading: false,
    remainingSeconds: 0,
    add: vi.fn(),
    cancelLogin: vi.fn(),
    accounts: [account],
  };
  if (page === 'center') return <AccountCard provider={provider} account={account} />;
  const context = {
    codexAccounts: [
      {
        id: account.id,
        email: account.identity,
        plan: 'prolite',
        active: true,
        subscriptionEndAt,
        quotaWindows: [{ label: '7d', remainingPercent: 69 }],
      },
    ],
    selectedCodexAccountId: account.id,
    isLoadingCodexAccounts: false,
    isAddingCodexAccount: false,
    codexOAuthRemainingSeconds: 0,
    refreshingCodexAccountIds: new Set(),
    codexAuthorizationFailedIds: new Set(),
    refreshCodexAccountQuota: actions.refresh,
  } as unknown as AppManagerContextType;
  return (
    <AppManagerContext.Provider value={context}>
      <CodexAccountSection showDivider={false} />
    </AppManagerContext.Provider>
  );
}
function textSpans(value: string) {
  return view!.root.findAllByType('span').filter((node) => node.children.includes(value));
}

it.each(['center', 'desktop'] as const)(
  '%s: missing subscription expiry leaves no dash or countdown',
  async (page) => {
    for (const missing of [null, undefined]) {
      await act(async () => {
        view = create(fixture(page, missing));
      });
      expect(textSpans('—')).toHaveLength(0);
      expect(textSpans('0m')).toHaveLength(0);
      expect(textSpans('Pro 100')).toHaveLength(1);
      act(() => view!.unmount());
      view = undefined;
    }
    expect(actions.invoke).not.toHaveBeenCalled();
    expect(actions.refresh).not.toHaveBeenCalled();
  }
);

it.each(['center', 'desktop'] as const)(
  '%s: expiry hides the countdown and an actual renewal shows the new deadline',
  async (page) => {
    await act(async () => {
      view = create(fixture(page, Date.now() / 1000 + 30));
    });
    expect(textSpans('1m')).toHaveLength(1);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    expect(textSpans('0m')).toHaveLength(0);
    expect(textSpans('1m')).toHaveLength(0);
    expect(textSpans('—')).toHaveLength(0);
    expect(textSpans('Pro 100')).toHaveLength(1);
    expect(actions.invoke).not.toHaveBeenCalled();
    expect(actions.refresh).not.toHaveBeenCalled();
    await act(async () => {
      view!.update(fixture(page, Date.now() / 1000 + 31 * 86400));
    });
    expect(textSpans('31d0h')).toHaveLength(1);
    await act(async () => {
      view!.update(fixture(page, null));
    });
    expect(textSpans('31d0h')).toHaveLength(0);
    expect(textSpans('—')).toHaveLength(0);
    expect(actions.invoke).not.toHaveBeenCalled();
    expect(actions.refresh).not.toHaveBeenCalled();
  }
);
