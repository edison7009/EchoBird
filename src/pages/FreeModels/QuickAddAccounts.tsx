import { useEffect, useLayoutEffect, useId, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Plus, ListPlus, Box, X, ChevronDown } from 'lucide-react';
import { accountProxySources, useAccountProviderFilter } from '../../hooks/useAccountCapability';
import {
  accountProviderIconSrc,
  accountModelPickerOptions,
  cacheAccountProxyModels,
  getAccountProxyModels,
  getCachedAccountProxyModels,
  listProxyAccounts,
  type ProxyAccount,
} from '../../api/accountProxy';
import type { AccountSource, ModelConfig } from '../../api/types';
import * as modelsApi from '../../api/models';
import { getModelIcon } from '../../components/cards/ModelCard';
import { PageActionButton } from '../../components/PageActionButton';
import { CapabilitySelection } from '../AppManager/CapabilityToggles';
import { accountProxyIdentityLabel, accountProxyPlanLabel } from '../AppManager/accountPlanLabel';
import { ACCOUNT_NAME_MASK, useAccountPrivacyStore } from '../../stores/accountPrivacyStore';
import { useNavigationStore } from '../../stores/navigationStore';
import { useI18n } from '../../hooks/useI18n';
import { useFreeModels } from './provider';
import {
  createCustomRouterGroup,
  MAX_ROUTER_GROUPS,
  ROUTER_GROUP_LIMIT,
  type CustomSwitchChoice,
} from '../../api/customRouterGroup';
import { SMART_ROUTER_SWITCH_TOOLS, isSmartRouterId } from '../../utils/smartRouter';

interface Channel {
  key: string;
  name: string;
  icon: string;
  account?: ProxyAccount;
  apiModels?: ModelConfig[];
  models: string[];
  loading: boolean;
  failed: boolean;
  column?: 0 | 1;
  estimatedModels?: number;
}
const sourceKey = (source: AccountSource) => JSON.stringify([source.provider, source.accountId]);
const choiceKey = (source: AccountSource, model: string) =>
  JSON.stringify([source.provider, source.accountId, model]);

const apiInput = (model: { baseUrl: string; anthropicUrl?: string; apiKey?: string }) => ({
  baseUrl: model.baseUrl.trim().replace(/\/+$/, ''),
  anthropicUrl: model.anthropicUrl?.trim().replace(/\/+$/, '') ?? '',
  apiKey: model.apiKey?.trim() ?? '',
});
const apiBinding = (model: { baseUrl: string; anthropicUrl?: string; apiKey?: string }) =>
  JSON.stringify(apiInput(model));
const channelChoiceKey = (row: Channel, id: string) =>
  row.account ? choiceKey(row.account.source, id) : JSON.stringify([row.key, id]);
function apiChannels(models: ModelConfig[]): Channel[] {
  const groups = new Map<string, Channel>();
  for (const model of models) {
    if (
      model.accountSource ||
      model.internalId === 'local-server' ||
      isSmartRouterId(model.internalId) ||
      (model.scope && model.scope !== 'modelCenter') ||
      model.modelType === 'DEMO' ||
      model.modelType === 'LOCAL' ||
      (!model.baseUrl.trim() && !model.anthropicUrl?.trim())
    )
      continue;
    const binding = apiBinding(model);
    const group = groups.get(binding);
    if (group) group.apiModels!.push(model);
    else
      groups.set(binding, {
        key: `api:${model.internalId}`,
        name: model.name,
        icon: '',
        apiModels: [model],
        models: [],
        loading: true,
        failed: false,
      });
  }
  return [...groups.values()];
}

function placeChannels(rows: Channel[]) {
  const heights = [0, 0];
  return rows.map((row) => {
    const cached = row.account ? getCachedAccountProxyModels(row.account.source) : undefined;
    const estimatedModels = cached?.length ?? 6;
    const column: 0 | 1 = heights[0] <= heights[1] ? 0 : 1;
    if (!row.failed) heights[column] += 64 + Math.max(1, Math.ceil(estimatedModels / 3)) * 32;
    return { ...row, column, estimatedModels };
  });
}
function ModelChoicesSkeleton({ count = 6 }: { count?: number }) {
  return (
    <div aria-hidden="true" className="flex flex-wrap gap-2 motion-safe:animate-pulse">
      {Array.from({ length: Math.min(12, Math.max(1, count)) }, (_, index) => (
        <div
          key={index}
          className={`h-6 rounded-full bg-cyber-border/30 ${index % 3 === 0 ? 'w-24' : index % 3 === 1 ? 'w-28' : 'w-20'}`}
        />
      ))}
    </div>
  );
}
function QuickAddSkeleton({ label }: { label: string }) {
  return (
    <div role="status" aria-label={label} className="grid grid-cols-1 gap-x-12 md:grid-cols-2">
      {[0, 1].map((column) => (
        <div key={column}>
          {[0, 1].map((index) => (
            <div key={index} aria-hidden="true" className="py-4">
              <div className="mb-3 flex h-5 items-center gap-2 motion-safe:animate-pulse">
                <div className="h-5 w-5 rounded bg-cyber-border/50" />
                <div className="h-4 w-48 max-w-[70%] rounded bg-cyber-border/50" />
              </div>
              <ModelChoicesSkeleton />
            </div>
          ))}
        </div>
      ))}
    </div>
  );
}

