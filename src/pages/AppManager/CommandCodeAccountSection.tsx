import { useAppManager } from './context';
import { ModelSwitchDivider } from './ModelSwitchDivider';
import {
  AccountSection,
  AccountSectionButton,
  AccountSectionRow,
} from './AccountSectionPrimitives';

export function CommandCodeAccountSection({ showDivider = true }: { showDivider?: boolean }) {
  const { commandCodeAccounts: group, isLaunching } = useAppManager();
  return (
    <AccountSection>
      <AccountSectionButton
        iconSrc="/icons/tools/commandcode.svg"
        busy={group.busy}
        disabled={isLaunching}
        remainingSeconds={group.remainingSeconds}
        onClick={() => void group.add()}
        onCancel={group.cancelLogin}
      />
      {group.accounts.length > 0 && (
        <div className="space-y-2">
          {group.accounts.map((account) => (
            <AccountSectionRow
              key={account.id}
              selected={group.selectedId === account.id}
              email={account.email || account.name || account.id}
              plan={account.plan}
              refreshing={group.refreshing.has(account.id)}
              authorizationFailed={group.authorizationFailedIds.has(account.id)}
              onSelect={() => group.select(account.id)}
              onRefresh={() => void group.refresh(account)}
              onDelete={() => void group.remove(account)}
              secondary={account.remaining == null ? undefined : <span>{account.remaining}</span>}
            />
          ))}
        </div>
      )}
      {showDivider && <ModelSwitchDivider />}
    </AccountSection>
  );
}
