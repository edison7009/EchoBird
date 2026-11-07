import { useAccountPlugin } from '../../hooks/useAccountPlugin';
import React from 'react';
import { Gift } from 'lucide-react';
import { useAppManager } from './context';
import { ModelSwitchDivider } from './ModelSwitchDivider';
import {
  AccountSection,
  AccountSectionButton,
  AccountSectionRewardButton,
  AccountSectionRow,
} from './AccountSectionPrimitives';
import { QuotaCountdown } from './QuotaCountdown';
import { useI18n } from '../../hooks/useI18n';
import { useWorkBuddyClaimedToday } from './workBuddyDailyCredits';
import { openAccountPlugins } from './openAccountPlugins';
export const WorkBuddyAccountSection: React.FC<{ showDivider?: boolean }> = ({
  showDivider = true,
}) => {
  const { workBuddyAccounts, selectedTool, setApplyError } = useAppManager();
  const { t } = useI18n();
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
  } = workBuddyAccounts;
  const claimedToday = useWorkBuddyClaimedToday(selectedTool === 'workbuddy');
  const formatCredits = (value?: number | null) =>
    value == null
      ? t('model.noUsageData')
      : value.toLocaleString(undefined, { maximumFractionDigits: 2, useGrouping: false });
  const installed = useAccountPlugin(selectedTool ?? 'workbuddy');
  return (
    <AccountSection>
      <AccountSectionButton
        iconSrc={`/icons/tools/${selectedTool}.png`}
        colorClassName="workbuddy-account-pill"
        busy={busy}
        remainingSeconds={remainingSeconds}
        idleLabel="plugins.category.account"
        onClick={() =>
          void (installed ? workBuddyAccounts.add() : openAccountPlugins()).catch((error) =>
            setApplyError(String(error))
          )
        }
        onCancel={() => workBuddyAccounts.cancelLogin()}
      />
      {accounts.length > 0 && (
        <div className="space-y-2">
          {accounts.map((account) => {
            const hasBase = account.baseRemaining != null && (account.baseTotal ?? 0) > 0;
            const hasReward = account.rewardRemaining != null && (account.rewardTotal ?? 0) > 0;
            const hasAddon = account.addonRemaining != null && account.addonRemaining > 0;
            const dailyClaimed = claimedToday(account.dailyClaimedAt);
            return (
              <AccountSectionRow
                key={account.id}
                colorClassName="workbuddy-account-pill"
                selected={selectedId === account.id}
                email={account.name}
                plan={account.plan}
                refreshing={refreshing.has(account.id)}
                authorizationFailed={workBuddyAccounts.authorizationFailedIds.has(account.id)}
                onSelect={() => select(account.id)}
                onRefresh={() => void refresh(account)}
                onDelete={() => void remove(account)}
                leadingAction={
                  account.edition === 'workbuddy' ? (
                    <AccountSectionRewardButton
                      email={account.name}
                      claimed={dailyClaimed}
                      disabled={refreshing.has(account.id)}
                      onClaim={() => void claimDaily(account)}
                    />
                  ) : undefined
                }
                secondary={
                  account.remaining == null &&
                  account.baseRemaining == null &&
                  account.rewardRemaining == null &&
                  account.addonRemaining == null ? undefined : (
                    <span className="flex h-[16px] min-w-0 items-center gap-1 overflow-hidden text-[11px] font-semibold whitespace-nowrap">
                      <span
                        className="flex flex-shrink-0 items-baseline gap-1"
                        aria-label={t('agent.baseCredits')}
                      >
                        {formatCredits(account.baseRemaining)}
                        {hasBase && (
                          <QuotaCountdown
                            resetAt={account.baseResetAt}
                            compact
                            label={t('agent.baseCreditsReset')}
                          />
                        )}
                      </span>
                      {hasAddon && (
                        <span className="flex-shrink-0" aria-label={t('agent.purchasedCredits')}>
                          {formatCredits(account.addonRemaining)}
                        </span>
                      )}
                      {account.edition === 'workbuddy' && !dailyClaimed ? (
                        <span
                          className="flex-shrink-0"
                          aria-label={t('agent.dailyCreditsUnclaimed')}
                        >
                          {t('agent.dailyCreditsUnclaimed')}
                        </span>
                      ) : (
                        hasReward && (
                          <span
                            className="flex flex-shrink-0 items-center gap-0.5"
                            aria-label={t('agent.rewardCredits')}
                          >
                            <Gift size={10} aria-hidden="true" />
                            {formatCredits(account.rewardRemaining)}
                          </span>
                        )
                      )}
                      {(account.rewardRemaining ?? 0) > 0 && (
                        <QuotaCountdown
                          resetAt={account.rewardNextExpiryAt}
                          compact
                          label={t('agent.rewardCreditsNextExpiry')}
                        />
                      )}
                    </span>
                  )
                }
              />
            );
          })}
        </div>
      )}
      {showDivider && <ModelSwitchDivider />}
    </AccountSection>
  );
};
