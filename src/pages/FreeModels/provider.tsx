import { modelRepository } from '../../stores/modelRepository';
import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useSyncExternalStore,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import { arrayMove } from '@dnd-kit/sortable';
import * as api from '../../api/tauri';
import { listProxyAccounts, type ProxyAccount } from '../../api/accountProxy';
import type { AccountProvider, AccountSource, ModelConfig } from '../../api/types';
import type { FreeModelDirectory, FreeModelEntry } from '../../api/freeModels';
import { useToast } from '../../components/Toast';
import { useI18n } from '../../hooks/useI18n';
import { useNavigationStore } from '../../stores/navigationStore';

const SCAN_STEP_MS = 230;

const SCAN_COMPLETE_MS = 320;

const ROUTER_LAYOUT_KEY = 'echobird-router-layout';

interface FreeModelsContextValue {
  routers: api.SmartRouterConfig[];
  activeRouterId: string;
  activeRouterEnabled: boolean;
  groupPending: boolean;
  selectRouter: (id: string) => Promise<void>;
  saveRouter: (settings: api.SmartRouterSettings) => Promise<void>;
  deleteRouter: (id: string) => Promise<void>;
  setGroupEnabled: (id: string, enabled: boolean) => Promise<void>;
  layout: 'cards' | 'list' | 'terminal';
  setLayout: (layout: 'cards' | 'list' | 'terminal') => void;
  activeDragId: string | null;
  setActiveDragId: (id: string | null) => void;
  catalog: FreeModelDirectory;
  models: FreeModelEntry[];
  customModels: RouteModelNode[];
  proxyAccounts: Partial<Record<AccountProvider, ProxyAccount[]>>;
  selectedIds: Set<string>;
  refreshing: boolean;
  scanProvider: string;
  scanProgress: number;
  routerBaseUrl: string;
  routerEnabled: boolean;
  routerTogglePending: boolean;
  pausedIds: Set<string>;
  candidateTogglePending: Set<string>;
  setCandidateEnabled: (id: string, enabled: boolean) => Promise<void>;
  setRouterEnabled: (enabled: boolean) => Promise<void>;
  refresh: () => Promise<void>;
  addSelectedModel: (model: RouteModelInput) => Promise<void>;
  updateSelectedModel: (model: RouteModelInput) => void;
  changeSelectedModel: (id: string, modelId: string) => Promise<ModelConfig | undefined>;
  savingModelIds: Set<string>;
  removeSelectedModel: (id: string) => Promise<void>;
  reorderSelectedModel: (activeId: string, overId: string) => Promise<void>;
}

interface RouteModelInput {
  internalId: string;
  name: string;
  baseUrl: string;
  anthropicUrl?: string;
  apiKey?: string;
  modelId: string;
  accountSource?: AccountSource;
  scope?: ModelConfig['scope'];
}

export interface RouteModelNode {
  id: string;
  internalId: string;
  provider: string;
  modelId: string;
  baseUrl: string;
  anthropicUrl?: string;
  apiKey?: string;
  accountSource?: AccountSource;
  scope?: ModelConfig['scope'];
}

const FreeModelsContext = createContext<FreeModelsContextValue | null>(null);

const emptyCatalog: FreeModelDirectory = {
  version: 1,
  updatedAt: '',
  models: [],
};

const providerPriority = ['nvidia-nim', 'openrouter'];

export function providerRank(model: FreeModelEntry): number {
  const rank = providerPriority.indexOf(model.providerId);
  return rank === -1 ? providerPriority.length : rank;
}

function groupModelsForScan(models: FreeModelEntry[]): FreeModelEntry[][] {
  const groups = new Map<string, FreeModelEntry[]>();
  models.forEach((model) => {
    const id = `${model.providerId}:${model.baseUrl}`;
    const group = groups.get(id);
    if (group) group.push(model);
    else groups.set(id, [model]);
  });
  return [...groups.values()].sort((left, right) => providerRank(left[0]) - providerRank(right[0]));
}

