import React from 'react';
import { Check, Gift, LoaderCircle, RefreshCw, Trash2, X } from 'lucide-react';
import { useI18n } from '../../hooks/useI18n';
import { accountPlanLabel } from './accountPlanLabel';
import { ACCOUNT_NAME_MASK, useAccountPrivacyStore } from '../../stores/accountPrivacyStore';
import type { TKey } from '../../i18n/types';
import { RoutingHint } from '../../components/RoutingToggle';

export const AccountSectionButton: React.FC<{
  iconSrc: string;
  busy: boolean;
  remainingSeconds: number;
  onClick: () => void;
  onCancel: () => void;
  disabled?: boolean;
  colorClassName?: string;
  secondary?: React.ReactNode;
  waitingLabel?: string;
  idleLabel?: TKey;
  /** Explains the action with the shared "?" beside the label. */
  hint?: string;
}> = ({
  iconSrc,
  busy,
  remainingSeconds,
  onClick,
  onCancel,
  disabled,
  colorClassName = '',
  secondary,
  waitingLabel,
  idleLabel,
  hint,
}) => {
  const { t } = useI18n();
  const label = busy
    ? (waitingLabel ?? t('agent.waitingForBrowser')).replace('{seconds}', String(remainingSeconds))
    : idleLabel
      ? t(idleLabel)
      : t('agent.addCurrentAccount');
  if (busy) {
    return (
      <div
        className={`account-pill ${colorClassName} relative mb-2 flex h-12 w-full items-center justify-center rounded-full pl-3 pr-10`}
      >
        <span className="pointer-events-none flex min-w-0 items-center gap-2.5 opacity-50">
          <img src={iconSrc} alt="" className="h-6 w-6 flex-shrink-0" />
          <span className="flex min-w-0 flex-col items-center text-center">
            <span className={`${secondary ? 'text-[14px]' : 'text-[17px]'} font-bold leading-5`}>
              {label}
            </span>
            {secondary && (
              <span className="flex h-4 items-center text-[12px] font-normal leading-4 text-cyber-text-secondary">
                {secondary}
              </span>
            )}
          </span>
        </span>
        <button
          type="button"
          aria-label={t('btn.cancel')}
          onClick={(event) => {
            event.stopPropagation();
            onCancel();
          }}
          className="account-icon-button absolute right-3 top-1/2 flex h-6 w-6 -translate-y-1/2 items-center justify-center rounded-full hover:opacity-100 focus-visible:outline focus-visible:outline-1"
        >
          <X size={16} aria-hidden="true" />
        </button>
      </div>
    );
  }
  if (secondary) {
    return (
      <div
        className={`account-pill ${colorClassName} relative mb-2 flex h-12 w-full items-center justify-center rounded-full px-3 transition-opacity ${disabled || busy ? 'opacity-50' : 'hover:opacity-90'}`}
      >
        <button
          type="button"
          aria-label={label}
          onClick={onClick}
          disabled={disabled || busy}
          className="absolute inset-0 rounded-full"
        />
        <span className="pointer-events-none flex items-center gap-2.5">
          <img src={iconSrc} alt="" className="h-6 w-6" />
          <span className="flex flex-col items-center">
            <span className="text-[17px] font-bold leading-6">{label}</span>
            <span className="pointer-events-auto relative flex h-4 items-center text-[12px] font-normal leading-4 text-cyber-text-secondary">
              {secondary}
            </span>
          </span>
        </span>
      </div>
    );
  }
  if (hint) {
    // The "?" sits outside the click target, so reading it never starts the action.
    return (
      <div
        className={`account-pill ${colorClassName} relative mb-2 flex h-12 w-full items-center justify-center rounded-full px-3 text-[17px] font-bold leading-6 transition-opacity ${disabled ? 'opacity-50' : 'hover:opacity-90'}`}
      >
        <button
          type="button"
          aria-label={label}
          onClick={onClick}
          disabled={disabled}
          className="absolute inset-0 rounded-full"
        />
        <span className="pointer-events-none flex translate-y-px items-center gap-2.5">
          <img src={iconSrc} alt="" className="h-6 w-6" />
          <span>{label}</span>
          <span className="pointer-events-auto relative flex">
            <RoutingHint hint={hint} />
          </span>
        </span>
      </div>
    );
  }
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled || busy}
      className={`account-pill ${colorClassName} mb-2 flex h-12 w-full items-center justify-center rounded-full px-3 text-[17px] font-bold leading-6 transition-opacity hover:opacity-90 disabled:opacity-50`}
    >
      <span className="flex translate-y-px items-center gap-2.5">
        <img src={iconSrc} alt="" className="h-6 w-6" />
        <span>{label}</span>
      </span>
    </button>
  );
};

export const AccountSectionRewardButton: React.FC<{
  email: string;
  claimed: boolean;
  disabled?: boolean;
  onClaim: () => void;
}> = ({ email, claimed, disabled, onClaim }) => {
  const { t } = useI18n();
  const hidden = useAccountPrivacyStore((state) => state.hidden);
  return (
    <button
      type="button"
      aria-label={`${t(claimed ? 'agent.dailyCreditsClaimed' : 'agent.claimDailyCredits')} ${hidden ? ACCOUNT_NAME_MASK : email}`}
      disabled={disabled || claimed}
      onClick={(event) => {
        event.stopPropagation();
        onClaim();
      }}
      className="account-icon-button flex h-5 w-5 items-center justify-center rounded-full disabled:opacity-40"
    >
      {claimed ? <Check size={12} aria-hidden="true" /> : <Gift size={12} aria-hidden="true" />}
    </button>
  );
};

