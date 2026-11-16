import { useEffect, useRef, useState } from 'react';
import * as api from '../../api/tauri';
import { useConfirm } from '../../components/ConfirmDialog';
import { runAccountApplication } from '../../features/accounts/runAccountApplication';
import { useI18n } from '../../hooks/useI18n';
import { useNavigationStore } from '../../stores/navigationStore';
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
  const activePage = useNavigationStore((state) => state.activePage);
  const generation = useRef(0);
  const snapshotPending = useRef<symbol | null>(null);
  const resetPending = useRef(false);
  const [resetting, setResetting] = useState(false);
  useEffect(() => {
    return () => {
      generation.current += 1;
      snapshotPending.current = null;
    };
  }, [enabled, activePage]);
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
    busy: managed.busy || resetting,
    snapshotBusy: managed.busy,
    resetting,
    warmUp: (account: api.ClaudeCodeAccount, onError?: (error: unknown) => void) =>
      managed.refresh(account, (target) => api.warmUpClaudeDesktopAccount(target.id), onError),
    cancelLogin: () => {
      snapshotPending.current = null;
      managed.cancelLogin();
    },
    add: async () => {
      if (!enabled || snapshotPending.current || resetPending.current) return;
      const pending = Symbol();
      snapshotPending.current = pending;
      const attempt = generation.current;
      try {
        const confirmed = await confirm({
          type: 'warning',
          title: t('agent.accountSnapshotNoticeTitle'),
          message: t('agent.accountSnapshotHint'),
          preserveLineBreaks: true,
        });
        if (confirmed && attempt === generation.current) await managed.add();
      } finally {
        if (snapshotPending.current === pending) snapshotPending.current = null;
      }
    },
    addNext: async () => {
      if (!enabled || snapshotPending.current || resetPending.current) return;
      resetPending.current = true;
      setResetting(true);
      const attempt = generation.current;
      const current = () => attempt === generation.current;
      await runAccountApplication(
        async () => {
          await api.resetClaudeDesktopLogin();
          if (!current()) return;
          managed.select(null);
          clearModel();
          await managed.reload();
          if (current()) await api.startTool('claudedesktop');
        },
        {
          current,
          report: (error) => showError(accountError(error, t)),
          finish: () => {
            resetPending.current = false;
            setResetting(false);
          },
        }
      );
    },
  };
}
