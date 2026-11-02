import { ArrowLeftRight } from 'lucide-react';
import type { KimiAccount } from '../../api/kimiAccounts';
import type { TKey } from '../../i18n';
import { useI18n } from '../../hooks/useI18n';
import { AccountSectionButton, AccountSectionRow } from './AccountSectionPrimitives';
import { accountPlanLabel } from './accountPlanLabel';
import { useAppManager } from './context';
import { ModelSwitchDivider } from './ModelSwitchDivider';
import { QuotaCountdown } from './QuotaCountdown';
import { QuotaPercent } from './QuotaPercent';
import { quotaTextClass } from '../../utils/quotaColors';

export function kimiQuotaLabel(name: string, t: (key: TKey) => string): string {
  const labels: Record<string, TKey> = {
    limit_5h: 'agent.kimi5h',
    limit_7d: 'agent.kimi7d',
    limit_month_total: 'agent.kimiMonthTotal',
    limit_month_code: 'agent.kimiMonthCode',
  };
  return labels[name] ? t(labels[name]) : name;
}

export function KimiAccountRegionChoice({ disabled }: { disabled?: boolean }) {
  const { kimiAccounts, isLaunching } = useAppManager();
  const { t } = useI18n();
  const label = kimiAccounts.region === 'cn' ? 'kimi.com' : 'kimi.ai';
  return (
    <button
      type="button"
      aria-label={t('agent.kimiSwitchRegion').replace('{region}', label)}
      disabled={disabled || kimiAccounts.busy || isLaunching}
      onClick={() => kimiAccounts.setRegion(kimiAccounts.region === 'cn' ? 'global' : 'cn')}
      className="inline-flex h-4 items-center gap-1 rounded-sm focus-visible:outline focus-visible:outline-1"
    >
      <span>{label}</span>
      <ArrowLeftRight size={12} aria-hidden="true" />
    </button>
  );
}

export function KimiAccountSection({ showDivider = true }: { showDivider?: boolean }) {
  const { kimiAccounts, isLaunching } = useAppManager();
  const { accounts, selectedId, select, busy, remainingSeconds, refreshing, add, refresh, remove } =
    kimiAccounts;
  const { t } = useI18n();
  const quota = (account: KimiAccount) => {
    if (account.quotaWindows.length > 0)
      return (
        <span className="flex h-[16px] min-w-0 items-center gap-1 overflow-hidden whitespace-nowrap text-[11px] font-semibold leading-[16px] text-cyber-text">
          {account.quotaWindows.map((window) => (
            <span
              key={window.name}
              className={`flex flex-shrink-0 items-center ${quotaTextClass(window.remainingPercent)}`}
            >
              <span>
                {kimiQuotaLabel(window.name, t)} <QuotaPercent percent={window.remainingPercent} />
              </span>
              <QuotaCountdown resetAt={window.resetAt} compact small parenthesized />
            </span>
          ))}
        </span>
      );
    if (account.wallet)
      return new Intl.NumberFormat(undefined, {
        ...(account.wallet.currency
          ? { style: 'currency', currency: account.wallet.currency }
          : { maximumFractionDigits: 2 }),
      }).format(account.wallet.remaining);
    return '—';
  };
  return (
    <section>
      <AccountSectionButton
        iconSrc="/icons/tools/kimidesktop.png"
        busy={busy}
        disabled={isLaunching}
        remainingSeconds={remainingSeconds}
        onClick={() => void add()}
        onCancel={kimiAccounts.cancelLogin}
        secondary={<KimiAccountRegionChoice />}
      />
      <div className="space-y-2">
        {accounts.map((account) => (
          <AccountSectionRow
            key={account.id}
            selected={selectedId === account.id}
            email={`${account.region === 'cn' ? 'kimi.com' : 'kimi.ai'} · ${account.email}`}
            plan={account.plan ? accountPlanLabel(account.plan) : null}
            refreshing={refreshing.has(account.id)}
            authorizationFailed={kimiAccounts.authorizationFailedIds.has(account.id)}
            onSelect={() => select(account.id)}
            onRefresh={() => void refresh(account)}
            onDelete={() => void remove(account)}
            secondary={quota(account)}
          />
        ))}
      </div>
      {showDivider && <ModelSwitchDivider />}
    </section>
  );
}
