import { useEffect, useRef } from 'react';
import * as api from '../../api/tauri';
import { useConfirm } from '../../components/ConfirmDialog';
import { useI18n } from '../../hooks/useI18n';
import { accountError } from '../../utils/accountError';
import { useManagedAccounts } from './useManagedAccounts';

export function useClaudeDesktopAccounts(
  enabled: boolean,
  hasModel: boolean,
  clearModel: () => void,
  showError: (error: string) => void
) {
  const { t } = useI18n();
  const confirm = useConfirm();
  // The attempt that also clears the sign-in for the next account, and
  // whether the plugin cleared it. An attempt the user cancelled, or one that
  // ended after leaving the tool, never opens Claude.
  const next = useRef(false);
  const saved = useRef<{ cleared: boolean } | null>(null);
  const abandoned = useRef(false);
  const enabledNow = useRef(enabled);
  useEffect(() => {
    enabledNow.current = enabled;
    if (!enabled) abandoned.current = true;
  }, [enabled]);
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
        const result = await api.pollClaudeDesktopLogin(id);
        if (next.current && result.account) saved.current = { cleared: result.cleared === true };
        return result.account;
      },
      cancel: api.cancelClaudeDesktopLogin,
      remove: (account) => api.deleteClaudeDesktopAccount(account.id),
      refresh: (account) => api.refreshClaudeDesktopAccount(account.id),
      label: (account) => account.email,
    }
  );
  // Both actions show the same steps as the hint beside the snapshot button.
  const confirmSnapshot = () =>
    confirm({
      type: 'warning',
      title: t('agent.accountSnapshotNoticeTitle'),
      message: t('agent.accountSnapshotHint'),
      preserveLineBreaks: true,
    });
  return {
    ...managed,
    warmUp: (account: api.ClaudeCodeAccount, onError?: (error: unknown) => void) =>
      managed.refresh(account, (target) => api.warmUpClaudeDesktopAccount(target.id), onError),
    cancelLogin: () => {
      abandoned.current = true;
      managed.cancelLogin();
    },
    // A snapshot only works after the user quits the client while signed in.
    add: async () => {
      if (await confirmSnapshot()) await managed.add();
    },
    // Signing out inside Claude Desktop would revoke the saved session.
    addNext: async () => {
      if (!(await confirmSnapshot())) return;
      next.current = true;
      saved.current = null;
      abandoned.current = false;
      try {
        await managed.add();
      } finally {
        next.current = false;
      }
      const attempt = saved.current as { cleared: boolean } | null;
      saved.current = null;
      // Failed, cancelled and abandoned attempts end here; errors are already shown.
      if (!attempt || abandoned.current || !enabledNow.current) return;
      // Plugins before 0.1.0-preview.10 save the snapshot but cannot clear it.
      if (!attempt.cleared) {
        showError(t('agent.accountSnapshotNextUnsupported'));
        return;
      }
      // The device is signed out now; no saved account matches it.
      managed.select(null);
      await api.startTool('claudedesktop').catch((error) => showError(accountError(error, t)));
    },
  };
}
