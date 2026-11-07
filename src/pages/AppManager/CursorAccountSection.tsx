import React from 'react';
import { useAppManager } from './context';
import {
  AccountSection,
  AccountSectionButton,
  AccountSectionRow,
} from './AccountSectionPrimitives';
import { QuotaCountdown } from './QuotaCountdown';
import { QuotaPercent } from './QuotaPercent';
import { quotaTextClass } from '../../utils/quotaColors';
import { ACCOUNT_NAME_MASK, useAccountPrivacyStore } from '../../stores/accountPrivacyStore';

export const CursorAccountSection: React.FC<{ tool: 'cursor' | 'cursorcli' | 'grokbot' }> = ({
  tool,
}) => {
  const { cursorAccounts, grokBotAccounts, isLaunching } = useAppManager();
  const hidden = useAccountPrivacyStore((state) => state.hidden);
  const {
    accounts,
    selectedId,
    select,
    busy,
    remainingSeconds,
    refreshing,
    authorizationFailedIds,
    refresh,
    add,
    remove,
  } = tool === 'grokbot' ? grokBotAccounts : cursorAccounts;
  return (
    <AccountSection>
      <AccountSectionButton
        iconSrc={tool === 'grokbot' ? '/icons/tools/grokbot.png' : '/icons/tools/cursor.svg'}
        busy={busy}
        disabled={isLaunching}
        remainingSeconds={remainingSeconds}
        onClick={() => void add()}
        onCancel={(tool === 'grokbot' ? grokBotAccounts : cursorAccounts).cancelLogin}
      />
      <div className="space-y-2">
        {accounts.map((account) => (
          <AccountSectionRow
            key={account.id}
            selected={selectedId === account.id}
            email={account.email}
            plan={
              account.usage?.plan?.trim().toLowerCase() === 'grok bot plan'
                ? null
                : account.usage?.plan
            }
            refreshing={refreshing.has(account.id)}
            authorizationFailed={authorizationFailedIds.has(account.id)}
            onRefresh={() => void refresh(account)}
            onSelect={() => select(account.id)}
            onDelete={() => void remove(account)}
            secondary={
              account.usage?.remainingPercent == null ? undefined : (
                <span
                  className={`flex h-[16px] items-center justify-between ${quotaTextClass(account.usage?.remainingPercent)}`}
                >
                  <span
                    role="progressbar"
                    aria-label={hidden ? ACCOUNT_NAME_MASK : account.email}
                    aria-valuemin={0}
                    aria-valuemax={100}
                    aria-valuenow={account.usage?.remainingPercent ?? undefined}
                    aria-valuetext={account.usage?.remainingPercent == null ? '—' : undefined}
                    className="h-1.5 min-w-[56px] max-w-[80px] flex-1 overflow-hidden rounded-full account-quota-track"
                  >
                    <span
                      className="quota-progress-fill block h-full rounded-full bg-cyber-bg"
                      style={{ width: `${account.usage?.remainingPercent ?? 0}%` }}
                    />
                  </span>
                  <span className="ml-[6px] w-[30px] flex-shrink-0 text-right text-[12px] font-semibold leading-[16px] text-cyber-text">
                    <QuotaPercent percent={account.usage?.remainingPercent} />
                  </span>
                  <QuotaCountdown resetAt={account.usage?.resetAt} />
                </span>
              )
            }
          />
        ))}
      </div>
    </AccountSection>
  );
};
