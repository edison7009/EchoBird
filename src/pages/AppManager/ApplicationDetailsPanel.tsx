import { useEffect, useRef, useState, type ReactNode } from 'react';
import * as api from '../../api/tauri';
import type { LocalTool } from '../../api/types';
import { DesktopToolImage, toolDisplayName } from '../../components/DesktopToolIcon';
import { useI18n } from '../../hooks/useI18n';
import { useSystemApplicationsStore } from '../../stores/systemApplicationsStore';
import { useNavigationStore } from '../../stores/navigationStore';

// Inventory objects change on refresh, so cached details expire with their snapshot.
const detailsCache = new WeakMap<LocalTool, api.SystemApplicationDetails>();
const detailRequests = new WeakMap<LocalTool, Promise<api.SystemApplicationDetails>>();

function fileSizeLabel(size: number, locale: string) {
  const unit = Math.min(3, Math.floor(Math.log(Math.max(1, size)) / Math.log(1024)));
  const value = new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(
    size / 1024 ** unit
  );
  return `${value} ${['B', 'KiB', 'MiB', 'GiB'][unit]}`;
}

export function useApplicationDetails(tool: LocalTool | null | undefined) {
  const [result, setResult] = useState<{
    tool: LocalTool;
    data?: api.SystemApplicationDetails;
    error?: string;
  } | null>(null);
  if (result && result.tool !== tool) setResult(null);
  useEffect(() => {
    if (!tool?.systemAppId || detailsCache.has(tool)) return;
    let active = true;
    let request = detailRequests.get(tool);
    if (!request) {
      request = api.getSystemApplicationDetails(tool.systemAppId).then(
        (data) => {
          detailsCache.set(tool, data);
          detailRequests.delete(tool);
          return data;
        },
        (error) => {
          detailRequests.delete(tool);
          throw error;
        }
      );
      detailRequests.set(tool, request);
    }
    void request.then(
      (data) => {
        if (active) setResult({ tool, data });
      },
      (error) => {
        if (active) setResult({ tool, error: String(error) });
      }
    );
    return () => {
      active = false;
    };
  }, [tool]);
  const current = result?.tool === tool ? result : null;
  const data = (tool && detailsCache.get(tool)) || current?.data;
  const path = tool?.detectedPath;
  const separator = path ? Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\')) : -1;
  const managedDirectory =
    !tool?.systemAppId && path && /^(?:[A-Za-z]:[\\/]|\/|\\\\)/.test(path)
      ? path.slice(0, separator + 1)
      : null;
  return {
    data,
    error: current?.error,
    loading: !!tool?.systemAppId && !data && !current?.error,
    directory: data?.directory || managedDirectory || null,
    configurationPaths: [...new Set(data?.configPaths ?? [])],
  };
}

export function ApplicationDetailsPanel({
  tool,
  state,
  children,
}: {
  tool: LocalTool;
  state: ReturnType<typeof useApplicationDetails>;
  children?: ReactNode;
}) {
  const { t, locale } = useI18n();
  const { data: details, loading, error } = state;
  const path = details?.path || (!tool.systemAppId ? tool.detectedPath : null);
  const version = details?.version || tool.version;
  const stableLayout = !!tool.systemAppId;
  const value = (content: ReactNode) => content ?? '—';

  return (
    <section className="px-3 pb-4 text-sm" aria-busy={loading || undefined}>
      <div className="flex justify-center py-6">
        <DesktopToolImage tool={tool} className="h-16 w-16" />
      </div>
      {children}
      <dl className="space-y-5 text-[13px]">
        {(stableLayout || version) && (
          <div className={`flex justify-between gap-4 ${stableLayout ? 'h-5' : ''}`}>
            <dt className="text-cyber-text-secondary">{t('plugins.version')}</dt>
            <dd className={`min-w-0 break-all text-right ${stableLayout ? 'overflow-y-auto' : ''}`}>
              {value(version)}
            </dd>
          </div>
        )}
        {(stableLayout || details?.fileSize != null) && (
          <div className={`flex justify-between gap-4 ${stableLayout ? 'h-5' : ''}`}>
            <dt className="text-cyber-text-secondary">{t('aiDesktop.fileSize')}</dt>
            <dd className="shrink-0">
              {value(details?.fileSize != null ? fileSizeLabel(details.fileSize, locale) : null)}
            </dd>
          </div>
        )}
        {(stableLayout || details?.modifiedAt != null) && (
          <div className="space-y-2">
            <dt className="text-cyber-text-secondary">{t('aiDesktop.modifiedAt')}</dt>
            <dd className={stableLayout ? 'h-5 overflow-y-auto' : undefined}>
              {value(
                details?.modifiedAt != null
                  ? new Date(details.modifiedAt).toLocaleString(locale)
                  : null
              )}
            </dd>
          </div>
        )}
        {(stableLayout || path) && (
          <div className="space-y-2">
            <dt className="text-cyber-text-secondary">{t('aiDesktop.fileLocation')}</dt>
            <dd className="select-text break-all leading-relaxed">{value(path)}</dd>
          </div>
        )}
        {(stableLayout || (details && details.launchTarget !== path)) && (
          <div className="space-y-2">
            <dt className="text-cyber-text-secondary">{t('aiDesktop.launchTarget')}</dt>
            <dd className="select-text break-all leading-relaxed">
              {value(details?.launchTarget)}
            </dd>
          </div>
        )}
        {state.configurationPaths.length > 0 && (
          <div className="space-y-2">
            <dt className="text-cyber-text-secondary">{t('aiDesktop.configurationLocations')}</dt>
            <dd className="select-text space-y-2 break-all leading-relaxed">
              {state.configurationPaths.map((location) => (
                <p key={location}>{location}</p>
              ))}
            </dd>
          </div>
        )}
      </dl>
      {error && (
        <p role="status" className="mt-4 break-words text-xs text-red-400">
          {error}
        </p>
      )}
    </section>
  );
}

