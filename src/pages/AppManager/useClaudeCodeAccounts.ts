import * as api from '../../api/tauri';
import { open as shellOpen } from '@tauri-apps/plugin-shell';
import { useManagedAccounts } from './useManagedAccounts';

export function useClaudeCodeAccounts(
  enabled: boolean,
  hasModel: boolean,
  clearModel: () => void,
  showError: (error: string) => void
) {
  const managed = useManagedAccounts<api.ClaudeCodeAccount, api.ClaudeCodeLogin>(
    'claudecode',
    enabled,
    hasModel,
    clearModel,
    showError,
    {
      list: api.listClaudeCodeAccounts,
      start: api.startClaudeCodeLogin,
      complete: api.completeClaudeCodeLogin,
      cancel: api.cancelClaudeCodeLogin,
      open: (login) => shellOpen(login.authorizationUrl),
      refresh: (account) => api.refreshClaudeCodeAccountQuota(account.id),
      remove: (account) => api.deleteClaudeCodeAccount(account.id),
      label: (account) => account.email,
    }
  );
  return {
    ...managed,
    setSelectedId: managed.select,
    warmUp: (account: api.ClaudeCodeAccount, onError?: (error: unknown) => void) =>
      managed.refresh(account, (target) => api.warmUpClaudeCodeAccount(target.id), onError),
  };
}
