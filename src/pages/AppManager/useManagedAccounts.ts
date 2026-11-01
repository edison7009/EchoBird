import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { accountError, isAccountAuthorizationError } from '../../utils/accountError';
import { useConfirm } from '../../components/ConfirmDialog';
import { useI18n } from '../../hooks/useI18n';
import { useNavigationStore } from '../../stores/navigationStore';
import { useRewardClaimStore } from '../../stores/rewardClaimStore';
import { useUsageIslandVisible } from '../../stores/usageIslandStore';

export const BROWSER_LOGIN_TIMEOUT_SECONDS = 10 * 60;

export interface ManagedAccount {
  id: string;
  active?: boolean;
}

export interface ManagedLogin {
  loginId: string;
  expiresAt: number;
}

export interface AccountClient<A extends ManagedAccount, L extends ManagedLogin> {
  list: () => Promise<A[]>;
  start: () => Promise<L>;
  cancel: (id: string) => Promise<unknown>;
  poll?: (id: string, nextStage: (expiresAt: number) => void) => Promise<A | null>;
  complete?: (id: string, code: string) => Promise<A>;
  open?: (login: L) => Promise<unknown>;
  captured?: (login: L) => A | null | undefined;
  remove: (account: A) => Promise<unknown>;
  refresh: (account: A) => Promise<A>;
  label: (account: A) => string;
  pollInterval?: number;
  loginTimeoutSeconds?: number;
  refreshLimit?: number;
  refreshError?: (account: A, message: string) => string;
}

