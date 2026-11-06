import { useAccountPlugin } from '../../hooks/useAccountPlugin';
import React from 'react';
import { useAppManager } from './context';
import { ModelSwitchDivider } from './ModelSwitchDivider';
import {
  AccountSectionButton,
  AccountSectionRow,
  AccountSectionWarmUpButton,
} from './AccountSectionPrimitives';
import { QuotaCountdown } from './QuotaCountdown';
import { QuotaPercent } from './QuotaPercent';
import { quotaTextClass } from '../../utils/quotaColors';
import { isFreePlan } from './accountPlanLabel';
import { openAccountPlugins } from './openAccountPlugins';
import { useI18n } from '../../hooks/useI18n';
export const ClaudeCodeAccountSection: React.FC<{ showDivider?: boolean; desktop?: boolean }> = ({
  showDivider = true,
  desktop = false,
}) => {
  const { claudeCodeAccounts, claudeDesktopAccounts, setApplyError } = useAppManager();
  const group = desktop ? claudeDesktopAccounts : claudeCodeAccounts;
  const { accounts, selectedId, select, busy, remainingSeconds, refreshing, refresh, remove } =
    group;
  const installed = useAccountPlugin(desktop ? 'claudedesktop' : 'claudecode');
  const warmable = useAccountPlugin(desktop ? 'claudedesktop' : 'claudecode', 'account.warmup');
  const { t } = useI18n();
  return (
    <section className={desktop ? 'space-y-2' : undefined}>
      <AccountSectionButton
        iconSrc={desktop ? '/icons/tools/claudedesktop.svg' : '/icons/tools/claudecode.svg'}
        busy={busy}
        remainingSeconds={remainingSeconds}
        // Claude Desktop cannot be signed in from here; it saves snapshots instead.
        idleLabel={desktop ? 'agent.accountSnapshot' : 'plugins.category.account'}
        hint={desktop && installed ? t('agent.accountSnapshotHint') : undefined}
        hintLineBreaks={desktop}
        waitingLabel={desktop ? t('agent.accountSnapshotSaving') : undefined}
        className={desktop ? '!mb-0' : undefined}
        onClick={() =>
          void (installed ? group.add() : openAccountPlugins()).catch((error) =>
            setApplyError(String(error))
          )
        }
        onCancel={() => group.cancelLogin()}
      />
      {desktop && installed && !busy && accounts.length > 0 && (
        <AccountSectionButton
          busy={false}
          remainingSeconds={0}
          idleLabel="agent.accountSnapshotNext"
          className="!mb-0"
          onClick={() =>
            void claudeDesktopAccounts.addNext().catch((error) => setApplyError(String(error)))
          }
          onCancel={() => {}}
        />
      )}
      {accounts.length > 0 && (
        <div className="space-y-2">
          {accounts.map((account) => (
            <AccountSectionRow
              key={account.id}
              selected={selectedId === account.id}
              email={account.email}
              plan={account.plan}
              refreshing={refreshing.has(account.id)}
              authorizationFailed={group.authorizationFailedIds.has(account.id)}
              onSelect={() => select(account.id)}
              onRefresh={() => void refresh(account)}
              onDelete={() => void remove(account)}
              leadingAction={
                warmable && !isFreePlan(account.plan) ? (
                  <AccountSectionWarmUpButton
                    email={account.email}
                    disabled={refreshing.has(account.id)}
                    onWarmUp={() => void group.warmUp(account)}
                  />
                ) : undefined
              }
              secondary={
                account.fiveHour == null && account.sevenDay == null ? undefined : (
                  <span className="flex h-[16px] min-w-0 items-center gap-1 overflow-hidden whitespace-nowrap text-[11px] font-semibold leading-[16px] text-cyber-text">
                    {account.fiveHour && (
                      <span
                        className={`flex flex-shrink-0 items-center gap-0.5 ${quotaTextClass(account.fiveHour.remainingPercent)}`}
                      >
                        <QuotaPercent percent={account.fiveHour.remainingPercent} />
                        {account.fiveHour?.resetAt ? (
                          <QuotaCountdown
                            resetAt={account.fiveHour?.resetAt}
                            compact
                            small
                            parenthesized
                          />
                        ) : (
                          <span className="text-[10px]">5h</span>
                        )}
                      </span>
                    )}
                    {account.fiveHour && account.sevenDay && <span aria-hidden="true">·</span>}
                    {account.sevenDay && (
                      <span
                        className={`flex flex-shrink-0 items-center gap-0.5 ${quotaTextClass(account.sevenDay.remainingPercent)}`}
                      >
                        <QuotaPercent percent={account.sevenDay.remainingPercent} />
                        {account.sevenDay?.resetAt ? (
                          <QuotaCountdown
                            resetAt={account.sevenDay?.resetAt}
                            compact
                            small
                            parenthesized
                          />
                        ) : (
                          <span className="text-[10px]">7d</span>
                        )}
                      </span>
                    )}
                  </span>
                )
              }
            />
          ))}
        </div>
      )}
      {showDivider && <ModelSwitchDivider />}
    </section>
  );
};
