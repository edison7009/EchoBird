import React, { Fragment } from 'react';
import { useAppManager } from './context';
import {
  AccountSection,
  AccountSectionButton,
  AccountSectionRow,
} from './AccountSectionPrimitives';
import { QuotaCountdown } from './QuotaCountdown';
import { QuotaPercent } from './QuotaPercent';
import { quotaTextClass } from '../../utils/quotaColors';

export const AntigravityAccountSection: React.FC = () => {
  const { antigravityAccounts, isLaunching, selectedTool } = useAppManager();
  const { accounts, selectedId, select, busy, remainingSeconds, refreshing, refresh, add, remove } =
    antigravityAccounts;
  const minimum = (account: (typeof accounts)[number], prefix: string) => {
    const values = account.quotas.filter((quota) => quota.name.startsWith(prefix));
    return values.reduce<(typeof values)[number] | null>(
      (lowest, quota) =>
        !lowest || quota.remainingPercent < lowest.remainingPercent ? quota : lowest,
      null
    );
  };
  return (
    <AccountSection>
      <AccountSectionButton
        iconSrc={`/icons/tools/${selectedTool}.png`}
        busy={busy}
        disabled={isLaunching}
        remainingSeconds={remainingSeconds}
        onClick={() => void add()}
        onCancel={antigravityAccounts.cancelLogin}
      />
      <div className="space-y-2">
        {accounts.map((account) => {
          const gemini = minimum(account, 'gemini');
          const claude = minimum(account, 'claude');
          const parts = [
            gemini && (
              <span className={quotaTextClass(gemini.remainingPercent)}>
                Gemini <QuotaPercent percent={gemini.remainingPercent} />
              </span>
            ),
            claude && (
              <span className={quotaTextClass(claude.remainingPercent)}>
                Claude <QuotaPercent percent={claude.remainingPercent} />
              </span>
            ),
          ].filter(Boolean);
          const hasResetTime = gemini?.resetAt || claude?.resetAt;
          const sharedResetAt =
            gemini?.resetAt && gemini.resetAt === claude?.resetAt ? gemini.resetAt : null;
          return (
            <AccountSectionRow
              key={account.id}
              selected={selectedId === account.id}
              email={account.email}
              plan={account.plan}
              secondary={
                hasResetTime ? (
                  <span className="flex min-w-0 items-center gap-1 overflow-hidden whitespace-nowrap text-[11px] leading-[16px]">
                    {gemini && (
                      <span className={quotaTextClass(gemini.remainingPercent)}>
                        G <QuotaPercent percent={gemini.remainingPercent} />
                        {!sharedResetAt && (
                          <>
                            {' '}
                            <QuotaCountdown resetAt={gemini.resetAt} compact small />
                          </>
                        )}
                      </span>
                    )}
                    {gemini && claude && <span>·</span>}
                    {claude && (
                      <span className={quotaTextClass(claude.remainingPercent)}>
                        C <QuotaPercent percent={claude.remainingPercent} />
                        {!sharedResetAt && (
                          <>
                            {' '}
                            <QuotaCountdown resetAt={claude.resetAt} compact small />
                          </>
                        )}
                      </span>
                    )}
                    {sharedResetAt && (
                      <span>
                        · <QuotaCountdown resetAt={sharedResetAt} compact small />
                      </span>
                    )}
                  </span>
                ) : parts.length ? (
                  <span>
                    {parts.map((part, index) => (
                      <Fragment key={index}>
                        {index > 0 && ' · '}
                        {part}
                      </Fragment>
                    ))}
                  </span>
                ) : undefined
              }
              refreshing={refreshing.has(account.id)}
              authorizationFailed={antigravityAccounts.authorizationFailedIds.has(account.id)}
              onRefresh={() => void refresh(account)}
              onSelect={() => select(account.id)}
              onDelete={() => void remove(account)}
            />
          );
        })}
      </div>
    </AccountSection>
  );
};
