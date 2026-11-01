import {
  communityAccountApplications,
  hasAccountPlugin,
  pluginToolProviders,
} from '../../hooks/useAccountPlugin';
import { usePluginToolAccounts } from './usePluginToolAccounts';
import { useCursorAccounts } from './useCursorAccounts';
import { useAntigravityAccounts } from './useAntigravityAccounts';
import { useDeepSeekAccounts } from './useDeepSeekAccounts';
import { useGrokAccounts } from './useGrokAccounts';
import { useKiloAccounts, useCopilotAccounts } from './useClientAccounts';
import { accountError } from '../../utils/accountError';
import { ClaudeCodeLoginDialog } from './ClaudeCodeLoginDialog';
import { useWorkBuddyAccounts } from './useWorkBuddyAccounts';
import { useTraeAccounts } from './useTraeAccounts';
import { useKiroAccounts } from './useKiroAccounts';
import { useZCodeAccounts } from './useZCodeAccounts';
import { useKimiAccounts } from './useKimiAccounts';
import { useCodeArtsAccounts } from './useCodeArtsAccounts';
import { switchCodeArtsAccount } from '../../api/codeArtsAccounts';
import { switchKimiAccount } from '../../api/kimiAccounts';
import { useDevinAccounts } from './useDevinAccounts';
import { useOpenDesignAccounts } from './useOpenDesignAccounts';
import { useMinimaxAccounts } from './useMinimaxAccounts';
import { useClineAccounts } from './useClineAccounts';
import { useQoderAccounts } from './useQoderAccounts';
import { useDuMateAccounts } from './useDuMateAccounts';
import { useDoubaoAccounts } from './useDoubaoAccounts';
import { useQwenWorkAccounts } from './useQwenWorkAccounts';
import { useClaudeCodeAccounts } from './useClaudeCodeAccounts';
import { useClaudeDesktopAccounts } from './useClaudeDesktopAccounts';
import React, { useState, useEffect, useCallback, useRef, useMemo } from 'react';
import { useCodexAccounts } from './useCodexAccounts';
import { useI18n } from '../../hooks/useI18n';
import * as api from '../../api/tauri';
import type { ModelConfig } from '../../api/types';
import { AppManagerContext } from './context';
import { useToolsStore } from '../../stores/toolsStore';
import { useNavigationStore } from '../../stores/navigationStore';
import { useUsageIslandVisible } from '../../stores/usageIslandStore';
import { useModelNexus } from '../ModelNexus/context';
import { useFreeModels } from '../FreeModels/provider';
import { getOfficialEndpoint, isOfficialModelSentinel } from '../../data/officialEndpoints';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { pluginTools, usePluginsStore } from '../../stores/pluginsStore';
import { launchPlugin } from '../../api/plugins';

// ===== Provider =====

interface AppManagerProviderProps {
  children: React.ReactNode;
}

