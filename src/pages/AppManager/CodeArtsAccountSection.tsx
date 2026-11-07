import { useI18n } from '../../hooks/useI18n';
import {
  AccountSection,
  AccountSectionButton,
  AccountSectionRewardButton,
  AccountSectionRow,
} from './AccountSectionPrimitives';
import { accountPlanLabel, isFreePlan } from './accountPlanLabel';
import { useAppManager } from './context';
import { ModelSwitchDivider } from './ModelSwitchDivider';
import { useWorkBuddyClaimedToday } from './workBuddyDailyCredits';
import { codeArtsRemainingPercent } from './codeArtsAccountDisplay';
import { QuotaCountdown } from './QuotaCountdown';

export function CodeArtsAccountSection({ showDivider = true }: { showDivider?: boolean }) {
  const { codeArtsAccounts: managed, isLaunching } = useAppManager();
  const { t, locale } = useI18n();
  const claimedToday = useWorkBuddyClaimedToday(managed.accounts.length > 0);
  return (
    <AccountSection>
      <AccountSectionButton
        iconSrc="/icons/tools/codearts.png"
        busy={managed.busy}
        disabled={isLaunching}
        remainingSeconds={managed.remainingSeconds}
        onClick={() => void managed.add()}
        onCancel={managed.cancelLogin}
      />
      <div className="space-y-2">
        {managed.accounts.map((account) => {
          const percent = codeArtsRemainingPercent(account);
          const credits =
            account.balance == null
              ? undefined
              : new Intl.NumberFormat(locale, { maximumFractionDigits: 2 }).format(account.balance);
          return (
            <AccountSectionRow
              key={account.id}
              selected={managed.selectedId === account.id}
              email={account.email}
              plan={account.plan ? accountPlanLabel(account.plan) : null}
              planPrefix={
                !isFreePlan(account.plan) && account.subscriptionEndAt ? (
                  <QuotaCountdown
                    resetAt={account.subscriptionEndAt}
                    compact
                    label={t('accountCenter.subscription')}
                  />
                ) : undefined
              }
              refreshing={managed.refreshing.has(account.id)}
              authorizationFailed={managed.authorizationFailedIds.has(account.id)}
              onSelect={() => managed.select(account.id)}
              onRefresh={() => void managed.refresh(account)}
              onDelete={() => void managed.remove(account)}
              leadingAction={
                <AccountSectionRewardButton
                  email={account.email}
                  claimed={claimedToday(account.dailyClaimedAt)}
                  disabled={managed.busy || managed.refreshing.has(account.id)}
                  onClaim={() => void managed.claimDaily(account)}
                />
              }
              secondary={
                percent != null ? (
                  <span className="flex h-[16px] min-w-0 items-center justify-between">
                    <span
                      role="progressbar"
                      aria-label={t('agent.availableCredits')}
                      aria-valuemin={0}
                      aria-valuemax={100}
                      aria-valuenow={percent}
                      className="h-1.5 min-w-[56px] max-w-[80px] flex-1 overflow-hidden rounded-full bg-cyber-border"
                    >
                      <span
                        className="quota-progress-fill block h-full rounded-full bg-cyber-bg"
                        style={{ width: `${percent}%` }}
                      />
                    </span>
                    <span className="ml-[6px] flex-shrink-0 text-right text-[12px] font-semibold leading-[16px] text-cyber-text">
                      {credits}
                    </span>
                  </span>
                ) : account.balance == null ? undefined : (
                  `${t('agent.availableCredits')} ${credits}`
                )
              }
            />
          );
        })}
      </div>
      {showDivider && <ModelSwitchDivider />}
    </AccountSection>
  );
}
