import React, { useState, useEffect, useCallback } from 'react';
import { useConfirm } from '../../components/ConfirmDialog';
import { useI18n } from '../../hooks/useI18n';
import * as api from '../../api/tauri';
import type { ModelConfig } from '../../api/types';
import { AppManagerContext } from './context';
import { useToolsStore } from '../../stores/toolsStore';
import { useNavigationStore } from '../../stores/navigationStore';

// ===== Provider =====

interface AppManagerProviderProps {
    children: React.ReactNode;
}

export const AppManagerProvider: React.FC<AppManagerProviderProps> = ({ children }) => {
    const { t, locale } = useI18n();
    const confirm = useConfirm();

    // From stores (replaces drilled props)
    const { detectedTools, setDetectedTools, isScanning, scanTools, modelProtocolSelection, setModelProtocolSelection } = useToolsStore();
    const { activePage, goToMother } = useNavigationStore();
    const isActive = activePage === 'apps';

    // Wrapped navigation: build prefill and go to Mother Agent (model check happens there)
    const handleGoToMother = useCallback(async (toolId: string, toolName: string) => {
        const prefill = t('mother.hintInstall').replace('{agent}', toolName);
        goToMother(prefill);
    }, [t, goToMother]);

    // Load models internally
    const [userModels, setUserModels] = useState<ModelConfig[]>([]);
    useEffect(() => {
        if (api.getModels) {
            api.getModels().then(setUserModels).catch(e => console.error('Load models failed:', e));
        }
    }, [isActive]);

    // Remote AI-installable IDs: fetch from echobird.ai/api/tools/install/index.json on activate
    const [aiInstallableIds, setAiInstallableIds] = useState<string[]>([]);
    useEffect(() => {
        if (!isActive) return;
        fetch('https://echobird.ai/api/tools/install/index.json', { signal: AbortSignal.timeout(6000) })
            .then(r => r.json())
            .then(data => { if (Array.isArray(data?.ids)) setAiInstallableIds(data.ids); })
            .catch(() => { /* network error — keep empty */ });
    }, [isActive]);

    // Internalized state
    const [selectedTool, setSelectedTool] = useState<string | null>(null);
    const [activeToolCategory, setActiveToolCategory] = useState<string>('ALL');
    const [launchAfterApply, setLaunchAfterApply] = useState(true);
    const [isLaunching, setIsLaunching] = useState(false);
    const [agreedConfigPolicy, setAgreedConfigPolicy] = useState(true);
    const [applyError, setApplyError] = useState<string | null>(null);

    // Tool model config (single selection - one model per tool)
    const [toolModelConfig, setToolModelConfig] = useState<Record<string, string | null>>({
        'claudecode': null,
        'openclaw': null,
        'opencode': null,
        'codex': null,
        'zeroclaw': null,
        'nanobot': null,
        'picoclaw': null,
        'openfang': null,
        'hermes': null,
    });

    // Set tool model (single selection) - UI state update
    const handleSelectModel = (toolId: string, modelId: string) => {
        setToolModelConfig(prev => ({
            ...prev,
            [toolId]: modelId
        }));
    };

    // Original model snapshot: persisted per-tool to localStorage on first detection
    const ORIGINAL_STORAGE_KEY = 'echobird:original-tool-model';
    const [originalToolModel, setOriginalToolModel] = useState<Record<string, string>>(() => {
        try {
            const raw = localStorage.getItem(ORIGINAL_STORAGE_KEY);
            return raw ? JSON.parse(raw) : {};
        } catch {
            return {};
        }
    });

    // Snapshot any newly-detected tool's activeModel as its "original" (one-shot per tool)
    useEffect(() => {
        if (!detectedTools.length) return;
        let dirty = false;
        const next = { ...originalToolModel };
        for (const tool of detectedTools) {
            if (tool.activeModel && next[tool.id] === undefined) {
                next[tool.id] = tool.activeModel;
                dirty = true;
            }
        }
        if (dirty) {
            setOriginalToolModel(next);
            try { localStorage.setItem(ORIGINAL_STORAGE_KEY, JSON.stringify(next)); } catch { /* quota / private mode */ }
        }
    }, [detectedTools]);

    // Get selected tool data
    const selectedToolData = detectedTools.find(t => t.id === selectedTool);

    // Apply model config to backend (internalized from App.tsx)
    const applyModelConfig = async (toolId: string, internalId: string): Promise<true | string | false> => {
        const model = userModels.find(m => m.internalId === internalId);
        if (!model) {
            console.error('Model not found:', internalId);
            return false;
        }

        const toolData = detectedTools.find(t => t.id === toolId);
        const toolProtocols = toolData?.apiProtocol || ['openai'];

        const userSelectedProtocol = modelProtocolSelection[model.modelId || ''] || modelProtocolSelection[internalId];
        const selectedProtocol = userSelectedProtocol || (toolProtocols[0] === 'anthropic' ? 'anthropic' : 'openai');

        const useAnthropicUrl = selectedProtocol === 'anthropic' && model.anthropicUrl;
        const apiUrl = useAnthropicUrl ? model.anthropicUrl! : model.baseUrl;

        console.debug(`[AppManager] Applying model to ${toolId}: protocol=${selectedProtocol}, url=${apiUrl}`);

        try {
            const result = await api.applyModelToTool(toolId, {
                id: model.internalId,
                name: model.name,
                baseUrl: apiUrl,
                apiKey: model.apiKey,
                model: model.modelId || '',
                proxyUrl: model.proxyUrl,
                protocol: selectedProtocol
            });

            if (result?.success) {
                console.debug(`[AppManager] Model ${model.name} applied to ${toolId}`);
                setDetectedTools(prev => prev.map(t =>
                    t.id === toolId ? { ...t, activeModel: model.modelId || model.internalId } : t
                ));
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

    // Restore: revert the tool's backend config to its original model and update UI selection
    const handleRestoreModel = async (toolId: string) => {
        const originalModelId = originalToolModel[toolId];
        if (!originalModelId) return;
        // Find a userModel whose modelId matches the original (the snapshot stores modelId, not internalId)
        const match = userModels.find(m => m.modelId === originalModelId || m.internalId === originalModelId);
        if (!match) {
            setApplyError(t('agent.restoreUnavailable').replace('{model}', originalModelId));
            return;
        }
        // Update UI selection
        setToolModelConfig(prev => ({ ...prev, [toolId]: match.internalId }));
        // Apply to backend so the tool actually uses the original again
        const result = await applyModelConfig(toolId, match.internalId);
        if (result !== true) {
            setApplyError(typeof result === 'string' ? result : t('key.destroyed'));
        }
    };

    // Launch handler
    const handleLaunch = async () => {
        if (!selectedTool || isLaunching) return;
        setIsLaunching(true);
        setTimeout(() => setIsLaunching(false), 3000); // 3 second cooldown

        const toolData = detectedTools.find(t => t.id === selectedTool);
        const isLaunchable = !!toolData?.launchFile;

        // Apply model config (if model selected) — skip for launchable tools (they get config via URL hash)
        if (!isLaunchable && toolModelConfig[selectedTool]) {
            const applyResult = await applyModelConfig(selectedTool, toolModelConfig[selectedTool]!);
            if (applyResult !== true) {
                setApplyError(typeof applyResult === 'string' ? applyResult : t('key.destroyed'));
                setIsLaunching(false);
                return;
            }
        }
        // Only launch tool when checkbox is checked
        if (launchAfterApply) {
            if (isLaunchable) {
                // Launchable tool (e.g. game): open independent window with model config
                const selectedModelId = toolModelConfig[selectedTool];
                const selectedModel = selectedModelId ? userModels.find(m => m.internalId === selectedModelId) : undefined;
                const modelConfig = selectedModel ? {
                    baseUrl: selectedModel.baseUrl,
                    anthropicUrl: selectedModel.anthropicUrl,
                    apiKey: selectedModel.apiKey,
                    model: selectedModel.modelId || selectedModel.name || 'unknown',
                    name: selectedModel.name,
                    protocol: modelProtocolSelection[selectedModel.modelId || ''] || 'openai',
                    locale,
                } : { locale };
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
                selectedTool, setSelectedTool,
                activeToolCategory, setActiveToolCategory,
                launchAfterApply, setLaunchAfterApply,
                isLaunching, agreedConfigPolicy, setAgreedConfigPolicy,
                toolModelConfig, handleSelectModel,
                originalToolModel, handleRestoreModel,
                selectedToolData, applyError, setApplyError,
                detectedTools, setDetectedTools,
                isScanning, scanTools,
                userModels, modelProtocolSelection, setModelProtocolSelection,
                handleLaunch,
                onGoToMother: handleGoToMother,
                aiInstallableIds,
            }}
        >
            {children}
        </AppManagerContext.Provider>
    );
};
