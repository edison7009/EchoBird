import { Gift } from 'lucide-react';
import { useI18n } from '../../hooks/useI18n';
import { useAppManager } from './context';
import {
  AccountSection,
  AccountSectionButton,
  AccountSectionRow,
} from './AccountSectionPrimitives';
import { QuotaCountdown } from './QuotaCountdown';
import {
  qwenWorkIdentity,
  qwenWorkCreditBalances,
  qwenWorkPlanLabel,
} from './qwenWorkAccountDisplay';

export function QwenWorkAccountSection() {
  const { qwenWorkAccounts: group, selectedTool, isLaunching } = useAppManager();
  const { t, locale } = useI18n();
  const formatCredits = (value: number | null) =>
    value == null
      ? '—'
      : new Intl.NumberFormat(locale, { maximumFractionDigits: 2, useGrouping: false }).format(
          value
        );
  return (
    <AccountSection>
      <AccountSectionButton
        iconSrc={`/icons/tools/${selectedTool}.svg`}
        busy={group.busy}
        disabled={isLaunching}
        remainingSeconds={group.remainingSeconds}
        onClick={() => void group.add()}
        onCancel={group.cancelLogin}
      />
      <div className="space-y-2">
        {group.accounts.map((account) => {
          const balances = qwenWorkCreditBalances(account);
          return (
            <AccountSectionRow
              key={account.id}
              selected={group.selectedId === account.id}
              email={qwenWorkIdentity(account)}
              plan={qwenWorkPlanLabel(account, t)}
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
              secondary={
                <span className="flex h-[16px] min-w-0 items-center gap-1 overflow-hidden text-[11px] font-semibold whitespace-nowrap">
                  <span className="flex-shrink-0" aria-label={t('agent.baseCredits')}>
                    {formatCredits(balances.package.remaining)}
                  </span>
                  {balances.reward && (
                    <span
                      className="flex flex-shrink-0 items-center gap-0.5"
                      aria-label={t('agent.rewardCredits')}
                    >
                      <Gift size={10} aria-hidden="true" />
                      {formatCredits(balances.reward.remaining)}
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
