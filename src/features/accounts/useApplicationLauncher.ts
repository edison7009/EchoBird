import type { Dispatch, SetStateAction, MutableRefObject } from 'react';
import * as api from '../../api/tauri';
import type { LocalTool, ModelConfig } from '../../api/types';
import { accountError } from '../../utils/accountError';
import { isOfficialModelSentinel } from '../../data/officialEndpoints';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { launchPlugin } from '../../api/plugins';
import { switchCodeArtsAccount } from '../../api/codeArtsAccounts';
import { switchKimiAccount } from '../../api/kimiAccounts';
import { useNavigationStore } from '../../stores/navigationStore';
import type { useI18n } from '../../hooks/useI18n';
import type { useAccountDirectory } from './useAccountDirectory';
import { runAccountApplication } from './runAccountApplication';

type Directory = ReturnType<typeof useAccountDirectory>;
interface LaunchOptions extends Pick<
  Directory,
  | 'clineSelectionRevisionRef'
  | 'accountNavigation'
  | 'isCodexTool'
  | 'claudeCodeAccounts'
  | 'claudeDesktopAccounts'
  | 'workBuddyEdition'
  | 'workBuddyAccounts'
  | 'traeAccounts'
  | 'kiroAccounts'
  | 'minimaxAccounts'
  | 'qoderEdition'
  | 'qoderAccounts'
  | 'dumateAccounts'
  | 'doubaoEdition'
  | 'doubaoAccounts'
  | 'doubaoSelectionRevisionRef'
  | 'qwenWorkEdition'
  | 'qwenWorkAccounts'
  | 'deepSeekAccounts'
  | 'grokAccounts'
  | 'manusAccounts'
  | 'cueAccounts'
  | 'clineAccounts'
  | 'kiloAccounts'
  | 'copilotAccounts'
  | 'grokBotAccounts'
  | 'cursorAccounts'
  | 'cursorRevisionRef'
  | 'antigravityAccounts'
  | 'kimiSelectionRevisionRef'
  | 'pluginAccountProvider'
  | 'pluginAccountTool'
  | 'pluginAccounts'
  | 'kimiAccounts'
  | 'zcodeAccounts'
  | 'codeArtsAccounts'
  | 'codeArtsSelectionRevisionRef'
  | 'devinAccounts'
  | 'devinRevisionRef'
  | 'openDesignAccounts'
  | 'selectedCodexAccountId'
  | 'loadCodexAccounts'
> {
  selectedTool: string | null;
  isLaunching: boolean;
  launchGeneration: MutableRefObject<number>;
  previewLaunch: MutableRefObject<boolean>;
  setIsLaunching: (value: boolean) => void;
  setApplyError: (error: string | null) => void;
  detectedTools: LocalTool[];
  userModels: ModelConfig[];
  toolModelConfig: Record<string, string | null>;
  setToolModelConfig: Dispatch<SetStateAction<Record<string, string | null>>>;
  setDetectedTools: Dispatch<SetStateAction<LocalTool[]>>;
  launchAfterApply: boolean;
  agreedConfigPolicy: boolean;
  activePage: ReturnType<typeof useNavigationStore.getState>['activePage'];
  locale: string;
  t: ReturnType<typeof useI18n>['t'];
  applyRestore: (id: string) => Promise<true | string | false>;
  applyModelConfig: (id: string, model: string) => Promise<true | string | false>;
}

