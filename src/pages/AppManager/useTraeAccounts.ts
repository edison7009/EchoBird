import * as api from '../../api/tauri';
import { useManagedAccounts } from './useManagedAccounts';
import { traeClaimUnavailable } from './traeDailyCredits';

export function useTraeAccounts(
  edition: api.TraeEdition,
  enabled: boolean,
  showError: (error: string) => void
) {
  const managed = useManagedAccounts<api.TraeAccount, api.TraeLogin>(
    edition,
    enabled,
    false,
    () => {},
    showError,
    {
      list: () => api.listTraeAccounts(edition),
      start: () => api.startTraeLogin(edition),
      poll: api.pollTraeLogin,
      cancel: api.cancelTraeLogin,
      open: (login) => api.openExternal(login.verificationUri),
      remove: (account) => api.deleteTraeAccount(edition, account.id),
      refresh: (account) => api.refreshTraeAccountQuota(edition, account.id),
      label: (account) => account.name,
    }
  );
  const claimDaily = (account: api.TraeAccount, onError?: (error: unknown) => void) => {
    const current = managed.accounts.find((row) => row.id === account.id);
    if (
      edition !== 'traecn' ||
      account.edition !== edition ||
      !current ||
      traeClaimUnavailable(current)
    )
      return Promise.resolve();
    return managed.refresh(current, (row) => api.claimTraeDailyCredits(edition, row.id), onError);
  };
  return { ...managed, claimDaily };
}
