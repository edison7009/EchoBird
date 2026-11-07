import { useI18n } from '../../hooks/useI18n';
import { useAppManager } from './context';
import {
  AccountSection,
  AccountSectionButton,
  AccountSectionRow,
} from './AccountSectionPrimitives';
import { ModelSwitchDivider } from './ModelSwitchDivider';
import { QuotaCountdown } from './QuotaCountdown';
import { clineBalanceLabel } from './clineAccountDisplay';

export function ClineAccountSection({ showDivider = true }: { showDivider?: boolean }) {
  const { clineAccounts: group, isLaunching } = useAppManager();
  const { t, locale } = useI18n();
  return (
    <AccountSection>
      <AccountSectionButton
        iconSrc="/icons/tools/clinedesktop.png"
        busy={group.busy}
        disabled={isLaunching}
        remainingSeconds={group.remainingSeconds}
        onClick={() => void group.add()}
        onCancel={group.cancelLogin}
        secondary={
          group.busy && group.login?.userCode ? (
            <span aria-label={t('agent.authorizationCode')} className="font-mono">
              {group.login.userCode}
            </span>
          ) : undefined
        }
      />
      <div className="space-y-2">
        {group.accounts.map((account) => (
          <AccountSectionRow
            key={account.id}
            selected={group.selectedId === account.id}
            email={account.email || account.name || account.id}
            plan={account.plan}
            planPrefix={
              account.subscriptionEndAt ? (
                <QuotaCountdown
                  resetAt={account.subscriptionEndAt}
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
              account.balanceMicros == null ? undefined : (
                <span className="truncate text-[12px] font-semibold leading-[16px]">
                  {clineBalanceLabel(account.balanceMicros, locale)}
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
