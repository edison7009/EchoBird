import React from 'react';
import { ArrowLeftRight } from 'lucide-react';
import { useI18n } from '../../hooks/useI18n';
import { useAppManager } from './context';
import { AccountSectionButton, AccountSectionRow } from './AccountSectionPrimitives';
import { ModelSwitchDivider } from './ModelSwitchDivider';
import { QuotaCountdown } from './QuotaCountdown';

export const ZCodeAccountSection: React.FC<{ showDivider?: boolean }> = ({
  showDivider = true,
}) => {
  const { zcodeAccounts, isLaunching } = useAppManager();
  const { t } = useI18n();
  const {
    accounts,
    selectedId,
    select,
    busy,
    remainingSeconds,
    refreshing,
    add,
    refresh,
    remove,
    provider,
    setProvider,
  } = zcodeAccounts;
  const providerLabel = provider === 'bigmodel' ? 'BigModel' : 'Z.ai';
  return (
    <section className={showDivider ? 'mb-3' : undefined}>
      <AccountSectionButton
        iconSrc="/icons/tools/zcode.png"
        busy={busy}
        disabled={isLaunching}
        remainingSeconds={remainingSeconds}
        onClick={() => void add()}
        secondary={
          <button
            type="button"
            aria-label={t('agent.zcodeSwitchProvider').replace('{provider}', providerLabel)}
            disabled={busy || isLaunching}
            onClick={() => setProvider(provider === 'bigmodel' ? 'zai' : 'bigmodel')}
            className="inline-flex h-4 items-center gap-1 rounded-sm focus-visible:outline focus-visible:outline-1"
          >
            <span>{providerLabel}</span>
            <ArrowLeftRight size={12} aria-hidden="true" />
          </button>
        }
      />
      <div className="space-y-2">
        {accounts.map((account) => {
          const subscriptionEndAt = ['trial', 'free'].includes(account.plan?.toLowerCase() ?? '')
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
                      : account.plan.replace(/^ZCode\s+/i, '')}
                  </span>
                ) : null
              }
              widePlan={(account.plan?.length ?? 0) >= 8}
              planPrefix={
                subscriptionEndAt ? (
                  <QuotaCountdown resetAt={subscriptionEndAt} compact />
                ) : undefined
              }
              refreshing={refreshing.has(account.id)}
              onSelect={() => select(account.id)}
              onRefresh={() => void refresh(account)}
              onDelete={() => void remove(account)}
              secondary={
                account.quotaWindows && account.quotaWindows.length > 1 ? (
                  <span className="flex h-[16px] min-w-0 items-center gap-1 overflow-hidden whitespace-nowrap text-[11px] font-semibold leading-[16px] text-cyber-text">
                    {account.quotaWindows.map((window, index) => (
                      <span key={index} className="flex flex-shrink-0 items-center">
                        {Math.round(window.remainingPercent)}%
                        <QuotaCountdown resetAt={window.resetAt} compact small parenthesized />
                      </span>
                    ))}
                  </span>
                ) : (
                  <span className="flex h-[16px] items-center justify-between">
                    <span className="h-1.5 min-w-[56px] max-w-[80px] flex-1 overflow-hidden rounded-full bg-cyber-border">
                      <span
                        className="block h-full rounded-full bg-cyber-bg"
                        style={{ width: `${account.remainingPercent ?? 0}%` }}
                      />
                    </span>
                    <span className="ml-[6px] w-[30px] flex-shrink-0 text-right text-[12px] font-semibold leading-[16px] text-cyber-text">
                      {account.remainingPercent == null
                        ? '—'
                        : `${Math.round(account.remainingPercent)}%`}
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
    </section>
  );
};