export const AccountSectionActions: React.FC<{
  email: string;
  refreshing?: boolean;
  disabled?: boolean;
  /** Defaults to `disabled`. */
  deleteDisabled?: boolean;
  onRefresh?: () => void;
  onDelete: () => void;
}> = ({ email, refreshing, disabled, deleteDisabled = disabled, onRefresh, onDelete }) => {
  const { t } = useI18n();
  return (
    <>
      {onRefresh && (
        <button
          type="button"
          aria-label={`${t('agent.refreshAccount')} ${email}`}
          disabled={disabled || refreshing}
          onClick={(event) => {
            event.stopPropagation();
            onRefresh();
          }}
          className="account-icon-button flex h-5 w-5 items-center justify-center rounded-full disabled:opacity-40"
        >
          {refreshing ? (
            <LoaderCircle size={12} className="animate-spin" aria-hidden="true" />
          ) : (
            <RefreshCw size={12} aria-hidden="true" />
          )}
        </button>
      )}
      <button
        type="button"
        aria-label={`${t('btn.delete')} ${email}`}
        disabled={deleteDisabled}
        onClick={(event) => {
          event.stopPropagation();
          onDelete();
        }}
        className="account-icon-button flex h-5 w-5 items-center justify-center rounded-full disabled:opacity-40"
      >
        <Trash2 size={11} aria-hidden="true" />
      </button>
    </>
  );
};

export const AccountSectionRow: React.FC<{
  selected: boolean;
  email: string;
  plan?: React.ReactNode;
  planPrefix?: React.ReactNode;
  widePlan?: boolean;
  singleLine?: boolean;
  authorizationFailed?: boolean;
  secondary?: React.ReactNode;
  onSelect: () => void;
  onDelete: () => void;
  refreshing?: boolean;
  onRefresh?: () => void;
  leadingAction?: React.ReactNode;
  colorClassName?: string;
}> = ({
  selected,
  email,
  plan,
  planPrefix,
  widePlan,
  singleLine,
  authorizationFailed,
  secondary,
  onSelect,
  onDelete,
  refreshing,
  onRefresh,
  leadingAction,
  colorClassName = '',
}) => {
  const { t } = useI18n();
  const hidden = useAccountPrivacyStore((state) => state.hidden);
  const identity = hidden ? ACCOUNT_NAME_MASK : email;
  return (
    <div
      role="radio"
      aria-checked={selected}
      tabIndex={0}
      onClick={onSelect}
      onKeyDown={(event) => {
        if (event.target !== event.currentTarget) return;
        if (event.key === 'Enter' || event.key === ' ') {
          event.preventDefault();
          onSelect();
        }
      }}
      className={`account-pill ${colorClassName} grid h-12 ${planPrefix ? 'grid-cols-[16px_minmax(0,1fr)_100px]' : widePlan ? 'grid-cols-[16px_minmax(0,1fr)_72px]' : leadingAction ? 'grid-cols-[16px_minmax(0,1fr)_60px]' : 'grid-cols-[16px_minmax(0,1fr)_44px]'} items-center gap-2 rounded-full border border-transparent px-3 transition-opacity hover:opacity-90`}
    >
      <span className="flex h-[16px] w-[16px] items-center justify-center rounded-full border-2 border-cyber-bg">
        {selected && <span className="h-[8px] w-[8px] rounded-full bg-cyber-bg" />}
      </span>
      <span
        className={`grid min-w-0 ${singleLine ? 'grid-cols-[minmax(0,1fr)_auto] gap-2' : 'grid-cols-[minmax(0,1fr)] auto-rows-[16px]'} items-center`}
      >
        <span className="block truncate text-[13px] font-semibold text-cyber-text">{identity}</span>
        <span className="flex h-[16px] items-center gap-2 text-[12px] leading-[16px] text-cyber-text">
          {authorizationFailed ? (
            <span role="status" className="truncate font-semibold">
              {t('accountCenter.authFailed')}
            </span>
          ) : (
            (secondary ?? <span className="truncate text-[11px]">{t('model.noUsageData')}</span>)
          )}
        </span>
      </span>
      <span className="grid min-w-0 grid-cols-1 auto-rows-[16px] items-center justify-items-center">
        {!singleLine && (
          <span
            className={`${planPrefix ? 'flex items-center gap-2 ' : ''}justify-self-end whitespace-nowrap text-[12px] font-semibold text-cyber-text`}
          >
            {planPrefix}
            {(typeof plan === 'string' ? accountPlanLabel(plan) : plan) || '—'}
          </span>
        )}
        <span
          className={`flex items-center justify-self-end ${leadingAction ? 'gap-0' : 'gap-1.5'}`}
        >
          {leadingAction}
          <AccountSectionActions
            email={identity}
            refreshing={refreshing}
            onRefresh={onRefresh}
            onDelete={onDelete}
          />
        </span>
      </span>
    </div>
  );
};
