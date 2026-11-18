import { X } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { useI18n } from '../hooks/useI18n';
import { useConfigBackupStore, type BackupMode } from '../stores/configBackupStore';
import { useAutoImportStore } from '../stores/autoImportStore';
import type { TKey } from '../i18n';

function errorKey(error: string, mode: BackupMode | null): TKey {
  if (error.includes('password')) return 'backup.passwordInvalid';
  if (/format|version|size/.test(error))
    return mode === 'backup' ? 'backup.invalidConfiguration' : 'backup.invalidFile';
  if (error.includes('changed')) return 'backup.changed';
  if (error.includes('credentials')) return 'backup.credentials';
  if (error.includes('busy')) return 'backup.busy';
  return 'backup.failed';
}
function BackupPasswordDialog() {
  const { t } = useI18n();
  const { challenge, error, cancel, run } = useConfigBackupStore();
  const [password, setPassword] = useState('');
  const [invalid, setInvalid] = useState<TKey | null>(null);
  const dialog = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    const previous = document.activeElement;
    input.current?.focus();
    return () => {
      if (previous instanceof HTMLElement && previous.isConnected) previous.focus();
    };
  }, []);
  if (!challenge) return null;
  const backingUp = challenge.mode === 'backup';
  return createPortal(
    <div
      className="fixed inset-0 z-[9999] flex items-center justify-center bg-black/40"
      onClick={(event) => event.stopPropagation()}
    >
      <div className="absolute inset-0" onClick={cancel} />
      <div
        ref={dialog}
        role="dialog"
        aria-modal="true"
        aria-labelledby="backup-password-title"
        className="relative w-[450px] max-w-[90vw] overflow-hidden rounded-xl border border-cyber-border/30 bg-cyber-surface shadow-2xl"
        onKeyDown={(event) => {
          if (event.key === 'Escape') {
            event.stopPropagation();
            cancel();
          }
          if (event.key === 'Tab') {
            const controls = dialog.current?.querySelectorAll<HTMLInputElement | HTMLButtonElement>(
              'input, button'
            );
            const first = controls?.[0];
            const last = controls?.[controls.length - 1];
            if (event.shiftKey && document.activeElement === first) {
              event.preventDefault();
              last?.focus();
            }
            if (!event.shiftKey && document.activeElement === last) {
              event.preventDefault();
              first?.focus();
            }
          }
        }}
      >
        <div className="h-px w-full bg-cyber-border" />
        <div className="flex items-center justify-between px-6 pb-4 pt-5">
          <div className="flex items-center gap-2">
            <span aria-hidden="true" className="font-mono text-sm text-cyber-text opacity-60">
              &gt;_
            </span>
            <h2 id="backup-password-title" className="text-base font-bold text-cyber-text">
              {t(backingUp ? 'backup.setPassword' : 'backup.enterPassword')}
            </h2>
          </div>
          <button
            type="button"
            onClick={cancel}
            aria-label={t('btn.close')}
            className="text-cyber-text-secondary close-button transition-colors"
          >
            <X size={18} />
          </button>
        </div>
        <form
          onSubmit={(event) => {
            event.preventDefault();
            if (!backingUp && !password) {
              setInvalid('backup.passwordInvalid');
              return;
            }
            const secret = password;
            setPassword('');
            setInvalid(null);
            void run(secret);
          }}
        >
          <div className="space-y-4 px-5 pb-5">
            {backingUp && (
              <p className="text-xs text-cyber-text-muted">{t('backup.passwordHint')}</p>
            )}
            <label className="block text-xs text-cyber-text-secondary">
              {t('backup.password')}
              <input
                ref={input}
                type="text"
                value={password}
                maxLength={256}
                autoComplete="off"
                spellCheck={false}
                autoCapitalize="off"
                onChange={(event) => {
                  setPassword(event.target.value);
                  setInvalid(null);
                }}
                className="mt-1 w-full rounded-button border border-cyber-border bg-cyber-input px-2 py-1.5 font-mono text-xs text-cyber-text focus:border-cyber-border focus:outline-none"
              />
            </label>
            {(invalid || error) && (
              <p role="alert" className="text-[12px] text-red-500">
                {t(invalid || errorKey(error!, challenge.mode))}
              </p>
            )}
          </div>
          <div className="flex border-t border-cyber-border">
            <button
              type="button"
              onClick={cancel}
              className="flex-1 border-r border-cyber-border px-4 py-3 text-[14px] font-semibold text-cyber-text-secondary transition-all hover:bg-cyber-elevated hover:text-cyber-text"
            >
              {t('btn.cancel')}
            </button>
            <button
              type="submit"
              className="flex-1 px-4 py-3 text-[14px] font-semibold text-cyber-text transition-all hover:bg-cyber-text/10"
            >
              {t(backingUp ? 'backup.action' : 'backup.restore')}
            </button>
          </div>
        </form>
      </div>
    </div>,
    document.body
  );
}
export function ConfigBackupActions() {
  const { t } = useI18n();
  const { phase, choose, cancel } = useConfigBackupStore();
  const importRunning = useAutoImportStore((state) => state.running);
  useEffect(() => () => cancel(), [cancel]);
  const disabled = phase !== 'idle' || importRunning;
  return (
    <div className="inline-flex flex-wrap items-center gap-1 font-normal text-[12px] text-cyber-text-secondary">
      <button
        type="button"
        disabled={disabled}
        onClick={() => void choose('backup', t('backup.directoryTitle'))}
        className="rounded bg-cyber-accent/10 px-2.5 py-1 text-[13px] text-cyber-accent transition-colors hover:bg-cyber-accent/15"
      >
        {t('backup.action')}
      </button>
      <button
        type="button"
        disabled={disabled}
        onClick={() => void choose('restore', t('backup.fileTitle'))}
        className="rounded bg-cyber-accent/10 px-2.5 py-1 text-[13px] text-cyber-accent transition-colors hover:bg-cyber-accent/15"
      >
        {t('backup.restore')}
      </button>
      {phase === 'password' && <BackupPasswordDialog />}
    </div>
  );
}

export function ConfigBackupStatus() {
  const { t } = useI18n();
  const { phase, mode, outcome, error } = useConfigBackupStore();
  return (
    <>
      {' '}
      {phase === 'working' && (
        <span role="status">{t(mode === 'backup' ? 'backup.backingUp' : 'backup.restoring')}</span>
      )}
      {phase === 'idle' && outcome && (
        <span role="status">
          {t(mode === 'backup' ? 'backup.saved' : 'backup.restored')
            .replace('{count}', String(outcome.records))
            .replace('{duplicates}', String(outcome.duplicates))
            .replace('{skipped}', String(outcome.skipped))}
          {mode === 'restore' && outcome.restartRequired && <> {t('backup.restartRequired')}</>}
          {mode === 'backup' && outcome.deviceBound > 0 && (
            <> {t('backup.deviceBound').replace('{count}', String(outcome.deviceBound))}</>
          )}
        </span>
      )}
      {phase === 'idle' && error && (
        <span role="alert" className="text-red-500">
          {t(errorKey(error, mode))}
        </span>
      )}
    </>
  );
}
