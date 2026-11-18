import { Check, FileJson, KeyRound, LoaderCircle, Pointer, UserRound } from 'lucide-react';
import { useI18n } from '../hooks/useI18n';
import { useAutoImportStore } from '../stores/autoImportStore';
import { useConfigBackupStore } from '../stores/configBackupStore';
import { DesktopPrimaryButton } from './DesktopPrimaryButton';

export function ImportGuideDemo() {
  const { t } = useI18n();
  const { running, progress, error } = useAutoImportStore();
  return (
    <>
      <div className="import-guide-demo" aria-hidden="true">
        <div className="import-guide-source">
          <FileJson size={24} />
        </div>
        <div className="import-guide-source">
          <KeyRound size={24} />
        </div>
        <div className="import-guide-source">
          <UserRound size={24} />
        </div>
        <div className="import-guide-destination">
          <img src="/brand/bird.png" alt="" />
        </div>
      </div>
      <div className="import-guide-action">
        <DesktopPrimaryButton
          className="relative"
          onClick={() => {
            if (useConfigBackupStore.getState().phase !== 'idle') return;
            void useAutoImportStore.getState().start();
          }}
          aria-busy={running || undefined}
        >
          <span
            className="relative inline-flex items-center justify-center gap-2"
            aria-live="polite"
          >
            {running ? (
              <LoaderCircle size={19} className="motion-safe:animate-spin" />
            ) : progress && !error ? (
              <Check size={19} />
            ) : null}
            {running
              ? t('autoImport.scanning')
              : error
                ? t('guide.retryImport')
                : progress
                  ? t('guide.importDone')
                  : t('settings.autoImport')}
            {!running && (!progress || error) && (
              <span className="import-guide-hand" aria-hidden="true">
                <Pointer size={22} />
              </span>
            )}
          </span>
        </DesktopPrimaryButton>
      </div>
    </>
  );
}
