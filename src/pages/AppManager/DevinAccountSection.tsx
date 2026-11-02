import { Fragment } from 'react';
import { useAppManager } from './context';
import { AccountSectionButton, AccountSectionRow } from './AccountSectionPrimitives';
import { QuotaCountdown } from './QuotaCountdown';
import { QuotaPercent } from './QuotaPercent';
import { quotaTextClass } from '../../utils/quotaColors';
import { devinQuotaLabel } from './devinAccountDisplay';

export function DevinAccountSection() {
  const { devinAccounts: group, isLaunching } = useAppManager();
  return (
    <section>
      <AccountSectionButton
        iconSrc="/icons/tools/devin.png"
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
            plan={account.plan ?? undefined}
            refreshing={group.refreshing.has(account.id)}
            authorizationFailed={group.authorizationFailedIds.has(account.id)}
            onSelect={() => group.select(account.id)}
            onRefresh={() => void group.refresh(account)}
            onDelete={() => void group.remove(account)}
            secondary={
              <span className="flex h-[16px] min-w-0 items-center gap-1 overflow-hidden whitespace-nowrap text-[11px] font-semibold leading-[16px] text-cyber-text">
                {account.quotaWindows.length
                  ? account.quotaWindows.map((window, index) => (
                      <Fragment key={window.label}>
                        {index > 0 && <span aria-hidden="true">·</span>}
                        <span
                          className={`flex flex-shrink-0 items-center gap-0.5 ${quotaTextClass(window.remainingPercent)}`}
                        >
                          <QuotaPercent percent={window.remainingPercent} />
                          {window.resetAt ? (
                            <QuotaCountdown resetAt={window.resetAt} compact small />
                          ) : (
                            <span className="text-[10px]">{devinQuotaLabel(window.label)}</span>
                          )}
                        </span>
                      </Fragment>
                    ))
                  : account.balanceUsd == null
                    ? '—'
                    : `$${account.balanceUsd.toFixed(2)}`}
              </span>
            }
          />
        ))}
      </div>
    </section>
  );
}
