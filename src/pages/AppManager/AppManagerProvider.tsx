import { isSmartRouterId } from '../../utils/smartRouter';
import { isDirectModelAllowedForTool } from '../../utils/directModelPolicy';
import { accountError } from '../../utils/accountError';
import { useApplicationLauncher } from '../../features/accounts/useApplicationLauncher';
import { AccountDirectoryContext } from '../../features/accounts/context';
import { useAccountDirectory } from '../../features/accounts/useAccountDirectory';
import { useModelRepository } from '../../hooks/useModelRepository';
import { pluginToolProviders } from '../../hooks/useAccountPlugin';
import { ClaudeCodeLoginDialog } from './ClaudeCodeLoginDialog';
import React, { useState, useEffect, useLayoutEffect, useCallback, useRef, useMemo } from 'react';
import { useI18n } from '../../hooks/useI18n';
import * as api from '../../api/tauri';
import { AppManagerContext } from './context';
import { useToolsStore } from '../../stores/toolsStore';
import { useNavigationStore } from '../../stores/navigationStore';
import { useFreeModels } from '../FreeModels/provider';
import { accountCenterProviders } from '../AccountCenter/accountCenterData';
import { useRewardClaimStore } from '../../stores/rewardClaimStore';
import { useOptionalToast } from '../../components/Toast';
import { getOfficialEndpoint, isOfficialModelSentinel } from '../../data/officialEndpoints';
import { pluginTools, usePluginsStore } from '../../stores/pluginsStore';

// ===== Provider =====

interface AppManagerProviderProps {
  children: React.ReactNode;
}

