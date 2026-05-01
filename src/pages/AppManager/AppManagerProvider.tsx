import React, { useState, useEffect, useCallback } from 'react';
import { useConfirm } from '../../components/ConfirmDialog';
import { useI18n } from '../../hooks/useI18n';
import * as api from '../../api/tauri';
import type { ModelConfig } from '../../api/types';
import { AppManagerContext } from './context';
import { useToolsStore } from '../../stores/toolsStore';
import { useNavigationStore } from '../../stores/navigationStore';
import { getOfficialEndpoint } from '../../data/officialEndpoints';

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

    // AI-installable IDs from bundled install/index.json (offline-first).
    const [aiInstallableIds, setAiInstallableIds] = useState<string[]>([]);
    useEffect(() => {
        if (!isActive) return;
        api.getInstallIndex()
            .then(s => {
                try {
                    const data = JSON.parse(s);
                    if (Array.isArray(data?.ids)) setAiInstallableIds(data.ids);
                } catch { /* malformed — keep empty */ }
            })
            .catch(() => { /* IPC error — keep empty */ });
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

    // Restore: write the tool's official vendor endpoint (e.g. ClaudeCode →
    // api.anthropic.com) so the user reverts from a third-party / proxy URL
    // back to the canonical one. Existing API key on disk is preserved by
    // reading it via getToolModelInfo before we apply.
    const handleRestoreModel = async (toolId: string) => {
        const official = getOfficialEndpoint(toolId);
        if (!official) return;

        // Reuse the API key that's already configured for this tool — most
        // users keep the same key and only swap URLs.
        let existingKey = '';
        try {
            const info = await api.getToolModelInfo(toolId);
            existingKey = info?.apiKey || '';
        } catch { /* fall through with empty key */ }

        const apiUrl = official.protocol === 'anthropic'
            ? (official.anthropicUrl || official.baseUrl)
            : official.baseUrl;

        try {
            const result = await api.applyModelToTool(toolId, {
                id: `__official__${toolId}`,
                name: official.name,
                baseUrl: apiUrl,
                apiKey: existingKey,
                model: official.modelId || '',
                protocol: official.protocol,
            });

            if (result?.success) {
                setDetectedTools(prev => prev.map(t =>
                    t.id === toolId ? { ...t, activeModel: official.modelId || official.name } : t
                ));
                setToolModelConfig(prev => ({ ...prev, [toolId]: null }));
            } else {
                setApplyError(result?.message || t('key.destroyed'));
            }
        } catch (err) {
            console.error('[AppManager] Restore-to-official failed:', err);
            setApplyError(String(err));
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
                handleRestoreModel,
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
