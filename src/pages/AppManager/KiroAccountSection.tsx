import React from 'react';
import { useAppManager } from './context';
import { useI18n } from '../../hooks/useI18n';
import {
  AccountSection,
  AccountSectionButton,
  AccountSectionRow,
} from './AccountSectionPrimitives';
import { QuotaCountdown } from './QuotaCountdown';

export const KiroAccountSection: React.FC = () => {
  const { selectedTool, kiroAccounts } = useAppManager();
  const { t } = useI18n();
  const { accounts, selectedId, select, busy, remainingSeconds, refreshing, add, refresh, remove } =
    kiroAccounts;
  const label = (kind: string) =>
    t(
      kind === 'credits'
        ? 'agent.baseCredits'
        : kind === 'trial' || kind === 'bonus'
          ? 'agent.rewardCredits'
          : 'agent.credits'
    );
  return (
    <AccountSection>
      <AccountSectionButton
        iconSrc={`/icons/tools/${selectedTool}.svg`}
        busy={busy}
        remainingSeconds={remainingSeconds}
        onClick={() => void add()}
        onCancel={kiroAccounts.cancelLogin}
      />
      {accounts.length > 0 && (
        <div className="space-y-2">
          {accounts.map((account) => (
            <AccountSectionRow
              key={account.id}
              selected={selectedId === account.id}
              email={account.name}
              plan={account.plan}
              refreshing={refreshing.has(account.id)}
              authorizationFailed={kiroAccounts.authorizationFailedIds.has(account.id)}
              onSelect={() => select(account.id)}
              onRefresh={() => void refresh(account)}
              onDelete={() => void remove(account)}
              secondary={
                <span className="flex h-[16px] min-w-0 items-center gap-1 overflow-hidden whitespace-nowrap text-[11px] font-semibold">
                  {account.quota.length
                    ? account.quota.map((quota, index) => (
                        <span
                          key={index}
                          className="flex min-w-0 items-baseline gap-1"
                          aria-label={label(quota.label)}
                        >
                          <span className="flex-shrink-0">
                            {quota.remaining == null
                              ? '—'
                              : quota.remaining.toLocaleString(undefined, {
                                  maximumFractionDigits: 2,
                                  useGrouping: false,
                                })}
                          </span>
                          {quota.remaining != null && (
                            <QuotaCountdown
                              resetAt={quota.resetAt}
                              compact
                              label={t('accountCenter.reset')}
                            />
                          )}
                        </span>
                      ))
                    : '—'}
                </span>
              }
            />
          ))}
        </div>
      )}
    </AccountSection>
  );
};
