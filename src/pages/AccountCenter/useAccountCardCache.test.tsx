import { useLayoutEffect } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AccountCardData, AccountProvider } from './accountCenterData';
import { useAccountCardCache } from './useAccountCardCache';
import { AccountCard } from './AccountCenter';

const STORAGE_KEY = 'echobird_account_card_cache';
type Options = { locale: string; active: boolean; toolsReady: boolean; scanning: boolean };
const ready: Options = { locale: 'en', active: true, toolsReady: true, scanning: false };
let renderer: ReactTestRenderer | undefined;
let current: ReturnType<typeof useAccountCardCache>;
let values: Map<string, string>;
let storage: { getItem: ReturnType<typeof vi.fn>; setItem: ReturnType<typeof vi.fn> };

function account(id: string, value = '75%'): AccountCardData {
  return {
    id,
    identity: `${id}@example.test`,
    plan: 'Pro',
    subscriptionEndAt: 1_900_000_000,
    metrics: [{ label: '7d', value, percent: 75, resetAt: 1_800_000_000 }],
    refreshing: false,
    authorizationFailed: false,
    refresh: vi.fn().mockResolvedValue(undefined),
    remove: vi.fn(),
  };
}

function provider(id: string, accounts: AccountCardData[], patch: Partial<AccountProvider> = {}) {
  return {
    id,
    name: id,
    icon: `/icons/${id}.svg`,
    installed: true,
    busy: false,
    loading: false,
    loaded: true,
    remainingSeconds: 0,
    add: vi.fn(),
    cancelLogin: vi.fn(),
    accounts,
    ...patch,
  } satisfies AccountProvider;
}

describe('rows of an uninstalled account plugin', () => {
  it('can be cleared but not refreshed', async () => {
    snapshot([provider('claudeCode', [account('a'), account('b')])]);
    await mount([provider('claudeCode', [], { loaded: false, pluginMissing: true })]);
    const [row] = current.providers[0].accounts;
    expect(row.pending).toBe(true);
    expect(row.cachedOnly).toBe(true);
    await act(async () => {
      row.remove();
    });
    expect(current.providers[0].accounts.map(({ id }) => id)).toEqual(['b']);
    expect(JSON.parse(values.get(STORAGE_KEY)!).providers.claudeCode).toHaveLength(1);
  });

  it('stay locked while a plugin is installed but still loading', async () => {
    snapshot([provider('claudeCode', [account('a')])]);
    await mount([provider('claudeCode', [], { loaded: false })]);
    expect(current.providers[0].accounts[0].cachedOnly).toBe(false);
  });
});

function snapshot(providers: AccountProvider[], locale = 'en') {
  const cached = Object.fromEntries(
    providers.map((entry) => [
      entry.id,
      entry.accounts.map(
        ({ refresh: _refresh, remove: _remove, claim, refreshing: _refreshing, ...row }) => ({
          ...row,
          claimable: Boolean(claim),
        })
      ),
    ])
  );
  values.set(STORAGE_KEY, JSON.stringify({ version: 1, locale, providers: cached }));
}

function Harness({ providers, options }: { providers: AccountProvider[]; options: Options }) {
  const result = useAccountCardCache(providers, options);
  useLayoutEffect(() => {
    current = result;
  });
  return null;
}

async function mount(providers: AccountProvider[], options: Partial<Options> = {}) {
  await act(async () => {
    renderer = create(<Harness providers={providers} options={{ ...ready, ...options }} />);
  });
}

async function update(providers: AccountProvider[], options: Partial<Options> = {}) {
  await act(async () => {
    renderer!.update(<Harness providers={providers} options={{ ...ready, ...options }} />);
  });
}

function rows() {
  return current.providers.flatMap((entry) => entry.accounts);
}

function persisted() {
  return JSON.parse(values.get(STORAGE_KEY)!) as {
    version: number;
    locale: string;
    providers: Record<string, Array<Record<string, unknown>>>;
  };
}

beforeEach(() => {
  values = new Map();
  storage = {
    getItem: vi.fn((key: string) => values.get(key) ?? null),
    setItem: vi.fn((key: string, value: string) => values.set(key, value)),
  };
  vi.stubGlobal('localStorage', storage);
});

