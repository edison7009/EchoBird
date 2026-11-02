import * as api from '../../api/tauri';
import { useManagedAccounts } from './useManagedAccounts';

export function useClaudeDesktopAccounts(
  enabled: boolean,
  hasModel: boolean,
  clearModel: () => void,
  showError: (error: string) => void
) {
  return useManagedAccounts<api.ClaudeCodeAccount, api.ClaudeDesktopLogin>(
    'claudedesktop',
    enabled,
    hasModel,
    clearModel,
    showError,
    {
      list: api.listClaudeDesktopAccounts,
      start: api.startClaudeDesktopLogin,
      poll: async (id) => (await api.pollClaudeDesktopLogin(id)).account,
      cancel: api.cancelClaudeDesktopLogin,
      remove: (account) => api.deleteClaudeDesktopAccount(account.id),
      refresh: (account) => api.refreshClaudeDesktopAccount(account.id),
      label: (account) => account.email,
    }
  );
}
