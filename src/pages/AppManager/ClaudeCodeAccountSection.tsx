import React from 'react';
import { useAppManager } from './context';
import { ModelSwitchDivider } from './ModelSwitchDivider';
import { AccountSectionButton, AccountSectionRow } from './AccountSectionPrimitives';
import { QuotaCountdown } from './QuotaCountdown';
export const ClaudeCodeAccountSection: React.FC<{ showDivider?: boolean }> = ({
  showDivider = true,
}) => {
  const { claudeCodeAccounts } = useAppManager();
  const { accounts, selectedId, select, busy, remainingSeconds, refreshing, add, refresh, remove } =
    claudeCodeAccounts;
  return (
    <section>
      <AccountSectionButton
        iconSrc="/icons/tools/claudecode.svg"
        colorClassName="claude-account-pill"
        busy={busy}
        remainingSeconds={remainingSeconds}
        onClick={() => void add()}
      />
      {accounts.length > 0 && (
        <div className="space-y-2">
          {accounts.map((account) => (
            <AccountSectionRow
              key={account.id}
              colorClassName="claude-account-pill"
              selected={selectedId === account.id}
              email={account.email}
              plan={account.plan}
              refreshing={refreshing.has(account.id)}
              onSelect={() => select(account.id)}
              onRefresh={() => void refresh(account)}
              onDelete={() => void remove(account)}
              secondary={
                <span className="flex h-[16px] items-center gap-2 whitespace-nowrap text-[12px] leading-[16px] text-cyber-text">
                  <span>
                    5h: {account.fiveHour == null ? '—' : `${account.fiveHour.remainingPercent}%`}{' '}
                    <QuotaCountdown resetAt={account.fiveHour?.resetAt} />
                  </span>
                  <span>
                    7d: {account.sevenDay == null ? '—' : `${account.sevenDay.remainingPercent}%`}{' '}
                    <QuotaCountdown resetAt={account.sevenDay?.resetAt} />
                  </span>
                </span>
              }
            />
          ))}
        </div>
      )}
      {showDivider && <ModelSwitchDivider />}
    </section>
  );
};
