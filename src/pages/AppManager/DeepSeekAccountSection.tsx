import React from 'react';
import { useAppManager } from './context';
import { ModelSwitchDivider } from './ModelSwitchDivider';
import {
  AccountSection,
  AccountSectionButton,
  AccountSectionRow,
} from './AccountSectionPrimitives';
export const DeepSeekAccountSection: React.FC<{ showDivider?: boolean }> = ({
  showDivider = true,
}) => {
  const { deepSeekAccounts } = useAppManager();
  const { accounts, selectedId, select, busy, remainingSeconds, refreshing, add, refresh, remove } =
    deepSeekAccounts;
  return (
    <AccountSection>
      <AccountSectionButton
        iconSrc="/icons/tools/dsh.png"
        colorClassName="deepseek-account-pill"
        busy={busy}
        remainingSeconds={remainingSeconds}
        onClick={() => void add()}
        onCancel={deepSeekAccounts.cancelLogin}
      />
      {accounts.length > 0 && (
        <div className="space-y-2">
          {accounts.map((account) => (
            <AccountSectionRow
              key={account.id}
              colorClassName="deepseek-account-pill"
              singleLine
              selected={selectedId === account.id}
              email={account.name}
              refreshing={refreshing.has(account.id)}
              authorizationFailed={deepSeekAccounts.authorizationFailedIds.has(account.id)}
              onSelect={() => select(account.id)}
              onRefresh={() => void refresh(account)}
              onDelete={() => void remove(account)}
              secondary={
                account.balances == null ? undefined : (
                  <span className="flex h-[16px] items-center justify-between">
                    <span className="truncate text-[12px] font-semibold leading-[16px] text-cyber-text">
                      {account.balances.length === 0
                        ? '0'
                        : account.balances
                            .map((balance) =>
                              new Intl.NumberFormat(undefined, {
                                style: 'currency',
                                currency: balance.currency,
                                maximumFractionDigits: 2,
                              }).format(balance.amount)
                            )
                            .join(' / ')}
                    </span>
                  </span>
                )
              }
            />
          ))}
        </div>
      )}
      {showDivider && <ModelSwitchDivider />}
    </AccountSection>
  );
};
