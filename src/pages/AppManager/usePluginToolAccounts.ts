import { accountPluginOperation } from '../../api/plugins';
import { openExternal } from '../../api/tauri';
import type { PluginToolProvider } from '../../hooks/useAccountPlugin';
import { BROWSER_LOGIN_TIMEOUT_SECONDS, useManagedAccounts } from './useManagedAccounts';

export interface PluginToolAccount {
  id: string;
  email: string;
  plan: string | null;
  remaining?: number | null;
  total?: number | null;
  active?: boolean;
}

interface PluginToolLogin {
  loginId: string;
  authorizationUrl: string;
  expiresAt: number;
}

export function usePluginToolAccounts(
  provider: PluginToolProvider | undefined,
  enabled: boolean,
  hasModel: boolean,
  clearModel: () => void,
  showError: (error: string) => void,
  navigationKey: string
) {
  const call = async <T>(operation: string, params: object = {}) => {
    if (!provider) throw new Error('accountError.pluginRequired');
    return (await accountPluginOperation(
      provider.pluginId,
      provider.providerId,
      operation,
      params
    )) as T;
  };
  const managed = useManagedAccounts<PluginToolAccount, PluginToolLogin & { id: string }>(
    provider?.providerId ?? 'plugin',
    enabled,
    hasModel,
    clearModel,
    showError,
    {
      list: () => call<PluginToolAccount[]>('accounts'),
      start: async () => {
        const login = await call<PluginToolLogin>('login.start');
        return { ...login, id: login.loginId };
      },
      open: (login) => openExternal(login.authorizationUrl),
      poll: (id) => call<PluginToolAccount | null>('login.poll', { loginId: id }),
      cancel: (id) => call('login.cancel', { loginId: id }),
      remove: (account) => call('account.remove', { accountId: account.id }),
      refresh: (account) => call<PluginToolAccount>('account.refresh', { accountId: account.id }),
      label: (account) => account.email,
      loginTimeoutSeconds: BROWSER_LOGIN_TIMEOUT_SECONDS,
    },
    navigationKey
  );
  return {
    ...managed,
    provider,
    claimDaily: provider?.operations?.includes('account.claim')
      ? (account: PluginToolAccount, onError?: (error: unknown) => void) =>
          managed.refresh(
            account,
            (row) => call<PluginToolAccount>('account.claim', { accountId: row.id }),
            onError
          )
      : undefined,
    warmUp: provider?.operations?.includes('account.warmup')
      ? (account: PluginToolAccount, onError?: (error: unknown) => void) =>
          managed.refresh(
            account,
            (row) => call<PluginToolAccount>('account.warmup', { accountId: row.id }),
            onError
          )
      : undefined,
    apply: (accountId: string) => call('account.apply', { accountId }),
  };
}
