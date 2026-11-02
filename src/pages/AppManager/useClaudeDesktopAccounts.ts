import * as api from '../../api/tauri';
import { useConfirm } from '../../components/ConfirmDialog';
import { useI18n } from '../../hooks/useI18n';
import { useManagedAccounts } from './useManagedAccounts';

export function useClaudeDesktopAccounts(
  enabled: boolean,
  hasModel: boolean,
  clearModel: () => void,
  showError: (error: string) => void
) {
  const { t } = useI18n();
  const confirm = useConfirm();
  const managed = useManagedAccounts<api.ClaudeCodeAccount, api.ClaudeDesktopLogin>(
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
  return {
    ...managed,
    // A snapshot only works after the user quits the client while signed in.
    add: async () => {
      const ready = await confirm({
        type: 'warning',
        title: t('agent.accountSnapshotNoticeTitle'),
        message: t('agent.accountSnapshotNotice'),
      });
      if (ready) await managed.add();
    },
  };
}