// Provider adapters supply protocol operations; all account navigation, timers,
// selection and request isolation live here, including manual-code OAuth.
export function useManagedAccounts<A extends ManagedAccount, L extends ManagedLogin>(
  scope: string,
  enabled: boolean,
  hasModel: boolean,
  clearModel: () => void,
  showError: (error: string) => void,
  client: AccountClient<A, L>,
  navigationKey = scope
) {
  const { t } = useI18n();
  const activePage = useNavigationStore((state) => state.activePage);
  // While the floating island keeps every group loaded, moving between pages without
  // accounts is not a new visit. Leaving the app or account page still is.
  const islandOpen = useUsageIslandVisible();
  const visitedPage =
    islandOpen && activePage !== 'apps' && activePage !== 'accounts' ? 'background' : activePage;
  const confirm = useConfirm();
  const [rows, setRows] = useState<Record<string, A[]>>({});
  const [selection, setSelection] = useState<Record<string, string | null>>({});
  const [authorizationFailures, setAuthorizationFailures] = useState<Record<string, Set<string>>>(
    {}
  );
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(false);
  const [settledListKey, setSettledListKey] = useState<symbol | null>(null);
  const [loadedScopes, setLoadedScopes] = useState(new Set<string>());
  const [listFailed, setListFailed] = useState(false);
  const [mutationRevision, setMutationRevision] = useState(0);
  const navigation = `${scope}:${visitedPage}:${navigationKey}:${enabled}`;
  // A repeated visit is pending before its zero-delay account read starts.
  const listKey = useMemo(() => Symbol(navigation), [navigation]);
  const [refreshing, setRefreshing] = useState(new Set<string>());
  const [remainingSeconds, setRemainingSeconds] = useState(0);
  const [login, setLogin] = useState<L | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [loginError, setLoginError] = useState<string | null>(null);
  const clientRef = useRef(client);
  const hasModelRef = useRef(hasModel);
  useEffect(() => {
    clientRef.current = client;
    hasModelRef.current = hasModel;
  }, [client, hasModel]);
  const generation = useRef(0);
  const listRequest = useRef(0);
  const attempt = useRef(0);
  const revision = useRef(0);
  const loginRevision = useRef(0);
  const adding = useRef(false);
  const submittingRef = useRef(false);
  const refreshRequests = useRef(new Map<string, symbol>());
  const pending = useRef<{ login: L; client: AccountClient<A, L> } | null>(null);
  const clearTimers = useRef<(() => void) | null>(null);

  const setAuthorizationFailed = (id: string, failed: boolean) => {
    setAuthorizationFailures((prev) => {
      const ids = new Set(prev[scope]);
      if (failed) ids.add(id);
      else ids.delete(id);
      return { ...prev, [scope]: ids };
    });
  };

  const invalidateRefresh = (id: string) => {
    const requests = refreshRequests.current;
    if (requests.delete(id)) setRefreshing(new Set(requests.keys()));
  };

  const stop = useCallback(() => {
    attempt.current += 1;
    adding.current = false;
    submittingRef.current = false;
    clearTimers.current?.();
    clearTimers.current = null;
    const previous = pending.current;
    pending.current = null;
    return previous;
  }, []);

  const resetLogin = () => {
    setBusy(false);
    setLogin(null);
    setSubmitting(false);
    setRemainingSeconds(0);
    setLoginError(null);
  };

  const cancelLogin = () => {
    const previous = stop();
    const current = generation.current;
    const cancelledAttempt = attempt.current;
    resetLogin();
    if (previous?.login.loginId)
      void previous.client.cancel(previous.login.loginId).catch((error) => {
        if (current === generation.current && cancelledAttempt === attempt.current)
          showError(accountError(error, t));
      });
  };

  const reload = useCallback(async () => {
    const current = generation.current;
    const request = ++listRequest.current;
    setLoading(true);
    setListFailed(false);
    try {
      const result = await clientRef.current.list();
      if (current !== generation.current || request !== listRequest.current) return;
      setRows((prev) => ({ ...prev, [scope]: result }));
      setLoadedScopes((prev) => (prev.has(scope) ? prev : new Set(prev).add(scope)));
      return result;
    } catch (error) {
      if (current === generation.current && request === listRequest.current) {
        setListFailed(true);
        throw error;
      }
    } finally {
      if (current === generation.current && request === listRequest.current) {
        setLoading(false);
        setSettledListKey(listKey);
      }
    }
  }, [scope, listKey]);

  const invalidateList = () => {
    listRequest.current += 1;
    setLoading(false);
    setSettledListKey(listKey);
    setListFailed(false);
    setMutationRevision((value) => value + 1);
  };

  useEffect(
    () =>
      useRewardClaimStore.subscribe((state, previous) => {
        if (enabled && state.revision !== previous.revision && state.updatedScope === scope)
          void reload().catch(() => {});
      }),
    [enabled, reload, scope]
  );

  useEffect(() => {
    const current = ++generation.current;
    const selectionAtStart = revision.current;
    const requests = refreshRequests.current;
    const timer = setTimeout(() => {
      setBusy(false);
      setLoading(false);
      setLogin(null);
      setSubmitting(false);
      setLoginError(null);
      setRemainingSeconds(0);
      setRefreshing(new Set());
      if (!enabled) return;
      void reload()
        .then((result) => {
          if (
            result &&
            current === generation.current &&
            !hasModelRef.current &&
            selectionAtStart === revision.current
          )
            setSelection((prev) => ({
              ...prev,
              [scope]: result.some((a) => a.id === prev[scope])
                ? prev[scope]
                : (result.find((a) => a.active)?.id ?? null),
            }));
        })
        .catch(() => {}); // Passive failures keep cached rows and never open a dialog.
    }, 0);
    return () => {
      clearTimeout(timer);
      generation.current += 1;
      requests.clear();
      const previous = stop();
      if (previous?.login.loginId)
        void previous.client.cancel(previous.login.loginId).catch(() => {});
    };
  }, [visitedPage, enabled, navigationKey, reload, scope, stop]);

  const select = (id: string | null) => {
    revision.current += 1;
    setSelection((prev) => ({ ...prev, [scope]: id }));
    if (id) clearModel();
  };

  const finishLogin = async (account: A) => {
    setMutationRevision((value) => value + 1);
    invalidateRefresh(account.id);
    setAuthorizationFailed(account.id, false);
    if (activePage !== 'accounts' && loginRevision.current === revision.current) select(account.id);
    setRows((prev) => ({
      ...prev,
      [scope]: [...(prev[scope] ?? []).filter((a) => a.id !== account.id), account],
    }));
    const previous = stop();
    resetLogin();
    if (previous?.login.loginId)
      void previous.client.cancel(previous.login.loginId).catch(() => {});
    const current = generation.current;
    const finishedAttempt = attempt.current;
    try {
      await reload();
    } catch (error) {
      if (current === generation.current && finishedAttempt === attempt.current)
        showError(accountError(error, t));
    }
  };

  const add = async () => {
    if (!enabled || adding.current) return;
    const current = generation.current;
    const activeAttempt = ++attempt.current;
    const active = () => current === generation.current && activeAttempt === attempt.current;
    const adapter = clientRef.current;
    const timeoutSeconds = adapter.loginTimeoutSeconds ?? 60;
    loginRevision.current = revision.current;
    adding.current = true;
    setBusy(true);
    setLoginError(null);
    setRemainingSeconds(timeoutSeconds);
    let expires = Date.now() / 1000 + timeoutSeconds;
    let continued = false;
    let ticker: ReturnType<typeof setInterval> | undefined;
    const expire = () => {
      if (!active()) return;
      const previous = stop();
      resetLogin();
      if (previous?.login.loginId)
        void previous.client.cancel(previous.login.loginId).catch(() => {});
      showError(t('accountError.expired'));
    };
    let deadline = setTimeout(expire, timeoutSeconds * 1000);
    // A native handoff can start one new stage; repeated progress must not extend it.
    const nextStage = (expiresAt: number) => {
      if (!active() || continued) return;
      continued = true;
      expires = Math.min(expiresAt, Date.now() / 1000 + timeoutSeconds);
      clearTimeout(deadline);
      deadline = setTimeout(expire, Math.max(0, (expires - Date.now() / 1000) * 1000));
      setRemainingSeconds(Math.max(0, Math.ceil(expires - Date.now() / 1000)));
    };
    clearTimers.current = () => {
      clearTimeout(deadline);
      clearInterval(ticker);
    };
    try {
      const started = await adapter.start();
      if (!active()) {
        if (started.loginId) void adapter.cancel(started.loginId).catch(() => {});
        return;
      }
      pending.current = { login: started, client: adapter };
      const captured = adapter.captured?.(started);
      if (captured) {
        await finishLogin(captured);
        return;
      }
      setLogin(started);
      expires = Math.min(started.expiresAt, expires);
      ticker = setInterval(() => {
        if (!active()) return;
        const seconds = Math.max(0, Math.ceil(expires - Date.now() / 1000));
        setRemainingSeconds(seconds);
        if (!seconds) expire();
      }, 250);
      await adapter.open?.(started);
      if (adapter.complete) return; // Manual authorization code uses the same pending session.
      while (active() && Date.now() / 1000 < expires) {
        const account = await adapter.poll!(started.loginId, nextStage);
        if (!active()) return;
        if (account) {
          await finishLogin(account);
          return;
        }
        await new Promise((resolve) => setTimeout(resolve, adapter.pollInterval ?? 1000));
      }
      if (active()) throw new Error('accountError.expired');
    } catch (error) {
      if (active()) {
        showError(accountError(error, t));
        if (adapter.complete) cancelLogin();
      }
    } finally {
      if (active() && (!adapter.complete || !pending.current)) {
        const previous = stop();
        resetLogin();
        if (previous?.login.loginId)
          void previous.client.cancel(previous.login.loginId).catch(() => {});
      }
    }
  };

  const completeLogin = async (code: string) => {
    const previous = pending.current;
    if (!previous?.client.complete || submittingRef.current) return;
    submittingRef.current = true;
    setSubmitting(true);
    setLoginError(null);
    try {
      const account = await previous.client.complete(previous.login.loginId, code.trim());
      if (pending.current !== previous) return;
      await finishLogin(account);
    } catch (error) {
      if (pending.current === previous) setLoginError(accountError(error, t));
    } finally {
      if (pending.current === previous) {
        submittingRef.current = false;
        setSubmitting(false);
      }
    }
  };

  const refresh = async (
    account: A,
    operation?: (account: A) => Promise<A>,
    onError?: (error: unknown) => void
  ) => {
    const requests = refreshRequests.current;
    const adapter = clientRef.current;
    if (!enabled || requests.has(account.id) || requests.size >= (adapter.refreshLimit ?? Infinity))
      return;
    const current = generation.current;
    const request = Symbol();
    requests.set(account.id, request);
    setRefreshing(new Set(requests.keys()));
    try {
      const updated = await (operation ?? adapter.refresh)(account);
      if (current === generation.current && requests.get(account.id) === request) {
        setAuthorizationFailed(account.id, false);
        invalidateList();
        setRows((prev) => ({
          ...prev,
          [scope]: (prev[scope] ?? []).map((a) => (a.id === updated.id ? updated : a)),
        }));
        return updated;
      }
    } catch (error) {
      if (current === generation.current && requests.get(account.id) === request) {
        if (isAccountAuthorizationError(error)) setAuthorizationFailed(account.id, true);
        if (onError) onError(error);
        else {
          const message = accountError(error, t);
          showError(adapter.refreshError?.(account, message) ?? message);
        }
      }
    } finally {
      if (requests.get(account.id) === request) {
        requests.delete(account.id);
        setRefreshing(new Set(requests.keys()));
      }
    }
  };

  const remove = async (account: A) => {
    if (!enabled) return;
    const current = generation.current;
    const adapter = clientRef.current;
    if (
      !(await confirm({
        title: t('agent.deleteAccountTitle'),
        message: t('agent.deleteAccountConfirm').replace('{email}', adapter.label(account)),
        confirmText: t('btn.delete'),
        type: 'danger',
      })) ||
      current !== generation.current
    )
      return;
    try {
      await adapter.remove(account);
      if (current !== generation.current) return;
      invalidateRefresh(account.id);
      setAuthorizationFailed(account.id, false);
      invalidateList();
      setRows((prev) => ({
        ...prev,
        [scope]: (prev[scope] ?? []).filter((a) => a.id !== account.id),
      }));
      setSelection((prev) => ({
        ...prev,
        [scope]: prev[scope] === account.id ? null : prev[scope],
      }));
    } catch (error) {
      if (current === generation.current) showError(accountError(error, t));
    }
  };

  return {
    accounts: rows[scope] ?? [],
    authorizationFailedIds: authorizationFailures[scope] ?? new Set<string>(),
    selectedId: enabled && !hasModel ? (selection[scope] ?? null) : null,
    select,
    busy,
    loading: enabled && (loading || settledListKey !== listKey),
    loaded: loadedScopes.has(scope),
    readFailed: listFailed,
    mutationRevision,
    refreshing,
    remainingSeconds,
    add,
    refresh,
    remove,
    reload,
    login: enabled ? login : null,
    submitting,
    loginError,
    cancelLogin,
    completeLogin,
  };
}
