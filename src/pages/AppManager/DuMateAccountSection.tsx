import { Gift } from 'lucide-react';
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
import { dumateCreditBalances, dumatePlanLabel, dumateRewardStatus } from './dumateAccountDisplay';

export function DuMateAccountSection() {
  const { dumateAccounts: group, isLaunching } = useAppManager();
  const { t, locale } = useI18n();
  const now = useUsageClock(group.accounts.some((a) => a.dailyReward != null));
  const format = (v: number | null) =>
    v == null
      ? '—'
      : new Intl.NumberFormat(locale, { maximumFractionDigits: 2, useGrouping: false }).format(v);
  return (
    <AccountSection>
      <AccountSectionButton
        iconSrc="/icons/tools/dumate.svg"
        busy={group.busy}
        disabled={isLaunching}
        remainingSeconds={group.remainingSeconds}
        onClick={() => void group.add()}
        onCancel={group.cancelLogin}
      />
      <div className="space-y-2">
        {group.accounts.map((account) => {
          const balances = dumateCreditBalances(account);
          const status = dumateRewardStatus(account, now / 1000);
          return (
            <AccountSectionRow
              key={account.id}
              selected={group.selectedId === account.id}
              email={account.name || account.id}
              plan={dumatePlanLabel(account, t)}
              planPrefix={
                account.expiresAt ? (
                  <QuotaCountdown
                    resetAt={account.expiresAt}
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
                status !== 'unavailable' ? (
                  <AccountSectionRewardButton
                    email={account.name}
                    claimed={status === 'claimed'}
                    disabled={isLaunching || group.refreshing.has(account.id)}
                    onClaim={() => void group.claimDaily(account)}
                  />
                ) : undefined
              }
              secondary={
                <span className="flex h-[16px] min-w-0 items-center gap-1 overflow-hidden text-[11px] font-semibold whitespace-nowrap">
                  <span aria-label={t('agent.baseCredits')}>
                    {format(balances.package.remaining)}
                  </span>
                  {status === 'claimable' ? (
                    <span>{t('agent.dailyCreditsUnclaimed')}</span>
                  ) : (
                    <span
                      className="flex items-center gap-0.5"
                      aria-label={t('agent.rewardCredits')}
                    >
                      <Gift size={10} aria-hidden="true" />
                      {format(balances.reward.remaining)}
                    </span>
                  )}
                </span>
              }
            />
          );
        })}
      </div>
    </AccountSection>
  );
}
