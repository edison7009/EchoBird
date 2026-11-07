import React from 'react';
import { useAppManager } from './context';
import { ModelSwitchDivider } from './ModelSwitchDivider';
import {
  AccountSection,
  AccountSectionButton,
  AccountSectionRow,
} from './AccountSectionPrimitives';

export const ClientAccountSection: React.FC<{
  provider: 'kilo' | 'copilot';
  showDivider?: boolean;
}> = ({ provider, showDivider = true }) => {
  const { kiloAccounts, copilotAccounts, isLaunching } = useAppManager();
  const group = provider === 'kilo' ? kiloAccounts : copilotAccounts;
  return (
    <AccountSection>
      <AccountSectionButton
        iconSrc={`/icons/tools/${provider}.png`}
        busy={group.busy}
        disabled={isLaunching}
        remainingSeconds={group.remainingSeconds}
        onClick={() => void group.add()}
        onCancel={group.cancelLogin}
      />
      {group.accounts.length > 0 && (
        <div className="space-y-2">
          {provider === 'kilo'
            ? kiloAccounts.accounts.map((account) => (
                <AccountSectionRow
                  key={account.id}
                  selected={kiloAccounts.selectedId === account.id}
                  email={account.email}
                  plan={account.plan}
                  refreshing={kiloAccounts.refreshing.has(account.id)}
                  authorizationFailed={kiloAccounts.authorizationFailedIds.has(account.id)}
                  onSelect={() => kiloAccounts.select(account.id)}
                  onRefresh={() => void kiloAccounts.refresh(account)}
                  onDelete={() => void kiloAccounts.remove(account)}
                  secondary={
                    account.balance == null ? undefined : (
                      <span className="text-[12px] font-semibold leading-[16px]">
                        {new Intl.NumberFormat(undefined, {
                          style: 'currency',
                          currency: 'USD',
                          maximumFractionDigits: 2,
                        }).format(account.balance)}
                      </span>
                    )
                  }
                />
              ))
            : copilotAccounts.accounts.map((account) => (
                <AccountSectionRow
                  key={account.id}
                  selected={copilotAccounts.selectedId === account.id}
                  email={account.login}
                  plan={account.plan}
                  refreshing={copilotAccounts.refreshing.has(account.id)}
                  authorizationFailed={copilotAccounts.authorizationFailedIds.has(account.id)}
                  onSelect={() => copilotAccounts.select(account.id)}
                  onRefresh={() => void copilotAccounts.refresh(account)}
                  onDelete={() => void copilotAccounts.remove(account)}
                  secondary={
                    account.remaining == null ? undefined : (
                      <span className="text-[12px] font-semibold leading-[16px]">
                        {new Intl.NumberFormat().format(account.remaining)}
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