function QuickAddChannelIcon({ row }: { row: Channel }) {
  const api = row.apiModels?.[0];
  const path = api
    ? getModelIcon('', api.baseUrl || api.anthropicUrl || '') || getModelIcon(row.name)
    : accountProviderIconSrc(row.icon);
  const [failedPath, setFailedPath] = useState<string | null>(null);
  return path && failedPath !== path ? (
    <img
      src={path}
      alt=""
      className="h-5 w-5 shrink-0 object-contain"
      onError={() => setFailedPath(path)}
    />
  ) : (
    <Box size={20} className="shrink-0 text-cyber-text-secondary" aria-hidden="true" />
  );
}

interface DesktopSwitchTarget {
  id: string;
  name: string;
}
export function CustomSwitchEntry({ toolId, toolName }: { toolId: string; toolName: string }) {
  if (!SMART_ROUTER_SWITCH_TOOLS.has(toolId)) return null;
  return <QuickAddAccounts desktopTool={{ id: toolId, name: toolName }} />;
}
export function QuickAddAccounts({ desktopTool }: { desktopTool?: DesktopSwitchTarget } = {}) {
  const { activeRouterId, groupPending, activeDragId, routers = [] } = useFreeModels();
  const page = useNavigationStore((state) => state.activePage);
  const tab = useNavigationStore((state) => state.smartAccessTab);
  const appsTab = useNavigationStore((state) => state.appsTab);
  if (!routers.length || routers.length >= MAX_ROUTER_GROUPS) return null;
  return (
    <QuickAddControl
      key={JSON.stringify([activeRouterId, page, tab, appsTab, desktopTool?.id])}
      disabled={groupPending || activeDragId != null}
      desktopTool={desktopTool}
    />
  );
}
function QuickAddControl({
  disabled,
  desktopTool,
}: {
  disabled: boolean;
  desktopTool?: DesktopSwitchTarget;
}) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  return (
    <>
      {desktopTool ? (
        <button
          type="button"
          disabled={disabled}
          onClick={() => setOpen(true)}
          className="mb-2 flex h-14 w-full items-center justify-center gap-2 rounded-card border border-dashed border-cyber-border text-sm text-cyber-text-secondary enabled:hover:bg-cyber-elevated disabled:opacity-50"
        >
          <Plus size={20} />
          {t('agent.addCustomSwitch')}
        </button>
      ) : (
        <PageActionButton
          aria-label={t('freeModels.quickAdd')}
          icon={Plus}
          disabled={disabled}
          onClick={() => setOpen(true)}
        >
          {t('freeModels.quickAdd')}
        </PageActionButton>
      )}
      {open && <QuickAddDialog desktopTool={desktopTool} onClose={() => setOpen(false)} />}
    </>
  );
}

