import React from 'react';
import { useAppManager } from './context';
import { AccountSectionButton, AccountSectionRow } from './AccountSectionPrimitives';
import { QuotaCountdown } from './QuotaCountdown';
import { QuotaPercent } from './QuotaPercent';
import { quotaTextClass } from '../../utils/quotaColors';
import { isFreePlan } from './accountPlanLabel';
import { useI18n } from '../../hooks/useI18n';
import { ACCOUNT_NAME_MASK, useAccountPrivacyStore } from '../../stores/accountPrivacyStore';

export const ManusAccountSection: React.FC<{ tool?: 'manus' | 'cue' }> = ({ tool = 'manus' }) => {
  const { manusAccounts, cueAccounts, isLaunching } = useAppManager();
  const accounts = tool === 'cue' ? cueAccounts : manusAccounts;
  const { t } = useI18n();
  const hidden = useAccountPrivacyStore((state) => state.hidden);
  return (
    <section>
      <AccountSectionButton
        iconSrc={`/icons/tools/${tool}.png`}
        busy={accounts.busy}
        disabled={isLaunching}
        remainingSeconds={accounts.remainingSeconds}
        onClick={() => void accounts.add()}
        onCancel={accounts.cancelLogin}
      />
      {accounts.accounts.length > 0 && (
        <div className="space-y-2">
          {accounts.accounts.map((account) => {
            const credits = 'credits' in account ? account.credits : null;
            const weekly = tool === 'cue' && 'weekly' in account ? account.weekly : null;
            const subscriptionEndAt =
              !isFreePlan(account.plan) && 'subscriptionEndAt' in account
                ? account.subscriptionEndAt
                : null;
            const missingWeekly = weekly && weekly.remainingPercent == null;
            return (
              <AccountSectionRow
                key={account.id}
                selected={accounts.selectedId === account.id}
                email={account.email}
                plan={account.plan}
                planPrefix={
                  subscriptionEndAt ? (
                    <QuotaCountdown
                      resetAt={subscriptionEndAt}
                      compact
                      label={t('accountCenter.subscription')}
                    />
                  ) : undefined
                }
                secondary={
                  missingWeekly ? undefined : weekly ? (
                    <span
                      className={`flex h-[16px] items-center justify-between ${subscriptionEndAt ? 'w-full min-w-0' : ''} ${quotaTextClass(weekly.remainingPercent)}`}
                    >
                      <span
                        role="progressbar"
                        aria-label={`7d ${hidden ? ACCOUNT_NAME_MASK : account.email}`}
                        aria-valuemin={0}
                        aria-valuemax={100}
                        aria-valuenow={weekly.remainingPercent ?? undefined}
                        aria-valuetext={weekly.remainingPercent == null ? '—' : undefined}
                        className={`h-1.5 ${subscriptionEndAt ? 'min-w-0' : 'min-w-[56px]'} max-w-[80px] flex-1 overflow-hidden rounded-full account-quota-track`}
                      >
                        <span
                          className="quota-progress-fill block h-full rounded-full bg-cyber-bg"
                          style={{ width: `${weekly.remainingPercent ?? 0}%` }}
                        />
                      </span>
                      <span className="ml-[6px] w-[30px] flex-shrink-0 text-right text-[12px] font-semibold leading-[16px] text-cyber-text">
                        <QuotaPercent percent={weekly.remainingPercent} />
                      </span>
                      <QuotaCountdown resetAt={weekly.resetAt} />
                    </span>
                  ) : credits ? (
                    `${credits.total} ${t('agent.credits')}`
                  ) : undefined
                }
                refreshing={accounts.refreshing.has(account.id)}
                authorizationFailed={accounts.authorizationFailedIds.has(account.id)}
                onSelect={() => accounts.select(account.id)}
                onRefresh={() => void accounts.refresh(account)}
                onDelete={() => void accounts.remove(account)}
              />
            );
          })}
        </div>
      )}
    </section>
  );
};
