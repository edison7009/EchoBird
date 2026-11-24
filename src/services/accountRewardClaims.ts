import * as api from '../api/tauri';
import * as codeArts from '../api/codeArtsAccounts';
import { useRewardClaimStore } from '../stores/rewardClaimStore';
import { useToolsStore } from '../stores/toolsStore';
import { workBuddyClaimedToday } from '../pages/AppManager/workBuddyDailyCredits';
import { traeClaimUnavailable } from '../pages/AppManager/traeDailyCredits';
import { minimaxRewardStatus } from '../pages/AppManager/minimaxAccountDisplay';
import { dumateRewardStatus } from '../pages/AppManager/dumateAccountDisplay';
import { qoderRewardStatus } from '../pages/AppManager/qoderAccountDisplay';

const canClaimStatus = (status: string) => status !== 'claimed' && status !== 'unavailable';

// Use the same saved accounts, eligibility checks and claim endpoints as the gift buttons.
function rewards<A>(
  scope: string,
  toolIds: string[],
  list: () => Promise<A[]>,
  claim: (account: A) => Promise<A>,
  canClaim: (account: A) => boolean
) {
  return {
    scope,
    installed: () =>
      useToolsStore
        .getState()
        .detectedTools.some((tool) => tool.installed && toolIds.includes(tool.id)),
    run: async (active: () => boolean) => {
      const accounts = await list();
      for (let i = 0; i < accounts.length && active(); i += 1) {
        if (!canClaim(accounts[i])) continue;
        try {
          accounts[i] = await claim(accounts[i]);
          useRewardClaimStore.getState().updated(scope);
        } catch (error) {
          console.warn(`Reward claim failed (${scope})`, error);
        }
      }
    },
  };
}

const providers = [
  rewards(
    'workbuddy',
    ['workbuddy'],
    () => api.listWorkBuddyAccounts('workbuddy'),
    (account) => api.claimWorkBuddyDailyCredits('workbuddy', account.id),
    (account) => !workBuddyClaimedToday(account.dailyClaimedAt)
  ),
  rewards(
    'traecn',
    ['traecn'],
    () => api.listTraeAccounts('traecn'),
    (account) => api.claimTraeDailyCredits('traecn', account.id),
    (account) => !traeClaimUnavailable(account)
  ),
  rewards(
    'minimax',
    ['minimaxcode', 'minimaxdesktop'],
    api.listMinimaxAccounts,
    (account) => api.claimMinimaxDailyCredits(account.id),
    (account) => canClaimStatus(minimaxRewardStatus(account))
  ),
  rewards(
    'dumate',
    ['dumate'],
    api.listDuMateAccounts,
    (account) => api.claimDuMateDailyCredits(account.id),
    (account) => canClaimStatus(dumateRewardStatus(account))
  ),
  ...(['doubaowork', 'dola'] as const).map((edition) =>
    rewards(
      edition,
      [edition],
      () => api.listDoubaoAccounts(edition),
      (account) => api.claimDoubaoDailyCredits(edition, account.id),
      (account) => account.dailyReward?.status !== 'unavailable'
    )
  ),
  ...(['qoder', 'qodercn'] as const).map((edition) =>
    rewards(
      edition,
      [edition],
      () => api.listQoderAccounts(edition),
      (account) => api.claimQoderDailyCredits(edition, account.id),
      (account) => canClaimStatus(qoderRewardStatus(account))
    )
  ),
  rewards(
    'codearts',
    ['codearts'],
    codeArts.listCodeArtsAccounts,
    (account) => codeArts.claimCodeArtsCredits(account.id),
    () => true
  ),
];

export async function claimAllSavedRewards(active: () => boolean): Promise<void> {
  const state = useRewardClaimStore.getState();
  if (!active() || !state.tryStart()) return;
  try {
    for (const provider of providers) {
      if (!active()) break;
      if (!provider.installed()) continue;
      try {
        await provider.run(() => active() && provider.installed());
      } catch (error) {
        console.warn(`Reward accounts could not be loaded (${provider.scope})`, error);
      }
    }
  } finally {
    state.finish();
  }
}