afterEach(() => {
  if (renderer) act(() => renderer!.unmount());
  renderer = undefined;
  vi.unstubAllGlobals();
});

describe('Account Center card cache', () => {
  it('preserves balance availability through the displayed snapshot, remount, and explicit refresh', async () => {
    const row = account('credits');
    row.metrics = [
      { label: 'agent.baseCredits', value: '2,000', showProgress: true, balanceState: 'available' },
      {
        label: 'agent.rewardCredits',
        value: '99.75',
        showProgress: true,
        balanceState: 'available',
      },
    ];
    const qwen = provider('qwenworkcn', [row]);
    await mount([qwen]);
    expect(rows()[0].metrics).toEqual(row.metrics);
    let html = renderToStaticMarkup(
      <AccountCard provider={current.providers[0]} account={rows()[0]} />
    );
    expect(html.match(/border-green-500/g)).toHaveLength(2);
    expect(html).not.toContain('aria-valuenow=');
    act(() => renderer!.unmount());
    renderer = undefined;
    await mount([{ ...qwen, accounts: [], loading: true, loaded: false }]);
    expect(rows()[0].metrics).toEqual(row.metrics);
    expect(rows()[0].pending).toBe(true);
    await update([
      {
        ...qwen,
        mutationRevision: 1,
        accounts: [
          {
            ...row,
            metrics: [
              { ...row.metrics[0], value: '0', balanceState: 'empty' },
              { label: 'agent.rewardCredits', value: '—', showProgress: true },
            ],
          },
        ],
      },
    ]);
    html = renderToStaticMarkup(
      <AccountCard provider={current.providers[0]} account={rows()[0]} />
    );
    expect(html).not.toContain('border-green-500');
    expect(html).toContain('model.noUsageData');
    expect(row.refresh).not.toHaveBeenCalled();
  });

  it('keeps the first view in its loading state until every provider finishes, then reveals all rows together', async () => {
    const alpha = provider('alpha', [], { loaded: false, loading: true });
    const beta = provider('beta', [], { loaded: false, loading: true });
    await mount([alpha, beta]);
    expect(current.loading).toBe(true);
    expect(rows()).toEqual([]);

    const loadedAlpha = { ...alpha, accounts: [account('alpha')], loaded: true, loading: false };
    await update([loadedAlpha, beta]);
    expect(current.loading).toBe(true);
    expect(rows()).toEqual([]);
    expect(storage.setItem).not.toHaveBeenCalled();

    await update([
      loadedAlpha,
      { ...beta, accounts: [account('beta')], loaded: true, loading: false },
    ]);
    expect(current.loading).toBe(false);
    expect(rows().map((row) => row.id)).toEqual(['alpha', 'beta']);
    expect(rows().every((row) => !row.pending)).toBe(true);
    expect(Object.keys(persisted().providers)).toEqual(['alpha', 'beta']);
  });

  it('restores every cached card on remount and replaces a background batch only after all reads finish', async () => {
    const alpha = provider('alpha', [account('alpha', 'old alpha')]);
    const beta = provider('beta', [account('beta', 'old beta')]);
    await mount([alpha, beta]);
    act(() => renderer!.unmount());
    renderer = undefined;

    const pendingAlpha = { ...alpha, accounts: [], loaded: false, loading: true };
    const pendingBeta = { ...beta, accounts: [], loaded: false, loading: true };
    await mount([pendingAlpha, pendingBeta]);
    expect(current.loading).toBe(true);
    expect(rows().map((row) => row.metrics[0].value)).toEqual(['old alpha', 'old beta']);
    expect(rows().every((row) => row.pending)).toBe(true);

    const newAlpha = { ...alpha, accounts: [account('alpha', 'new alpha')] };
    await update([newAlpha, pendingBeta]);
    expect(rows().map((row) => row.metrics[0].value)).toEqual(['old alpha', 'old beta']);
    expect(rows().map((row) => row.pending)).toEqual([false, true]);
    expect(rows()[0].refresh).toBe(newAlpha.accounts[0].refresh);
    expect(persisted().providers.alpha[0].metrics).toEqual(alpha.accounts[0].metrics);

    await update([newAlpha, { ...beta, accounts: [account('beta', 'new beta')] }]);
    expect(rows().map((row) => row.metrics[0].value)).toEqual(['new alpha', 'new beta']);
    expect(rows().every((row) => !row.pending)).toBe(true);
  });

  it('retains cached rows for a failed provider while accepting the other successful reads', async () => {
    const alpha = provider('alpha', [account('alpha', 'cached')]);
    const beta = provider('beta', [account('beta')]);
    snapshot([alpha, beta]);
    await mount([
      { ...alpha, accounts: [], loaded: false, readFailed: true },
      { ...beta, accounts: [account('new-beta')] },
    ]);
    expect(current.loading).toBe(false);
    expect(rows().map((row) => row.id)).toEqual(['alpha', 'new-beta']);
    expect(rows()[0].pending).toBe(true);
    expect(persisted().providers.alpha[0].id).toBe('alpha');
    expect(persisted().providers.beta[0].id).toBe('new-beta');
  });

  it('treats a successful empty read as a removal and does not resurrect it on the next mount', async () => {
    const alpha = provider('alpha', [account('deleted')]);
    const beta = provider('beta', [account('kept')]);
    snapshot([alpha, beta]);
    await mount([{ ...alpha, accounts: [] }, beta]);
    expect(rows().map((row) => row.id)).toEqual(['kept']);
    expect(persisted().providers.alpha).toEqual([]);
    act(() => renderer!.unmount());
    renderer = undefined;
    await mount([
      { ...alpha, accounts: [], loaded: false, loading: true },
      { ...beta, accounts: [], loaded: false, loading: true },
    ]);
    expect(rows().map((row) => row.id)).toEqual(['kept']);
  });

  it('synchronizes additions, deletions, and refreshed quota without requiring another page entry', async () => {
    const first = account('first');
    const alpha = provider('alpha', [first]);
    const beta = provider('beta', []);
    await mount([alpha, beta]);
    const second = account('second');
    await update([{ ...alpha, accounts: [first, second] }, beta]);
    expect(persisted().providers.alpha.map((row) => row.id)).toEqual(['first', 'second']);
    const refreshed = { ...second, metrics: [{ label: '7d', value: '20%', percent: 20 }] };
    await update([{ ...alpha, accounts: [refreshed] }, beta]);
    expect(rows().map((row) => row.id)).toEqual(['second']);
    expect(persisted().providers.alpha).toHaveLength(1);
    expect(persisted().providers.alpha[0].metrics).toEqual(refreshed.metrics);
  });

  it('rebinds actions to the latest live account even when its displayed values did not change', async () => {
    const first = account('same');
    const alpha = provider('alpha', [first]);
    await mount([alpha, provider('beta', [])]);
    const latest = account('same');
    await update([{ ...alpha, accounts: [latest] }, provider('beta', [])]);
    await rows()[0].refresh();
    rows()[0].remove();
    expect(first.refresh).not.toHaveBeenCalled();
    expect(first.remove).not.toHaveBeenCalled();
    expect(latest.refresh).toHaveBeenCalledOnce();
    expect(latest.remove).toHaveBeenCalledOnce();
  });

  it('synchronizes an explicit mutation immediately while another provider is still reading', async () => {
    const alpha = provider('alpha', [account('deleted')], { mutationRevision: 0 });
    const beta = provider('beta', [account('kept')], { mutationRevision: 0 });
    snapshot([alpha, beta]);
    const pendingBeta = { ...beta, accounts: [], loading: true, loaded: false };
    await mount([{ ...alpha, loading: true }, pendingBeta]);
    await update([{ ...alpha, accounts: [account('added')], mutationRevision: 1 }, pendingBeta]);
    expect(rows().map((row) => row.id)).toEqual(['added', 'kept']);
    expect(persisted().providers.alpha.map((row) => row.id)).toEqual(['added']);
    expect(persisted().providers.beta.map((row) => row.id)).toEqual(['kept']);
  });

  it.each([false, true])(
    'does not commit a passive partial batch when leaving the page (cached: %s)',
    async (cached) => {
      const alpha = provider('alpha', [account('old-alpha')]);
      const beta = provider('beta', [account('old-beta')]);
      if (cached) snapshot([alpha, beta]);
      const initial = values.get(STORAGE_KEY);
      const loadedAlpha = { ...alpha, accounts: [account('new-alpha')] };
      const pendingBeta = { ...beta, accounts: [], loading: true, loaded: false };
      await mount([{ ...alpha, loading: true, loaded: false }, pendingBeta]);
      await update([loadedAlpha, pendingBeta]);
      await update([loadedAlpha, { ...pendingBeta, loading: false }], { active: false });
      expect(values.get(STORAGE_KEY)).toBe(initial);
      expect(storage.setItem).not.toHaveBeenCalled();
      expect(rows().map((row) => row.id)).toEqual(cached ? ['old-alpha', 'old-beta'] : []);
    }
  );

  it('saves an explicit mutation on another page without publishing an unrelated partial read', async () => {
    const alpha = provider('alpha', [account('old-alpha')], { mutationRevision: 0 });
    const beta = provider('beta', [account('old-beta')], { mutationRevision: 0 });
    snapshot([alpha, beta]);
    await mount([alpha, beta]);
    await update(
      [
        { ...alpha, accounts: [account('new-alpha')], mutationRevision: 1 },
        { ...beta, accounts: [account('partial-beta')] },
      ],
      { active: false }
    );
    expect(rows().map((row) => row.id)).toEqual(['new-alpha', 'old-beta']);
    expect(persisted().providers.alpha.map((row) => row.id)).toEqual(['new-alpha']);
    expect(persisted().providers.beta.map((row) => row.id)).toEqual(['old-beta']);
  });

  it('merges mutations into the saved complete grid until the first complete live list succeeds', async () => {
    const alpha = provider('alpha', [account('a'), account('b')], { mutationRevision: 0 });
    const beta = provider('beta', []);
    snapshot([alpha, beta]);
    const failed = { ...alpha, accounts: [], loaded: false, readFailed: true };
    await mount([failed, beta]);
    expect(rows().map((row) => row.id)).toEqual(['a', 'b']);

    const added = {
      ...failed,
      accounts: [account('c')],
      mutationRevision: 1,
      loading: true,
      readFailed: false,
    };
    await update([added, beta]);
    expect(rows().map((row) => row.id)).toEqual(['a', 'b', 'c']);
    expect(persisted().providers.alpha.map((row) => row.id)).toEqual(['a', 'b', 'c']);

    const failedAgain = { ...added, loading: false, readFailed: true };
    await update([failedAgain, beta]);
    expect(rows().map((row) => row.id)).toEqual(['a', 'b', 'c']);

    const refreshed = {
      ...failedAgain,
      accounts: [account('c', '20%')],
      mutationRevision: 2,
      readFailed: false,
    };
    await update([refreshed, beta]);
    expect(rows().map((row) => row.id)).toEqual(['a', 'b', 'c']);
    expect(rows().find((row) => row.id === 'c')?.metrics[0].value).toBe('20%');
    expect(persisted().providers.alpha.map((row) => row.id)).toEqual(['a', 'b', 'c']);

    await update([{ ...refreshed, accounts: [], mutationRevision: 3 }, beta]);
    expect(rows().map((row) => row.id)).toEqual(['a', 'b']);
    expect(persisted().providers.alpha.map((row) => row.id)).toEqual(['a', 'b']);

    await update([
      { ...refreshed, accounts: [], mutationRevision: 3, loaded: true, readFailed: false },
      beta,
    ]);
    expect(rows()).toEqual([]);
    expect(persisted().providers.alpha).toEqual([]);
  });

  it('does not attach actions from another live account to cached-only rows', async () => {
    const cached = { ...account('cached'), claim: vi.fn().mockResolvedValue(undefined) };
    const live = { ...account('other'), claim: vi.fn().mockResolvedValue(undefined) };
    const alpha = provider('alpha', [cached]);
    snapshot([alpha, provider('beta', [])]);
    await mount([
      { ...alpha, accounts: [live], loading: true, loaded: false },
      provider('beta', [], { loading: true, loaded: false }),
    ]);
    const row = rows()[0];
    expect(row.id).toBe('cached');
    expect(row.pending).toBe(true);
    await row.refresh();
    row.remove();
    await row.claim?.();
    expect(cached.refresh).not.toHaveBeenCalled();
    expect(cached.remove).not.toHaveBeenCalled();
    expect(cached.claim).not.toHaveBeenCalled();
    expect(live.refresh).not.toHaveBeenCalled();
    expect(live.remove).not.toHaveBeenCalled();
    expect(live.claim).not.toHaveBeenCalled();
  });

  it('persists only display fields and claim availability, without credentials or callbacks', async () => {
    const secret = {
      ...account('safe'),
      detail: 'Displayed detail',
      claim: vi.fn().mockResolvedValue(undefined),
      refreshing: true,
      token: 'private-token-value',
      accessToken: 'private-access-value',
      refreshToken: 'private-refresh-value',
      apiKey: 'private-key-value',
      nativeAccount: { password: 'private-password-value' },
    };
    await mount([provider('alpha', [secret]), provider('beta', [])]);
    const raw = values.get(STORAGE_KEY)!;
    const saved = persisted().providers.alpha[0];
    expect(saved.identity).toBe('safe@example.test');
    expect(saved.detail).toBe('Displayed detail');
    expect(saved.claimable).toBe(true);
    for (const key of [
      'refresh',
      'remove',
      'claim',
      'refreshing',
      'pending',
      'token',
      'accessToken',
      'refreshToken',
      'apiKey',
      'nativeAccount',
    ]) {
      expect(saved).not.toHaveProperty(key);
    }
    expect(raw).not.toContain('private-');
  });

  it.each(['{broken', JSON.stringify({ version: 9, locale: 'en', providers: {} })])(
    'falls back to normal first loading when the stored cache is invalid: %s',
    async (raw) => {
      values.set(STORAGE_KEY, raw);
      await mount([
        provider('alpha', [], { loading: true, loaded: false }),
        provider('beta', [], { loading: true, loaded: false }),
      ]);
      expect(current.loading).toBe(true);
      expect(rows()).toEqual([]);
      await update([provider('alpha', [account('fresh')]), provider('beta', [])]);
      expect(current.loading).toBe(false);
      expect(rows()[0].id).toBe('fresh');
      expect(persisted().version).toBe(1);
    }
  );

  it('keeps the page usable when local storage cannot be read or written', async () => {
    storage.getItem.mockImplementation(() => {
      throw new Error('storage unavailable');
    });
    storage.setItem.mockImplementation(() => {
      throw new Error('storage full');
    });
    await mount([provider('alpha', [], { loading: true, loaded: false }), provider('beta', [])]);
    expect(current.loading).toBe(true);
    await update([provider('alpha', [account('live')]), provider('beta', [])]);
    expect(current.loading).toBe(false);
    expect(rows()[0].id).toBe('live');
    expect(rows()[0].pending).toBeFalsy();
  });

  it('does not reuse localized display data from a different locale', async () => {
    snapshot([provider('alpha', [account('english')]), provider('beta', [])]);
    await mount(
      [
        provider('alpha', [], { loading: true, loaded: false }),
        provider('beta', [], { loading: true, loaded: false }),
      ],
      { locale: 'zh' }
    );
    expect(current.loading).toBe(true);
    expect(rows()).toEqual([]);
    await update([provider('alpha', [account('chinese')]), provider('beta', [])], { locale: 'zh' });
    expect(rows()[0].id).toBe('chinese');
    expect(persisted().locale).toBe('zh');
  });

  it.each([
    { toolsReady: false, scanning: false },
    { toolsReady: true, scanning: true },
  ])('preserves the cache until tool discovery has settled: %j', async (options) => {
    const alpha = provider('alpha', [account('saved')]);
    snapshot([alpha, provider('beta', [])]);
    const initial = values.get(STORAGE_KEY);
    await mount(
      [provider('alpha', [], { installed: false }), provider('beta', [], { installed: false })],
      options
    );
    expect(rows().map((row) => row.id)).toEqual(['saved']);
    expect(rows()[0].pending).toBe(true);
    expect(values.get(STORAGE_KEY)).toBe(initial);
    expect(storage.setItem).not.toHaveBeenCalled();
    await update([alpha, provider('beta', [])]);
    expect(rows()[0].pending).toBeFalsy();
  });
});
