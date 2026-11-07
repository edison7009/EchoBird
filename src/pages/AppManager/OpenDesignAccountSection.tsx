import { useAppManager } from './context';
import {
  AccountSection,
  AccountSectionButton,
  AccountSectionRow,
} from './AccountSectionPrimitives';
import { QuotaCountdown } from './QuotaCountdown';
import { QuotaPercent } from './QuotaPercent';
import { quotaTextClass } from '../../utils/quotaColors';
import { accountPlanLabel } from './accountPlanLabel';
import { ModelSwitchDivider } from './ModelSwitchDivider';

export function OpenDesignAccountSection({ showDivider = false }: { showDivider?: boolean }) {
  const { openDesignAccounts: group, isLaunching } = useAppManager();
  return (
    <AccountSection>
      <AccountSectionButton
        iconSrc="/icons/tools/opendesign.png"
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
            email={account.email}
            plan={account.plan ? accountPlanLabel(account.plan) : undefined}
            planPrefix={
              account.subscriptionEndAt != null ? (
                <QuotaCountdown resetAt={account.subscriptionEndAt} compact />
              ) : undefined
            }
            refreshing={group.refreshing.has(account.id)}
            authorizationFailed={group.authorizationFailedIds.has(account.id)}
            onSelect={() => group.select(account.id)}
            onRefresh={() => void group.refresh(account)}
            onDelete={() => void group.remove(account)}
            secondary={
              <span className="flex h-[16px] min-w-0 items-center gap-1 overflow-hidden whitespace-nowrap text-[11px] font-semibold leading-[16px] text-cyber-text">
                {account.quotaWindows.length
                  ? account.quotaWindows.map((window) => (
                      <span
                        key={window.label}
                        className={`flex flex-shrink-0 items-center ${quotaTextClass(window.remainingPercent)}`}
                      >
                        <QuotaPercent percent={window.remainingPercent} />
                        <QuotaCountdown resetAt={window.resetAt} compact small parenthesized />
                      </span>
                    ))
                  : account.balanceUsd == null
                    ? '—'
                    : `$${account.balanceUsd.toFixed(2)}`}
              </span>
            }
          />
        ))}
      </div>
      {showDivider && <ModelSwitchDivider />}
    </AccountSection>
  );
}
