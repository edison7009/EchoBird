import { usePluginToolAccounts } from '../../pages/AppManager/usePluginToolAccounts';
import { useCursorAccounts } from '../../pages/AppManager/useCursorAccounts';
import { useAntigravityAccounts } from '../../pages/AppManager/useAntigravityAccounts';
import { useDeepSeekAccounts } from '../../pages/AppManager/useDeepSeekAccounts';
import { useGrokAccounts } from '../../pages/AppManager/useGrokAccounts';
import { useKiloAccounts, useCopilotAccounts } from '../../pages/AppManager/useClientAccounts';
import { useWorkBuddyAccounts } from '../../pages/AppManager/useWorkBuddyAccounts';
import { useTraeAccounts } from '../../pages/AppManager/useTraeAccounts';
import { useKiroAccounts } from '../../pages/AppManager/useKiroAccounts';
import { useZCodeAccounts } from '../../pages/AppManager/useZCodeAccounts';
import { useKimiAccounts } from '../../pages/AppManager/useKimiAccounts';
import { useCodeArtsAccounts } from '../../pages/AppManager/useCodeArtsAccounts';
import { useDevinAccounts } from '../../pages/AppManager/useDevinAccounts';
import { useOpenDesignAccounts } from '../../pages/AppManager/useOpenDesignAccounts';
import { useMinimaxAccounts } from '../../pages/AppManager/useMinimaxAccounts';
import { useClineAccounts } from '../../pages/AppManager/useClineAccounts';
import { useQoderAccounts } from '../../pages/AppManager/useQoderAccounts';
import { useDuMateAccounts } from '../../pages/AppManager/useDuMateAccounts';
import { useDoubaoAccounts } from '../../pages/AppManager/useDoubaoAccounts';
import { useQwenWorkAccounts } from '../../pages/AppManager/useQwenWorkAccounts';
import { useClaudeCodeAccounts } from '../../pages/AppManager/useClaudeCodeAccounts';
import { useClaudeDesktopAccounts } from '../../pages/AppManager/useClaudeDesktopAccounts';
import { useCodexAccounts } from '../../pages/AppManager/useCodexAccounts';
import { useCallback, useEffect, useRef } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import type * as api from '../../api/tauri';
import type { LocalTool } from '../../api/types';
import {
  communityAccountApplications,
  hasAccountPlugin,
  pluginToolProviders,
} from '../../hooks/useAccountPlugin';
import type { usePluginsStore } from '../../stores/pluginsStore';
import type { useNavigationStore } from '../../stores/navigationStore';
import { useUsageIslandVisible } from '../../stores/usageIslandStore';

interface DirectoryOptions {
  selectedTool: string | null;
  activePage: ReturnType<typeof useNavigationStore.getState>['activePage'];
  isActive: boolean;
  developerMode: boolean;
  developerTab: string;
  detectedTools: LocalTool[];
  installedPlugins: ReturnType<typeof usePluginsStore.getState>['installed'];
  pluginProviders: ReturnType<typeof pluginToolProviders>;
  usingCachedTools: boolean;
  toolModelConfig: Record<string, string | null>;
  setToolModelConfig: Dispatch<SetStateAction<Record<string, string | null>>>;
  setApplyError: (error: string | null) => void;
}