export const AppManagerProvider: React.FC<AppManagerProviderProps> = ({ children }) => {
  const { t, locale } = useI18n();
  const showToast = useOptionalToast();
  const [accountCardOrder, setAccountCardOrder] = useState<string[]>(() => {
    try {
      const raw = localStorage.getItem('echobird_account_card_order');
      const saved: unknown = raw ? JSON.parse(raw) : [];
      return Array.isArray(saved) ? saved.filter((id): id is string => typeof id === 'string') : [];
    } catch {
      return [];
    }
  });
  const accountOrderIndex = new Map(accountCardOrder.map((id, index) => [id, index]));
  const orderedAccounts = <A extends { id: string }>(provider: string, accounts: A[]) =>
    [...accounts].sort(
      (a, b) =>
        (accountOrderIndex.get(`${provider}:${a.id}`) ?? Infinity) -
        (accountOrderIndex.get(`${provider}:${b.id}`) ?? Infinity)
    );

  // From stores (replaces drilled props)
  const {
    detectedTools: scannedTools,
    setDetectedTools,
    isScanning,
    scanTools,
    usingCachedTools,
  } = useToolsStore();
  const installedPlugins = usePluginsStore((state) => state.installed);
  const loadPlugins = usePluginsStore((state) => state.load);
  const developerMode = useNavigationStore((state) => state.developerMode);
  const appsTab = useNavigationStore((state) => state.appsTab);
  const pluginProviders = useMemo(
    () => pluginToolProviders(installedPlugins, developerMode),
    [installedPlugins, developerMode]
  );
  const detectedTools = useMemo(
    () => [
      ...scannedTools.map((tool) => {
        const icon = pluginProviders.find((entry) => entry.application === tool.id)?.icon;
        return icon ? { ...tool, icon } : tool;
      }),
      ...(developerMode ? pluginTools(installedPlugins) : []),
    ],
    [scannedTools, installedPlugins, developerMode, pluginProviders]
  );
  const pluginToolIds = pluginProviders.map((entry) => entry.application).join();
  const scannedPluginTools = useRef(pluginToolIds);
  useEffect(() => {
    if (scannedPluginTools.current === pluginToolIds) return;
    scannedPluginTools.current = pluginToolIds;
    void scanTools();
  }, [pluginToolIds, scanTools]);
  useEffect(() => {
    if (developerMode) void loadPlugins();
  }, [developerMode, loadPlugins]);
  const { activePage, goToMother } = useNavigationStore();
  const isActive = activePage === 'apps' && (appsTab === 'desktop' || appsTab === 'install');

  // Wrapped navigation: build prefill and go to Mother Agent (model check happens there)
  const handleGoToMother = useCallback(
    async (_toolId: string, toolName: string) => {
      const prefill = t('mother.hintInstall').replace('{agent}', toolName);
      goToMother(prefill);
    },
    [t, goToMother]
  );

  const [toolModelConfig, setToolModelConfig] = useState<Record<string, string | null>>({
    claudecode: null,
    openclaw: null,
    opencode: null,
    codex: null,
    hermes: null,
    zcode: null,
  });

  // Reload application models on entry and after model changes.
  const {
    models: userModels,
    loaded: modelsLoaded,
    reload: reloadModels,
  } = useModelRepository(undefined, isActive);
  const { routerEnabled, routers } = useFreeModels();
  const routerIdsKey = JSON.stringify(routers?.map((router) => router.id) ?? []);
  useEffect(() => {
    if (!isActive || !modelsLoaded) return;
    const configuredRouterIds: string[] = JSON.parse(routerIdsKey);
    let cancelled = false;
    queueMicrotask(() => {
      if (cancelled) return;
      setToolModelConfig((previous) => {
        const removed = Object.keys(previous).filter(
          (id) =>
            previous[id] &&
            !isOfficialModelSentinel(previous[id]!) &&
            (isSmartRouterId(previous[id]!) && routers !== undefined
              ? !configuredRouterIds.includes(previous[id]!)
              : !userModels.some((model) => model.internalId === previous[id]))
        );
        if (!removed.length) return previous;
        return { ...previous, ...Object.fromEntries(removed.map((id) => [id, null])) };
      });
    });
    return () => {
      cancelled = true;
    };
  }, [isActive, modelsLoaded, userModels, routerIdsKey, routers]);

  // AI-installable IDs from bundled install/index.json (offline-first).
  const [aiInstallableIds, setAiInstallableIds] = useState<string[]>([]);
  useEffect(() => {
    if (!isActive) return;
    api
      .getInstallIndex()
      .then((s) => {
        try {
          const data = JSON.parse(s);
          if (Array.isArray(data?.ids)) setAiInstallableIds(data.ids);
        } catch {
          /* malformed — keep empty */
        }
      })
      .catch(() => {
        /* IPC error — keep empty */
      });
  }, [isActive]);

  // Internalized state
  const [selectedToolId, setSelectedTool] = useState<string | null>(null);
  const selectedTool =
    (isActive && ['reversi', 'translator'].includes(selectedToolId ?? '')) ||
    (selectedToolId?.startsWith('plugin:') &&
      !detectedTools.some((tool) => tool.id === selectedToolId))
      ? null
      : selectedToolId;
  const [isLaunching, setIsLaunching] = useState(false);
  const launchGeneration = useRef(0);
  const previewLaunch = useRef(false);
  const [applyError, setApplyError] = useState<string | null>(null);
  useEffect(
    () =>
      useNavigationStore.subscribe((state, previous) => {
        if (previous.developerMode && !state.developerMode && previewLaunch.current) {
          launchGeneration.current += 1;
          previewLaunch.current = false;
          setIsLaunching(false);
          setApplyError(null);
        }
      }),
    []
  );
  // Bottom-bar checkbox states are persisted across sessions — users get tired of
  // re-checking the same boxes every launch. Default both to true so picking an app
  // and clicking the big button "just launches it"; users who only want to rewrite
  // config without launching can uncheck the launch box.
  const readBool = (key: string, fallback: boolean): boolean => {
    try {
      const v = localStorage.getItem(key);
      return v === null ? fallback : v === 'true';
    } catch {
      return fallback;
    }
  };
  const writeBool = (key: string, v: boolean) => {
    try {
      localStorage.setItem(key, String(v));
    } catch {
      /* private mode */
    }
  };
  const [launchAfterApply, setLaunchAfterApplyRaw] = useState<boolean>(() =>
    readBool('echobird_appmgr_launch_after', true)
  );
  const setLaunchAfterApply = (v: boolean) => {
    setLaunchAfterApplyRaw(v);
    writeBool('echobird_appmgr_launch_after', v);
  };
  const [agreedConfigPolicy, setAgreedConfigPolicyRaw] = useState<boolean>(() =>
    readBool('echobird_appmgr_apply_config', true)
  );
  const setAgreedConfigPolicy = (v: boolean) => {
    setAgreedConfigPolicyRaw(v);
    writeBool('echobird_appmgr_apply_config', v);
  };
  const [codexWebSearch, setCodexWebSearchRaw] = useState<boolean>(() =>
    readBool('echobird_codex_web_search', true)
  );
  const [claudeDesktopWebSearch, setClaudeDesktopWebSearch] = useState<boolean>(() =>
    readBool('echobird_claudedesktop_web_search', true)
  );
  const [claudeCodeWebSearch, setClaudeCodeWebSearch] = useState<boolean>(() =>
    readBool('echobird_claudecode_web_search', true)
  );
  const [zcodeWebSearch, setZCodeWebSearch] = useState<boolean>(() =>
    readBool('echobird_zcode_web_search', false)
  );
  const [zcodeImageInput, setZCodeImageInput] = useState<boolean>(() =>
    readBool('echobird_zcode_image_input', true)
  );
  const [workBuddyCapabilities, setWorkBuddyCapabilities] = useState(() => ({
    workbuddy: {
      imageInput: readBool('echobird_workbuddy_image_input', true),
      oneMContext: readBool('echobird_workbuddy_1m_mode', false),
    },
    workbuddyai: {
      imageInput: readBool('echobird_workbuddyai_image_input', true),
      oneMContext: readBool('echobird_workbuddyai_1m_mode', false),
    },
  }));
  const [mimoCapabilities, setMimoCapabilities] = useState(() => ({
    mimocode: {
      imageInput: readBool('echobird_mimocode_image_input', true),
      oneMContext: readBool('echobird_mimocode_1m_mode', false),
    },
    mimodesktop: {
      imageInput: readBool('echobird_mimodesktop_image_input', true),
      oneMContext: readBool('echobird_mimodesktop_1m_mode', false),
    },
  }));
  const [minimaxCapabilities, setMinimaxCapabilities] = useState(() => ({
    imageInput: readBool('echobird_minimax_image_input', true),
    oneMContext: readBool('echobird_minimax_1m_mode', false),
  }));
  const [qoderCapabilities, setQoderCapabilities] = useState(() => ({
    qoder: {
      imageInput: readBool('echobird_qoder_image_input', true),
      oneMContext: readBool('echobird_qoder_1m_mode', false),
    },
    qodercn: {
      imageInput: readBool('echobird_qodercn_image_input', true),
      oneMContext: readBool('echobird_qodercn_1m_mode', false),
    },
  }));
  const [dshCapabilities, setDshCapabilities] = useState(() => ({
    imageInput: readBool('echobird_dsh_image_input', true),
    oneMContext: readBool('echobird_dsh_1m_mode', false),
  }));
  // Claude Code 1M-context toggle. When on,
  // apply_claudecode appends `[1m]` to the model id (MODEL / OPUS / SONNET / FABLE env
  // vars only — HAIKU + SUBAGENT stay bare) so Claude Code budgets the 1M
  // window. CC strips the suffix before sending upstream, so the provider
  // still sees the bare id. Default off: only opt in for an upstream model
  // that supports 1M. Persisted under its own localStorage key
  // (echobird_claudecode_1m_mode) — deliberately not reusing the v5.3.8
  // echobird_claude_1m_mode key, which shared semantics with Claude Desktop.
  const [claude1mMode, setClaude1mModeRaw] = useState<boolean>(() =>
    readBool('echobird_claudecode_1m_mode', false)
  );
  const [claudeDesktop1mMode, setClaudeDesktop1mModeRaw] = useState<boolean>(() =>
    readBool('echobird_claudedesktop_1m_mode', false)
  );
  const viewMode = useNavigationStore((state) => state.toolTab);
  const setViewMode = (mode: 'desktop' | 'install') => {
    useNavigationStore.getState().setAppsTab(mode);
    if (mode === viewMode) return;
    setSelectedTool(null);
    setApplyError(null);
  };
  const mimoTool =
    selectedTool === 'mimocode' || selectedTool === 'mimodesktop' ? selectedTool : null;
  const mimoValues = mimoCapabilities[mimoTool ?? 'mimodesktop'];
  const selectedToolData = detectedTools.find(
    (t) => t.id === selectedTool && (!isActive || t.installed === (viewMode === 'desktop'))
  );
  const {
    clineSelectionRevisionRef,
    accountNavigation,
    pluginAvailable,
    isCodexTool,
    codexManaged,
    claudeCodeAccounts,
    claudeDesktopAccounts,
    workBuddyEdition,
    workBuddyCnAccounts,
    workBuddyAiAccounts,
    workBuddyAccounts,
    traeCnAccounts,
    traeGlobalAccounts,
    traeAccounts,
    kiroAccounts,
    minimaxAccounts,
    qoderEdition,
    qoderGlobalAccounts,
    qoderCnAccounts,
    qoderAccounts,
    dumateAccounts,
    doubaoEdition,
    doubaoWorkAccounts,
    dolaAccounts,
    doubaoAccounts,
    doubaoSelectionRevisionRef,
    qwenWorkEdition,
    qwenWorkGlobalAccounts,
    qwenWorkCnAccounts,
    qwenWorkAccounts,
    deepSeekAccounts,
    grokAccounts,
    manusAccounts,
    cueAccounts,
    clineAccounts,
    kiloAccounts,
    copilotAccounts,
    grokBotAccounts,
    cursorAccounts,
    cursorRevisionRef,
    antigravityAccounts,
    kimiSelectionRevisionRef,
    pluginAccountProvider,
    pluginAccountTool,
    pluginAccounts,
    kimiAccounts,
    zcodeAccounts,
    codeArtsAccounts,
    codeArtsSelectionRevisionRef,
    devinAccounts,
    devinRevisionRef,
    openDesignAccounts,
    codexAccounts,
    selectedCodexAccountId,
    selectCodexAccount,
    isLoadingCodexAccounts,
    codexAccountsLoaded,
    codexAccountsReadFailed,
    codexAccountsMutationRevision,
    isAddingCodexAccount,
    codexOAuthRemainingSeconds,
    refreshingCodexAccountIds,
    codexAuthorizationFailedIds,
    loadCodexAccounts,
    addCodexAccount,
    cancelCodexLogin,
    deleteCodexAccount,
    refreshCodexAccountQuota,
  } = useAccountDirectory({
    selectedTool,
    activePage,
    isActive,
    developerMode,
    appsTab,
    detectedTools,
    installedPlugins,
    pluginProviders,
    usingCachedTools,
    toolModelConfig,
    setToolModelConfig,
    setApplyError,
  });

  const [isRefreshingSelectedTool, setIsRefreshingSelectedTool] = useState(false);
  const [selectedToolAction, setSelectedToolAction] = useState<'claim' | 'warmUp' | null>(null);
  const modelsRefreshTime = useRef<number | null>(null);
  const toolActionPending = useRef(false);
  const toolActionScope = useRef({ key: '' });
  const scopeKey = `${activePage}:${selectedTool}`;
  useLayoutEffect(() => {
    toolActionScope.current = { key: scopeKey };
    return () => {
      toolActionScope.current = { key: 'left' };
    };
  }, [scopeKey]);
  type RefreshAccount = { id: string };
  type RefreshGroup = {
    accounts: RefreshAccount[];
    reload: () => Promise<unknown>;
    refresh: (
      account: RefreshAccount,
      operation?: undefined,
      onError?: (error: unknown) => void
    ) => Promise<unknown>;
  };
  const refreshSelectedTool = useCallback(async () => {
    if (!selectedTool || !isActive || toolActionPending.current) return;
    if (!useRewardClaimStore.getState().tryStart()) return;
    toolActionPending.current = true;
    const scope = toolActionScope.current;
    const current = () => toolActionScope.current === scope;
    const now = Date.now();
    setIsRefreshingSelectedTool(true);
    const refreshModels =
      modelsRefreshTime.current === null || now - modelsRefreshTime.current >= 30_000;
    const accountGroup = (() => {
      switch (selectedTool) {
        case 'codex':
        case 'chatgptdesktop':
          return codexManaged as unknown as RefreshGroup;
        case 'claudecode':
          return claudeCodeAccounts as unknown as RefreshGroup;
        case 'claudedesktop':
          return claudeDesktopAccounts as unknown as RefreshGroup;
        case 'workbuddy':
        case 'workbuddyai':
          return workBuddyAccounts as unknown as RefreshGroup;
        case 'trae':
        case 'traecn':
          return traeAccounts as unknown as RefreshGroup;
        case 'kirocrew':
        case 'kirocli':
          return kiroAccounts as unknown as RefreshGroup;
        case 'minimaxcode':
        case 'minimaxdesktop':
          return minimaxAccounts as unknown as RefreshGroup;
        case 'qoder':
        case 'qodercn':
          return qoderAccounts as unknown as RefreshGroup;
        case 'dumate':
          return dumateAccounts as unknown as RefreshGroup;
        case 'doubaowork':
        case 'dola':
          return doubaoAccounts as unknown as RefreshGroup;
        case 'qwenwork':
        case 'qwenworkcn':
          return qwenWorkAccounts as unknown as RefreshGroup;
        case 'dsh':
          return deepSeekAccounts as unknown as RefreshGroup;
        case 'grok':
          return grokAccounts as unknown as RefreshGroup;
        case 'manus':
          return manusAccounts as unknown as RefreshGroup;
        case 'cue':
          return cueAccounts as unknown as RefreshGroup;
        case 'cline':
        case 'clinedesktop':
          return clineAccounts as unknown as RefreshGroup;
        case 'kilo':
        case 'kilodesktop':
          return kiloAccounts as unknown as RefreshGroup;
        case 'copilot':
        case 'copilotdesktop':
          return copilotAccounts as unknown as RefreshGroup;
        case 'grokbot':
          return grokBotAccounts as unknown as RefreshGroup;
        case 'cursor':
        case 'cursorcli':
          return cursorAccounts as unknown as RefreshGroup;
        case 'antigravity':
        case 'antigravitydesktop':
          return antigravityAccounts as unknown as RefreshGroup;
        case 'kimi':
        case 'kimicode':
        case 'kimidesktop':
          return kimiAccounts as unknown as RefreshGroup;
        case 'zcode':
          return zcodeAccounts as unknown as RefreshGroup;
        case 'codearts':
          return codeArtsAccounts as unknown as RefreshGroup;
        case 'devin':
        case 'devincli':
          return devinAccounts as unknown as RefreshGroup;
        case 'opendesign':
          return openDesignAccounts as unknown as RefreshGroup;
        default:
          if (selectedTool === pluginAccountProvider?.application)
            return pluginAccounts as unknown as RefreshGroup;
          return undefined;
      }
    })();
    const refreshAccounts = async () => {
      if (!accountGroup) return;
      const listed = await accountGroup.reload();
      if (!current()) return;
      const accounts = Array.isArray(listed) ? (listed as RefreshAccount[]) : accountGroup.accounts;
      let failed = 0;
      let completed = 0;
      // The managed lifecycle reports errors through its callback and limits
      // concurrency for some providers. Serial execution avoids silently skipping rows.
      for (const account of accounts) {
        if (!current()) break;
        let error = false;
        try {
          await accountGroup.refresh(account, undefined, () => {
            error = true;
          });
        } catch {
          error = true;
        }
        completed += 1;
        if (error) failed += 1;
      }
      if (failed && current()) {
        showToast(
          'warning',
          t('accountCenter.batchRefreshSummary')
            .replace('{success}', String(completed - failed))
            .replace('{failed}', String(failed))
        );
      }
    };
    try {
      const models = async () => {
        if (!refreshModels) return;
        let usageResult: Promise<boolean> | undefined;
        const results = await Promise.allSettled([
          reloadModels(),
          (async () => {
            window.dispatchEvent(
              new CustomEvent('echobird-refresh-model-usage', {
                detail: {
                  respond: (result: Promise<boolean>) => {
                    usageResult = result;
                  },
                },
              })
            );
            return usageResult ? await usageResult : false;
          })(),
        ]);
        if (results[0].status === 'rejected' && current()) {
          setApplyError(accountError(results[0].reason, t));
        }
        if (
          results[0].status === 'fulfilled' &&
          results[1].status === 'fulfilled' &&
          results[1].value === true
        ) {
          modelsRefreshTime.current = Date.now();
        }
      };
      const results = await Promise.allSettled([models(), refreshAccounts()]);
      const failure = results.find((result) => result.status === 'rejected');
      if (failure?.status === 'rejected' && current())
        setApplyError(accountError(failure.reason, t));
    } catch (error) {
      if (current()) setApplyError(accountError(error, t));
    } finally {
      toolActionPending.current = false;
      useRewardClaimStore.getState().finish();
      setIsRefreshingSelectedTool(false);
    }
  }, [
    selectedTool,
    isActive,
    reloadModels,
    codexManaged,
    claudeCodeAccounts,
    claudeDesktopAccounts,
    workBuddyAccounts,
    traeAccounts,
    kiroAccounts,
    minimaxAccounts,
    qoderAccounts,
    dumateAccounts,
    doubaoAccounts,
    qwenWorkAccounts,
    deepSeekAccounts,
    grokAccounts,
    manusAccounts,
    cueAccounts,
    clineAccounts,
    kiloAccounts,
    copilotAccounts,
    grokBotAccounts,
    cursorAccounts,
    antigravityAccounts,
    kimiAccounts,
    zcodeAccounts,
    codeArtsAccounts,
    devinAccounts,
    openDesignAccounts,
    pluginAccountProvider,
    pluginAccounts,
    showToast,
    t,
  ]);

  // Set tool model (single selection) - UI state update
  const handleSelectModel = (toolId: string, modelId: string) => {
    if (toolId === 'codearts') {
      codeArtsSelectionRevisionRef.current += 1;
      codeArtsAccounts.select(null);
    }
    if (toolId === 'cline' || toolId === 'clinedesktop') {
      clineSelectionRevisionRef.current += 1;
      clineAccounts.select(null);
    }
    if (toolId === 'minimaxcode' || toolId === 'minimaxdesktop') minimaxAccounts.select(null);
    if (toolId === 'doubaowork') doubaoWorkAccounts.select(null);
    if (toolId === 'dola') dolaAccounts.select(null);
    if (toolId === 'qoder' || toolId === 'qodercn') qoderAccounts.select(null);
    if (toolId === 'zcode') zcodeAccounts.select(null);
    if (toolId === 'kimicode' || toolId === 'kimidesktop') kimiAccounts.select(null);
    if (toolId === pluginAccounts.provider?.application) pluginAccounts.select(null);
    if (toolId === 'opendesign') openDesignAccounts.select(null);
    if (toolId === 'cursor' || toolId === 'cursorcli') cursorAccounts.select(null);
    if (toolId === 'grokbot') grokBotAccounts.select(null);
    if (toolId === 'dsh') deepSeekAccounts.select(null);
    if (toolId === 'grok') grokAccounts.select(null);
    if (toolId === 'manus') manusAccounts.select(null);
    if (toolId === 'cue') cueAccounts.select(null);
    if (toolId === 'kilo' || toolId === 'kilodesktop') kiloAccounts.select(null);
    if (toolId === 'copilot' || toolId === 'copilotdesktop') copilotAccounts.select(null);
    if (toolId === 'claudecode') claudeCodeAccounts.setSelectedId(null);
    if (toolId === 'claudedesktop') claudeDesktopAccounts.select(null);
    if (toolId === 'workbuddy' || toolId === 'workbuddyai') workBuddyAccounts.select(null);
    if (toolId === 'codex' || toolId === 'chatgptdesktop') {
      codexManaged.select(null);
    }
    setToolModelConfig((prev) => ({
      ...prev,
      [toolId]: modelId,
    }));
  };

  // Apply model config to backend (internalized from App.tsx).
  const applyModelConfig = async (
    toolId: string,
    internalId: string,
    oneMOverride?: boolean,
    webSearchOverride?: boolean,
    imageInputOverride?: boolean
  ): Promise<true | string | false> => {
    const model = userModels.find((m) => m.internalId === internalId);
    if (!model) {
      console.error('Model not found:', internalId);
      return false;
    }

    if (!isDirectModelAllowedForTool(model, toolId)) return t('agent.directModelUnavailable');

    const toolData = detectedTools.find((t) => t.id === toolId);
    const toolProtocols = toolData?.apiProtocol || ['openai'];

    // Default to the protocol the model's URL actually speaks — NOT toolProtocols[0].
    // For a single-URL model, defaulting to toolProtocols[0] would
    // write an OpenAI-only model as @ai-sdk/anthropic against an OpenAI URL (or an
    // Anthropic-only model as openai-compatible), 404-ing every call at runtime.
    // Only a both-URL model falls back to toolProtocols[0].
    const modelHasBoth = !!(model.baseUrl && model.anthropicUrl);
    const defaultProtocol = modelHasBoth
      ? toolProtocols[0] === 'anthropic'
        ? 'anthropic'
        : 'openai'
      : model.anthropicUrl
        ? 'anthropic'
        : 'openai';
    const selectedProtocol = defaultProtocol;

    const useAnthropicUrl = selectedProtocol === 'anthropic' && model.anthropicUrl;
    const apiUrl = useAnthropicUrl ? model.anthropicUrl! : model.baseUrl;

    // eslint-disable-next-line no-console
    console.debug(
      `[AppManager] Applying model to ${toolId}: protocol=${selectedProtocol}, url=${apiUrl}`
    );

    // Claude apps connect directly to the selected Anthropic endpoint.
    const isClaudeDesktopApp = toolId === 'claudedesktop';
    const isClaudeCodeApp = toolId === 'claudecode';
    const isClaudeApp = isClaudeDesktopApp || isClaudeCodeApp;
    const effective1m = isClaudeDesktopApp
      ? (oneMOverride ?? claudeDesktop1mMode)
      : isClaudeCodeApp && (oneMOverride ?? claude1mMode);

    try {
      const result = await api.applyModelToTool(toolId, {
        id: model.internalId,
        name: model.name,
        baseUrl: apiUrl,
        apiKey: model.apiKey,
        model: model.modelId || '',
        protocol: selectedProtocol,
        ...(isClaudeApp
          ? {
              oneMContext: effective1m,
              webSearch:
                webSearchOverride ??
                (isClaudeDesktopApp ? claudeDesktopWebSearch : claudeCodeWebSearch),
            }
          : {}),
        ...(['codex', 'chatgptdesktop'].includes(toolId)
          ? {
              webSearch: webSearchOverride ?? codexWebSearch,
            }
          : {}),
        ...(toolId === 'zcode'
          ? {
              webSearch: webSearchOverride ?? zcodeWebSearch,
              imageInput: imageInputOverride ?? zcodeImageInput,
            }
          : {}),
        ...(toolId === 'workbuddy' || toolId === 'workbuddyai'
          ? {
              imageInput: imageInputOverride ?? workBuddyCapabilities[toolId].imageInput,
              oneMContext: oneMOverride ?? workBuddyCapabilities[toolId].oneMContext,
            }
          : {}),
        ...(toolId === 'mimocode' || toolId === 'mimodesktop'
          ? {
              imageInput: imageInputOverride ?? mimoCapabilities[toolId].imageInput,
              oneMContext: oneMOverride ?? mimoCapabilities[toolId].oneMContext,
            }
          : {}),
        ...(toolId === 'minimaxcode' || toolId === 'minimaxdesktop'
          ? {
              imageInput: imageInputOverride ?? minimaxCapabilities.imageInput,
              oneMContext: oneMOverride ?? minimaxCapabilities.oneMContext,
            }
          : {}),
        ...(toolId === 'qoder' || toolId === 'qodercn'
          ? {
              imageInput: imageInputOverride ?? qoderCapabilities[toolId].imageInput,
              oneMContext: oneMOverride ?? qoderCapabilities[toolId].oneMContext,
            }
          : {}),
        ...(toolId === 'dsh'
          ? {
              imageInput: imageInputOverride ?? dshCapabilities.imageInput,
              oneMContext: oneMOverride ?? dshCapabilities.oneMContext,
            }
          : {}),
      });

      if (result?.success) {
        const sharedTools = ['cline', 'clinedesktop'].includes(toolId)
          ? ['cline', 'clinedesktop']
          : ['minimaxcode', 'minimaxdesktop'];
        setDetectedTools((prev) =>
          prev.map((t) =>
            t.id === toolId || (sharedTools.includes(toolId) && sharedTools.includes(t.id))
              ? { ...t, activeModel: model.modelId || model.internalId }
              : t
          )
        );
        if (sharedTools.includes(toolId)) {
          setToolModelConfig((prev) => ({
            ...prev,
            ...Object.fromEntries(sharedTools.map((id) => [id, model.internalId])),
          }));
        }
        return true;
      } else {
        console.error('[AppManager] Failed to apply model:', result?.message);
        const message = result?.message;
        return message?.startsWith('accountError.') ? accountError(message, t) : message || false;
      }
    } catch (error) {
      console.error('[AppManager] Error applying model to tool:', error);
      return false;
    }
  };

  // Restore the shared Web Search preference and apply it on an explicit flip.
  const setCodexWebSearch = (value: boolean) => {
    if (isLaunching) return;
    setCodexWebSearchRaw(value);
    writeBool('echobird_codex_web_search', value);
    if (!isCodexTool || !selectedTool || !selectedToolData?.installed || selectedCodexAccountId)
      return;
    const modelId = toolModelConfig[selectedTool];
    if (!modelId || isOfficialModelSentinel(modelId)) return;
    setIsLaunching(true);
    setApplyError(null);
    void applyModelConfig(selectedTool, modelId, undefined, value)
      .then((result) => {
        if (result !== true)
          setApplyError(typeof result === 'string' ? result : t('key.destroyed'));
      })
      .finally(() => setIsLaunching(false));
  };

  const setClaudeWebSearch = (tool: 'claudedesktop' | 'claudecode', value: boolean) => {
    if (isLaunching || selectedTool !== tool) return;
    const desktop = tool === 'claudedesktop';
    (desktop ? setClaudeDesktopWebSearch : setClaudeCodeWebSearch)(value);
    writeBool(`echobird_${tool}_web_search`, value);
    if (
      !selectedToolData?.installed ||
      (desktop ? claudeDesktopAccounts.selectedId : claudeCodeAccounts.selectedId)
    )
      return;
    const modelId = toolModelConfig[tool];
    if (!modelId || isOfficialModelSentinel(modelId)) return;
    setIsLaunching(true);
    setApplyError(null);
    void applyModelConfig(tool, modelId, undefined, value)
      .then((result) => {
        if (result !== true)
          setApplyError(typeof result === 'string' ? result : t('key.destroyed'));
      })
      .finally(() => setIsLaunching(false));
  };

  const setZCodeCapability = (capability: 'webSearch' | 'imageInput', value: boolean) => {
    if (isLaunching || selectedTool !== 'zcode') return;
    (capability === 'webSearch' ? setZCodeWebSearch : setZCodeImageInput)(value);
    writeBool(`echobird_zcode_${capability === 'webSearch' ? 'web_search' : 'image_input'}`, value);
    if (!selectedToolData?.installed || zcodeAccounts.selectedId) return;
    const modelId = toolModelConfig.zcode;
    if (!modelId || isOfficialModelSentinel(modelId)) return;
    setIsLaunching(true);
    setApplyError(null);
    void applyModelConfig(
      'zcode',
      modelId,
      undefined,
      capability === 'webSearch' ? value : zcodeWebSearch,
      capability === 'imageInput' ? value : zcodeImageInput
    )
      .then((result) => {
        if (result !== true)
          setApplyError(typeof result === 'string' ? result : t('key.destroyed'));
      })
      .finally(() => setIsLaunching(false));
  };

  const setWorkBuddyCapability = (capability: 'imageInput' | 'oneMContext', value: boolean) => {
    if (isLaunching || !workBuddyEdition) return;
    const tool = workBuddyEdition;
    const values = { ...workBuddyCapabilities[tool], [capability]: value };
    setWorkBuddyCapabilities((prev) => ({ ...prev, [tool]: values }));
    writeBool(`echobird_${tool}_${capability === 'imageInput' ? 'image_input' : '1m_mode'}`, value);
    if (!selectedToolData?.installed || workBuddyAccounts.selectedId) return;
    const modelId = toolModelConfig[tool];
    if (!modelId || isOfficialModelSentinel(modelId)) return;
    setIsLaunching(true);
    setApplyError(null);
    void applyModelConfig(tool, modelId, values.oneMContext, undefined, values.imageInput)
      .then((result) => {
        if (result !== true)
          setApplyError(typeof result === 'string' ? result : t('key.destroyed'));
      })
      .finally(() => setIsLaunching(false));
  };

  const setMimoCapability = (capability: 'imageInput' | 'oneMContext', value: boolean) => {
    if (isLaunching || !mimoTool) return;
    const tool = mimoTool;
    const values = { ...mimoCapabilities[tool], [capability]: value };
    setMimoCapabilities((prev) => ({ ...prev, [tool]: values }));
    writeBool(`echobird_${tool}_${capability === 'imageInput' ? 'image_input' : '1m_mode'}`, value);
    if (!selectedToolData?.installed) return;
    const modelId = toolModelConfig[tool];
    if (!modelId || isOfficialModelSentinel(modelId)) return;
    setIsLaunching(true);
    setApplyError(null);
    void applyModelConfig(tool, modelId, values.oneMContext, undefined, values.imageInput)
      .then((result) => {
        if (result !== true)
          setApplyError(typeof result === 'string' ? result : t('key.destroyed'));
      })
      .finally(() => setIsLaunching(false));
  };

  const setMinimaxCapability = (capability: 'imageInput' | 'oneMContext', value: boolean) => {
    if (isLaunching || (selectedTool !== 'minimaxcode' && selectedTool !== 'minimaxdesktop'))
      return;
    const values = { ...minimaxCapabilities, [capability]: value };
    setMinimaxCapabilities(values);
    writeBool(`echobird_minimax_${capability === 'imageInput' ? 'image_input' : '1m_mode'}`, value);
    if (!selectedToolData?.installed || minimaxAccounts.selectedId) return;
    const modelId = toolModelConfig[selectedTool];
    if (!modelId || isOfficialModelSentinel(modelId)) return;
    setIsLaunching(true);
    setApplyError(null);
    void applyModelConfig(selectedTool, modelId, values.oneMContext, undefined, values.imageInput)
      .then((result) => {
        if (result !== true)
          setApplyError(typeof result === 'string' ? result : t('key.destroyed'));
      })
      .finally(() => setIsLaunching(false));
  };

  const setQoderCapability = (capability: 'imageInput' | 'oneMContext', value: boolean) => {
    if (isLaunching || (selectedTool !== 'qoder' && selectedTool !== 'qodercn')) return;
    const tool = selectedTool;
    const values = { ...qoderCapabilities[tool], [capability]: value };
    setQoderCapabilities((prev) => ({ ...prev, [tool]: values }));
    writeBool(`echobird_${tool}_${capability === 'imageInput' ? 'image_input' : '1m_mode'}`, value);
    if (!selectedToolData?.installed || qoderAccounts.selectedId) return;
    const modelId = toolModelConfig[tool];
    if (!modelId || isOfficialModelSentinel(modelId)) return;
    setIsLaunching(true);
    setApplyError(null);
    void applyModelConfig(tool, modelId, values.oneMContext, undefined, values.imageInput)
      .then((result) => {
        if (result !== true)
          setApplyError(typeof result === 'string' ? result : t('key.destroyed'));
      })
      .finally(() => setIsLaunching(false));
  };

  const setDshCapability = (capability: 'imageInput' | 'oneMContext', value: boolean) => {
    if (isLaunching || selectedTool !== 'dsh') return;
    const values = { ...dshCapabilities, [capability]: value };
    setDshCapabilities(values);
    writeBool(`echobird_dsh_${capability === 'imageInput' ? 'image_input' : '1m_mode'}`, value);
    if (!selectedToolData?.installed || deepSeekAccounts.selectedId) return;
    const modelId = toolModelConfig.dsh;
    if (!modelId || isOfficialModelSentinel(modelId)) return;
    setIsLaunching(true);
    setApplyError(null);
    void applyModelConfig('dsh', modelId, values.oneMContext, undefined, values.imageInput)
      .then((result) => {
        if (result !== true)
          setApplyError(typeof result === 'string' ? result : t('key.destroyed'));
      })
      .finally(() => setIsLaunching(false));
  };

  const setClaudeOneM = (tool: 'claudedesktop' | 'claudecode', value: boolean) => {
    if (isLaunching || selectedTool !== tool) return;
    const desktop = tool === 'claudedesktop';
    (desktop ? setClaudeDesktop1mModeRaw : setClaude1mModeRaw)(value);
    writeBool(`echobird_${tool}_1m_mode`, value);
    if (
      !selectedToolData?.installed ||
      (desktop ? claudeDesktopAccounts.selectedId : claudeCodeAccounts.selectedId)
    )
      return;
    const modelId = toolModelConfig[tool];
    if (!modelId || isOfficialModelSentinel(modelId)) return;
    setIsLaunching(true);
    setApplyError(null);
    void applyModelConfig(tool, modelId, value)
      .then((result) => {
        if (result !== true)
          setApplyError(typeof result === 'string' ? result : t('key.destroyed'));
      })
      .finally(() => setIsLaunching(false));
  };
  const setClaude1mMode = (value: boolean) => setClaudeOneM('claudecode', value);
  const setClaudeDesktop1mMode = (value: boolean) => setClaudeOneM('claudedesktop', value);

  // Restore = delete the tool's config file. The tool itself regenerates
  // a vendor-default config on next launch, so restore is symmetric with
  // a fresh install. Backend also clears the ~/.echobird/{tool}.json relay
  // for "custom" tools.
  const applyRestore = async (toolId: string): Promise<true | string | false> => {
    try {
      const result = await api.restoreToolToOfficial(toolId);
      if (result?.success) {
        const official = getOfficialEndpoint(toolId);
        setDetectedTools((prev) =>
          prev.map((t) => (t.id === toolId ? { ...t, activeModel: official?.name || '' } : t))
        );
        return true;
      }
      return result?.message || false;
    } catch (err) {
      console.error('[AppManager] Restore-to-official failed:', err);
      return String(err);
    }
  };

  // Direct restore — kept exported on context for any callers that want to
  // bypass the bottom-bar flow. The card click now selects (no immediate
  // apply); the actual restore runs from handleLaunch when the official
  // sentinel is the pending selection.
  const handleRestoreModel = async (toolId: string) => {
    const result = await applyRestore(toolId);
    if (result !== true) {
      setApplyError(typeof result === 'string' ? result : t('key.destroyed'));
    }
  };

  const handleLaunch = useApplicationLauncher({
    clineSelectionRevisionRef,
    accountNavigation,
    isCodexTool,
    claudeCodeAccounts,
    claudeDesktopAccounts,
    workBuddyEdition,
    workBuddyAccounts,
    traeAccounts,
    kiroAccounts,
    minimaxAccounts,
    qoderEdition,
    qoderAccounts,
    dumateAccounts,
    doubaoEdition,
    doubaoAccounts,
    doubaoSelectionRevisionRef,
    qwenWorkEdition,
    qwenWorkAccounts,
    deepSeekAccounts,
    grokAccounts,
    manusAccounts,
    cueAccounts,
    clineAccounts,
    kiloAccounts,
    copilotAccounts,
    grokBotAccounts,
    cursorAccounts,
    cursorRevisionRef,
    antigravityAccounts,
    kimiSelectionRevisionRef,
    pluginAccountProvider,
    pluginAccountTool,
    pluginAccounts,
    kimiAccounts,
    zcodeAccounts,
    codeArtsAccounts,
    codeArtsSelectionRevisionRef,
    devinAccounts,
    devinRevisionRef,
    openDesignAccounts,
    selectedCodexAccountId,
    loadCodexAccounts,
    selectedTool,
    isLaunching,
    launchGeneration,
    previewLaunch,
    setIsLaunching,
    setApplyError,
    detectedTools,
    userModels,
    toolModelConfig,
    setToolModelConfig,
    setDetectedTools,
    launchAfterApply,
    agreedConfigPolicy,
    activePage,
    locale,
    t,
    applyRestore,
    applyModelConfig,
  });

  const accountDirectoryView = {
    accountCardOrder,
    setAccountCardOrder,
    claudeCodeAccounts: {
      ...claudeCodeAccounts,
      accounts: pluginAvailable('claudecode')
        ? orderedAccounts('claudecode', claudeCodeAccounts.accounts)
        : [],
    },
    claudeDesktopAccounts: {
      ...claudeDesktopAccounts,
      accounts: pluginAvailable('claudedesktop')
        ? orderedAccounts('claudedesktop', claudeDesktopAccounts.accounts)
        : [],
    },
    workBuddyAccounts: {
      ...workBuddyAccounts,
      accounts: pluginAvailable(workBuddyEdition ?? 'workbuddy')
        ? orderedAccounts(workBuddyEdition ?? 'workbuddy', workBuddyAccounts.accounts)
        : [],
    },
    workBuddyAccountGroups: {
      workbuddy: {
        ...workBuddyCnAccounts,
        accounts: pluginAvailable('workbuddy')
          ? orderedAccounts('workbuddy', workBuddyCnAccounts.accounts)
          : [],
      },
      workbuddyai: {
        ...workBuddyAiAccounts,
        accounts: pluginAvailable('workbuddyai')
          ? orderedAccounts('workbuddyai', workBuddyAiAccounts.accounts)
          : [],
      },
    },
    kiroAccounts: {
      ...kiroAccounts,
      accounts: orderedAccounts('kiro', kiroAccounts.accounts),
    },
    traeAccounts: {
      ...traeAccounts,
      accounts: orderedAccounts(selectedTool === 'trae' ? 'trae' : 'traecn', traeAccounts.accounts),
    },
    traeAccountGroups: {
      trae: {
        ...traeGlobalAccounts,
        accounts: orderedAccounts('trae', traeGlobalAccounts.accounts),
      },
      traecn: {
        ...traeCnAccounts,
        accounts: orderedAccounts('traecn', traeCnAccounts.accounts),
      },
    },
    zcodeAccounts: {
      ...zcodeAccounts,
      accounts: orderedAccounts('zcode', zcodeAccounts.accounts),
    },
    pluginAccounts,
    kimiAccounts: {
      ...kimiAccounts,
      accounts: orderedAccounts('kimi', kimiAccounts.accounts),
    },
    codeArtsAccounts: {
      ...codeArtsAccounts,
      accounts: orderedAccounts('codearts', codeArtsAccounts.accounts),
    },
    devinAccounts: {
      ...devinAccounts,
      accounts: orderedAccounts('devin', devinAccounts.accounts),
    },
    openDesignAccounts: {
      ...openDesignAccounts,
      accounts: orderedAccounts('opendesign', openDesignAccounts.accounts),
    },
    minimaxAccounts: {
      ...minimaxAccounts,
      accounts: orderedAccounts('minimax', minimaxAccounts.accounts),
    },
    qoderAccounts: {
      ...qoderAccounts,
      accounts: orderedAccounts(qoderEdition, qoderAccounts.accounts),
    },
    dumateAccounts: {
      ...dumateAccounts,
      accounts: orderedAccounts('dumate', dumateAccounts.accounts),
    },
    doubaoAccounts: {
      ...doubaoAccounts,
      accounts: pluginAvailable(doubaoEdition)
        ? orderedAccounts(doubaoEdition, doubaoAccounts.accounts)
        : [],
    },
    doubaoAccountGroups: {
      doubaowork: {
        ...doubaoWorkAccounts,
        accounts: pluginAvailable('doubaowork')
          ? orderedAccounts('doubaowork', doubaoWorkAccounts.accounts)
          : [],
      },
      dola: {
        ...dolaAccounts,
        accounts: pluginAvailable('dola') ? orderedAccounts('dola', dolaAccounts.accounts) : [],
      },
    },
    qwenWorkAccounts: {
      ...qwenWorkAccounts,
      accounts: orderedAccounts(qwenWorkEdition, qwenWorkAccounts.accounts),
    },
    qwenWorkAccountGroups: {
      qwenwork: {
        ...qwenWorkGlobalAccounts,
        accounts: orderedAccounts('qwenwork', qwenWorkGlobalAccounts.accounts),
      },
      qwenworkcn: {
        ...qwenWorkCnAccounts,
        accounts: orderedAccounts('qwenworkcn', qwenWorkCnAccounts.accounts),
      },
    },
    qoderAccountGroups: {
      qoder: {
        ...qoderGlobalAccounts,
        accounts: orderedAccounts('qoder', qoderGlobalAccounts.accounts),
      },
      qodercn: {
        ...qoderCnAccounts,
        accounts: orderedAccounts('qodercn', qoderCnAccounts.accounts),
      },
    },
    deepSeekAccounts: {
      ...deepSeekAccounts,
      accounts: orderedAccounts('dsh', deepSeekAccounts.accounts),
    },
    grokAccounts: { ...grokAccounts, accounts: orderedAccounts('grok', grokAccounts.accounts) },
    clineAccounts: {
      ...clineAccounts,
      accounts: orderedAccounts('cline', clineAccounts.accounts),
    },
    kiloAccounts: { ...kiloAccounts, accounts: orderedAccounts('kilo', kiloAccounts.accounts) },
    copilotAccounts: {
      ...copilotAccounts,
      accounts: orderedAccounts('copilot', copilotAccounts.accounts),
    },
    manusAccounts: {
      ...manusAccounts,
      accounts: orderedAccounts('manus', manusAccounts.accounts),
    },
    cueAccounts: {
      ...cueAccounts,
      accounts: orderedAccounts('cue', cueAccounts.accounts),
    },
    grokBotAccounts: {
      ...grokBotAccounts,
      accounts: orderedAccounts('grokbot', grokBotAccounts.accounts),
    },
    cursorAccounts: {
      ...cursorAccounts,
      accounts: orderedAccounts('cursor', cursorAccounts.accounts),
    },
    antigravityAccounts: {
      ...antigravityAccounts,
      accounts: orderedAccounts('antigravity', antigravityAccounts.accounts),
    },
    codexAccounts: orderedAccounts('codex', codexAccounts),
    selectedCodexAccountId,
    setSelectedCodexAccountId: selectCodexAccount,
    isLoadingCodexAccounts,
    codexAccountsLoaded,
    codexAccountsReadFailed,
    codexAccountsMutationRevision,
    isAddingCodexAccount,
    codexOAuthRemainingSeconds,
    refreshingCodexAccountIds,
    codexAuthorizationFailedIds,
    addCodexAccount,
    cancelCodexLogin,
    refreshCodexAccountQuota,
    deleteCodexAccount,
    detectedTools,
    isScanning,
    isLaunching,
    applyError,
    setApplyError,
  };

  const latestAccountDirectory = useRef(accountDirectoryView);
  useLayoutEffect(() => {
    latestAccountDirectory.current = accountDirectoryView;
  });
  const runSelectedToolAction = useCallback(
    async (action: 'claim' | 'warmUp') => {
      if (!selectedTool || !isActive || toolActionPending.current) return;
      if (!useRewardClaimStore.getState().tryStart()) return;
      toolActionPending.current = true;
      const scope = toolActionScope.current;
      const current = () => toolActionScope.current === scope;
      const providerId =
        (
          {
            chatgptdesktop: 'codex',
            minimaxcode: 'minimax',
            minimaxdesktop: 'minimax',
          } as Record<string, string>
        )[selectedTool] ?? selectedTool;
      const findProvider = () =>
        accountCenterProviders(latestAccountDirectory.current, t, locale).find(
          (provider) => provider.id === providerId || provider.id === `plugin:${selectedTool}`
        );
      setSelectedToolAction(action);
      let failed = 0;
      let completed = 0;
      try {
        const targets = findProvider()?.accounts ?? [];
        for (const target of targets) {
          if (!current()) break;
          const account = findProvider()?.accounts.find((row) => row.id === target.id);
          const operation = account?.[action === 'claim' ? 'claim' : 'warmUp'];
          if (!account || account.pending || account.refreshing || !operation) continue;
          let error = false;
          let deviceClaimed: unknown;
          try {
            deviceClaimed = await operation(() => {
              error = true;
            });
          } catch {
            error = true;
          }
          completed += 1;
          if (error) failed += 1;
          // Some reward providers claim once per device rather than per account.
          if (action === 'claim' && deviceClaimed === true) break;
        }
        if (failed && current()) {
          const prefix = action === 'claim' ? 'batchClaim' : 'batchWarmUp';
          showToast(
            'warning',
            t(`accountCenter.${prefix}Summary` as const)
              .replace('{success}', String(completed - failed))
              .replace('{failed}', String(failed))
          );
        }
      } catch (error) {
        if (current()) setApplyError(accountError(error, t));
      } finally {
        toolActionPending.current = false;
        useRewardClaimStore.getState().finish();
        setSelectedToolAction(null);
      }
    },
    [selectedTool, isActive, t, locale, showToast]
  );
  const claimSelectedTool = useCallback(
    () => runSelectedToolAction('claim'),
    [runSelectedToolAction]
  );
  const warmUpSelectedTool = useCallback(
    () => runSelectedToolAction('warmUp'),
    [runSelectedToolAction]
  );

  return (
    <AccountDirectoryContext.Provider value={accountDirectoryView}>
      <AppManagerContext.Provider
        value={{
          smartRouterEnabled: routerEnabled,
          selectedTool,
          setSelectedTool,
          launchAfterApply,
          setLaunchAfterApply,
          agreedConfigPolicy,
          setAgreedConfigPolicy,
          toolModelConfig,
          handleSelectModel,
          handleRestoreModel,
          ...accountDirectoryView,
          selectedToolData,
          applyError,
          setApplyError,
          detectedTools,
          setDetectedTools,
          isScanning,
          scanTools,
          userModels,
          codexWebSearch,
          zcodeWebSearch,
          zcodeImageInput,
          setZCodeCapability,
          workBuddyImageInput: workBuddyCapabilities[workBuddyEdition ?? 'workbuddy'].imageInput,
          setWorkBuddyImageInput: (value) => setWorkBuddyCapability('imageInput', value),
          workBuddy1mMode: workBuddyCapabilities[workBuddyEdition ?? 'workbuddy'].oneMContext,
          setWorkBuddy1mMode: (value) => setWorkBuddyCapability('oneMContext', value),
          mimoImageInput: mimoValues.imageInput,
          mimo1mMode: mimoValues.oneMContext,
          setMimoCapability,
          minimaxImageInput: minimaxCapabilities.imageInput,
          minimax1mMode: minimaxCapabilities.oneMContext,
          setMinimaxCapability,
          qoderImageInput: qoderCapabilities[qoderEdition].imageInput,
          qoder1mMode: qoderCapabilities[qoderEdition].oneMContext,
          setQoderCapability,
          dshImageInput: dshCapabilities.imageInput,
          dsh1mMode: dshCapabilities.oneMContext,
          setDshCapability,
          setCodexWebSearch,
          claudeDesktopWebSearch,
          claudeCodeWebSearch,
          setClaudeWebSearch,
          claudeDesktop1mMode,
          setClaudeDesktop1mMode,
          claude1mMode,
          setClaude1mMode,
          handleLaunch,
          onGoToMother: handleGoToMother,
          aiInstallableIds: [
            ...aiInstallableIds,
            ...pluginProviders.map((entry) => entry.application),
          ],
          viewMode,
          setViewMode,
          refreshSelectedTool,
          isRefreshingSelectedTool,
          claimSelectedTool,
          warmUpSelectedTool,
          selectedToolAction,
        }}
      >
        {children}
        {claudeCodeAccounts.login && (
          <ClaudeCodeLoginDialog
            submitting={claudeCodeAccounts.submitting}
            error={claudeCodeAccounts.loginError}
            onClose={claudeCodeAccounts.cancelLogin}
            onSubmit={claudeCodeAccounts.completeLogin}
          />
        )}
      </AppManagerContext.Provider>
    </AccountDirectoryContext.Provider>
  );
};
