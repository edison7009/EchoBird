import { useEffect, useState, type ReactNode } from 'react';
import * as api from '../../api/tauri';
import type { LocalTool } from '../../api/types';
import { DesktopToolImage, toolDisplayName } from '../../components/DesktopToolIcon';
import { useI18n } from '../../hooks/useI18n';
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
            <dd
              className={`select-text break-all leading-relaxed ${stableLayout ? 'h-16 overflow-y-auto' : ''}`}
            >
              {value(path)}
            </dd>
          </div>
        )}
        {(stableLayout || (details && details.launchTarget !== path)) && (
          <div className="space-y-2">
            <dt className="text-cyber-text-secondary">{t('aiDesktop.launchTarget')}</dt>
            <dd
              className={`select-text break-all text-xs leading-relaxed text-cyber-text-secondary ${stableLayout ? 'h-14 overflow-y-auto' : ''}`}
            >
              {value(details?.launchTarget)}
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
  disabled,
}: {
  tool: LocalTool;
  directory?: string | null;
  path?: string | null;
  launchTarget?: string | null;
  disabled: boolean;
}) {
  const { t, locale } = useI18n();
  const activePage = useNavigationStore((state) => state.activePage);
  const appsTab = useNavigationStore((state) => state.appsTab);
  const goToMother = useNavigationStore((state) => state.goToMother);
  const [folderError, setFolderError] = useState<{ id: string; message: string } | null>(null);
  const detectedPath = tool.detectedPath || tool.path;
  const shellTarget = detectedPath?.startsWith('shell:') ? detectedPath : null;
  const filePath = path || (!shellTarget ? detectedPath : null);
  const target = launchTarget || tool.launchUri || shellTarget;
  const buttonClassName = `flex-1 rounded-lg border border-cyber-border px-3 py-2 text-[13px] text-cyber-text-secondary transition-colors enabled:hover:bg-cyber-elevated enabled:hover:text-cyber-text ${tool.systemAppId ? '' : 'disabled:opacity-40'}`;
  const uninstallMessage = [
    t('mother.hintUninstall').replace('{agent}', () => toolDisplayName(tool, locale)),
    filePath && `${t('aiDesktop.fileLocation')} ${filePath}`,
    target && target !== filePath && `${t('aiDesktop.launchTarget')} ${target}`,
  ]
    .filter(Boolean)
    .join('\n');
  if (
    activePage !== 'apps' ||
    appsTab === 'install' ||
    !tool.installed ||
    tool.plugin ||
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
          onClick={() => goToMother(uninstallMessage)}
          className={buttonClassName}
        >
          {t('btn.uninstall')}
        </button>
      </div>
    </div>
  );
}
