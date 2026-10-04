import React from 'react';
import { LoaderCircle, RefreshCw, Trash2 } from 'lucide-react';
import { useI18n } from '../../hooks/useI18n';

export const AccountSectionButton: React.FC<{
  iconSrc: string;
  busy: boolean;
  remainingSeconds: number;
  onClick: () => void;
  disabled?: boolean;
  colorClassName?: string;
  secondary?: React.ReactNode;
}> = ({ iconSrc, busy, remainingSeconds, onClick, disabled, colorClassName = '', secondary }) => {
  const { t } = useI18n();
  const label = busy
    ? t('agent.waitingForBrowser').replace('{seconds}', String(remainingSeconds))
    : t('agent.addCurrentAccount');
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
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled || busy}
      className={`account-pill ${colorClassName} mb-2 flex h-12 w-full items-center justify-center rounded-full px-3 text-[17px] font-bold leading-6 transition-opacity hover:opacity-90 disabled:opacity-50`}
    >
      <span className="flex translate-y-px items-center gap-2.5">
        <img src={iconSrc} alt="" className="h-6 w-6" />
        <span>
          {busy
            ? t('agent.waitingForBrowser').replace('{seconds}', String(remainingSeconds))
            : t('agent.addCurrentAccount')}
        </span>
      </span>
    </button>
  );
};

export const AccountSectionRow: React.FC<{
  selected: boolean;
  email: string;
  plan?: React.ReactNode;
  planPrefix?: React.ReactNode;
  widePlan?: boolean;
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
  secondary,
  onSelect,
  onDelete,
  refreshing,
  onRefresh,
  leadingAction,
  colorClassName = '',
}) => {
  const { t } = useI18n();
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
      <span className="grid min-w-0 grid-cols-[minmax(0,1fr)] auto-rows-[16px] items-center">
        <span className="block truncate text-[13px] font-semibold text-cyber-text">{email}</span>
        <span className="flex h-[16px] items-center gap-2 text-[12px] text-cyber-text">
          {secondary ?? '—'}
        </span>
      </span>
      <span className="grid auto-rows-[16px] items-center justify-items-center">
        <span
          className={`${planPrefix ? 'flex items-center gap-2 ' : ''}whitespace-nowrap text-[12px] font-semibold text-cyber-text`}
        >
          {planPrefix}
          {plan || '—'}
        </span>
        <span className={`flex items-center ${leadingAction ? 'gap-0' : 'gap-1.5'}`}>
          {leadingAction}
          {onRefresh && (
            <button
              type="button"
              aria-label={`${t('agent.refreshAccount')} ${email}`}
              disabled={refreshing}
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
            onClick={(event) => {
              event.stopPropagation();
              onDelete();
            }}
            className="account-icon-button flex h-5 w-5 items-center justify-center rounded-full"
          >
            <Trash2 size={11} aria-hidden="true" />
          </button>
        </span>
      </span>
    </div>
  );
};
