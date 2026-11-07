import React from 'react';
import { useAppManager } from './context';
import { ModelSwitchDivider } from './ModelSwitchDivider';
import {
  AccountSection,
  AccountSectionButton,
  AccountSectionRow,
} from './AccountSectionPrimitives';

export const GrokAccountSection: React.FC<{ showDivider?: boolean }> = ({ showDivider = true }) => {
  const { grokAccounts, isLaunching } = useAppManager();
  return (
    <AccountSection>
      <AccountSectionButton
        iconSrc="/icons/tools/grok.svg"
        busy={grokAccounts.busy}
        disabled={isLaunching}
        remainingSeconds={grokAccounts.remainingSeconds}
        onClick={() => void grokAccounts.add()}
        onCancel={grokAccounts.cancelLogin}
      />
      {grokAccounts.accounts.length > 0 && (
        <div className="space-y-2">
          {grokAccounts.accounts.map((account) => (
            <AccountSectionRow
              key={account.id}
              selected={grokAccounts.selectedId === account.id}
              email={account.email}
              plan={account.plan}
              refreshing={grokAccounts.refreshing.has(account.id)}
              authorizationFailed={grokAccounts.authorizationFailedIds.has(account.id)}
              onSelect={() => grokAccounts.select(account.id)}
              onRefresh={() => void grokAccounts.refresh(account)}
              onDelete={() => void grokAccounts.remove(account)}
            />
          ))}
        </div>
      )}
      {showDivider && <ModelSwitchDivider />}
    </AccountSection>
  );
};
