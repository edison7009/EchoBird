import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { useI18n } from '../hooks/useI18n';
import { useConfigBackupStore } from '../stores/configBackupStore';
import { useAutoImportStore } from '../stores/autoImportStore';
import type { TKey } from '../i18n';

function errorKey(error: string): TKey {
  if (error.includes('passwordLength')) return 'backup.passwordLength';
  if (error.includes('password')) return 'backup.passwordInvalid';
  if (/format|version|size/.test(error)) return 'backup.invalidFile';
  if (error.includes('changed')) return 'backup.changed';
  if (error.includes('credentials')) return 'backup.credentials';
  if (error.includes('busy')) return 'backup.busy';
  return 'backup.failed';
}
function BackupPasswordDialog() {
  const { t } = useI18n();
  const { challenge, error, cancel, run } = useConfigBackupStore();
  const [password, setPassword] = useState('');
  const [confirmation, setConfirmation] = useState('');
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
        className="relative w-[400px] max-w-[90vw] rounded-xl border border-cyber-border bg-cyber-bg p-5 shadow-2xl"
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
        <h2 id="backup-password-title" className="text-[16px] font-semibold text-cyber-text">
          {t(backingUp ? 'backup.setPassword' : 'backup.enterPassword')}
        </h2>
        <form
          className="mt-4 space-y-3"
          onSubmit={(event) => {
            event.preventDefault();
            if (!password || (backingUp && Array.from(password).length < 8)) {
              setInvalid('backup.passwordLength');
              return;
            }
            if (backingUp && password !== confirmation) {
              setInvalid('backup.passwordMismatch');
              return;
            }
            const secret = password;
            setPassword('');
            setConfirmation('');
            setInvalid(null);
            void run(secret);
          }}
        >
          <label className="block text-[13px] text-cyber-text-secondary">
            {t('backup.password')}
            <input
              ref={input}
              type="password"
              value={password}
              maxLength={256}
              autoComplete={backingUp ? 'new-password' : 'off'}
              onChange={(event) => {
                setPassword(event.target.value);
                setInvalid(null);
              }}
              className="mt-1 w-full rounded-md border border-cyber-border bg-cyber-input px-3 py-2 text-[14px] text-cyber-text"
            />
          </label>
          {backingUp && (
            <label className="block text-[13px] text-cyber-text-secondary">
              {t('backup.confirmPassword')}
              <input
                type="password"
                value={confirmation}
                maxLength={256}
                autoComplete="new-password"
                onChange={(event) => {
                  setConfirmation(event.target.value);
                  setInvalid(null);
                }}
                className="mt-1 w-full rounded-md border border-cyber-border bg-cyber-input px-3 py-2 text-[14px] text-cyber-text"
              />
            </label>
          )}
          {backingUp && (
            <p className="text-[12px] text-cyber-text-muted">{t('backup.passwordHint')}</p>
          )}
          {(invalid || error) && (
            <p role="alert" className="text-[12px] text-red-500">
              {t(invalid || errorKey(error!))}
            </p>
          )}
          <div className="flex justify-end gap-5 pt-2 text-[13px]">
            <button
              type="button"
              onClick={cancel}
              className="text-cyber-text-secondary hover:text-cyber-text"
            >
              {t('btn.cancel')}
            </button>
            <button type="submit" className="text-cyber-accent hover:text-cyber-accent-secondary">
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
        className="rounded bg-cyber-accent/10 px-2.5 py-1 text-[13px] text-cyber-accent transition-colors hover:bg-cyber-accent/15 disabled:opacity-50"
      >
        {t('backup.action')}
      </button>
      <button
        type="button"
        disabled={disabled}
        onClick={() => void choose('restore', t('backup.fileTitle'))}
        className="rounded bg-cyber-accent/10 px-2.5 py-1 text-[13px] text-cyber-accent transition-colors hover:bg-cyber-accent/15 disabled:opacity-50"
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
          {mode === 'backup' && outcome.deviceBound > 0 && (
            <> {t('backup.deviceBound').replace('{count}', String(outcome.deviceBound))}</>
          )}
        </span>
      )}
      {phase === 'idle' && error && (
        <span role="alert" className="text-red-500">
          {t(errorKey(error))}
        </span>
      )}
    </>
  );
}