export function useApplicationLauncher({
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
}: LaunchOptions) {
  // Launch handler
  const handleLaunch = async () => {
    if (!selectedTool || isLaunching) return;
    const systemAppId = detectedTools.find((tool) => tool.id === selectedTool)?.systemAppId;
    if (systemAppId) {
      setIsLaunching(true);
      setApplyError(null);
      try {
        await api.launchSystemApplication(systemAppId);
      } catch (error) {
        setApplyError(String(error));
      } finally {
        setIsLaunching(false);
      }
      return;
    }
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
      (selectedTool === 'claudecode' && !!claudeCodeAccounts.selectedId!) ||
      (selectedTool === 'claudedesktop' && !!claudeDesktopAccounts.selectedId!);
    if (
      selectedTool !== 'vscode' &&
      selectedTool !== 'copilotdesktop' &&
      !workBuddyEdition &&
      !switchingClaudeAccount &&
      !((selectedTool === 'trae' || selectedTool === 'traecn') && traeAccounts.selectedId!) &&
      !(['minimaxcode', 'minimaxdesktop'].includes(selectedTool) && minimaxAccounts.selectedId!) &&
      !(['qoder', 'qodercn'].includes(selectedTool) && qoderAccounts.selectedId!) &&
      !(selectedTool === 'dumate' && dumateAccounts.selectedId!) &&
      !(['doubaowork', 'dola'].includes(selectedTool) && doubaoAccounts.selectedId!) &&
      !(['qwenwork', 'qwenworkcn'].includes(selectedTool) && qwenWorkAccounts.selectedId!) &&
      !((selectedTool === 'kirocrew' || selectedTool === 'kirocli') && kiroAccounts.selectedId!) &&
      !(selectedTool === 'zcode' && zcodeAccounts.selectedId!) &&
      !(selectedTool === 'codearts' && codeArtsAccounts.selectedId!) &&
      !(['kimicode', 'kimidesktop'].includes(selectedTool) && kimiAccounts.selectedId!) &&
      !(selectedTool === pluginAccountTool && pluginAccounts.selectedId!) &&
      !(['devin', 'devincli'].includes(selectedTool ?? '') && devinAccounts.selectedId!) &&
      !(selectedTool === 'opendesign' && openDesignAccounts.selectedId!) &&
      !(selectedTool === 'dsh' && deepSeekAccounts.selectedId!) &&
      !(selectedTool === 'grok' && grokAccounts.selectedId!) &&
      !(['cline', 'clinedesktop'].includes(selectedTool) && clineAccounts.selectedId!) &&
      !(['kilo', 'kilodesktop'].includes(selectedTool) && kiloAccounts.selectedId!) &&
      !(['copilot', 'copilotdesktop'].includes(selectedTool) && copilotAccounts.selectedId!) &&
      !(selectedTool === 'manus' && manusAccounts.selectedId!) &&
      !(selectedTool === 'cue' && cueAccounts.selectedId!) &&
      !(['cursor', 'cursorcli'].includes(selectedTool) && cursorAccounts.selectedId!) &&
      !(selectedTool === 'grokbot' && grokBotAccounts.selectedId!) &&
      !(
        (selectedTool === 'antigravity' || selectedTool === 'antigravitydesktop') &&
        antigravityAccounts.selectedId!
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
      try {
        const launchSettings = await api.getCliLaunchSettings();
        if (launchSettings?.[selectedTool]?.directoryMode === 'fixed') return undefined;
      } catch (error) {
        console.warn('[AppManager] CLI launch settings unavailable:', error);
      }
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
      antigravityAccounts.selectedId!
    ) {
      try {
        await api.switchAntigravityAccount(antigravityAccounts.selectedId!);
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
      ((selectedTool === 'kilo' || selectedTool === 'kilodesktop') && kiloAccounts.selectedId!) ||
      ((selectedTool === 'copilot' || selectedTool === 'copilotdesktop') &&
        copilotAccounts.selectedId!)
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
          const cwd = await pickWorkingDirectory();
          if (cwd === null) return;
          await api.startTool(selectedTool, toolData?.startCommand, cwd);
        }
      } catch (error) {
        setApplyError(accountError(error, t));
      } finally {
        finishLaunch();
      }
      return;
    } else if (selectedTool === 'dsh' && deepSeekAccounts.selectedId!) {
      await runAccountApplication(
        async () => {
          await api.switchDeepSeekAccount(deepSeekAccounts.selectedId!, locale);
          setDetectedTools((prev) =>
            prev.map((tool) => (tool.id === 'dsh' ? { ...tool, activeModel: '' } : tool))
          );
          await deepSeekAccounts.reload();
          if (launchAfterApply) await api.startTool('dsh');
        },
        {
          current: () => true,
          report: (error) => setApplyError(accountError(error, t)),
          finish: finishLaunch,
        }
      );
      return;
    } else if (selectedTool === 'grokbot' && grokBotAccounts.selectedId!) {
      await runAccountApplication(
        async () => {
          await api.switchGrokBotAccount(grokBotAccounts.selectedId!);
          await grokBotAccounts.reload();
          if (launchAfterApply) await api.startTool('grokbot');
        },
        {
          current: () => true,
          report: (error) => setApplyError(accountError(error, t)),
          finish: finishLaunch,
        }
      );
      return;
    } else if (['cursor', 'cursorcli'].includes(selectedTool) && cursorAccounts.selectedId!) {
      const revision = cursorRevisionRef.current;
      const current = () =>
        revision === cursorRevisionRef.current &&
        accountNavigation.current.tool === selectedTool &&
        accountNavigation.current.page === activePage;
      await runAccountApplication(
        async () => {
          const cwd = launchAfterApply ? await pickWorkingDirectory() : undefined;
          if (cwd === null || !current()) return;
          if (selectedTool === 'cursorcli')
            await api.switchCursorAccount(cursorAccounts.selectedId!, 'cursorcli');
          else await api.switchCursorAccount(cursorAccounts.selectedId!);
          if (current()) await cursorAccounts.reload();
          if (launchAfterApply && current()) {
            if (selectedTool === 'cursorcli')
              await api.startTool(selectedTool, toolData?.startCommand, cwd);
            else await api.startTool('cursor');
          }
        },
        {
          current: current,
          report: (error) => setApplyError(accountError(error, t)),
          finish: finishLaunch,
        }
      );
      return;
    } else if (
      (selectedTool === 'manus' && manusAccounts.selectedId!) ||
      (selectedTool === 'cue' && cueAccounts.selectedId!)
    ) {
      await runAccountApplication(
        async () => {
          await (selectedTool === 'cue' ? cueAccounts : manusAccounts).switchAccount();
          if (launchAfterApply) await api.startTool(selectedTool);
        },
        {
          current: () => true,
          report: (error) => setApplyError(accountError(error, t)),
          finish: finishLaunch,
        }
      );
      return;
    } else if (selectedTool === 'grok' && grokAccounts.selectedId!) {
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
    } else if (['cline', 'clinedesktop'].includes(selectedTool) && clineAccounts.selectedId!) {
      const selectionRevision = clineSelectionRevisionRef.current;
      const current = () =>
        selectionRevision === clineSelectionRevisionRef.current &&
        accountNavigation.current.tool === selectedTool &&
        accountNavigation.current.page === activePage;
      try {
        const cwd = launchAfterApply ? await pickWorkingDirectory() : undefined;
        if (cwd === null || !current()) return;
        await api.switchClineAccount(clineAccounts.selectedId!, selectedTool as api.ClineTool);
        setDetectedTools((prev) =>
          prev.map((tool) =>
            ['cline', 'clinedesktop'].includes(tool.id) ? { ...tool, activeModel: '' } : tool
          )
        );
        if (selectionRevision === clineSelectionRevisionRef.current)
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
      minimaxAccounts.selectedId!
    ) {
      await runAccountApplication(
        async () => {
          const cwd = launchAfterApply ? await pickWorkingDirectory() : undefined;
          if (cwd === null) return;
          await api.switchMinimaxAccount(
            minimaxAccounts.selectedId!,
            selectedTool as api.MinimaxTool
          );
          setDetectedTools((prev) =>
            prev.map((tool) =>
              ['minimaxcode', 'minimaxdesktop'].includes(tool.id)
                ? { ...tool, activeModel: '' }
                : tool
            )
          );
          await minimaxAccounts.reload();
          if (launchAfterApply) await api.startTool(selectedTool, toolData?.startCommand, cwd);
        },
        {
          current: () => true,
          report: (error) => setApplyError(accountError(error, t)),
          finish: finishLaunch,
        }
      );
      return;
    } else if (['doubaowork', 'dola'].includes(selectedTool) && doubaoAccounts.selectedId!) {
      const navigation = accountNavigation.current;
      const selectionRevision = doubaoSelectionRevisionRef.current;
      const current = () =>
        accountNavigation.current === navigation &&
        doubaoSelectionRevisionRef.current === selectionRevision;
      await runAccountApplication(
        async () => {
          await api.switchDoubaoAccount(doubaoEdition, doubaoAccounts.selectedId!);
          if (current()) {
            await doubaoAccounts.reload();
            if (launchAfterApply && current())
              await api.startTool(selectedTool, toolData?.startCommand);
          }
        },
        {
          current: current,
          report: (error) => setApplyError(accountError(error, t)),
          finish: finishLaunch,
        }
      );
      return;
    } else if (selectedTool === 'dumate' && dumateAccounts.selectedId!) {
      const navigation = accountNavigation.current;
      const current = () => accountNavigation.current === navigation;
      await runAccountApplication(
        async () => {
          await api.switchDuMateAccount(dumateAccounts.selectedId!);
          if (current()) {
            await dumateAccounts.reload();
            if (launchAfterApply && current())
              await api.startTool(selectedTool, toolData?.startCommand);
          }
        },
        {
          current: current,
          report: (error) => setApplyError(accountError(error, t)),
          finish: finishLaunch,
        }
      );
      return;
    } else if (['qwenwork', 'qwenworkcn'].includes(selectedTool) && qwenWorkAccounts.selectedId!) {
      const navigation = accountNavigation.current;
      const current = () => accountNavigation.current === navigation;
      await runAccountApplication(
        async () => {
          await api.switchQwenWorkAccount(qwenWorkEdition, qwenWorkAccounts.selectedId!);
          if (current()) {
            await qwenWorkAccounts.reload();
            if (launchAfterApply && current())
              await api.startTool(selectedTool, toolData?.startCommand);
          }
        },
        {
          current: current,
          report: (error) => setApplyError(accountError(error, t)),
          finish: finishLaunch,
        }
      );
      return;
    } else if (['qoder', 'qodercn'].includes(selectedTool) && qoderAccounts.selectedId!) {
      await runAccountApplication(
        async () => {
          await api.switchQoderAccount(
            qoderEdition,
            qoderAccounts.selectedId!,
            selectedTool as api.QoderTool
          );
          await qoderAccounts.reload();
          if (launchAfterApply) await api.startTool(selectedTool, toolData?.startCommand);
        },
        {
          current: () => true,
          report: (error) => setApplyError(accountError(error, t)),
          finish: finishLaunch,
        }
      );
      return;
    } else if (selectedTool === 'codearts' && codeArtsAccounts.selectedId!) {
      const selectionRevision = codeArtsSelectionRevisionRef.current;
      const current = () =>
        selectionRevision === codeArtsSelectionRevisionRef.current &&
        accountNavigation.current.tool === selectedTool &&
        accountNavigation.current.page === activePage;
      await runAccountApplication(
        async () => {
          const cwd = launchAfterApply ? await pickWorkingDirectory() : undefined;
          if (cwd === null || !current()) return;
          await switchCodeArtsAccount(codeArtsAccounts.selectedId!);
          if (current()) {
            await codeArtsAccounts.reload();
          }
          if (launchAfterApply && current())
            await api.startTool(selectedTool, toolData?.startCommand, cwd);
        },
        {
          current: current,
          report: (error) => setApplyError(accountError(error, t)),
          finish: finishLaunch,
        }
      );
      return;
    } else if (
      (selectedTool === 'devin' || selectedTool === 'devincli') &&
      devinAccounts.selectedId!
    ) {
      const revision = devinRevisionRef.current;
      const current = () =>
        revision === devinRevisionRef.current &&
        accountNavigation.current.tool === selectedTool &&
        accountNavigation.current.page === activePage;
      await runAccountApplication(
        async () => {
          const cwd = launchAfterApply ? await pickWorkingDirectory() : undefined;
          if (cwd === null || !current()) return;
          await api.switchDevinAccount(devinAccounts.selectedId!, selectedTool);
          if (current()) await devinAccounts.reload();
          if (launchAfterApply && current())
            await api.startTool(selectedTool, toolData?.startCommand, cwd);
        },
        {
          current: current,
          report: (error) => setApplyError(accountError(error, t)),
          finish: finishLaunch,
        }
      );
      return;
    } else if (selectedTool === 'opendesign' && openDesignAccounts.selectedId!) {
      await runAccountApplication(
        async () => {
          await api.switchOpenDesignAccount(openDesignAccounts.selectedId!);
          setDetectedTools((prev) =>
            prev.map((tool) => (tool.id === 'opendesign' ? { ...tool, activeModel: '' } : tool))
          );
          await openDesignAccounts.reload();
          if (launchAfterApply) await api.startTool('opendesign');
        },
        {
          current: () => true,
          report: (error) => setApplyError(accountError(error, t)),
          finish: finishLaunch,
        }
      );
      return;
    } else if (selectedTool === pluginAccountTool && pluginAccounts.selectedId!) {
      const current = () =>
        accountNavigation.current.tool === selectedTool &&
        accountNavigation.current.page === activePage;
      await runAccountApplication(
        async () => {
          const cwd = launchAfterApply ? await pickWorkingDirectory() : undefined;
          if (cwd === null || !current()) return;
          await pluginAccounts.apply(pluginAccounts.selectedId!);
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
        },
        {
          current: current,
          report: (error) => setApplyError(accountError(error, t)),
          finish: finishLaunch,
        }
      );
      return;
    } else if (['kimicode', 'kimidesktop'].includes(selectedTool) && kimiAccounts.selectedId!) {
      const selectionRevision = kimiSelectionRevisionRef.current;
      const current = () =>
        selectionRevision === kimiSelectionRevisionRef.current &&
        accountNavigation.current.tool === selectedTool &&
        accountNavigation.current.page === activePage;
      await runAccountApplication(
        async () => {
          const cwd = launchAfterApply ? await pickWorkingDirectory() : undefined;
          if (cwd === null || !current()) return;
          await switchKimiAccount(kimiAccounts.selectedId!);
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
        },
        {
          current: current,
          report: (error) => setApplyError(accountError(error, t)),
          finish: finishLaunch,
        }
      );
      return;
    } else if (selectedTool === 'zcode' && zcodeAccounts.selectedId!) {
      await runAccountApplication(
        async () => {
          await api.switchZCodeAccount(zcodeAccounts.selectedId!);
          setDetectedTools((prev) =>
            prev.map((tool) => (tool.id === 'zcode' ? { ...tool, activeModel: '' } : tool))
          );
          await zcodeAccounts.reload();
          if (launchAfterApply) await api.startTool('zcode', toolData?.startCommand);
        },
        {
          current: () => true,
          report: (error) => setApplyError(accountError(error, t)),
          finish: finishLaunch,
        }
      );
      return;
    } else if (workBuddyEdition && workBuddyAccounts.selectedId!) {
      await runAccountApplication(
        async () => {
          await api.switchWorkBuddyAccount(workBuddyEdition, workBuddyAccounts.selectedId!);
          await workBuddyAccounts.reload();
          if (launchAfterApply) await api.startTool(workBuddyEdition, toolData?.startCommand);
        },
        {
          current: () => true,
          report: (error) => setApplyError(accountError(error, t)),
          finish: finishLaunch,
        }
      );
      return;
    } else if (
      (selectedTool === 'kirocrew' || selectedTool === 'kirocli') &&
      kiroAccounts.selectedId!
    ) {
      try {
        await api.switchKiroAccount(selectedTool, kiroAccounts.selectedId!);
        await kiroAccounts.reload();
        accountApplied = true;
      } catch (error) {
        setApplyError(accountError(error, t));
        finishLaunch();
        return;
      }
    } else if ((selectedTool === 'trae' || selectedTool === 'traecn') && traeAccounts.selectedId!) {
      try {
        await api.switchTraeAccount(selectedTool, traeAccounts.selectedId!);
        await traeAccounts.reload();
        if (launchAfterApply) await api.startTool(selectedTool, toolData?.startCommand);
      } catch (error) {
        setApplyError(accountError(error, t));
      } finally {
        finishLaunch();
      }
      return;
    } else if (selectedTool === 'claudedesktop' && claudeDesktopAccounts.selectedId!) {
      await runAccountApplication(
        async () => {
          await api.switchClaudeDesktopAccount(claudeDesktopAccounts.selectedId!);
          const restored = await applyRestore(selectedTool);
          if (restored !== true)
            throw new Error(typeof restored === 'string' ? restored : t('key.destroyed'));
          await claudeDesktopAccounts.reload();
          if (launchAfterApply) await api.startTool(selectedTool, toolData?.startCommand);
        },
        {
          current: () => true,
          report: (error) => setApplyError(accountError(error, t)),
          finish: finishLaunch,
        }
      );
      return;
    } else if (selectedTool === 'claudecode' && claudeCodeAccounts.selectedId!) {
      try {
        const restored = await applyRestore(selectedTool);
        if (restored !== true)
          throw new Error(typeof restored === 'string' ? restored : t('key.destroyed'));
        await api.switchClaudeCodeAccount(claudeCodeAccounts.selectedId!);
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
        if ((selectedTool === 'kirocrew' || selectedTool === 'kirocli') && kiroAccounts.selectedId!)
          finishLaunch();
      }
    } else if ((selectedTool === 'kirocrew' || selectedTool === 'kirocli') && accountApplied) {
      finishLaunch();
    }
    if (selectedTool === 'vscode' || selectedTool === 'copilotdesktop' || workBuddyEdition)
      startCooldown();
  };

  return handleLaunch;
}
