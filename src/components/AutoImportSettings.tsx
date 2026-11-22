import { ConfigBackupActions, ConfigBackupStatus } from './ConfigBackupActions';
import { useConfigBackupStore } from '../stores/configBackupStore';
import { Download } from 'lucide-react';
import { DesktopPrimaryButton } from './DesktopPrimaryButton';
import { ScanProgress } from './ScanProgress';
import { useI18n } from '../hooks/useI18n';
import { useAutoImportStore } from '../stores/autoImportStore';

export function AutoImportSettings() {
  const { t } = useI18n();
  const { running, progress, error, start } = useAutoImportStore();
  const backupBusy = useConfigBackupStore((state) => state.phase !== 'idle');
  const backupFeedback = useConfigBackupStore(
    (state) =>
      state.phase === 'working' || (state.phase === 'idle' && !!(state.outcome || state.error))
  );
  const skipped = progress?.issues.reduce((count, issue) => count + issue.count, 0) ?? 0;
  const details = progress && (
    <>
      {t('autoImport.models')}: {progress.models} · {t('autoImport.accounts')}: {progress.accounts}
      {' · '}
      {t('autoImport.duplicates')}: {progress.duplicates}
      {skipped > 0 && (
        <>
          {' · '}
          {t('autoImport.skipped')}: {skipped}
        </>
      )}
    </>
  );
  return (
    <section
      aria-label={t('autoImport.title')}
      className="grid grid-cols-[minmax(0,1fr)_10rem] grid-rows-[auto_auto] items-start gap-x-4 gap-y-1 px-2"
    >
      <div className="col-start-1 row-start-1 flex self-end items-center gap-2 text-[14px] font-medium text-cyber-text-secondary">
        <Download size={14} className="shrink-0" />
        <span>{t('autoImport.title')}</span>
        <ConfigBackupActions />
      </div>
      {running ? (
        <ScanProgress
          layout="settings"
          progress={progress?.total ? progress.completed / progress.total : null}
          ariaLabel={t('settings.autoImport')}
          label={
            progress?.total ? (
              <>
                {t('autoImport.scanning')} {Math.min(progress.completed + 1, progress.total)}/
                {progress.total}
              </>
            ) : null
          }
          connectingLabel={t('autoImport.scanning')}
        />
      ) : (
        <>
          <div className="col-start-2 row-span-2 row-start-1 h-14 w-40">
            <DesktopPrimaryButton
              className="settings-import-button"
              disabled={backupBusy}
              onClick={() => {
                useConfigBackupStore.setState({ outcome: null, error: null });
                void start();
              }}
            >
              {t('settings.autoImport')}
            </DesktopPrimaryButton>
          </div>
          <p
            role={error ? 'alert' : progress ? 'status' : undefined}
            className={`col-start-1 row-start-2 min-h-5 pl-[22px] text-[12px] leading-5 ${error ? 'text-red-500' : 'text-cyber-text-secondary'}`}
          >
            {backupFeedback ? (
              <ConfigBackupStatus />
            ) : error ? (
              <>
                {t('autoImport.failed')}
                {details && <> · {details}</>}
              </>
            ) : (
              details || t('autoImport.description')
            )}
          </p>
        </>
      )}
    </section>
  );
}