export const AppManagerProvider: React.FC<AppManagerProviderProps> = ({ children }) => {
  const { t, locale } = useI18n();
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
  const developerTab = useNavigationStore((state) => state.developerTab);
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
  const isActive = activePage === 'apps';

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
  const [userModels, setUserModels] = useState<ModelConfig[]>([]);
  const { userModels: modelNexusModels } = useModelNexus();
  const { routerEnabled, routerBaseUrl, routers } = useFreeModels();
  const routerIdsKey = JSON.stringify(routers?.map((router) => router.id) ?? []);
  useEffect(() => {
    if (!api.getModels || !isActive) return;
    // Guard against out-of-order resolution: rapid navigation can
    // overlap getModels() calls, and a slower earlier one resolving last would
    // clobber userModels with stale data.
    let ignore = false;
    api
      .getModels()
      .then((models) => {
        if (ignore) return;
        setUserModels(models);
        setToolModelConfig((prev) => {
          const configuredRouterIds: string[] = JSON.parse(routerIdsKey);
          const removed = Object.keys(prev).filter(
            (toolId) =>
              prev[toolId] &&
              !isOfficialModelSentinel(prev[toolId]!) &&
              !configuredRouterIds.includes(prev[toolId]!) &&
              !models.some((model) => model.internalId === prev[toolId])
          );
          if (removed.length === 0) return prev;
          const next = { ...prev };
          removed.forEach((toolId) => (next[toolId] = null));
          return next;
        });
      })
      .catch((e) => console.error('Load models failed:', e));
    return () => {
      ignore = true;
    };
  }, [isActive, modelNexusModels, routerEnabled, routerBaseUrl, routerIdsKey]);

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
  const clineSelectionRevision = useRef(0);
  const accountNavigation = useRef({ tool: selectedTool, page: activePage });
  useEffect(() => {
    clineSelectionRevision.current += 1;
    accountNavigation.current = { tool: selectedTool, page: activePage };
  }, [selectedTool, activePage]);
  const usageIslandOpen = useUsageIslandVisible();
  const accountPage =
    usageIslandOpen ||
    activePage === 'accounts' ||
    (activePage === 'developer' && developerMode && developerTab === 'usageIsland');
  const accountsEnabled =
    isActive && detectedTools.some((tool) => tool.id === selectedTool && tool.installed);
  const pluginAvailable = (id: string) => hasAccountPlugin(id, installedPlugins, developerMode);
  // Account Management lists EchoBird's saved accounts whether or not the
  // client is installed; the built-in account backends read client files only
  // when it is. Plugin-provided accounts keep the install requirement because
  // their list runs in the plugin, outside that guard.
  const accountGroupEnabled = (...ids: string[]) =>
    ids.every((id) => !communityAccountApplications.includes(id) || pluginAvailable(id)) &&
    !usingCachedTools &&
    (accountPage
      ? !ids.some((id) => communityAccountApplications.includes(id)) ||
        detectedTools.some((tool) => ids.includes(tool.id) && tool.installed)
      : accountsEnabled && ids.includes(selectedTool ?? ''));
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
  const isCodexTool = selectedTool === 'codex' || selectedTool === 'chatgptdesktop';
  const codexManaged = useCodexAccounts(
    accountGroupEnabled('codex', 'chatgptdesktop'),
    !!(selectedTool && toolModelConfig[selectedTool]),
    () => setToolModelConfig((prev) => ({ ...prev, codex: null, chatgptdesktop: null })),
    setApplyError,
    accountPage ? 'accounts' : (selectedTool ?? '')
  );
  const {
    accounts: codexAccounts,
    selectedId: selectedCodexAccountId,
    select: selectCodexAccount,
    loading: isLoadingCodexAccounts,
    loaded: codexAccountsLoaded,
    readFailed: codexAccountsReadFailed,
    mutationRevision: codexAccountsMutationRevision,
    busy: isAddingCodexAccount,
    remainingSeconds: codexOAuthRemainingSeconds,
    refreshing: refreshingCodexAccountIds,
    authorizationFailedIds: codexAuthorizationFailedIds,
    reload: loadCodexAccounts,
    add: addCodexAccount,
    cancelLogin: cancelCodexLogin,
    remove: deleteCodexAccount,
    refresh: refreshCodexAccountQuota,
  } = codexManaged;

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
  const [viewMode, setViewModeRaw] = useState<'desktop' | 'install'>('desktop');
  const setViewMode = (mode: 'desktop' | 'install') => {
    if (mode === viewMode) return;
    setViewModeRaw(mode);
    setSelectedTool(null);
    setApplyError(null);
  };
  const clearClaudeCodeModel = useCallback(() => {
    setToolModelConfig((prev) => ({ ...prev, claudecode: null }));
  }, []);
  const claudeCodeAccounts = useClaudeCodeAccounts(
    accountGroupEnabled('claudecode'),
    !!toolModelConfig.claudecode,
    clearClaudeCodeModel,
    setApplyError
  );
  const clearClaudeDesktopModel = useCallback(() => {
    setToolModelConfig((prev) => ({ ...prev, claudedesktop: null }));
  }, []);
  const claudeDesktopAccounts = useClaudeDesktopAccounts(
    accountGroupEnabled('claudedesktop'),
    !!toolModelConfig.claudedesktop,
    clearClaudeDesktopModel,
    setApplyError
  );

  const workBuddyEdition =
    selectedTool === 'workbuddy' || selectedTool === 'workbuddyai' ? selectedTool : null;
  const mimoTool =
    selectedTool === 'mimocode' || selectedTool === 'mimodesktop' ? selectedTool : null;
  const mimoValues = mimoCapabilities[mimoTool ?? 'mimodesktop'];
  const clearWorkBuddyModel = useCallback((edition: api.WorkBuddyEdition) => {
    setToolModelConfig((prev) => ({ ...prev, [edition]: null }));
  }, []);
  const workBuddyCnAccounts = useWorkBuddyAccounts(
    'workbuddy',
    !!toolModelConfig.workbuddy,
    clearWorkBuddyModel,
    setApplyError,
    accountGroupEnabled('workbuddy')
  );
  const workBuddyAiAccounts = useWorkBuddyAccounts(
    'workbuddyai',
    !!toolModelConfig.workbuddyai,
    clearWorkBuddyModel,
    setApplyError,
    accountGroupEnabled('workbuddyai')
  );
  const workBuddyAccounts =
    workBuddyEdition === 'workbuddyai' ? workBuddyAiAccounts : workBuddyCnAccounts;

  const traeCnAccounts = useTraeAccounts('traecn', accountGroupEnabled('traecn'), setApplyError);
  const traeGlobalAccounts = useTraeAccounts('trae', accountGroupEnabled('trae'), setApplyError);
  const traeAccounts = selectedTool === 'trae' ? traeGlobalAccounts : traeCnAccounts;
  const kiroEdition =
    selectedTool === 'kirocli' && accountGroupEnabled('kirocli')
      ? 'kirocli'
      : accountGroupEnabled('kirocrew')
        ? 'kirocrew'
        : 'kirocli';
  const kiroAccounts = useKiroAccounts(
    kiroEdition,
    accountGroupEnabled('kirocrew', 'kirocli'),
    setApplyError
  );

  const clearMinimaxModel = useCallback(() => {
    setToolModelConfig((prev) => ({ ...prev, minimaxcode: null, minimaxdesktop: null }));
  }, []);
  const minimaxAccounts = useMinimaxAccounts(
    accountGroupEnabled('minimaxcode', 'minimaxdesktop'),
    !!(
      selectedTool &&
      ['minimaxcode', 'minimaxdesktop'].includes(selectedTool) &&
      toolModelConfig[selectedTool]
    ),
    clearMinimaxModel,
    setApplyError,
    accountPage ? 'accounts' : (selectedTool ?? '')
  );
  const qoderEdition: api.QoderEdition = selectedTool === 'qodercn' ? 'qodercn' : 'qoder';
  const clearQoderGlobalModel = useCallback(() => {
    setToolModelConfig((prev) => ({ ...prev, qoder: null }));
  }, []);
  const clearQoderCnModel = useCallback(() => {
    setToolModelConfig((prev) => ({ ...prev, qodercn: null }));
  }, []);
  const qoderGlobalAccounts = useQoderAccounts(
    'qoder',
    accountGroupEnabled('qoder'),
    !!toolModelConfig.qoder,
    clearQoderGlobalModel,
    setApplyError,
    accountPage ? 'accounts' : (selectedTool ?? '')
  );
  const qoderCnAccounts = useQoderAccounts(
    'qodercn',
    accountGroupEnabled('qodercn'),
    !!toolModelConfig.qodercn,
    clearQoderCnModel,
    setApplyError,
    accountPage ? 'accounts' : (selectedTool ?? '')
  );
  const qoderAccounts = qoderEdition === 'qodercn' ? qoderCnAccounts : qoderGlobalAccounts;
  const clearDuMateModel = useCallback(() => {
    setToolModelConfig((prev) => ({ ...prev, dumate: null }));
  }, []);
  const dumateAccounts = useDuMateAccounts(
    accountGroupEnabled('dumate') ||
      (!usingCachedTools &&
        activePage === 'freeModels' &&
        detectedTools.some((tool) => tool.id === 'dumate' && tool.installed)),
    clearDuMateModel,
    setApplyError,
    activePage === 'freeModels' ? 'dumate' : (selectedTool ?? '')
  );
  const doubaoEdition: api.DoubaoEdition = selectedTool === 'dola' ? 'dola' : 'doubaowork';
  const clearDoubaoWorkModel = useCallback(() => {
    setToolModelConfig((prev) => ({ ...prev, doubaowork: null }));
  }, []);
  const clearDolaModel = useCallback(() => {
    setToolModelConfig((prev) => ({ ...prev, dola: null }));
  }, []);
  const doubaoWorkAccounts = useDoubaoAccounts(
    'doubaowork',
    accountGroupEnabled('doubaowork') ||
      (pluginAvailable('doubaowork') &&
        !usingCachedTools &&
        activePage === 'freeModels' &&
        detectedTools.some((tool) => tool.id === 'doubaowork' && tool.installed)),
    !!toolModelConfig.doubaowork,
    clearDoubaoWorkModel,
    setApplyError,
    activePage === 'freeModels' ? 'doubaowork' : (selectedTool ?? '')
  );
  const dolaAccounts = useDoubaoAccounts(
    'dola',
    accountGroupEnabled('dola') ||
      (pluginAvailable('dola') &&
        !usingCachedTools &&
        activePage === 'freeModels' &&
        detectedTools.some((tool) => tool.id === 'dola' && tool.installed)),
    !!toolModelConfig.dola,
    clearDolaModel,
    setApplyError,
    activePage === 'freeModels' ? 'dola' : (selectedTool ?? '')
  );
  const doubaoAccounts = doubaoEdition === 'dola' ? dolaAccounts : doubaoWorkAccounts;
  const doubaoSelectionRevision = useRef(0);
  useEffect(() => {
    doubaoSelectionRevision.current += 1;
  }, [activePage, selectedTool, doubaoAccounts.selectedId]);
  const qwenWorkEdition: api.QwenWorkEdition =
    selectedTool === 'qwenworkcn' ? 'qwenworkcn' : 'qwenwork';
  const clearQwenWorkModel = useCallback(() => {
    setToolModelConfig((prev) => ({ ...prev, qwenwork: null }));
  }, []);
  const clearQwenWorkCnModel = useCallback(() => {
    setToolModelConfig((prev) => ({ ...prev, qwenworkcn: null }));
  }, []);
  const qwenWorkGlobalAccounts = useQwenWorkAccounts(
    'qwenwork',
    accountGroupEnabled('qwenwork'),
    clearQwenWorkModel,
    setApplyError,
    accountPage ? 'accounts' : (selectedTool ?? '')
  );
  const qwenWorkCnAccounts = useQwenWorkAccounts(
    'qwenworkcn',
    accountGroupEnabled('qwenworkcn'),
    clearQwenWorkCnModel,
    setApplyError,
    accountPage ? 'accounts' : (selectedTool ?? '')
  );
  const qwenWorkAccounts =
    qwenWorkEdition === 'qwenworkcn' ? qwenWorkCnAccounts : qwenWorkGlobalAccounts;

  const clearDeepSeekModel = useCallback(() => {
    setToolModelConfig((prev) => ({ ...prev, dsh: null }));
  }, []);
  const deepSeekAccounts = useDeepSeekAccounts(
    accountGroupEnabled('dsh'),
    !!toolModelConfig.dsh,
    clearDeepSeekModel,
    setApplyError
  );
  const clearGrokModel = useCallback(
    () => setToolModelConfig((prev) => ({ ...prev, grok: null })),
    []
  );
  const selectedToolData = detectedTools.find(
    (t) => t.id === selectedTool && (!isActive || t.installed === (viewMode === 'desktop'))
  );
  const grokAccounts = useGrokAccounts(
    accountGroupEnabled('grok'),
    !!toolModelConfig.grok,
    clearGrokModel,
    setApplyError
  );
  const manusAccounts = useGrokAccounts(
    accountGroupEnabled('manus'),
    false,
    () => {},
    setApplyError,
    'manus'
  );
  const cueAccounts = useGrokAccounts(
    accountGroupEnabled('cue'),
    false,
    () => {},
    setApplyError,
    'cue'
  );

  const clearClineModel = useCallback(() => {
    clineSelectionRevision.current += 1;
    setToolModelConfig((prev) => ({ ...prev, cline: null, clinedesktop: null }));
  }, []);
  const clineAccounts = useClineAccounts(
    accountGroupEnabled('cline', 'clinedesktop'),
    !!(
      selectedTool &&
      ['cline', 'clinedesktop'].includes(selectedTool) &&
      toolModelConfig[selectedTool]
    ),
    clearClineModel,
    setApplyError,
    accountPage ? 'accounts' : (selectedTool ?? '')
  );

  const clearKiloModel = useCallback(
    () => setToolModelConfig((prev) => ({ ...prev, kilo: null, kilodesktop: null })),
    []
  );
  const kiloAccounts = useKiloAccounts(
    accountGroupEnabled('kilo', 'kilodesktop'),
    !!(
      selectedTool &&
      ['kilo', 'kilodesktop'].includes(selectedTool) &&
      toolModelConfig[selectedTool]
    ),
    clearKiloModel,
    setApplyError,
    accountPage ? 'accounts' : (selectedTool ?? '')
  );
  const clearCopilotModel = useCallback(
    () => setToolModelConfig((prev) => ({ ...prev, copilot: null, copilotdesktop: null })),
    []
  );
  const copilotAccounts = useCopilotAccounts(
    accountGroupEnabled('copilot', 'copilotdesktop'),
    !!(
      selectedTool &&
      ['copilot', 'copilotdesktop'].includes(selectedTool) &&
      toolModelConfig[selectedTool]
    ),
    clearCopilotModel,
    setApplyError,
    selectedTool === 'copilotdesktop' ||
      (accountPage && !detectedTools.some((tool) => tool.id === 'copilot' && tool.installed))
      ? 'copilotdesktop'
      : 'copilot',
    accountPage ? 'accounts' : (selectedTool ?? '')
  );

  const clearGrokBotModel = useCallback(
    () => setToolModelConfig((prev) => ({ ...prev, grokbot: null })),
    []
  );
  const grokBotAccounts = useCursorAccounts(
    'grokbot',
    accountGroupEnabled('grokbot'),
    clearGrokBotModel,
    setApplyError
  );

  const clearCursorModel = useCallback(
    () => setToolModelConfig((prev) => ({ ...prev, cursor: null, cursorcli: null })),
    []
  );
  const cursorAccounts = useCursorAccounts(
    selectedTool === 'cursorcli' ||
      !detectedTools.some((tool) => tool.id === 'cursor' && tool.installed)
      ? 'cursorcli'
      : 'cursor',
    accountGroupEnabled('cursor', 'cursorcli'),
    clearCursorModel,
    setApplyError
  );
  const cursorRevision = useRef(0);
  useEffect(() => {
    cursorRevision.current += 1;
  }, [activePage, selectedTool, cursorAccounts.selectedId]);
  const antigravityAccounts = useAntigravityAccounts(
    accountGroupEnabled('antigravity', 'antigravitydesktop'),
    accountPage
      ? 'antigravitydesktop'
      : selectedTool === 'antigravity'
        ? 'antigravity'
        : 'antigravitydesktop',
    setApplyError
  );

  const kimiTool = selectedTool === 'kimicode' ? 'kimicode' : 'kimidesktop';
  const kimiModel = toolModelConfig[kimiTool];
  const kimiSelectionRevision = useRef(0);
  const pluginAccountProvider =
    pluginProviders.find((entry) => entry.application === selectedTool) ?? pluginProviders[0];
  const pluginAccountTool = pluginAccountProvider?.application ?? '';
  const pluginAccounts = usePluginToolAccounts(
    pluginAccountProvider,
    !!pluginAccountTool &&
      !usingCachedTools &&
      detectedTools.some(
        (tool) => pluginAccountProvider?.applications.includes(tool.id) && tool.installed
      ) &&
      (accountPage || (accountsEnabled && selectedTool === pluginAccountTool)),
    !!toolModelConfig[pluginAccountTool],
    () => setToolModelConfig((prev) => ({ ...prev, [pluginAccountTool]: null })),
    setApplyError,
    accountPage ? 'plugin' : pluginAccountTool
  );
  const kimiAccounts = useKimiAccounts(
    accountGroupEnabled('kimicode', 'kimidesktop'),
    !!kimiModel,
    () => setToolModelConfig((prev) => ({ ...prev, [kimiTool]: null })),
    setApplyError,
    accountPage ? 'kimi' : kimiTool
  );
  useEffect(() => {
    kimiSelectionRevision.current += 1;
  }, [selectedTool, activePage, kimiAccounts.selectedId, kimiModel]);
  const zcodeAccounts = useZCodeAccounts(
    accountGroupEnabled('zcode'),
    !!toolModelConfig.zcode,
    () => setToolModelConfig((prev) => ({ ...prev, zcode: null })),
    setApplyError
  );
  const codeArtsAccounts = useCodeArtsAccounts(
    accountGroupEnabled('codearts') ||
      (!usingCachedTools &&
        activePage === 'freeModels' &&
        detectedTools.some((tool) => tool.id === 'codearts' && tool.installed)),
    !!toolModelConfig.codearts,
    () => setToolModelConfig((prev) => ({ ...prev, codearts: null })),
    setApplyError,
    `${activePage}:${selectedTool ?? ''}`
  );
  const codeArtsModel = toolModelConfig.codearts;
  const codeArtsSelectionRevision = useRef(0);
  useEffect(() => {
    codeArtsSelectionRevision.current += 1;
  }, [activePage, selectedTool, codeArtsAccounts.selectedId, codeArtsModel]);
  const devinTool =
    selectedTool === 'devincli' ||
    !detectedTools.some((tool) => tool.id === 'devin' && tool.installed)
      ? 'devincli'
      : 'devin';
  const devinAccounts = useDevinAccounts(
    devinTool,
    accountGroupEnabled('devin', 'devincli'),
    setApplyError,
    accountPage ? 'devin' : devinTool
  );
  const devinRevision = useRef(0);
  useEffect(() => {
    devinRevision.current += 1;
  }, [activePage, selectedTool, devinAccounts.selectedId]);
  const openDesignAccounts = useOpenDesignAccounts(
    accountGroupEnabled('opendesign'),
    !!toolModelConfig.opendesign,
    () => setToolModelConfig((prev) => ({ ...prev, opendesign: null })),
    setApplyError
  );

  // Set tool model (single selection) - UI state update
  const handleSelectModel = (toolId: string, modelId: string) => {
    if (toolId === 'codearts') {
      codeArtsSelectionRevision.current += 1;
      codeArtsAccounts.select(null);
    }
    if (toolId === 'cline' || toolId === 'clinedesktop') {
      clineSelectionRevision.current += 1;
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

  // Launch handler
  const handleLaunch = async () => {
    if (!selectedTool || isLaunching) return;
    if (['cursor', 'cursorcli'].includes(selectedTool) && cursorAccounts.busy) return;
    if (selectedTool === 'codearts' && codeArtsAccounts.busy) return;
    if (selectedTool === 'zcode' && zcodeAccounts.busy) return;
    if (['kimicode', 'kimidesktop'].includes(selectedTool) && kimiAccounts.busy) return;
    if (selectedTool === pluginAccountTool && pluginAccounts.busy) return;
    if (['devin', 'devincli'].includes(selectedTool ?? '') && devinAccounts.busy) return;
    if (selectedTool === 'opendesign' && openDesignAccounts.busy) return;
    if (selectedTool === 'claudedesktop' && claudeDesktopAccounts.busy) return;
    if (selectedTool === 'cue' && cueAccounts.busy) return;
    if (['cline', 'clinedesktop'].includes(selectedTool) && clineAccounts.busy) return;
    if (['kilo', 'kilodesktop'].includes(selectedTool) && kiloAccounts.busy) return;
    if (['copilot', 'copilotdesktop'].includes(selectedTool) && copilotAccounts.busy) return;
    if ((selectedTool === 'trae' || selectedTool === 'traecn') && traeAccounts.busy) return;
    if (['minimaxcode', 'minimaxdesktop'].includes(selectedTool) && minimaxAccounts.busy) return;
    if (['qoder', 'qodercn'].includes(selectedTool) && qoderAccounts.busy) return;
    if (selectedTool === 'dumate' && dumateAccounts.busy) return;
    if (['doubaowork', 'dola'].includes(selectedTool) && doubaoAccounts.busy) return;
    if (['qwenwork', 'qwenworkcn'].includes(selectedTool) && qwenWorkAccounts.busy) return;
    if ((selectedTool === 'kirocrew' || selectedTool === 'kirocli') && kiroAccounts.busy) return;
    const generation = ++launchGeneration.current;
    const previewGeneration = useNavigationStore.getState().developerGeneration;
    const finishLaunch = () => {
      if (launchGeneration.current === generation) {
        previewLaunch.current = false;
        setIsLaunching(false);
      }
    };
    const startCooldown = () => {
      setTimeout(finishLaunch, 3000);
    };
    setIsLaunching(true);
    const switchingClaudeAccount =
      (selectedTool === 'claudecode' && !!claudeCodeAccounts.selectedId) ||
      (selectedTool === 'claudedesktop' && !!claudeDesktopAccounts.selectedId);
    if (
      selectedTool !== 'vscode' &&
      selectedTool !== 'copilotdesktop' &&
      !workBuddyEdition &&
      !switchingClaudeAccount &&
      !((selectedTool === 'trae' || selectedTool === 'traecn') && traeAccounts.selectedId) &&
      !(['minimaxcode', 'minimaxdesktop'].includes(selectedTool) && minimaxAccounts.selectedId) &&
      !(['qoder', 'qodercn'].includes(selectedTool) && qoderAccounts.selectedId) &&
      !(selectedTool === 'dumate' && dumateAccounts.selectedId) &&
      !(['doubaowork', 'dola'].includes(selectedTool) && doubaoAccounts.selectedId) &&
      !(['qwenwork', 'qwenworkcn'].includes(selectedTool) && qwenWorkAccounts.selectedId) &&
      !((selectedTool === 'kirocrew' || selectedTool === 'kirocli') && kiroAccounts.selectedId) &&
      !(selectedTool === 'zcode' && zcodeAccounts.selectedId) &&
      !(selectedTool === 'codearts' && codeArtsAccounts.selectedId) &&
      !(['kimicode', 'kimidesktop'].includes(selectedTool) && kimiAccounts.selectedId) &&
      !(selectedTool === pluginAccountTool && pluginAccounts.selectedId) &&
      !(['devin', 'devincli'].includes(selectedTool ?? '') && devinAccounts.selectedId) &&
      !(selectedTool === 'opendesign' && openDesignAccounts.selectedId) &&
      !(selectedTool === 'dsh' && deepSeekAccounts.selectedId) &&
      !(selectedTool === 'grok' && grokAccounts.selectedId) &&
      !(['cline', 'clinedesktop'].includes(selectedTool) && clineAccounts.selectedId) &&
      !(['kilo', 'kilodesktop'].includes(selectedTool) && kiloAccounts.selectedId) &&
      !(['copilot', 'copilotdesktop'].includes(selectedTool) && copilotAccounts.selectedId) &&
      !(selectedTool === 'manus' && manusAccounts.selectedId) &&
      !(selectedTool === 'cue' && cueAccounts.selectedId) &&
      !(['cursor', 'cursorcli'].includes(selectedTool) && cursorAccounts.selectedId) &&
      !(selectedTool === 'grokbot' && grokBotAccounts.selectedId) &&
      !(
        (selectedTool === 'antigravity' || selectedTool === 'antigravitydesktop') &&
        antigravityAccounts.selectedId
      )
    )
      startCooldown();

    const toolData = detectedTools.find((t) => t.id === selectedTool);
    previewLaunch.current = !!toolData?.plugin;
    if (toolData?.plugin) {
      try {
        const model = userModels.find(
          (model) => model.internalId === toolModelConfig[selectedTool]
        );
        await launchPlugin(toolData.plugin.id, toolData.plugin.pageId, model);
      } catch (error) {
        if (
          launchGeneration.current === generation &&
          previewGeneration === useNavigationStore.getState().developerGeneration
        )
          setApplyError(String(error));
      } finally {
        finishLaunch();
      }
      return;
    }
    const noModelConfig = !!toolData?.noModelConfig;
    const pickWorkingDirectory = async () => {
      if (toolData?.category !== 'CLI Code' || toolData.noCwdPrompt) return undefined;
      const picked = await openDialog({ directory: true, multiple: false });
      return typeof picked === 'string' && picked ? picked : null;
    };

    // An account switch honors "launch directly"; only model flows force-launch.
    let accountApplied = false;
    // An OpenAI account and a third-party API model are one exclusive choice.
    // Selecting an account restores the official provider before writing that
    // account's auth snapshot; selecting a model clears the account choice.
    if (
      (selectedTool === 'antigravity' || selectedTool === 'antigravitydesktop') &&
      antigravityAccounts.selectedId
    ) {
      try {
        await api.switchAntigravityAccount(antigravityAccounts.selectedId);
        await antigravityAccounts.reload();
        accountApplied = true;
      } catch (error) {
        setApplyError(accountError(error, t));
        finishLaunch();
        return;
      }
      startCooldown();
    } else if (isCodexTool && selectedCodexAccountId) {
      const restoreResult = await applyRestore(selectedTool);
      if (restoreResult !== true) {
        setApplyError(
          typeof restoreResult === 'string' ? accountError(restoreResult, t) : t('key.destroyed')
        );
        finishLaunch();
        return;
      }
      try {
        await api.switchCodexAccount(selectedCodexAccountId);
        await loadCodexAccounts();
        accountApplied = true;
      } catch (error) {
        setApplyError(accountError(error, t));
        finishLaunch();
        return;
      }
    } else if (
      ((selectedTool === 'kilo' || selectedTool === 'kilodesktop') && kiloAccounts.selectedId) ||
      ((selectedTool === 'copilot' || selectedTool === 'copilotdesktop') &&
        copilotAccounts.selectedId)
    ) {
      try {
        if (selectedTool === 'kilo' || selectedTool === 'kilodesktop') {
          await api.switchKiloAccount(kiloAccounts.selectedId!, selectedTool);
          await kiloAccounts.reload();
        } else {
          await api.switchCopilotAccount(
            copilotAccounts.selectedId!,
            selectedTool as api.CopilotTool
          );
          await copilotAccounts.reload();
        }
        setDetectedTools((prev) =>
          prev.map((tool) => (tool.id === selectedTool ? { ...tool, activeModel: '' } : tool))
        );
        if (launchAfterApply) {
          let cwd: string | undefined;
          if (toolData?.category === 'CLI Code' && !toolData.noCwdPrompt) {
            const picked = await openDialog({ directory: true, multiple: false });
            if (typeof picked !== 'string' || !picked) return;
            cwd = picked;
          }
          await api.startTool(selectedTool, toolData?.startCommand, cwd);
        }
      } catch (error) {
        setApplyError(accountError(error, t));
      } finally {
        finishLaunch();
      }
      return;
    } else if (selectedTool === 'dsh' && deepSeekAccounts.selectedId) {
      try {
        await api.switchDeepSeekAccount(deepSeekAccounts.selectedId, locale);
        setDetectedTools((prev) =>
          prev.map((tool) => (tool.id === 'dsh' ? { ...tool, activeModel: '' } : tool))
        );
        await deepSeekAccounts.reload();
        if (launchAfterApply) await api.startTool('dsh');
      } catch (error) {
        setApplyError(accountError(error, t));
      } finally {
        finishLaunch();
      }
      return;
    } else if (selectedTool === 'grokbot' && grokBotAccounts.selectedId) {
      try {
        await api.switchGrokBotAccount(grokBotAccounts.selectedId);
        await grokBotAccounts.reload();
        if (launchAfterApply) await api.startTool('grokbot');
      } catch (error) {
        setApplyError(accountError(error, t));
      } finally {
        finishLaunch();
      }
      return;
    } else if (['cursor', 'cursorcli'].includes(selectedTool) && cursorAccounts.selectedId) {
      const revision = cursorRevision.current;
      const current = () =>
        revision === cursorRevision.current &&
        accountNavigation.current.tool === selectedTool &&
        accountNavigation.current.page === activePage;
      try {
        const cwd = launchAfterApply ? await pickWorkingDirectory() : undefined;
        if (cwd === null || !current()) return;
        if (selectedTool === 'cursorcli')
          await api.switchCursorAccount(cursorAccounts.selectedId, 'cursorcli');
        else await api.switchCursorAccount(cursorAccounts.selectedId);
        if (current()) await cursorAccounts.reload();
        if (launchAfterApply && current()) {
          if (selectedTool === 'cursorcli')
            await api.startTool(selectedTool, toolData?.startCommand, cwd);
          else await api.startTool('cursor');
        }
      } catch (error) {
        if (current()) setApplyError(accountError(error, t));
      } finally {
        finishLaunch();
      }
      return;
    } else if (
      (selectedTool === 'manus' && manusAccounts.selectedId) ||
      (selectedTool === 'cue' && cueAccounts.selectedId)
    ) {
      try {
        await (selectedTool === 'cue' ? cueAccounts : manusAccounts).switchAccount();
        if (launchAfterApply) await api.startTool(selectedTool);
      } catch (error) {
        setApplyError(accountError(error, t));
      } finally {
        finishLaunch();
      }
      return;
    } else if (selectedTool === 'grok' && grokAccounts.selectedId) {
      try {
        const restored = await applyRestore('grok');
        if (restored !== true)
          throw new Error(typeof restored === 'string' ? restored : t('key.destroyed'));
        await grokAccounts.switchAccount();
        accountApplied = true;
        startCooldown();
      } catch (error) {
        setApplyError(accountError(error, t));
        finishLaunch();
        return;
      }
    } else if (['cline', 'clinedesktop'].includes(selectedTool) && clineAccounts.selectedId) {
      const selectionRevision = clineSelectionRevision.current;
      const current = () =>
        selectionRevision === clineSelectionRevision.current &&
        accountNavigation.current.tool === selectedTool &&
        accountNavigation.current.page === activePage;
      try {
        const cwd = launchAfterApply ? await pickWorkingDirectory() : undefined;
        if (cwd === null || !current()) return;
        await api.switchClineAccount(clineAccounts.selectedId, selectedTool as api.ClineTool);
        setDetectedTools((prev) =>
          prev.map((tool) =>
            ['cline', 'clinedesktop'].includes(tool.id) ? { ...tool, activeModel: '' } : tool
          )
        );
        if (selectionRevision === clineSelectionRevision.current)
          setToolModelConfig((prev) => ({ ...prev, cline: null, clinedesktop: null }));
        await clineAccounts.reload();
        if (launchAfterApply && current())
          await api.startTool(selectedTool, toolData?.startCommand, cwd);
      } catch (error) {
        if (current()) setApplyError(accountError(error, t));
      } finally {
        finishLaunch();
      }
      return;
    } else if (
      ['minimaxcode', 'minimaxdesktop'].includes(selectedTool) &&
      minimaxAccounts.selectedId
    ) {
      try {
        const cwd = launchAfterApply ? await pickWorkingDirectory() : undefined;
        if (cwd === null) return;
        await api.switchMinimaxAccount(minimaxAccounts.selectedId, selectedTool as api.MinimaxTool);
        setDetectedTools((prev) =>
          prev.map((tool) =>
            ['minimaxcode', 'minimaxdesktop'].includes(tool.id)
              ? { ...tool, activeModel: '' }
              : tool
          )
        );
        await minimaxAccounts.reload();
        if (launchAfterApply) await api.startTool(selectedTool, toolData?.startCommand, cwd);
      } catch (error) {
        setApplyError(accountError(error, t));
      } finally {
        finishLaunch();
      }
      return;
    } else if (['doubaowork', 'dola'].includes(selectedTool) && doubaoAccounts.selectedId) {
      const navigation = accountNavigation.current;
      const selectionRevision = doubaoSelectionRevision.current;
      const current = () =>
        accountNavigation.current === navigation &&
        doubaoSelectionRevision.current === selectionRevision;
      try {
        await api.switchDoubaoAccount(doubaoEdition, doubaoAccounts.selectedId);
        if (current()) {
          await doubaoAccounts.reload();
          if (launchAfterApply && current())
            await api.startTool(selectedTool, toolData?.startCommand);
        }
      } catch (error) {
        if (current()) setApplyError(accountError(error, t));
      } finally {
        finishLaunch();
      }
      return;
    } else if (selectedTool === 'dumate' && dumateAccounts.selectedId) {
      const navigation = accountNavigation.current;
      const current = () => accountNavigation.current === navigation;
      try {
        await api.switchDuMateAccount(dumateAccounts.selectedId);
        if (current()) {
          await dumateAccounts.reload();
          if (launchAfterApply && current())
            await api.startTool(selectedTool, toolData?.startCommand);
        }
      } catch (error) {
        if (current()) setApplyError(accountError(error, t));
      } finally {
        finishLaunch();
      }
      return;
    } else if (['qwenwork', 'qwenworkcn'].includes(selectedTool) && qwenWorkAccounts.selectedId) {
      const navigation = accountNavigation.current;
      const current = () => accountNavigation.current === navigation;
      try {
        await api.switchQwenWorkAccount(qwenWorkEdition, qwenWorkAccounts.selectedId);
        if (current()) {
          await qwenWorkAccounts.reload();
          if (launchAfterApply && current())
            await api.startTool(selectedTool, toolData?.startCommand);
        }
      } catch (error) {
        if (current()) setApplyError(accountError(error, t));
      } finally {
        finishLaunch();
      }
      return;
    } else if (['qoder', 'qodercn'].includes(selectedTool) && qoderAccounts.selectedId) {
      try {
        await api.switchQoderAccount(
          qoderEdition,
          qoderAccounts.selectedId,
          selectedTool as api.QoderTool
        );
        await qoderAccounts.reload();
        if (launchAfterApply) await api.startTool(selectedTool, toolData?.startCommand);
      } catch (error) {
        setApplyError(accountError(error, t));
      } finally {
        finishLaunch();
      }
      return;
    } else if (selectedTool === 'codearts' && codeArtsAccounts.selectedId) {
      const selectionRevision = codeArtsSelectionRevision.current;
      const current = () =>
        selectionRevision === codeArtsSelectionRevision.current &&
        accountNavigation.current.tool === selectedTool &&
        accountNavigation.current.page === activePage;
      try {
        const cwd = launchAfterApply ? await pickWorkingDirectory() : undefined;
        if (cwd === null || !current()) return;
        await switchCodeArtsAccount(codeArtsAccounts.selectedId);
        if (current()) {
          await codeArtsAccounts.reload();
        }
        if (launchAfterApply && current())
          await api.startTool(selectedTool, toolData?.startCommand, cwd);
      } catch (error) {
        if (current()) setApplyError(accountError(error, t));
      } finally {
        finishLaunch();
      }
      return;
    } else if (
      (selectedTool === 'devin' || selectedTool === 'devincli') &&
      devinAccounts.selectedId
    ) {
      const revision = devinRevision.current;
      const current = () =>
        revision === devinRevision.current &&
        accountNavigation.current.tool === selectedTool &&
        accountNavigation.current.page === activePage;
      try {
        const cwd = launchAfterApply ? await pickWorkingDirectory() : undefined;
        if (cwd === null || !current()) return;
        await api.switchDevinAccount(devinAccounts.selectedId, selectedTool);
        if (current()) await devinAccounts.reload();
        if (launchAfterApply && current())
          await api.startTool(selectedTool, toolData?.startCommand, cwd);
      } catch (error) {
        if (current()) setApplyError(accountError(error, t));
      } finally {
        finishLaunch();
      }
      return;
    } else if (selectedTool === 'opendesign' && openDesignAccounts.selectedId) {
      try {
        await api.switchOpenDesignAccount(openDesignAccounts.selectedId);
        setDetectedTools((prev) =>
          prev.map((tool) => (tool.id === 'opendesign' ? { ...tool, activeModel: '' } : tool))
        );
        await openDesignAccounts.reload();
        if (launchAfterApply) await api.startTool('opendesign');
      } catch (error) {
        setApplyError(accountError(error, t));
      } finally {
        finishLaunch();
      }
      return;
    } else if (selectedTool === pluginAccountTool && pluginAccounts.selectedId) {
      const current = () =>
        accountNavigation.current.tool === selectedTool &&
        accountNavigation.current.page === activePage;
      try {
        const cwd = launchAfterApply ? await pickWorkingDirectory() : undefined;
        if (cwd === null || !current()) return;
        await pluginAccounts.apply(pluginAccounts.selectedId);
        setDetectedTools((prev) =>
          prev.map((tool) =>
            pluginAccountProvider?.applications.includes(tool.id)
              ? { ...tool, activeModel: '' }
              : tool
          )
        );
        if (current()) {
          setToolModelConfig((prev) => ({
            ...prev,
            ...Object.fromEntries(
              (pluginAccountProvider?.applications ?? []).map((id) => [id, null])
            ),
          }));
          await pluginAccounts.reload();
        }
        if (launchAfterApply && current())
          await api.startTool(selectedTool, toolData?.startCommand, cwd);
      } catch (error) {
        if (current()) setApplyError(accountError(error, t));
      } finally {
        finishLaunch();
      }
      return;
    } else if (['kimicode', 'kimidesktop'].includes(selectedTool) && kimiAccounts.selectedId) {
      const selectionRevision = kimiSelectionRevision.current;
      const current = () =>
        selectionRevision === kimiSelectionRevision.current &&
        accountNavigation.current.tool === selectedTool &&
        accountNavigation.current.page === activePage;
      try {
        const cwd = launchAfterApply ? await pickWorkingDirectory() : undefined;
        if (cwd === null || !current()) return;
        await switchKimiAccount(kimiAccounts.selectedId);
        setDetectedTools((prev) =>
          prev.map((tool) =>
            ['kimicode', 'kimidesktop'].includes(tool.id) ? { ...tool, activeModel: '' } : tool
          )
        );
        if (current()) {
          setToolModelConfig((prev) => ({ ...prev, kimicode: null, kimidesktop: null }));
          await kimiAccounts.reload();
        }
        if (launchAfterApply && current())
          await api.startTool(selectedTool, toolData?.startCommand, cwd);
      } catch (error) {
        if (current()) setApplyError(accountError(error, t));
      } finally {
        finishLaunch();
      }
      return;
    } else if (selectedTool === 'zcode' && zcodeAccounts.selectedId) {
      try {
        await api.switchZCodeAccount(zcodeAccounts.selectedId);
        setDetectedTools((prev) =>
          prev.map((tool) => (tool.id === 'zcode' ? { ...tool, activeModel: '' } : tool))
        );
        await zcodeAccounts.reload();
        if (launchAfterApply) await api.startTool('zcode', toolData?.startCommand);
      } catch (error) {
        setApplyError(accountError(error, t));
      } finally {
        finishLaunch();
      }
      return;
    } else if (workBuddyEdition && workBuddyAccounts.selectedId) {
      try {
        await api.switchWorkBuddyAccount(workBuddyEdition, workBuddyAccounts.selectedId);
        await workBuddyAccounts.reload();
        if (launchAfterApply) await api.startTool(workBuddyEdition, toolData?.startCommand);
      } catch (error) {
        setApplyError(accountError(error, t));
      } finally {
        finishLaunch();
      }
      return;
    } else if (
      (selectedTool === 'kirocrew' || selectedTool === 'kirocli') &&
      kiroAccounts.selectedId
    ) {
      try {
        await api.switchKiroAccount(selectedTool, kiroAccounts.selectedId);
        await kiroAccounts.reload();
        accountApplied = true;
      } catch (error) {
        setApplyError(accountError(error, t));
        finishLaunch();
        return;
      }
    } else if ((selectedTool === 'trae' || selectedTool === 'traecn') && traeAccounts.selectedId) {
      try {
        await api.switchTraeAccount(selectedTool, traeAccounts.selectedId);
        await traeAccounts.reload();
        if (launchAfterApply) await api.startTool(selectedTool, toolData?.startCommand);
      } catch (error) {
        setApplyError(accountError(error, t));
      } finally {
        finishLaunch();
      }
      return;
    } else if (selectedTool === 'claudedesktop' && claudeDesktopAccounts.selectedId) {
      try {
        await api.switchClaudeDesktopAccount(claudeDesktopAccounts.selectedId);
        const restored = await applyRestore(selectedTool);
        if (restored !== true)
          throw new Error(typeof restored === 'string' ? restored : t('key.destroyed'));
        await claudeDesktopAccounts.reload();
        if (launchAfterApply) await api.startTool(selectedTool, toolData?.startCommand);
      } catch (error) {
        setApplyError(accountError(error, t));
      } finally {
        finishLaunch();
      }
      return;
    } else if (selectedTool === 'claudecode' && claudeCodeAccounts.selectedId) {
      try {
        const restored = await applyRestore(selectedTool);
        if (restored !== true)
          throw new Error(typeof restored === 'string' ? restored : t('key.destroyed'));
        await api.switchClaudeCodeAccount(claudeCodeAccounts.selectedId);
        await claudeCodeAccounts.reload();
        accountApplied = true;
        startCooldown();
      } catch (error) {
        setApplyError(accountError(error, t));
        finishLaunch();
        return;
      }
    } else if (!noModelConfig && agreedConfigPolicy && toolModelConfig[selectedTool]) {
      const pending = toolModelConfig[selectedTool]!;
      const applyResult = isOfficialModelSentinel(pending)
        ? await applyRestore(selectedTool)
        : await applyModelConfig(selectedTool, pending);
      if (applyResult !== true) {
        setApplyError(typeof applyResult === 'string' ? applyResult : t('key.destroyed'));
        finishLaunch();
        return;
      }
    }
    // Launch tool when "launch directly" is checked, or unconditionally for desktop apps
    if (
      launchAfterApply ||
      (!accountApplied &&
        (noModelConfig || ['qoder', 'qodercn', 'claudescience'].includes(selectedTool)))
    ) {
      // CLI clients may request a working directory before launch.
      let cwd: string | undefined;
      try {
        const picked = await pickWorkingDirectory();
        if (picked === null) {
          finishLaunch();
          return;
        }
        cwd = picked;
      } catch (e) {
        console.error('[AppManager] folder picker failed:', e);
        setApplyError(e instanceof Error ? e.message : String(e));
        finishLaunch();
        return;
      }
      try {
        await api.startTool(selectedTool, toolData?.startCommand, cwd);
      } catch (err) {
        console.error('Failed to launch tool:', err);
        setApplyError(err instanceof Error ? err.message : String(err));
      } finally {
        if ((selectedTool === 'kirocrew' || selectedTool === 'kirocli') && kiroAccounts.selectedId)
          finishLaunch();
      }
    } else if ((selectedTool === 'kirocrew' || selectedTool === 'kirocli') && accountApplied) {
      finishLaunch();
    }
    if (selectedTool === 'vscode' || selectedTool === 'copilotdesktop' || workBuddyEdition)
      startCooldown();
  };

  return (
    <AppManagerContext.Provider
      value={{
        smartRouterEnabled: routerEnabled,
        selectedTool,
        setSelectedTool,
        launchAfterApply,
        setLaunchAfterApply,
        isLaunching,
        agreedConfigPolicy,
        setAgreedConfigPolicy,
        toolModelConfig,
        handleSelectModel,
        handleRestoreModel,
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
          accounts: orderedAccounts(
            selectedTool === 'trae' ? 'trae' : 'traecn',
            traeAccounts.accounts
          ),
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
  );
};