export function useAccountDirectory({
  selectedTool,
  activePage,
  isActive,
  developerMode,
  developerTab,
  detectedTools,
  installedPlugins,
  pluginProviders,
  usingCachedTools,
  toolModelConfig,
  setToolModelConfig,
  setApplyError,
}: DirectoryOptions) {
  const clineSelectionRevisionRef = useRef(0);
  const accountNavigation = useRef({ tool: selectedTool, page: activePage });
  useEffect(() => {
    clineSelectionRevisionRef.current += 1;
    accountNavigation.current = { tool: selectedTool, page: activePage };
  }, [selectedTool, activePage]);
  const usageIslandOpen = useUsageIslandVisible();
  const accountPage =
    activePage === 'accounts' ||
    (activePage === 'developer' && developerMode && developerTab === 'usageIsland');
  // The floating island needs saved accounts on every page, but tool pages keep their own target.
  const accountDataNeeded = accountPage || usageIslandOpen;
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
    (accountDataNeeded
      ? !ids.some((id) => communityAccountApplications.includes(id)) ||
        detectedTools.some((tool) => ids.includes(tool.id) && tool.installed)
      : accountsEnabled && ids.includes(selectedTool ?? ''));
  // Groups the open island keeps loaded on other tools' pages share one key, so tool clicks
  // don't reload them.
  const toolNavigation = (...ids: string[]) =>
    accountPage ? 'accounts' : ids.includes(selectedTool ?? '') ? (selectedTool ?? '') : '';
  const isCodexTool = selectedTool === 'codex' || selectedTool === 'chatgptdesktop';
  const codexManaged = useCodexAccounts(
    accountGroupEnabled('codex', 'chatgptdesktop'),
    !!(selectedTool && toolModelConfig[selectedTool]),
    () => setToolModelConfig((prev) => ({ ...prev, codex: null, chatgptdesktop: null })),
    setApplyError,
    toolNavigation('codex', 'chatgptdesktop')
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

  const clearClaudeCodeModel = useCallback(() => {
    setToolModelConfig((prev) => ({ ...prev, claudecode: null }));
  }, [setToolModelConfig]);
  const claudeCodeAccounts = useClaudeCodeAccounts(
    accountGroupEnabled('claudecode'),
    !!toolModelConfig.claudecode,
    clearClaudeCodeModel,
    setApplyError
  );
  const clearClaudeDesktopModel = useCallback(() => {
    setToolModelConfig((prev) => ({ ...prev, claudedesktop: null }));
  }, [setToolModelConfig]);
  const claudeDesktopAccounts = useClaudeDesktopAccounts(
    accountGroupEnabled('claudedesktop'),
    !!toolModelConfig.claudedesktop,
    clearClaudeDesktopModel,
    setApplyError
  );

  const workBuddyEdition: api.WorkBuddyEdition | null =
    selectedTool === 'workbuddy' || selectedTool === 'workbuddyai' ? selectedTool : null;
  const clearWorkBuddyModel = useCallback(
    (edition: api.WorkBuddyEdition) => {
      setToolModelConfig((prev) => ({ ...prev, [edition]: null }));
    },
    [setToolModelConfig]
  );
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
  }, [setToolModelConfig]);
  const minimaxAccounts = useMinimaxAccounts(
    accountGroupEnabled('minimaxcode', 'minimaxdesktop'),
    !!(
      selectedTool &&
      ['minimaxcode', 'minimaxdesktop'].includes(selectedTool) &&
      toolModelConfig[selectedTool]
    ),
    clearMinimaxModel,
    setApplyError,
    toolNavigation('minimaxcode', 'minimaxdesktop')
  );
  const qoderEdition: api.QoderEdition = selectedTool === 'qodercn' ? 'qodercn' : 'qoder';
  const clearQoderGlobalModel = useCallback(() => {
    setToolModelConfig((prev) => ({ ...prev, qoder: null }));
  }, [setToolModelConfig]);
  const clearQoderCnModel = useCallback(() => {
    setToolModelConfig((prev) => ({ ...prev, qodercn: null }));
  }, [setToolModelConfig]);
  const qoderGlobalAccounts = useQoderAccounts(
    'qoder',
    accountGroupEnabled('qoder'),
    !!toolModelConfig.qoder,
    clearQoderGlobalModel,
    setApplyError,
    toolNavigation('qoder')
  );
  const qoderCnAccounts = useQoderAccounts(
    'qodercn',
    accountGroupEnabled('qodercn'),
    !!toolModelConfig.qodercn,
    clearQoderCnModel,
    setApplyError,
    toolNavigation('qodercn')
  );
  const qoderAccounts = qoderEdition === 'qodercn' ? qoderCnAccounts : qoderGlobalAccounts;
  const clearDuMateModel = useCallback(() => {
    setToolModelConfig((prev) => ({ ...prev, dumate: null }));
  }, [setToolModelConfig]);
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
  }, [setToolModelConfig]);
  const clearDolaModel = useCallback(() => {
    setToolModelConfig((prev) => ({ ...prev, dola: null }));
  }, [setToolModelConfig]);
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
  const doubaoSelectionRevisionRef = useRef(0);
  useEffect(() => {
    doubaoSelectionRevisionRef.current += 1;
  }, [activePage, selectedTool, doubaoAccounts.selectedId]);
  const qwenWorkEdition: api.QwenWorkEdition =
    selectedTool === 'qwenworkcn' ? 'qwenworkcn' : 'qwenwork';
  const clearQwenWorkModel = useCallback(() => {
    setToolModelConfig((prev) => ({ ...prev, qwenwork: null }));
  }, [setToolModelConfig]);
  const clearQwenWorkCnModel = useCallback(() => {
    setToolModelConfig((prev) => ({ ...prev, qwenworkcn: null }));
  }, [setToolModelConfig]);
  const qwenWorkGlobalAccounts = useQwenWorkAccounts(
    'qwenwork',
    accountGroupEnabled('qwenwork'),
    clearQwenWorkModel,
    setApplyError,
    toolNavigation('qwenwork')
  );
  const qwenWorkCnAccounts = useQwenWorkAccounts(
    'qwenworkcn',
    accountGroupEnabled('qwenworkcn'),
    clearQwenWorkCnModel,
    setApplyError,
    toolNavigation('qwenworkcn')
  );
  const qwenWorkAccounts =
    qwenWorkEdition === 'qwenworkcn' ? qwenWorkCnAccounts : qwenWorkGlobalAccounts;

  const clearDeepSeekModel = useCallback(() => {
    setToolModelConfig((prev) => ({ ...prev, dsh: null }));
  }, [setToolModelConfig]);
  const deepSeekAccounts = useDeepSeekAccounts(
    accountGroupEnabled('dsh'),
    !!toolModelConfig.dsh,
    clearDeepSeekModel,
    setApplyError
  );
  const clearGrokModel = useCallback(
    () => setToolModelConfig((prev) => ({ ...prev, grok: null })),
    [setToolModelConfig]
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
    clineSelectionRevisionRef.current += 1;
    setToolModelConfig((prev) => ({ ...prev, cline: null, clinedesktop: null }));
  }, [setToolModelConfig]);
  const clineAccounts = useClineAccounts(
    accountGroupEnabled('cline', 'clinedesktop'),
    !!(
      selectedTool &&
      ['cline', 'clinedesktop'].includes(selectedTool) &&
      toolModelConfig[selectedTool]
    ),
    clearClineModel,
    setApplyError,
    toolNavigation('cline', 'clinedesktop')
  );

  const clearKiloModel = useCallback(
    () => setToolModelConfig((prev) => ({ ...prev, kilo: null, kilodesktop: null })),
    [setToolModelConfig]
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
    toolNavigation('kilo', 'kilodesktop')
  );
  const clearCopilotModel = useCallback(
    () => setToolModelConfig((prev) => ({ ...prev, copilot: null, copilotdesktop: null })),
    [setToolModelConfig]
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
    toolNavigation('copilot', 'copilotdesktop')
  );

  const clearGrokBotModel = useCallback(
    () => setToolModelConfig((prev) => ({ ...prev, grokbot: null })),
    [setToolModelConfig]
  );
  const grokBotAccounts = useCursorAccounts(
    'grokbot',
    accountGroupEnabled('grokbot'),
    clearGrokBotModel,
    setApplyError
  );

  const clearCursorModel = useCallback(
    () => setToolModelConfig((prev) => ({ ...prev, cursor: null, cursorcli: null })),
    [setToolModelConfig]
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
  const cursorRevisionRef = useRef(0);
  useEffect(() => {
    cursorRevisionRef.current += 1;
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
  const kimiSelectionRevisionRef = useRef(0);
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
      (accountDataNeeded || (accountsEnabled && selectedTool === pluginAccountTool)),
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
    kimiSelectionRevisionRef.current += 1;
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
  const codeArtsSelectionRevisionRef = useRef(0);
  useEffect(() => {
    codeArtsSelectionRevisionRef.current += 1;
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
  const devinRevisionRef = useRef(0);
  useEffect(() => {
    devinRevisionRef.current += 1;
  }, [activePage, selectedTool, devinAccounts.selectedId]);
  const openDesignAccounts = useOpenDesignAccounts(
    accountGroupEnabled('opendesign'),
    !!toolModelConfig.opendesign,
    () => setToolModelConfig((prev) => ({ ...prev, opendesign: null })),
    setApplyError
  );

  return {
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
  };
}