function wait(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

export function useFreeModels() {
  const value = useContext(FreeModelsContext);
  if (!value) throw new Error('useFreeModels must be used within FreeModelsProvider');
  return value;
}

export function FreeModelsProvider({ children }: { children: ReactNode }) {
  const { t } = useI18n();
  const { showToast } = useToast();
  const activePage = useNavigationStore((state) => state.activePage);
  const [activeDragId, setActiveDragId] = useState<string | null>(null);
  const [layout, setLayoutState] = useState<'cards' | 'list' | 'terminal'>(() => {
    try {
      const saved = localStorage.getItem(ROUTER_LAYOUT_KEY);
      return saved === 'list' || saved === 'terminal' ? saved : 'cards';
    } catch {
      return 'cards';
    }
  });
  const setLayout = useCallback((next: 'cards' | 'list' | 'terminal') => {
    setLayoutState(next);
    try {
      localStorage.setItem(ROUTER_LAYOUT_KEY, next);
    } catch {
      // Keep the choice for this session when local storage is unavailable.
    }
  }, []);
  const [catalog, setCatalog] = useState<FreeModelDirectory>(emptyCatalog);
  const [routeNodes, setCustomModels] = useState<RouteModelNode[]>([]);
  const { entities, removedIds } = useSyncExternalStore(
    modelRepository.subscribe,
    modelRepository.getSnapshot
  );
  const customModels = useMemo(
    () =>
      routeNodes
        .filter((node) => !removedIds.has(node.internalId))
        .map((node) => {
          const model = entities[node.internalId];
          return model
            ? {
                ...node,
                provider: model.name,
                modelId: model.modelId ?? '',
                baseUrl: model.baseUrl,
                anthropicUrl: model.anthropicUrl,
                apiKey: model.apiKey,
                accountSource: model.accountSource,
                scope: model.scope,
              }
            : node;
        }),
    [routeNodes, entities, removedIds]
  );
  const [proxyAccounts, setProxyAccounts] = useState<
    Partial<Record<AccountProvider, ProxyAccount[]>>
  >({});
  const [selectedIds, setSelectedIds] = useState<Set<string>>(() => new Set());
  const [refreshing, setRefreshing] = useState(false);
  const [scanProvider, setScanProvider] = useState('');
  const [scanProgress, setScanProgress] = useState(0);
  const [routerBaseUrl, setRouterBaseUrl] = useState('127.0.0.1:53683/v1');
  const [routerEnabled, setRouterEnabledState] = useState(true);
  const [routers, setRouters] = useState<api.SmartRouterConfig[]>([]);
  const [activeRouterId, setActiveRouterId] = useState('smart-router');
  const activeRouterRef = useRef('smart-router');
  const [groupPending, setGroupPending] = useState(false);
  const groupTransition = useRef(false);
  const routerArgs = (): [] | [string] =>
    activeRouterRef.current === 'smart-router' ? [] : [activeRouterRef.current];
  const [routerTogglePending, setRouterTogglePending] = useState(false);
  const routerTogglePendingRef = useRef(false);
  const [routerLoaded, setRouterLoaded] = useState(false);
  const [pausedIds, setPausedIds] = useState<Set<string>>(() => new Set());
  const [candidateTogglePending, setCandidateTogglePending] = useState<Set<string>>(
    () => new Set()
  );
  const candidateTogglePendingRef = useRef(new Set<string>());
  const [savingModelIds, setSavingModelIds] = useState<Set<string>>(() => new Set());
  const savingModelIdsRef = useRef(new Set<string>());
  const refreshInFlightRef = useRef(false);
  const catalogRefreshedRef = useRef(false);
  const routerMutationRef = useRef<Promise<void>>(Promise.resolve());
  const selectedIdsRef = useRef<Set<string>>(new Set());
  const routerLoadedRef = useRef(false);
  const models = catalog.models;
  const accountProvidersKey = JSON.stringify(
    [
      ...new Set(
        customModels.flatMap((model) => (model.accountSource ? [model.accountSource.provider] : []))
      ),
    ].sort()
  );

  useEffect(() => {
    if (activePage !== 'freeModels') return;
    const providers: AccountProvider[] = JSON.parse(accountProvidersKey);
    if (providers.length === 0) return;
    let ignore = false;
    void Promise.allSettled(providers.map((provider) => listProxyAccounts(provider))).then(
      (results) => {
        if (ignore) return;
        setProxyAccounts((previous) => {
          const next = { ...previous };
          results.forEach((result, index) => {
            if (result.status === 'fulfilled') next[providers[index]] = result.value;
          });
          return next;
        });
      }
    );
    return () => {
      ignore = true;
    };
  }, [activePage, accountProvidersKey]);

  useEffect(() => {
    let cancelled = false;
    void api
      .getCachedFreeModelDirectory()
      .then((cached) => {
        if (!cancelled && !catalogRefreshedRef.current && cached) setCatalog(cached);
      })
      .catch((error) => console.error('Load cached free model directory failed:', error));
    return () => {
      cancelled = true;
    };
  }, []);

  const loadRouter = useCallback(async (preferredId = activeRouterRef.current) => {
    routerLoadedRef.current = false;
    const modelRevision = modelRepository.getSnapshot().revision;
    const groups = await api.listSmartRouters();
    const router = groups.find((group) => group.id === preferredId) ?? groups[0];
    if (!router) throw new Error('No smart router configured');
    const configuredModels = await api.getSmartRouterCandidates(
      ...(router.id === 'smart-router' ? [] : [router.id])
    );
    modelRepository.ingest(configuredModels, modelRevision);
    activeRouterRef.current = router.id;
    setRouters(groups);
    setActiveRouterId(router.id);
    const modelsById = new Map(configuredModels.map((model) => [model.internalId, model]));
    const routeModels = router.candidateIds.flatMap((internalId) => {
      const model = modelsById.get(internalId);
      if (!model) return [];
      return [
        {
          id: internalId,
          internalId,
          provider: model.name,
          modelId: model.modelId ?? '',
          baseUrl: model.baseUrl,
          anthropicUrl: model.anthropicUrl,
          apiKey: model.apiKey,
          accountSource: model.accountSource,
          scope: model.scope,
        },
      ];
    });
    setCustomModels(routeModels);
    // Deleted models have no visible row and must not occupy an addition slot.
    // Keep the local server configured while it is temporarily offline.
    const candidateIds = router.candidateIds.filter(
      (id) => modelsById.has(id) || id === 'local-server'
    );
    const next = new Set(candidateIds);
    selectedIdsRef.current = next;
    setSelectedIds((current) => {
      if (
        current.size === next.size &&
        [...current].every((id, index) => id === candidateIds[index])
      ) {
        return current;
      }
      return next;
    });
    setRouterBaseUrl(router.baseUrl.replace(/^https?:\/\//, ''));
    setRouterEnabledState(router.masterEnabled);
    setPausedIds(new Set(router.disabledCandidateIds ?? []));
    setRouterLoaded(true);
    routerLoadedRef.current = true;
  }, []);

  const changeGroup = useCallback(async (change: () => Promise<void>) => {
    if (groupTransition.current) return;
    groupTransition.current = true;
    setGroupPending(true);
    const operation = routerMutationRef.current.catch(() => undefined).then(change);
    routerMutationRef.current = operation;
    try {
      await operation;
    } finally {
      groupTransition.current = false;
      setGroupPending(false);
    }
  }, []);

  const selectRouter = useCallback(
    async (id: string) => {
      try {
        await changeGroup(() => loadRouter(id));
      } catch (error) {
        console.error('Load smart router failed:', error);
        showToast('error', t('error.requestFailed'));
      }
    },
    [changeGroup, loadRouter, showToast, t]
  );

  const saveRouter = useCallback(
    async (settings: api.SmartRouterSettings) => {
      await changeGroup(async () => {
        const saved = await api.saveSmartRouter(settings);
        try {
          await loadRouter(saved.id);
        } catch (error) {
          console.error('Refresh saved smart router failed:', error);
          showToast('error', t('error.requestFailed'));
        }
      });
    },
    [changeGroup, loadRouter, showToast, t]
  );

  const deleteRouter = useCallback(
    async (id: string) => {
      await changeGroup(async () => {
        await api.deleteSmartRouter(id);
        try {
          await loadRouter();
        } catch (error) {
          console.error('Refresh deleted smart router failed:', error);
          showToast('error', t('error.requestFailed'));
        }
      });
    },
    [changeGroup, loadRouter, showToast, t]
  );

  const setGroupEnabled = useCallback(
    async (id: string, enabled: boolean) => {
      try {
        await changeGroup(async () => {
          const saved = await api.setSmartRouterGroupEnabled(id, enabled);
          setRouters((current) =>
            current.map((router) => (router.id === saved.id ? saved : router))
          );
        });
      } catch (error) {
        console.error('Toggle smart router group failed:', error);
        showToast('error', t('error.requestFailed'));
      }
    },
    [changeGroup, showToast, t]
  );

  useEffect(() => {
    if (routerLoadedRef.current && activePage !== 'freeModels') return;
    // Keep reads and their state updates in the mutation queue, so a late
    // page-entry response cannot overwrite a newer addition or priority order.
    const load = routerMutationRef.current.catch(() => undefined).then(() => loadRouter());
    routerMutationRef.current = load;
    void load.catch((error) => console.error('Load smart router config failed:', error));
  }, [activePage, loadRouter]);

  const setRouterEnabled = useCallback(
    async (enabled: boolean) => {
      if (routerTogglePendingRef.current) return;
      routerTogglePendingRef.current = true;
      setRouterTogglePending(true);
      const change = routerMutationRef.current
        .catch(() => undefined)
        .then(async () => {
          await api.setSmartRouterEnabled(enabled);
          const groups = await api.listSmartRouters();
          const masterEnabled = groups[0].masterEnabled;
          setRouters(groups);
          setRouterEnabledState(masterEnabled);
        });
      routerMutationRef.current = change;
      try {
        await change;
      } catch (error) {
        console.error('Toggle smart router failed:', error);
        showToast('error', t('error.requestFailed'));
      } finally {
        routerTogglePendingRef.current = false;
        setRouterTogglePending(false);
      }
    },
    [showToast, t]
  );

  const setCandidateEnabled = useCallback(
    async (id: string, enabled: boolean) => {
      if (groupTransition.current) return;
      if (candidateTogglePendingRef.current.has(id)) return;
      const routerId = activeRouterRef.current;
      candidateTogglePendingRef.current.add(id);
      setCandidateTogglePending(new Set(candidateTogglePendingRef.current));
      const change = routerMutationRef.current
        .catch(() => undefined)
        .then(async () => {
          if (activeRouterRef.current !== routerId)
            throw new Error('Smart router selection changed');
          const router = await api.setSmartRouterCandidateEnabled(id, enabled, ...routerArgs());
          setPausedIds(new Set(router.disabledCandidateIds ?? []));
        });
      routerMutationRef.current = change;
      try {
        await change;
      } catch (error) {
        console.error('Toggle smart router candidate failed:', error);
        showToast('error', t('error.requestFailed'));
      } finally {
        candidateTogglePendingRef.current.delete(id);
        setCandidateTogglePending(new Set(candidateTogglePendingRef.current));
      }
    },
    [showToast, t]
  );

  const refresh = useCallback(async () => {
    if (refreshInFlightRef.current) return;
    refreshInFlightRef.current = true;
    setScanProvider('');
    setScanProgress(0);
    setRefreshing(true);
    try {
      const remote = await api.getFreeModelDirectory();
      if (!remote) {
        showToast('warning', t('freeModels.fetchFailed'));
        return;
      }

      catalogRefreshedRef.current = true;
      setCatalog({ ...remote, models: [] });
      const groups = groupModelsForScan(remote.models);
      const revealedModels: FreeModelEntry[] = [];
      for (const [index, group] of groups.entries()) {
        setScanProvider(group[0]?.provider ?? '');
        await wait(SCAN_STEP_MS);
        revealedModels.push(...group);
        setCatalog({ ...remote, models: [...revealedModels] });
        setScanProgress((index + 1) / groups.length);
      }
      await wait(SCAN_COMPLETE_MS);
    } catch (error) {
      console.error('Fetch free model directory failed:', error);
      showToast('error', t('freeModels.fetchFailed'));
    } finally {
      refreshInFlightRef.current = false;
      setScanProvider('');
      setScanProgress(0);
      setRefreshing(false);
    }
  }, [showToast, t]);

  const addSelectedModel = useCallback(
    async (model: RouteModelInput) => {
      if (groupTransition.current) throw new Error('Smart router selection is changing');
      const routerId = activeRouterRef.current;
      const id = model.internalId;
      const addition = routerMutationRef.current
        .catch(() => undefined)
        .then(async () => {
          // Retry a failed initial/refresh read before constructing a full-list
          // write. An unread configuration must never be treated as an empty one.
          if (!routerLoadedRef.current) await loadRouter();
          if (activeRouterRef.current !== routerId)
            throw new Error('Smart router selection changed');
          const candidateIds = [...new Set([...selectedIdsRef.current, id])];
          const router = await api.setSmartRouterCandidates(candidateIds, ...routerArgs());
          if (!router.candidateIds.includes(id)) {
            throw new Error(`Smart Router rejected candidate: ${id}`);
          }

          const nextIds = new Set(router.candidateIds);
          selectedIdsRef.current = nextIds;
          setSelectedIds(nextIds);
          setPausedIds(new Set(router.disabledCandidateIds ?? []));
          setCustomModels((current) => [
            ...current.filter((entry) => entry.id !== id),
            {
              id,
              internalId: model.internalId,
              provider: model.name,
              modelId: model.modelId,
              baseUrl: model.baseUrl,
              anthropicUrl: model.anthropicUrl,
              apiKey: model.apiKey,
              accountSource: model.accountSource,
              scope: model.scope,
            },
          ]);
          setRouterBaseUrl(router.baseUrl.replace(/^https?:\/\//, ''));
        });
      routerMutationRef.current = addition;
      await addition;
    },
    [loadRouter]
  );

  const updateSelectedModel = useCallback(
    (model: RouteModelInput) => {
      if (groupTransition.current) return;
      modelRepository.ingest([{ ...model, apiKey: model.apiKey ?? '' }]);
      routerMutationRef.current = routerMutationRef.current
        .catch(() => undefined)
        .then(async () => {
          setCustomModels((current) =>
            current.map((entry) =>
              entry.id === model.internalId
                ? {
                    ...entry,
                    provider: model.name,
                    modelId: model.modelId,
                    baseUrl: model.baseUrl,
                    anthropicUrl: model.anthropicUrl,
                    apiKey: model.apiKey,
                    accountSource: model.accountSource,
                    scope: model.scope ?? entry.scope,
                  }
                : entry
            )
          );
          await loadRouter();
        })
        .catch((error) => console.error('Refresh smart router after model update failed:', error));
    },
    [loadRouter]
  );

  const changeSelectedModel = useCallback(
    async (id: string, modelId: string) => {
      if (groupTransition.current) return;
      if (savingModelIdsRef.current.has(id)) return;
      const routerId = activeRouterRef.current;
      savingModelIdsRef.current.add(id);
      setSavingModelIds(new Set(savingModelIdsRef.current));
      const change = routerMutationRef.current
        .catch(() => undefined)
        .then(async () => {
          if (!routerLoadedRef.current) await loadRouter();
          if (activeRouterRef.current !== routerId)
            throw new Error('Smart router selection changed');
          if (!selectedIdsRef.current.has(id))
            throw new Error(`Smart router model not found: ${id}`);
          const updated = await api.updateModel(id, { modelId });
          if (!updated) throw new Error(`Smart router model not found: ${id}`);
          modelRepository.ingest([updated]);
          setCustomModels((current) =>
            current.map((entry) =>
              entry.id === id ? { ...entry, modelId: updated.modelId ?? modelId } : entry
            )
          );
          return updated;
        });
      routerMutationRef.current = change.then(
        () => undefined,
        () => undefined
      );
      try {
        return await change;
      } finally {
        savingModelIdsRef.current.delete(id);
        setSavingModelIds(new Set(savingModelIdsRef.current));
      }
    },
    [loadRouter]
  );

  const removeSelectedModel = useCallback(
    async (id: string) => {
      if (groupTransition.current) return;
      const routerId = activeRouterRef.current;
      const removal = routerMutationRef.current
        .catch(() => undefined)
        .then(async () => {
          if (!routerLoadedRef.current) await loadRouter();
          if (activeRouterRef.current !== routerId)
            throw new Error('Smart router selection changed');
          const router = await api.removeSmartRouterCandidate(id, ...routerArgs());
          const nextIds = new Set(router.candidateIds);
          selectedIdsRef.current = nextIds;
          setSelectedIds(nextIds);
          setCustomModels((current) => current.filter((model) => model.id !== id));
          setRouterBaseUrl(router.baseUrl.replace(/^https?:\/\//, ''));
          setRouters((groups) => groups.map((group) => (group.id === router.id ? router : group)));
          setPausedIds(new Set(router.disabledCandidateIds ?? []));
        });
      routerMutationRef.current = removal;
      try {
        await removal;
      } catch (error) {
        console.error('Remove smart router candidate failed:', error);
        showToast('error', t('error.requestFailed'));
      }
    },
    [loadRouter, showToast, t]
  );

  const reorderSelectedModel = useCallback(
    async (activeId: string, overId: string) => {
      if (groupTransition.current) return;
      const routerId = activeRouterRef.current;
      const reorder = routerMutationRef.current
        .catch(() => undefined)
        .then(async () => {
          if (!routerLoadedRef.current) await loadRouter();
          if (activeRouterRef.current !== routerId)
            throw new Error('Smart router selection changed');
          const previous = selectedIdsRef.current;
          const ids = [...previous];
          const from = ids.indexOf(activeId);
          const to = ids.indexOf(overId);
          if (from < 0 || to < 0 || from === to) return;
          const candidateIds = arrayMove(ids, from, to);
          setSelectedIds(new Set(candidateIds));
          try {
            const router = await api.setSmartRouterCandidates(candidateIds, ...routerArgs());
            const next = new Set(router.candidateIds);
            selectedIdsRef.current = next;
            setSelectedIds(next);
          } catch (error) {
            setSelectedIds(previous);
            throw error;
          }
        });
      routerMutationRef.current = reorder;
      try {
        await reorder;
      } catch (error) {
        console.error('Save smart router order failed:', error);
        showToast('error', t('error.requestFailed'));
      }
    },
    [loadRouter, showToast, t]
  );

  return (
    <FreeModelsContext.Provider
      value={{
        routers,
        activeRouterId,
        activeRouterEnabled: routers.find((group) => group.id === activeRouterId)?.enabled ?? false,
        groupPending,
        selectRouter,
        saveRouter,
        deleteRouter,
        setGroupEnabled,
        layout,
        setLayout,
        activeDragId,
        setActiveDragId,
        catalog,
        models,
        customModels,
        proxyAccounts,
        selectedIds,
        refreshing,
        scanProvider,
        scanProgress,
        routerBaseUrl,
        routerEnabled,
        routerTogglePending: routerTogglePending || !routerLoaded,
        pausedIds,
        candidateTogglePending,
        setCandidateEnabled,
        setRouterEnabled,
        refresh,
        addSelectedModel,
        updateSelectedModel,
        changeSelectedModel,
        savingModelIds,
        removeSelectedModel,
        reorderSelectedModel,
      }}
    >
      {children}
    </FreeModelsContext.Provider>
  );
}
