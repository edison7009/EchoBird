import React from 'react';
import { ArrowLeftRight } from 'lucide-react';
import { useI18n } from '../../hooks/useI18n';
import { useAppManager } from './context';
import {
  AccountSection,
  AccountSectionButton,
  AccountSectionRow,
} from './AccountSectionPrimitives';
import { ModelSwitchDivider } from './ModelSwitchDivider';
import { QuotaCountdown } from './QuotaCountdown';
import { QuotaPercent } from './QuotaPercent';
import { quotaTextClass } from '../../utils/quotaColors';
import { accountPlanLabel } from './accountPlanLabel';

export const ZCodeAccountProviderChoice: React.FC<{ disabled?: boolean }> = ({ disabled }) => {
  const { zcodeAccounts, isLaunching } = useAppManager();
  const { t } = useI18n();
  const { busy, provider, setProvider } = zcodeAccounts;
  const providerLabel = provider === 'bigmodel' ? 'BigModel' : 'Z.ai';
  return (
    <button
      type="button"
      aria-label={t('agent.zcodeSwitchProvider').replace('{provider}', providerLabel)}
      disabled={disabled || busy || isLaunching}
      onClick={() => setProvider(provider === 'bigmodel' ? 'zai' : 'bigmodel')}
      className="inline-flex h-4 items-center gap-1 rounded-sm focus-visible:outline focus-visible:outline-1"
    >
      <span>{providerLabel}</span>
      <ArrowLeftRight size={12} aria-hidden="true" />
    </button>
  );
};

export const ZCodeAccountButton: React.FC<{ disabled?: boolean }> = ({ disabled }) => {
  const { zcodeAccounts, isLaunching } = useAppManager();
  const { busy, remainingSeconds, add } = zcodeAccounts;
  return (
    <AccountSectionButton
      iconSrc="/icons/tools/zcode.png"
      busy={busy}
      disabled={disabled || isLaunching}
      remainingSeconds={remainingSeconds}
      onClick={() => void add()}
      onCancel={zcodeAccounts.cancelLogin}
      secondary={<ZCodeAccountProviderChoice disabled={disabled} />}
    />
  );
};

export const ZCodeAccountSection: React.FC<{ showDivider?: boolean }> = ({
  showDivider = true,
}) => {
  const { zcodeAccounts } = useAppManager();
  const { t } = useI18n();
  const { accounts, selectedId, select, refreshing, refresh, remove } = zcodeAccounts;
  return (
    <AccountSection>
      <ZCodeAccountButton />
      <div className="space-y-2">
        {accounts.map((account) => {
          const subscriptionEndAt = ['trial', 'free', 'no plan'].includes(
            account.plan?.toLowerCase() ?? ''
          )
            ? null
            : account.subscriptionEndAt;
          return (
            <AccountSectionRow
              key={account.id}
              selected={selectedId === account.id}
              email={`${account.provider === 'bigmodel' ? 'BigModel' : 'Z.ai'} · ${account.email}`}
              plan={
                account.plan ? (
                  <span
                    className={`block truncate ${subscriptionEndAt ? 'max-w-[48px]' : 'max-w-[72px]'}`}
                  >
                    {account.plan === 'Trial'
                      ? t('agent.zcodeTrial')
                      : account.plan === 'No Plan'
                        ? t('agent.zcodeNoPlan')
                        : accountPlanLabel(account.plan.replace(/^ZCode\s+/i, ''))}
                  </span>
                ) : null
              }
              planPrefix={
                subscriptionEndAt ? (
                  <QuotaCountdown resetAt={subscriptionEndAt} compact />
                ) : undefined
              }
              refreshing={refreshing.has(account.id)}
              authorizationFailed={zcodeAccounts.authorizationFailedIds.has(account.id)}
              onSelect={() => select(account.id)}
              onRefresh={() => void refresh(account)}
              onDelete={() => void remove(account)}
              secondary={
                !account.quotaWindows?.some((window) => window.remainingPercent != null) &&
                account.remainingPercent == null ? undefined : account.quotaWindows &&
                  account.quotaWindows.length > 1 ? (
                  <span className="flex h-[16px] min-w-0 items-center gap-1 overflow-hidden whitespace-nowrap text-[11px] font-semibold leading-[16px] text-cyber-text">
                    {account.quotaWindows.map((window, index) => (
                      <span
                        key={index}
                        className={`flex flex-shrink-0 items-center ${quotaTextClass(window.remainingPercent)}`}
                      >
                        <QuotaPercent percent={window.remainingPercent} />
                        <QuotaCountdown resetAt={window.resetAt} compact small parenthesized />
                      </span>
                    ))}
                  </span>
                ) : (
                  <span
                    className={`flex h-[16px] items-center justify-between ${quotaTextClass(account.remainingPercent)}`}
                  >
                    <span className="h-1.5 min-w-[56px] max-w-[80px] flex-1 overflow-hidden rounded-full account-quota-track">
                      <span
                        className="quota-progress-fill block h-full rounded-full bg-cyber-bg"
                        style={{ width: `${account.remainingPercent ?? 0}%` }}
                      />
                    </span>
                    <span className="ml-[6px] w-[30px] flex-shrink-0 text-right text-[12px] font-semibold leading-[16px] text-cyber-text">
                      <QuotaPercent percent={account.remainingPercent} />
                    </span>
                    <QuotaCountdown resetAt={account.resetAt} />
                  </span>
                )
              }
            />
          );
        })}
      </div>
      {showDivider && <ModelSwitchDivider />}
    </AccountSection>
  );
};
