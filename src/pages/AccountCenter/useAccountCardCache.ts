import { useEffect, useRef, useState } from 'react';
import type { AccountCardData, AccountMetric, AccountProvider } from './accountCenterData';

const CACHE_KEY = 'echobird_account_card_cache';

export type CachedAccount = Pick<
  AccountCardData,
  | 'id'
  | 'identity'
  | 'detail'
  | 'plan'
  | 'subscriptionEndAt'
  | 'metrics'
  | 'authorizationFailed'
  | 'claimDisabled'
  | 'dailyClaimedAt'
  | 'dailyClaimed'
  | 'claimStatusExpiresAt'
  | 'showUnclaimed'
> & { claimable: boolean };

interface Snapshot {
  version: 1;
  locale: string;
  providers: Record<string, CachedAccount[]>;
}

const optionalNumber = (value: unknown) =>
  value == null || (typeof value === 'number' && Number.isFinite(value));
const optionalString = (value: unknown) => value == null || typeof value === 'string';
const optionalBoolean = (value: unknown) => value == null || typeof value === 'boolean';

function validAccount(value: CachedAccount): boolean {
  return (
    value != null &&
    typeof value.id === 'string' &&
    typeof value.identity === 'string' &&
    optionalString(value.detail) &&
    optionalString(value.plan) &&
    optionalNumber(value.subscriptionEndAt) &&
    typeof value.authorizationFailed === 'boolean' &&
    typeof value.claimable === 'boolean' &&
    optionalBoolean(value.claimDisabled) &&
    optionalBoolean(value.dailyClaimed) &&
    optionalBoolean(value.showUnclaimed) &&
    optionalNumber(value.dailyClaimedAt) &&
    optionalNumber(value.claimStatusExpiresAt) &&
    Array.isArray(value.metrics) &&
    value.metrics.every(
      (metric: AccountMetric) =>
        metric != null &&
        typeof metric.label === 'string' &&
        typeof metric.value === 'string' &&
        optionalNumber(metric.percent) &&
        optionalNumber(metric.resetAt) &&
        optionalString(metric.resetLabel) &&
        optionalBoolean(metric.showProgress) &&
        (metric.balanceState == null ||
          metric.balanceState === 'available' ||
          metric.balanceState === 'empty')
    )
  );
}

function readSnapshot(locale: string): Snapshot | null {
  try {
    const value = JSON.parse(localStorage.getItem(CACHE_KEY) ?? 'null') as Snapshot | null;
    if (
      value?.version === 1 &&
      value.locale === locale &&
      value.providers != null &&
      typeof value.providers === 'object' &&
      !Array.isArray(value.providers) &&
      Object.values(value.providers).every(
        (accounts) => Array.isArray(accounts) && accounts.every(validAccount)
      )
    )
      return value;
  } catch {
    // Account loading remains usable when browser storage is unavailable.
  }
  return null;
}

export function readCachedAccounts(locale: string): Record<string, CachedAccount[]> {
  return readSnapshot(locale)?.providers ?? {};
}

function summarize(account: AccountCardData): CachedAccount {
  return {
    id: account.id,
    identity: account.identity,
    detail: account.detail,
    plan: account.plan,
    subscriptionEndAt: account.subscriptionEndAt,
    metrics: account.metrics.map(
      ({ label, value, percent, resetAt, resetLabel, showProgress, balanceState }) => ({
        label,
        value,
        percent,
        resetAt,
        resetLabel,
        showProgress,
        balanceState,
      })
    ),
    authorizationFailed: account.authorizationFailed,
    claimable: Boolean(account.claim),
    claimDisabled: account.claimDisabled,
    dailyClaimedAt: account.dailyClaimedAt,
    dailyClaimed: account.dailyClaimed,
    claimStatusExpiresAt: account.claimStatusExpiresAt,
    showUnclaimed: account.showUnclaimed,
  };
}

const idleAction = async () => {};

