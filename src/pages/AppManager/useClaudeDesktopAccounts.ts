import { useRef } from 'react';
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
  // Set for the attempt that also clears the sign-in for the next account.
  const next = useRef(false);
  const prepared = useRef(false);
  const managed = useManagedAccounts<api.ClaudeCodeAccount, api.ClaudeDesktopLogin>(
    'claudedesktop',
    enabled,
    hasModel,
    clearModel,
    showError,
    {
      list: api.listClaudeDesktopAccounts,
      start: () => api.startClaudeDesktopLogin(next.current),
      poll: async (id) => {
        const { account } = await api.pollClaudeDesktopLogin(id);
        prepared.current = next.current && !!account;
        return account;
      },
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
    // Signing out inside Claude Desktop would revoke the saved session.
    addNext: async () => {
      const ready = await confirm({
        type: 'warning',
        title: t('agent.accountSnapshotNextTitle'),
        message: t('agent.accountSnapshotNextNotice'),
      });
      if (!ready) return;
      next.current = true;
      prepared.current = false;
      try {
        await managed.add();
      } finally {
        next.current = false;
      }
      if (prepared.current) {
        await api.startTool('claudedesktop').catch((error) => showError(String(error)));
      }
    },
  };
}
