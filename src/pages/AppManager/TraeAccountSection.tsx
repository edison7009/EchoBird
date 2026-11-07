import React from 'react';
import { Gift } from 'lucide-react';
import { useAppManager } from './context';
import { useI18n } from '../../hooks/useI18n';
import { useUsageClock } from '../../hooks/useUsageClock';
import {
  AccountSection,
  AccountSectionButton,
  AccountSectionRewardButton,
  AccountSectionRow,
} from './AccountSectionPrimitives';
import { traeClaimUnavailable, useTraeClaimedToday } from './traeDailyCredits';
import { QuotaCountdown } from './QuotaCountdown';
import { useAccountPlugin } from '../../hooks/useAccountPlugin';
import { openAccountPlugins } from './openAccountPlugins';

export const TraeAccountSection: React.FC = () => {
  const { traeAccounts, selectedTool, setApplyError } = useAppManager();
  const installed = useAccountPlugin(selectedTool ?? 'trae');
  const { t } = useI18n();
  const claimedToday = useTraeClaimedToday(selectedTool === 'traecn');
  const {
    accounts,
    selectedId,
    select,
    busy,
    remainingSeconds,
    refreshing,
    refresh,
    remove,
    claimDaily,
  } = traeAccounts;
  const now = useUsageClock(
    accounts.some(
      (account) =>
        account.subscriptionEndAt != null ||
        account.baseResetAt != null ||
        account.rewardNextExpiryAt != null
    )
  );
  const credits = (value?: number | null, currency?: string | null) =>
    value == null
      ? t('model.noUsageData')
      : `${currency === 'USD' ? '$' : ''}${value.toLocaleString(undefined, { maximumFractionDigits: 2, useGrouping: false })}`;
  return (
    <AccountSection>
      <AccountSectionButton
        iconSrc={`/icons/tools/${selectedTool}.svg`}
        busy={busy}
        remainingSeconds={remainingSeconds}
        idleLabel="plugins.category.account"
        onClick={() =>
          void (installed ? traeAccounts.add() : openAccountPlugins()).catch((error) =>
            setApplyError(String(error))
          )
        }
        onCancel={traeAccounts.cancelLogin}
      />
      {accounts.length > 0 && (
        <div className="space-y-2">
          {accounts.map((account) => {
            const hasBase =
              account.baseRemaining != null &&
              !(account.baseRemaining === 0 && account.baseTotal === 0);
            const hasReward = account.rewardRemaining != null;
            const hasAddon = account.addonRemaining != null && account.addonRemaining > 0;
            const grouped = selectedTool === 'traecn' && (hasBase || hasReward || hasAddon);
            const subscriptionEndAt =
              account.subscriptionEndAt != null && account.subscriptionEndAt > now / 1000
                ? account.subscriptionEndAt
                : undefined;
            const baseResetAt =
              account.baseResetAt != null && account.baseResetAt > now / 1000
                ? account.baseResetAt
                : undefined;
            const rewardNextExpiryAt =
              account.rewardNextExpiryAt != null && account.rewardNextExpiryAt > now / 1000
                ? account.rewardNextExpiryAt
                : undefined;
            const sharedExpiry =
              hasBase && hasReward && baseResetAt != null && baseResetAt === rewardNextExpiryAt;
            const separateDates =
              hasBase &&
              hasReward &&
              baseResetAt != null &&
              rewardNextExpiryAt != null &&
              !sharedExpiry;
            const baseDate = sharedExpiry ? undefined : baseResetAt;
            const dailyClaimed = claimedToday(account.dailyClaimedAt);
            return (
              <AccountSectionRow
                key={account.id}
                selected={selectedId === account.id}
                email={account.name}
                plan={account.plan}
                planPrefix={
                  subscriptionEndAt ? (
                    <QuotaCountdown
                      resetAt={subscriptionEndAt}
                      compact
                      label={t('accountCenter.subscription')}
                    />
                  ) : undefined
                }
                refreshing={refreshing.has(account.id)}
                authorizationFailed={traeAccounts.authorizationFailedIds.has(account.id)}
                onSelect={() => select(account.id)}
                onRefresh={() => void refresh(account)}
                onDelete={() => void remove(account)}
                leadingAction={
                  selectedTool === 'traecn' ? (
                    <AccountSectionRewardButton
                      email={account.name}
                      claimed={dailyClaimed}
                      disabled={
                        busy || refreshing.size > 0 || traeClaimUnavailable(account, accounts)
                      }
                      onClaim={() => void claimDaily(account)}
                    />
                  ) : undefined
                }
                secondary={
                  !grouped &&
                  account.remaining == null &&
                  !(
                    selectedTool === 'traecn' &&
                    !dailyClaimed &&
                    account.rewardEnabled === true
                  ) ? undefined : (
                    <span
                      className={`flex h-[16px] min-w-0 items-center gap-1 text-[11px] font-semibold whitespace-nowrap ${grouped ? 'w-full' : 'overflow-hidden'}`}
                    >
                      {grouped ? (
                        <>
                          {hasBase && (
                            <span
                              className={`flex items-baseline gap-1 ${baseDate != null ? 'min-w-min flex-auto' : 'flex-shrink-0'}`}
                              aria-label={t('agent.baseCredits')}
                            >
                              <span className="flex-shrink-0">
                                {credits(account.baseRemaining)}
                              </span>
                              {baseDate != null && (
                                <span
                                  className={`min-w-0 w-0 flex-1 truncate [&>span]:block [&>span]:truncate [&>span]:text-left ${separateDates ? '[&>span]:!text-[10px]' : ''}`}
                                >
                                  <QuotaCountdown
                                    resetAt={baseDate}
                                    compact
                                    label={t('agent.baseCreditsReset')}
                                  />
                                </span>
                              )}
                            </span>
                          )}
                          {hasAddon && (
                            <span
                              className="flex-shrink-0"
                              aria-label={t('agent.purchasedCredits')}
                            >
                              {credits(account.addonRemaining)}
                            </span>
                          )}
                          {hasReward && (
                            <span
                              className={`flex items-baseline gap-1 ${rewardNextExpiryAt != null ? 'min-w-min flex-auto' : 'flex-shrink-0'}`}
                              aria-label={t('agent.rewardCredits')}
                            >
                              <span className="flex flex-shrink-0 items-center gap-0.5">
                                <Gift size={10} aria-hidden="true" />
                                {credits(account.rewardRemaining)}
                              </span>
                              {rewardNextExpiryAt != null && (
                                <span
                                  className={`min-w-0 w-0 flex-1 truncate [&>span]:block [&>span]:truncate [&>span]:text-left ${separateDates ? '[&>span]:!text-[10px]' : ''}`}
                                >
                                  <QuotaCountdown
                                    resetAt={rewardNextExpiryAt}
                                    compact
                                    label={t('agent.rewardCreditsNextExpiry')}
                                  />
                                </span>
                              )}
                            </span>
                          )}
                        </>
                      ) : (
                        <span
                          className="flex flex-shrink-0 items-baseline gap-1"
                          aria-label={t(
                            selectedTool === 'trae' ? 'accountCenter.quota' : 'agent.credits'
                          )}
                        >
                          {credits(
                            account.remaining,
                            selectedTool === 'trae' ? account.currency : undefined
                          )}
                          {account.remaining != null && (account.total ?? 0) > 0 && (
                            <QuotaCountdown
                              resetAt={account.expiresAt}
                              compact
                              label={t('accountCenter.reset')}
                            />
                          )}
                        </span>
                      )}
                      {selectedTool === 'traecn' &&
                        !dailyClaimed &&
                        account.rewardEnabled === true && (
                          <span
                            className="flex-shrink-0"
                            aria-label={t('agent.dailyCreditsUnclaimed')}
                          >
                            {t('agent.dailyCreditsUnclaimed')}
                          </span>
                        )}
                    </span>
                  )
                }
              />
            );
          })}
        </div>
      )}
    </AccountSection>
  );
};
