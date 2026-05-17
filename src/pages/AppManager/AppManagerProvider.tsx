import React, { useState, useEffect, useCallback } from 'react';
import { useConfirm } from '../../components/ConfirmDialog';
import { useI18n } from '../../hooks/useI18n';
import * as api from '../../api/tauri';
import type { ModelConfig } from '../../api/types';
import { AppManagerContext } from './context';
import { useToolsStore } from '../../stores/toolsStore';
import { useNavigationStore } from '../../stores/navigationStore';
import { getOfficialEndpoint, isOfficialModelSentinel } from '../../data/officialEndpoints';

// ===== Provider =====

interface AppManagerProviderProps {
  children: React.ReactNode;
}

export const AppManagerProvider: React.FC<AppManagerProviderProps> = ({ children }) => {
  const { t, locale } = useI18n();
  const _confirm = useConfirm();

  // From stores (replaces drilled props)
  const {
    detectedTools,
    setDetectedTools,
    isScanning,
    scanTools,
    modelProtocolSelection,
    setModelProtocolSelection,
  } = useToolsStore();
  const { activePage, goToMother } = useNavigationStore();
  const isActive = activePage === 'apps';

  // Wrapped navigation: build prefill and go to Mother Agent (model check happens there)
  const handleGoToMother = useCallback(
    async (toolId: string, toolName: string) => {
      const prefill = t('mother.hintInstall').replace('{agent}', toolName);
      goToMother(prefill);
    },
    [t, goToMother]
  );

  // Load models internally
  const [userModels, setUserModels] = useState<ModelConfig[]>([]);
  useEffect(() => {
    if (api.getModels) {
      api
        .getModels()
        .then(setUserModels)
        .catch((e) => console.error('Load models failed:', e));
    }
  }, [isActive]);

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
  const [selectedTool, setSelectedTool] = useState<string | null>(null);
  const [activeToolCategory, setActiveToolCategory] = useState<string>('ALL');
  const [isLaunching, setIsLaunching] = useState(false);
  const [applyError, setApplyError] = useState<string | null>(null);

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
  // Codex-only routing toggle. When ON, apply_codex writes the real
  // upstream URL + api key to ~/.codex/* instead of the 127.0.0.1
  // proxy URL — Codex talks to relay stations (cc-vibe.com etc.)
  // directly. Default OFF: legacy behavior, proxy in the path.
  // Both Codex CLI and Codex Desktop share ~/.codex/config.toml, so
  // this is a single flag for the whole Codex family, not per-app.
  const [codexRelayMode, setCodexRelayModeRaw] = useState<boolean>(() =>
    readBool('echobird_codex_relay_mode', false)
  );

  // Tool model config (single selection - one model per tool)
  const [toolModelConfig, setToolModelConfig] = useState<Record<string, string | null>>({
    claudecode: null,
    openclaw: null,
    opencode: null,
    codex: null,
    zeroclaw: null,
    nanobot: null,
    picoclaw: null,
    openfang: null,
    hermes: null,
  });

  // Set tool model (single selection) - UI state update
  const handleSelectModel = (toolId: string, modelId: string) => {
    setToolModelConfig((prev) => ({
      ...prev,
      [toolId]: modelId,
    }));
  };

  // Get selected tool data
  const selectedToolData = detectedTools.find((t) => t.id === selectedTool);

  // Apply model config to backend (internalized from App.tsx).
  // `relayOverride` lets callers (most importantly setCodexRelayMode)
  // bypass the captured codexRelayMode value when re-applying after
  // a toggle flip — React would otherwise stale-close on the old value.
  const applyModelConfig = async (
    toolId: string,
    internalId: string,
    relayOverride?: boolean
  ): Promise<true | string | false> => {
    const model = userModels.find((m) => m.internalId === internalId);
    if (!model) {
      console.error('Model not found:', internalId);
      return false;
    }

    const toolData = detectedTools.find((t) => t.id === toolId);
    const toolProtocols = toolData?.apiProtocol || ['openai'];

    const userSelectedProtocol =
      modelProtocolSelection[model.modelId || ''] || modelProtocolSelection[internalId];
    const selectedProtocol =
      userSelectedProtocol || (toolProtocols[0] === 'anthropic' ? 'anthropic' : 'openai');

    const useAnthropicUrl = selectedProtocol === 'anthropic' && model.anthropicUrl;
    const apiUrl = useAnthropicUrl ? model.anthropicUrl! : model.baseUrl;

    // eslint-disable-next-line no-console
    console.debug(
      `[AppManager] Applying model to ${toolId}: protocol=${selectedProtocol}, url=${apiUrl}`
    );

    // Codex apps honor the relay-mode toggle from the right panel.
    // Other tools ignore the field — apply_codex is the only consumer
    // and it short-circuits on tool_id mismatch.
    const isCodexApp = toolId === 'codex' || toolId === 'codexdesktop';
    const effectiveRelay = relayOverride ?? codexRelayMode;

    try {
      const result = await api.applyModelToTool(toolId, {
        id: model.internalId,
        name: model.name,
        baseUrl: apiUrl,
        apiKey: model.apiKey,
        model: model.modelId || '',
        protocol: selectedProtocol,
        ...(isCodexApp ? { relayMode: effectiveRelay } : {}),
      });

      if (result?.success) {
        console.debug(`[AppManager] Model ${model.name} applied to ${toolId}`);
        setDetectedTools((prev) =>
          prev.map((t) =>
            t.id === toolId ? { ...t, activeModel: model.modelId || model.internalId } : t
          )
        );
        return true;
      } else {
        console.error('[AppManager] Failed to apply model:', result?.message);
        return result?.message || false;
      }
    } catch (error) {
      console.error('[AppManager] Error applying model to tool:', error);
      return false;
    }
  };

  // Relay-mode setter: flipping the toggle while a Codex app already
  // has an active model needs to rewrite ~/.codex/config.toml + auth.json
  // with the new shape immediately. Otherwise the user sees no effect
  // until the next launch. Fire-and-forget: errors surface via the
  // existing applyError modal on the next launch click.
  const setCodexRelayMode = useCallback(
    (v: boolean) => {
      setCodexRelayModeRaw(v);
      writeBool('echobird_codex_relay_mode', v);
      const codexToolId = (['codex', 'codexdesktop'] as const).find((id) => !!toolModelConfig[id]);
      if (!codexToolId) return;
      const pendingInternalId = toolModelConfig[codexToolId];
      if (!pendingInternalId || isOfficialModelSentinel(pendingInternalId)) return;
      void applyModelConfig(codexToolId, pendingInternalId, v).then((result) => {
        if (result !== true) {
          setApplyError(typeof result === 'string' ? result : t('key.destroyed'));
        }
      });
    },
    // Re-bind whenever the currently-selected model can change.
    // applyModelConfig itself is recreated on every render, so we
    // exclude it from deps to avoid an effect storm — the closure
    // captures the latest values either way via the relayOverride arg.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [toolModelConfig, t]
  );

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
    setIsLaunching(true);
    setTimeout(() => setIsLaunching(false), 3000); // 3 second cooldown

    const toolData = detectedTools.find((t) => t.id === selectedTool);
    const isLaunchable = !!toolData?.launchFile;
    const noModelConfig = !!toolData?.noModelConfig;

    // Write model config to file only when the "apply via official config" checkbox is on.
    // Launchable tools (e.g. games) always pass config via URL hash, never via file write.
    // no-model-config tools (e.g. desktop apps) skip config writes entirely.
    if (!noModelConfig && agreedConfigPolicy && !isLaunchable && toolModelConfig[selectedTool]) {
      const pending = toolModelConfig[selectedTool]!;
      const applyResult = isOfficialModelSentinel(pending)
        ? await applyRestore(selectedTool)
        : await applyModelConfig(selectedTool, pending);
      if (applyResult !== true) {
        setApplyError(typeof applyResult === 'string' ? applyResult : t('key.destroyed'));
        setIsLaunching(false);
        return;
      }
    }
    // Launch tool when "launch directly" is checked, or unconditionally for desktop apps
    if (launchAfterApply || noModelConfig) {
      if (isLaunchable) {
        // Launchable tool (e.g. game): open independent window with model config
        const selectedModelId = toolModelConfig[selectedTool];
        const selectedModel = selectedModelId
          ? userModels.find((m) => m.internalId === selectedModelId)
          : undefined;
        const modelConfig = selectedModel
          ? {
              baseUrl: selectedModel.baseUrl,
              anthropicUrl: selectedModel.anthropicUrl,
              apiKey: selectedModel.apiKey,
              model: selectedModel.modelId || selectedModel.name || 'unknown',
              name: selectedModel.name,
              protocol: modelProtocolSelection[selectedModel.modelId || ''] || 'openai',
              locale,
            }
          : { locale };
        const result = await api.launchGame(selectedTool, toolData!.launchFile!, modelConfig);
        if (result && !result.success) {
          console.error('Failed to launch:', result.message);
        }
      } else {
        try {
          await api.startTool(selectedTool, toolData?.startCommand);
        } catch (err) {
          console.error('Failed to launch tool:', err);
        }
      }
    }
  };

  return (
    <AppManagerContext.Provider
      value={{
        selectedTool,
        setSelectedTool,
        activeToolCategory,
        setActiveToolCategory,
        launchAfterApply,
        setLaunchAfterApply,
        isLaunching,
        agreedConfigPolicy,
        setAgreedConfigPolicy,
        toolModelConfig,
        handleSelectModel,
        handleRestoreModel,
        selectedToolData,
        applyError,
        setApplyError,
        detectedTools,
        setDetectedTools,
        isScanning,
        scanTools,
        userModels,
        modelProtocolSelection,
        setModelProtocolSelection,
        codexRelayMode,
        setCodexRelayMode,
        handleLaunch,
        onGoToMother: handleGoToMother,
        aiInstallableIds,
      }}
    >
      {children}
    </AppManagerContext.Provider>
  );
};
