import { ArrowLeftRight, Gift } from 'lucide-react';
import { useI18n } from '../../hooks/useI18n';
import { useUsageClock } from '../../hooks/useUsageClock';
import { useAppManager } from './context';
import {
  AccountSectionButton,
  AccountSectionRewardButton,
  AccountSectionRow,
} from './AccountSectionPrimitives';
import { ModelSwitchDivider } from './ModelSwitchDivider';
import { QuotaCountdown } from './QuotaCountdown';
import { QuotaPercent } from './QuotaPercent';
import { quotaTextClass } from '../../utils/quotaColors';
import { isFreePlan, minimaxPlanLabel } from './accountPlanLabel';
import { minimaxCreditBuckets, minimaxRewardStatus } from './minimaxAccountDisplay';

export function MinimaxAccountRegionChoice({ disabled }: { disabled?: boolean }) {
  const { minimaxAccounts, isLaunching } = useAppManager();
  const { t } = useI18n();
  const label = t(
    minimaxAccounts.region === 'cn' ? 'agent.minimaxRegionCn' : 'agent.minimaxRegionGlobal'
  );
  return (
    <button
      type="button"
      aria-label={t('agent.minimaxSwitchRegion').replace('{region}', label)}
      disabled={disabled || minimaxAccounts.busy || isLaunching}
      onClick={() => minimaxAccounts.setRegion(minimaxAccounts.region === 'cn' ? 'global' : 'cn')}
      className="inline-flex h-4 items-center gap-1 rounded-sm focus-visible:outline focus-visible:outline-1"
    >
      <span>{label}</span>
      {minimaxAccounts.busy && minimaxAccounts.login?.userCode ? (
        <span className="font-mono">· {minimaxAccounts.login.userCode}</span>
      ) : (
        <ArrowLeftRight size={12} aria-hidden="true" />
      )}
    </button>
  );
}

export function MinimaxAccountSection({ showDivider = true }: { showDivider?: boolean }) {
  const { minimaxAccounts: group, isLaunching } = useAppManager();
  const { t, locale } = useI18n();
  const now = useUsageClock(group.accounts.some((account) => account.dailyReward != null));
  return (
    <section>
      <AccountSectionButton
        iconSrc="/icons/tools/minimaxdesktop.svg"
        busy={group.busy}
        disabled={isLaunching}
        remainingSeconds={group.remainingSeconds}
        onClick={() => void group.add()}
        onCancel={group.cancelLogin}
        secondary={<MinimaxAccountRegionChoice />}
      />
      <div className="space-y-2">
        {group.accounts.map((account) => (
          <AccountSectionRow
            key={account.id}
            selected={group.selectedId === account.id}
            email={`${t(account.region === 'cn' ? 'agent.minimaxRegionCn' : 'agent.minimaxRegionGlobal')} · ${account.email}`}
            plan={<>{minimaxPlanLabel(account.plan) || '—'}</>}
            planPrefix={
              !isFreePlan(minimaxPlanLabel(account.plan)) && account.subscriptionEndAt ? (
                <QuotaCountdown resetAt={account.subscriptionEndAt} compact />
              ) : undefined
            }
            refreshing={group.refreshing.has(account.id)}
            authorizationFailed={group.authorizationFailedIds.has(account.id)}
            onSelect={() => group.select(account.id)}
            onRefresh={() => void group.refresh(account)}
            onDelete={() => void group.remove(account)}
            leadingAction={
              <AccountSectionRewardButton
                email={account.email}
                claimed={minimaxRewardStatus(account, now / 1000) === 'claimed'}
                disabled={
                  isLaunching ||
                  group.refreshing.has(account.id) ||
                  minimaxRewardStatus(account, now / 1000) === 'unavailable'
                }
                onClaim={() => void group.claimDaily(account)}
              />
            }
            secondary={
              !account.quota?.windows.length &&
              account.quota?.credits == null &&
              !minimaxCreditBuckets(account.quota).length ? undefined : (
                <span className="flex h-4 min-w-0 items-center gap-2 overflow-hidden whitespace-nowrap text-[11px] font-semibold">
                  {account.quota?.windows.length ? (
                    account.quota.windows.map((window, index) => (
                      <span
                        key={index}
                        className={`flex flex-shrink-0 items-baseline gap-1 ${quotaTextClass(Math.max(0, Math.min(100, 100 - window.usedPercent)))}`}
                      >
                        <span>{window.name}</span>
                        <QuotaPercent
                          percent={Math.max(0, Math.min(100, 100 - window.usedPercent))}
                        />
                        <QuotaCountdown resetAt={window.resetsAt} compact small />
                      </span>
                    ))
                  ) : minimaxCreditBuckets(account.quota).length ? (
                    minimaxCreditBuckets(account.quota).map((bucket) => (
                      <span key={bucket.label} className="flex flex-shrink-0 items-baseline gap-1">
                        <span
                          className={
                            bucket.label === 'agent.grantedCredits' && bucket.remaining != null
                              ? 'inline-flex items-center gap-0.5'
                              : undefined
                          }
                          aria-label={t(bucket.label)}
                        >
                          {bucket.label === 'agent.grantedCredits' && bucket.remaining != null && (
                            <Gift size={10} aria-hidden="true" />
                          )}
                          {bucket.remaining == null
                            ? t('model.noUsageData')
                            : new Intl.NumberFormat(locale, { maximumFractionDigits: 2 }).format(
                                bucket.remaining
                              )}
                        </span>
                        <QuotaCountdown resetAt={bucket.expiresAt} compact small />
                      </span>
                    ))
                  ) : (
                    <span>
                      {account.quota?.credits == null
                        ? t('model.noUsageData')
                        : new Intl.NumberFormat(locale, { maximumFractionDigits: 2 }).format(
                            account.quota.credits
                          )}
                    </span>
                  )}
                </span>
              )
            }
          />
        ))}
      </div>
      {showDivider && <ModelSwitchDivider />}
    </section>
  );
}