export function ApplicationActions({
  tool,
  directory,
  path,
  launchTarget,
  configurationPaths = [],
  disabled,
}: {
  tool: LocalTool;
  directory?: string | null;
  path?: string | null;
  launchTarget?: string | null;
  configurationPaths?: string[];
  disabled: boolean;
}) {
  const { t, locale } = useI18n();
  const activePage = useNavigationStore((state) => state.activePage);
  const appsTab = useNavigationStore((state) => state.appsTab);
  const goToMother = useNavigationStore((state) => state.goToMother);
  const inMyApplications = useSystemApplicationsStore((state) =>
    state.myApplicationIds.includes(tool.systemAppId ?? '')
  );
  const setInMyApplications = useSystemApplicationsStore((state) => state.setInMyApplications);
  const [folderError, setFolderError] = useState<{ id: string; message: string } | null>(null);
  const detectedPath = tool.detectedPath || tool.path;
  const shellTarget = detectedPath?.startsWith('shell:') ? detectedPath : null;
  const filePath = path || (!shellTarget ? detectedPath : null);
  const target = launchTarget || tool.launchUri || shellTarget;
  const buttonClassName = `flex-1 rounded-lg border border-cyber-border px-3 py-2 text-[13px] text-cyber-text-secondary transition-colors enabled:hover:bg-cyber-elevated enabled:hover:text-cyber-text ${tool.systemAppId ? '' : 'disabled:opacity-40'}`;
  const uninstallAttempt = useRef(0);
  const uninstallPending = useRef(false);
  useEffect(() => {
    uninstallAttempt.current += 1;
    uninstallPending.current = false;
    return () => {
      uninstallAttempt.current += 1;
      uninstallPending.current = false;
    };
  }, [tool, activePage, appsTab]);
  const uninstallMessage = (paths: string[]) =>
    [
      t('mother.hintUninstall').replace('{agent}', () => toolDisplayName(tool, locale)),
      filePath && `${t('aiDesktop.fileLocation')} ${filePath}`,
      target && target !== filePath && `${t('aiDesktop.launchTarget')} ${target}`,
      paths.length > 0 &&
        `${t('aiDesktop.configurationLocations')}\n${[...new Set(paths)].map((location) => JSON.stringify(location)).join('\n')}\n${t('aiDesktop.configurationLocationsHint')}`,
    ]
      .filter(Boolean)
      .join('\n');
  if (
    activePage !== 'apps' ||
    appsTab === 'install' ||
    !tool.installed ||
    ['reversi', 'translator', 'claudeduet'].includes(tool.id)
  )
    return null;
  return (
    <div className="mb-6">
      {folderError?.id === tool.id && (
        <p role="status" className="mb-3 break-words text-xs text-red-400">
          {folderError.message}
        </p>
      )}
      <div className="flex gap-2">
        <button
          type="button"
          disabled={disabled || !directory}
          onClick={() => {
            if (!directory) return;
            setFolderError(null);
            void api
              .openFolder(directory)
              .catch((error) => setFolderError({ id: tool.id, message: String(error) }));
          }}
          className={buttonClassName}
        >
          {t('aiDesktop.openLocation')}
        </button>
        <button
          type="button"
          disabled={disabled}
          onClick={async () => {
            if (disabled || uninstallPending.current) return;
            if (tool.systemAppId) {
              goToMother(uninstallMessage(configurationPaths));
              return;
            }
            uninstallPending.current = true;
            const attempt = uninstallAttempt.current;
            let paths: string[] = [];
            try {
              paths = await api.getToolConfigurationPaths(tool.id);
            } catch {
              // Path lookup is best effort; the assistant can still investigate the app.
            }
            if (attempt !== uninstallAttempt.current) return;
            uninstallPending.current = false;
            goToMother(uninstallMessage(paths));
          }}
          className={buttonClassName}
        >
          {t('aiDesktop.thoroughUninstall')}
        </button>
      </div>
      {tool.systemAppId && !tool.defaultInMyApplications && (
        <button
          type="button"
          className={`${buttonClassName} mt-2 w-full`}
          disabled={disabled}
          aria-pressed={inMyApplications}
          onClick={() => {
            if (disabled || !tool.systemAppId) return;
            setInMyApplications(tool.systemAppId, !inMyApplications);
          }}
        >
          {t(inMyApplications ? 'aiDesktop.removeFromMy' : 'aiDesktop.addToMy')}
        </button>
      )}
    </div>
  );
}
