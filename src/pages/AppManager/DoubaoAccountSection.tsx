import { useAccountPlugin } from '../../hooks/useAccountPlugin';
import { Gift } from 'lucide-react';
import { useI18n } from '../../hooks/useI18n';
import { useAppManager } from './context';
import {
  AccountSectionButton,
  AccountSectionRewardButton,
  AccountSectionRow,
} from './AccountSectionPrimitives';
import { QuotaCountdown } from './QuotaCountdown';
import { QuotaPercent } from './QuotaPercent';
import { quotaTextClass } from '../../utils/quotaColors';
import { doubaoRemainingPercent } from './doubaoAccountDisplay';
import { openAccountPlugins } from './openAccountPlugins';

export function DoubaoAccountSection() {
  const { doubaoAccounts: group, selectedTool, isLaunching, setApplyError } = useAppManager();
  const { t, locale } = useI18n();
  const installed = useAccountPlugin(selectedTool ?? 'doubaowork');
  return (
    <section>
      <AccountSectionButton
        iconSrc={`/icons/tools/${selectedTool}.png`}
        busy={group.busy}
        remainingSeconds={group.remainingSeconds}
        idleLabel="plugins.category.account"
        onClick={() =>
          void (installed ? group.add() : openAccountPlugins()).catch((error) =>
            setApplyError(String(error))
          )
        }
        onCancel={() => group.cancelLogin()}
      />
      <div className="space-y-2">
        {group.accounts.map((account) => (
          <AccountSectionRow
            key={account.id}
            selected={group.selectedId === account.id}
            email={account.name || account.email || account.id}
            plan={account.plan ?? undefined}
            planPrefix={
              account.expiresAt ? (
                <QuotaCountdown
                  resetAt={account.expiresAt}
                  compact
                  label={t('accountCenter.subscription')}
                />
              ) : undefined
            }
            refreshing={group.refreshing.has(account.id)}
            authorizationFailed={group.authorizationFailedIds.has(account.id)}
            onSelect={() => group.select(account.id)}
            onRefresh={() => void group.refresh(account)}
            onDelete={() => void group.remove(account)}
            leadingAction={
              account.dailyReward &&
              ['available', 'claimed'].includes(account.dailyReward.status) ? (
                <AccountSectionRewardButton
                  email={account.name}
                  claimed={account.dailyReward.status === 'claimed'}
                  disabled={isLaunching || group.refreshing.has(account.id)}
                  onClaim={() => void group.claimDaily(account)}
                />
              ) : undefined
            }
            secondary={
              <span className="flex h-[16px] min-w-0 items-center gap-2 overflow-hidden text-[11px] font-semibold whitespace-nowrap">
                {account.quota?.windows.map((window, index) => {
                  const percent = doubaoRemainingPercent(window);
                  return (
                    <span
                      key={index}
                      className={`flex flex-shrink-0 items-center gap-1 ${quotaTextClass(window.unlimited ? null : percent)}`}
                    >
                      {window.unlimited ? <span>∞</span> : <QuotaPercent percent={percent} />}
                      <QuotaCountdown resetAt={window.resetAt} compact small />
                    </span>
                  );
                })}
                {account.quota?.bonus ? (
                  <span
                    className="flex flex-shrink-0 items-center gap-0.5"
                    aria-label={t('agent.rewardCredits')}
                  >
                    <Gift size={10} aria-hidden="true" />
                    {account.quota.bonus.remaining == null
                      ? '—'
                      : new Intl.NumberFormat(locale, {
                          maximumFractionDigits: 2,
                          useGrouping: false,
                        }).format(account.quota.bonus.remaining)}
                    <QuotaCountdown
                      resetAt={account.quota.bonus.expiresAt}
                      compact
                      small
                      label={t('agent.rewardCreditsNextExpiry')}
                    />
                  </span>
                ) : null}
                {!account.quota?.windows.length && !account.quota?.bonus && (
                  <span>
                    {account.remaining == null
                      ? '—'
                      : new Intl.NumberFormat(locale, {
                          maximumFractionDigits: 2,
                          useGrouping: false,
                        }).format(account.remaining)}
                  </span>
                )}
              </span>
            }
          />
        ))}
      </div>
    </section>
  );
}