export function useAccountCardCache(
  providers: AccountProvider[],
  {
    locale,
    active,
    toolsReady,
    scanning,
  }: { locale: string; active: boolean; toolsReady: boolean; scanning: boolean }
) {
  const [saved, setSaved] = useState(() => readSnapshot(locale));
  const snapshot = saved?.locale === locale ? saved : null;
  const pending = !toolsReady || scanning || providers.some((provider) => provider.loading);
  const revisions = useRef(
    new Map(providers.map((provider) => [provider.id, provider.mutationRevision ?? 0]))
  );
  const liveIds = useRef(
    new Map(
      providers.map((provider) => [provider.id, new Set(provider.accounts.map(({ id }) => id))])
    )
  );

  useEffect(() => {
    const mutated = new Set(
      providers
        .filter(
          (provider) =>
            (revisions.current.get(provider.id) ?? 0) !== (provider.mutationRevision ?? 0)
        )
        .map((provider) => provider.id)
    );
    revisions.current = new Map(
      providers.map((provider) => [provider.id, provider.mutationRevision ?? 0])
    );
    const previousLiveIds = liveIds.current;
    liveIds.current = new Map(
      providers.map((provider) => [provider.id, new Set(provider.accounts.map(({ id }) => id))])
    );
    // Leaving cancels the batch; only explicit mutations may update it off-page.
    const complete = active && !pending;
    if ((!complete && !mutated.size) || (!active && !snapshot)) return;
    const next: Snapshot = {
      version: 1,
      locale,
      providers: Object.fromEntries(
        providers.map((provider) => {
          const cached = snapshot?.providers[provider.id] ?? [];
          if (mutated.has(provider.id)) {
            const current = provider.accounts.map(summarize);
            if (provider.loaded) return [provider.id, current];
            // A new login is only a partial list when the initial read failed.
            const merged = new Map(cached.map((account) => [account.id, account]));
            for (const id of previousLiveIds.get(provider.id) ?? [])
              if (!liveIds.current.get(provider.id)?.has(id)) merged.delete(id);
            for (const account of current) merged.set(account.id, account);
            return [provider.id, [...merged.values()]];
          }
          return [
            provider.id,
            complete && provider.loaded && !provider.readFailed
              ? provider.accounts.map(summarize)
              : cached,
          ];
        })
      ),
    };
    const serialized = JSON.stringify(next);
    if (serialized === JSON.stringify(snapshot)) return;
    setSaved(next);
    try {
      localStorage.setItem(CACHE_KEY, serialized);
    } catch {
      // Keep the current session's snapshot if persistence is unavailable.
    }
  }, [active, locale, pending, providers, snapshot]);

  // Clearing a row only tidies the page: the plugin keeps its saved account,
  // so reinstalling the plugin shows it again.
  const forget = (providerId: string, accountId: string) => {
    setSaved((previous) => {
      if (!previous?.providers[providerId]) return previous;
      const next: Snapshot = {
        ...previous,
        providers: {
          ...previous.providers,
          [providerId]: previous.providers[providerId].filter(({ id }) => id !== accountId),
        },
      };
      try {
        localStorage.setItem(CACHE_KEY, JSON.stringify(next));
      } catch {
        // The row stays cleared for this session.
      }
      return next;
    });
  };

  return {
    loading: pending || !snapshot,
    providers: providers.map((provider) => ({
      ...provider,
      accounts: (snapshot?.providers[provider.id] ?? []).map((summary): AccountCardData => {
        const live = provider.accounts.find((account) => account.id === summary.id);
        const cachedOnly = !live && Boolean(provider.pluginMissing);
        return {
          ...summary,
          pending: !live,
          cachedOnly,
          refreshing: live?.refreshing ?? false,
          authorizationFailed: live?.authorizationFailed ?? summary.authorizationFailed,
          refresh: live?.refresh ?? idleAction,
          remove: live?.remove ?? (cachedOnly ? () => forget(provider.id, summary.id) : idleAction),
          claim: summary.claimable ? (live?.claim ?? idleAction) : undefined,
        };
      }),
    })),
  };
}
