import {
  AccountSection,
  AccountSectionButton,
  AccountSectionRow,
} from './AccountSectionPrimitives';
import { accountPlanLabel } from './accountPlanLabel';
import { useAppManager } from './context';
import { ModelSwitchDivider } from './ModelSwitchDivider';

export function PluginToolAccountSection({ showDivider = true }: { showDivider?: boolean }) {
  const { pluginAccounts, isLaunching } = useAppManager();
  const { accounts, selectedId, select, busy, remainingSeconds, refreshing, add, refresh, remove } =
    pluginAccounts;
  return (
    <AccountSection>
      <AccountSectionButton
        iconSrc={pluginAccounts.provider?.icon ?? ''}
        busy={busy}
        disabled={isLaunching}
        remainingSeconds={remainingSeconds}
        onClick={() => void add()}
        onCancel={pluginAccounts.cancelLogin}
      />
      <div className="space-y-2">
        {accounts.map((account) => (
          <AccountSectionRow
            key={account.id}
            selected={selectedId === account.id}
            email={account.email}
            plan={account.plan ? accountPlanLabel(account.plan) : null}
            refreshing={refreshing.has(account.id)}
            authorizationFailed={pluginAccounts.authorizationFailedIds.has(account.id)}
            onSelect={() => select(account.id)}
            onRefresh={() => void refresh(account)}
            onDelete={() => void remove(account)}
            secondary={account.remaining == null ? '—' : String(account.remaining)}
          />
        ))}
      </div>
      {showDivider && <ModelSwitchDivider />}
    </AccountSection>
  );
}
