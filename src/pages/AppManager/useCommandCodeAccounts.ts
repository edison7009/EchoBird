import { useLayoutEffect, useRef } from 'react';
import { open } from '@tauri-apps/plugin-shell';
import * as api from '../../api/commandcode';
import { BROWSER_LOGIN_TIMEOUT_SECONDS, useManagedAccounts } from './useManagedAccounts';

export function useCommandCodeAccounts(
  enabled: boolean,
  hasModel: boolean,
  clearModel: () => void,
  showError: (error: string) => void
) {
  const managed = useManagedAccounts<api.CommandCodeAccount, api.CommandCodeLogin>(
    'commandcode',
    enabled,
    hasModel,
    clearModel,
    showError,
    {
      list: api.listCommandCodeAccounts,
      start: api.startCommandCodeLogin,
      poll: api.pollCommandCodeLogin,
      cancel: api.cancelCommandCodeLogin,
      open: (login) => open(login.authorizationUrl),
      refresh: (account) => api.refreshCommandCodeAccount(account.id),
      remove: (account) => api.deleteCommandCodeAccount(account.id),
      label: (account) => account.email || account.name || account.id,
      loginTimeoutSeconds: BROWSER_LOGIN_TIMEOUT_SECONDS,
    }
  );
  const selected = useRef(managed.selectedId);
  useLayoutEffect(() => {
    selected.current = managed.selectedId;
  }, [managed.selectedId]);
  return {
    ...managed,
    isCurrentSelection: (id: string) => selected.current === id,
    switchAccount: async () => {
      if (!managed.selectedId) return;
      await api.switchCommandCodeAccount(managed.selectedId);
      await managed.reload();
    },
  };
}
