import { useI18n } from '../../hooks/useI18n';
import { useUsageClock } from '../../hooks/useUsageClock';
import { useAppManager } from './context';
import {
  AccountSection,
  AccountSectionButton,
  AccountSectionRewardButton,
  AccountSectionRow,
} from './AccountSectionPrimitives';
import { QuotaCountdown } from './QuotaCountdown';
import { qoderPlanLabel } from './accountPlanLabel';
import { qoderExpiry, qoderRewardStatus, qoderSubscriptionExpiry } from './qoderAccountDisplay';
import { ModelSwitchDivider } from './ModelSwitchDivider';

export function QoderAccountSection({ showDivider = false }: { showDivider?: boolean }) {
  const { qoderAccounts: group, isLaunching } = useAppManager();
  const { t, locale } = useI18n();
  const now = useUsageClock(group.accounts.some((account) => account.dailyReward != null));
  return (
    <AccountSection>
      <AccountSectionButton
        iconSrc="/icons/tools/qoder.svg"
        busy={group.busy}
        disabled={isLaunching}
        remainingSeconds={group.remainingSeconds}
        onClick={() => void group.add()}
        onCancel={group.cancelLogin}
      />
      <div className="space-y-2">
        {group.accounts.map((account) => (
          <AccountSectionRow
            key={account.id}
            selected={group.selectedId === account.id}
            email={account.name || account.email || account.id}
            plan={qoderPlanLabel(account.plan, account.edition, t)}
            planPrefix={
              qoderSubscriptionExpiry(account) ? (
                <QuotaCountdown
                  resetAt={qoderSubscriptionExpiry(account)}
                  compact
                  label={t('accountCenter.subscription')}
                />
              ) : undefined
            }
            refreshing={group.refreshing.has(account.id)}
            authorizationFailed={group.authorizationFailedIds.has(account.id)}
            onSelect={() => group.select(account.id)}
            onRefresh={() => void group.refresh(account)}
            onDelete={() => void group.remove(account)}
            leadingAction={
              <AccountSectionRewardButton
                email={account.name || account.email || account.id}
                claimed={qoderRewardStatus(account, now / 1000) === 'claimed'}
                disabled={
                  isLaunching ||
                  group.refreshing.has(account.id) ||
                  qoderRewardStatus(account, now / 1000) === 'unavailable'
                }
                onClaim={() => void group.claimDaily(account)}
              />
            }
            secondary={
              account.remaining == null ? undefined : (
                <span className="flex h-4 items-baseline gap-1 text-[12px] font-semibold leading-4">
                  <span>
                    {new Intl.NumberFormat(locale, { maximumFractionDigits: 2 }).format(
                      account.remaining
                    )}
                  </span>
                  {account.quotaWindows.length === 1 && (
                    <QuotaCountdown
                      resetAt={qoderExpiry(account.quotaWindows[0].expiresAt)}
                      compact
                    />
                  )}
                </span>
              )
            }
          />
        ))}
      </div>
      {showDivider && <ModelSwitchDivider />}
    </AccountSection>
  );
}
