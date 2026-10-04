import { useRef, useState } from 'react';
import * as api from '../../api/tauri';
import { useManagedAccounts } from './useManagedAccounts';

export function useZCodeAccounts(
  enabled: boolean,
  hasModel: boolean,
  clearModel: () => void,
  showError: (error: string) => void
) {
  const [provider, setProvider] = useState<api.ZCodeProvider>('bigmodel');
  const pollInterval = useRef(1000);
  const managed = useManagedAccounts<api.ZCodeAccount, api.ZCodeLogin>(
    'zcode',
    enabled,
    hasModel,
    clearModel,
    showError,
    {
      list: api.listZCodeAccounts,
      start: () => api.startZCodeLogin(provider),
      open: (login) => {
        pollInterval.current = login.pollIntervalSeconds * 1000;
        return api.openExternal(login.verificationUri);
      },
      poll: async (id) => {
        const account = await api.pollZCodeLogin(id);
        if (!account) await new Promise((resolve) => setTimeout(resolve, pollInterval.current));
        return account;
      },
      pollInterval: 0,
      cancel: api.cancelZCodeLogin,
      remove: (account) => api.deleteZCodeAccount(account.id),
      refresh: (account) => api.refreshZCodeAccountQuota(account.id),
      label: (account) => account.email,
    }
  );
  return {
    ...managed,
    provider,
    setProvider: (value: api.ZCodeProvider) => {
      if (!managed.busy) setProvider(value);
    },
  };
}