function QuickAddDialog({
  onClose,
  desktopTool,
}: {
  onClose: () => void;
  desktopTool?: DesktopSwitchTarget;
}) {
  const { t } = useI18n();
  const available = useAccountProviderFilter();
  const { customModels, activeRouterId, selectRouter } = useFreeModels();
  const initialRouter = useRef(activeRouterId);
  const page = useNavigationStore((state) => state.activePage);
  const tab = useNavigationStore((state) => state.smartAccessTab);
  const privacy = useAccountPrivacyStore((state) => state.hidden);
  const [channels, setChannels] = useState<Channel[]>([]);
  const [loading, setLoading] = useState(true);
  const [selected, setSelected] = useState(new Set<string>());
  const [collapsed, setCollapsed] = useState(new Set<string>());
  const channelId = useId();
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const alive = useRef(true);
  const savingRef = useRef(false);
  const dialog = useRef<HTMLDivElement>(null);
  const initializedAccounts = useRef(new Set<string>());
  const origin = useRef({ page, tab, appsTab: useNavigationStore.getState().appsTab });
  const isCurrent = () =>
    alive.current &&
    useNavigationStore.getState().activePage === origin.current.page &&
    (desktopTool
      ? useNavigationStore.getState().appsTab === origin.current.appsTab
      : useNavigationStore.getState().smartAccessTab === origin.current.tab);
  useLayoutEffect(() => {
    alive.current =
      activeRouterId === initialRouter.current &&
      (desktopTool ? page === 'apps' : page === 'freeModels' && tab === 'configuration');
    return () => {
      alive.current = false;
    };
  }, [activeRouterId, page, tab, desktopTool]);
  useEffect(() => {
    const previous = document.activeElement;
    dialog.current?.focus();
    return () => {
      if (previous instanceof HTMLElement && previous.isConnected) previous.focus();
    };
  }, []);
  useEffect(() => {
    let cancelled = false;
    const providers = accountProxySources().filter(available);
    const update = (key: string, patch: Partial<Channel>) => {
      if (!cancelled)
        setChannels((rows) => rows.map((row) => (row.key === key ? { ...row, ...patch } : row)));
    };
    void (async () => {
      setLoading(true);
      const [results, savedModels] = await Promise.all([
        Promise.allSettled(providers.map((p) => listProxyAccounts(p.id))),
        modelsApi.getModels().catch(() => []),
      ]);
      if (cancelled) return;
      const rows = results.flatMap((result, index): Channel[] => {
        const p = providers[index];
        return result.status === 'fulfilled'
          ? result.value.map((account) => ({
              key: sourceKey(account.source),
              name: p.name,
              icon: p.icon,
              account,
              models: [],
              loading: true,
              failed: false,
            }))
          : [{ key: p.id, name: p.name, icon: p.icon, models: [], loading: false, failed: true }];
      });
      rows.push(...apiChannels(savedModels));
      const placedRows = placeChannels(rows);
      setChannels(placedRows);
      setLoading(false);
      const queue = placedRows.filter((row) => row.account || row.apiModels);
      let cursor = 0;
      await Promise.all(
        Array.from({ length: Math.min(4, queue.length) }, async () => {
          while (!cancelled && cursor < queue.length) {
            const row = queue[cursor++];
            try {
              let options: string[];
              if (row.account) {
                const account = row.account;
                const cached = getCachedAccountProxyModels(account.source);
                const models = cached ?? (await getAccountProxyModels(account.source));
                if (cancelled) return;
                cacheAccountProxyModels(account.source, models);
                options = accountModelPickerOptions(models, '', account.modelLabels);
              } else {
                const input = apiInput(row.apiModels![0]);
                const savedIds = row.apiModels!.flatMap((model) =>
                  model.modelId?.trim() ? [model.modelId] : []
                );
                try {
                  const catalog = await modelsApi.fetchUpstreamModelCatalog(input);
                  options = [...catalog.models.map((model) => model.id), ...savedIds];
                } catch {
                  const cached = await modelsApi
                    .getCachedUpstreamModelCatalog(input)
                    .catch(() => null);
                  options = [...(cached?.models.map((model) => model.id) ?? []), ...savedIds];
                  if (!options.length) throw new Error('No available API models');
                }
              }
              if (cancelled) return;
              options = [...new Set(options)].filter((id) => id.trim());
              update(row.key, { models: options, loading: false });
              if (!initializedAccounts.current.has(row.key)) {
                initializedAccounts.current.add(row.key);
                const defaults = options.slice(0, 2);
                setSelected(
                  (selection) =>
                    new Set([...selection, ...defaults.map((id) => channelChoiceKey(row, id))])
                );
              }
            } catch {
              update(row.key, { loading: false, failed: true });
            }
          }
        })
      );
      // These providers publish display labels while loading their directory, as in AccountModelFields.
      const labelProviders = new Set([
        'qoder',
        'qoderCn',
        'kiro',
        'doubaowork',
        'dola',
        'workBuddy',
        'workBuddyAi',
        'kilo',
        'cline',
        'zcode',
      ]);
      await Promise.allSettled(
        providers
          .filter(
            (p) => labelProviders.has(p.id) && rows.some((r) => r.account?.source.provider === p.id)
          )
          .map(async (p) => {
            if (cancelled) return;
            const accounts = await listProxyAccounts(p.id);
            if (!cancelled)
              setChannels((current) =>
                current.map((row) => {
                  const account = accounts.find((a) => sourceKey(a.source) === row.key);
                  return account ? { ...row, account } : row;
                })
              );
          })
      );
    })();
    return () => {
      cancelled = true;
    };
  }, [available]);
  const close = () => {
    if (!savingRef.current) onClose();
  };
  const visibleChannels = channels.filter((row) => !row.failed);
  const catalogsPending = loading || channels.some((row) => row.loading);
  const confirm = async () => {
    if (savingRef.current || selected.size === 0 || catalogsPending) return;
    savingRef.current = true;
    setSaving(true);
    setError(null);
    try {
      const choices: CustomSwitchChoice[] = [];
      for (const row of channels) {
        if (!row.account && !row.apiModels) continue;
        for (const modelId of row.models) {
          if (!selected.has(channelChoiceKey(row, modelId))) continue;
          const source = row.account?.source;
          const api = row.apiModels?.[0];
          const input = {
            name: row.account ? accountProxyIdentityLabel(row.account, t) : row.name,
            baseUrl: api?.baseUrl ?? '',
            anthropicUrl: api?.anthropicUrl ?? '',
            apiKey: api?.apiKey ?? '',
            modelId,
            accountSource: source,
            scope: 'smartRouter' as const,
          };
          const existingApi = row.apiModels?.find((model) => model.modelId === modelId);
          const accountModel = source
            ? customModels.find(
                (model) =>
                  model.accountSource &&
                  sourceKey(model.accountSource) === sourceKey(source) &&
                  model.modelId === modelId
              )
            : undefined;
          const existing =
            existingApi ??
            (accountModel
              ? { ...input, internalId: accountModel.internalId, scope: accountModel.scope }
              : undefined);
          choices.push({ existing, input });
        }
      }
      const group = await createCustomRouterGroup(
        choices,
        t('freeModels.router.title'),
        isCurrent,
        desktopTool ? `${desktopTool.name} · ${t('agent.customSwitch')}` : undefined
      );
      if (!isCurrent()) return;
      onClose();
      await selectRouter(group.id);
    } catch (error) {
      if (alive.current) setError(String(error).includes(ROUTER_GROUP_LIMIT) ? 'limit' : 'failed');
    } finally {
      savingRef.current = false;
      if (alive.current) setSaving(false);
    }
  };
  return createPortal(
    <div
      className="fixed inset-0 z-[9998] flex items-center justify-center"
      onKeyDown={(event) => {
        if (event.key === 'Escape') {
          event.stopPropagation();
          close();
        }
        if (event.key === 'Tab') {
          const controls =
            dialog.current?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)');
          const first = controls?.[0],
            last = controls?.[controls.length - 1];
          if (
            event.shiftKey &&
            (document.activeElement === first || document.activeElement === dialog.current)
          ) {
            event.preventDefault();
            last?.focus();
          } else if (!event.shiftKey && document.activeElement === last) {
            event.preventDefault();
            first?.focus();
          }
        }
      }}
    >
      <div className="absolute inset-0" onClick={close} />
      <div
        ref={dialog}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-labelledby="quick-add-title"
        className="relative flex h-[90vh] w-[90vw] flex-col overflow-hidden rounded-xl border border-cyber-border/30 bg-cyber-surface shadow-2xl outline-none"
      >
        <div className="flex items-center justify-between px-6 pb-4 pt-6 md:px-12 md:pt-12">
          <h2
            id="quick-add-title"
            className="flex items-center gap-2 text-base font-bold text-cyber-text"
          >
            <ListPlus size={18} className="text-cyber-text-secondary" aria-hidden="true" />
            {t('freeModels.selectSwitchModels')}
          </h2>
          <button
            type="button"
            disabled={saving}
            onClick={close}
            aria-label={t('btn.close')}
            className="close-button text-cyber-text-secondary disabled:opacity-50"
          >
            <X size={18} />
          </button>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto px-6 py-2 md:px-12">
          {loading && <QuickAddSkeleton label={t('accountCenter.loading')} />}
          {!loading && visibleChannels.length === 0 && (
            <p className="col-span-full py-6 text-sm text-cyber-text-secondary">
              {t(
                channels.length === 0 ? 'freeModels.quickAdd.empty' : 'freeModels.quickAdd.noModels'
              )}
            </p>
          )}
          <div className="grid grid-cols-1 items-start gap-x-12 md:grid-cols-2">
            {([0, 1] as const).map((column) => (
              <div key={column} className="min-w-0" data-quick-add-column={column}>
                {visibleChannels
                  .filter((row) => row.column === column)
                  .map((row) => {
                    const choices = row.models;
                    const isCollapsed = collapsed.has(row.key);
                    const modelsId = `${channelId}-channel-${channels.indexOf(row)}`;
                    return (
                      <section
                        key={row.key}
                        className="min-w-0 py-4 [&_.capability-toggle]:max-w-full [&_.capability-toggle]:truncate [&_.capability-toggle[aria-pressed='false']]:border-dashed"
                      >
                        <button
                          type="button"
                          disabled={saving}
                          aria-expanded={!isCollapsed}
                          aria-controls={modelsId}
                          onClick={() =>
                            setCollapsed((current) => {
                              const next = new Set(current);
                              if (next.has(row.key)) next.delete(row.key);
                              else next.add(row.key);
                              return next;
                            })
                          }
                          className={`flex w-full flex-wrap items-center gap-x-3 gap-y-2 rounded-button text-left text-sm text-cyber-text outline-none focus-visible:ring-2 focus-visible:ring-cyber-accent disabled:opacity-50 ${isCollapsed ? '' : 'mb-3'}`}
                        >
                          <span className="flex min-w-0 flex-1 basis-full items-center gap-2">
                            <QuickAddChannelIcon row={row} />
                            <span
                              className={
                                row.apiModels
                                  ? 'min-w-0 truncate font-semibold'
                                  : 'shrink-0 font-semibold'
                              }
                            >
                              {row.name}
                            </span>
                            {row.apiModels && (
                              <span className="min-w-0 truncate text-cyber-text-secondary">
                                {row.apiModels[0].baseUrl || row.apiModels[0].anthropicUrl}
                              </span>
                            )}
                            {row.account && (
                              <>
                                <span className="min-w-0 truncate text-cyber-text-secondary">
                                  {privacy
                                    ? ACCOUNT_NAME_MASK
                                    : accountProxyIdentityLabel(row.account, t)}
                                </span>
                                <span className="shrink-0 whitespace-nowrap text-xs text-cyber-text-secondary">
                                  {accountProxyPlanLabel(row.account, t)}
                                </span>
                              </>
                            )}
                            <ChevronDown
                              size={14}
                              aria-hidden="true"
                              className={`ml-auto shrink-0 text-cyber-text-muted ${isCollapsed ? '-rotate-90' : ''}`}
                            />
                          </span>
                        </button>
                        <div id={modelsId} hidden={isCollapsed}>
                          {!isCollapsed &&
                            (row.loading ? (
                              <div role="status" aria-label={t('voice.loading')}>
                                <ModelChoicesSkeleton count={row.estimatedModels} />
                              </div>
                            ) : row.models.length === 0 ? (
                              <p className="text-xs text-cyber-text-secondary">
                                {t('freeModels.quickAdd.noModels')}
                              </p>
                            ) : (
                              <>
                                <CapabilitySelection
                                  disabled={saving}
                                  toggles={choices.map((id) => ({
                                    id,
                                    label: row.account?.modelLabels?.[id] ?? id,
                                    checked: selected.has(channelChoiceKey(row, id)),
                                    onChange: (checked) =>
                                      setSelected((current) => {
                                        const next = new Set(current);
                                        const key = channelChoiceKey(row, id);
                                        if (checked) next.add(key);
                                        else next.delete(key);
                                        return next;
                                      }),
                                  }))}
                                />
                              </>
                            ))}
                        </div>
                      </section>
                    );
                  })}
              </div>
            ))}
          </div>
        </div>
        <div className="shrink-0 border-t border-cyber-border">
          {error && (
            <p role="alert" className="px-6 py-3 text-xs text-red-500">
              {t(error === 'limit' ? 'freeModels.router.limit' : 'freeModels.quickAdd.failed')}
            </p>
          )}
          <div className="flex">
            <button
              type="button"
              disabled={saving}
              onClick={close}
              className="flex-1 border-r border-cyber-border px-4 py-3 text-[14px] font-semibold text-cyber-text-secondary transition-all enabled:hover:bg-cyber-elevated enabled:hover:text-cyber-text disabled:opacity-50"
            >
              {t('btn.cancel')}
            </button>
            <button
              type="button"
              disabled={saving || selected.size === 0 || catalogsPending}
              aria-busy={saving || undefined}
              onClick={() => void confirm()}
              className="flex-1 px-4 py-3 text-[14px] font-semibold text-cyber-text transition-all enabled:hover:bg-cyber-text/10 disabled:text-cyber-text-muted"
            >
              {t(saving ? 'freeModels.quickAdd.saving' : 'common.confirm')}
            </button>
          </div>
        </div>
      </div>
    </div>,
    document.body
  );
}
