// App Manager Page — Tool detection, launch, model configuration
// Extracted from App.tsx with Provider pattern for shared state

import { useState, useEffect, useMemo, createContext, useContext, useCallback } from 'react';
import { Server as ServerIcon, Box as BoxIcon } from 'lucide-react';
import { ToolCard, getModelIcon } from '../components';
import { useConfirm } from '../components/ConfirmDialog';
import { useI18n } from '../hooks/useI18n';
import * as api from '../api/tauri';
import type { ModelConfig } from '../api/types';
import type { LocalTool } from '../api/types';


// ===== Context =====

interface AppManagerContextType {
    // Internalized state
    selectedTool: string | null;
    setSelectedTool: (id: string | null) => void;
    activeToolCategory: string;
    setActiveToolCategory: (cat: string) => void;
    launchAfterApply: boolean;
    setLaunchAfterApply: (v: boolean) => void;
    isLaunching: boolean;
    agreedConfigPolicy: boolean;
    setAgreedConfigPolicy: (v: boolean) => void;
    toolModelConfig: Record<string, string | null>;
    handleSelectModel: (toolId: string, modelId: string) => void;
    selectedToolData: LocalTool | undefined;
    applyError: string | null;
    setApplyError: (v: string | null) => void;
    // Shared props (from App.tsx)
    detectedTools: LocalTool[];
    setDetectedTools: React.Dispatch<React.SetStateAction<LocalTool[]>>;
    isScanning: boolean;
    scanTools: () => Promise<void>;
    userModels: ModelConfig[];
    modelProtocolSelection: Record<string, 'openai' | 'anthropic'>;
    setModelProtocolSelection: React.Dispatch<React.SetStateAction<Record<string, 'openai' | 'anthropic'>>>;
    // Launch handler
    handleLaunch: () => Promise<void>;
    // Navigation — internal handler: (toolId, toolName) => fetch install info → call prop
    onGoToMother: (toolId: string, toolName: string) => void;
    // Remote AI-installable tool IDs (from echobird.ai/api/tools/install/index.json)
    aiInstallableIds: string[];
}

const AppManagerContext = createContext<AppManagerContextType | null>(null);
const useAppManager = () => {
    const ctx = useContext(AppManagerContext);
    if (!ctx) throw new Error('useAppManager must be used within AppManagerProvider');
    return ctx;
};

// Tool categories
const toolCategories = ['ALL', 'AgentOS', 'IDE', 'CLI', 'AutoTrading', 'Game', 'Utility'] as const;

// ===== Provider =====

interface AppManagerProviderProps {
    detectedTools: LocalTool[];
    setDetectedTools: React.Dispatch<React.SetStateAction<LocalTool[]>>;
    isScanning: boolean;
    scanTools: () => Promise<void>;
    modelProtocolSelection: Record<string, 'openai' | 'anthropic'>;
    setModelProtocolSelection: React.Dispatch<React.SetStateAction<Record<string, 'openai' | 'anthropic'>>>;
    isActive?: boolean;
    onGoToMother?: (prefill: string) => void;
    children: React.ReactNode;
}

export const AppManagerProvider: React.FC<AppManagerProviderProps> = ({
    detectedTools, setDetectedTools, isScanning, scanTools,
    modelProtocolSelection, setModelProtocolSelection,
    isActive,
    onGoToMother = (_prefill: string) => { },
    children,
}) => {
    const { t } = useI18n();
    const confirm = useConfirm();

    // Wrapped navigation: build prefill and go to Mother Agent (model check happens there)
    const handleGoToMother = useCallback(async (toolId: string, toolName: string) => {
        const prefill = t('mother.hintInstall').replace('{agent}', toolName);
        onGoToMother(prefill);
    }, [t, onGoToMother]);

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
        'workbuddy': null,
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
                } : undefined;
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

// ===== Main Content (tool cards grid) =====

export const AppManagerMain: React.FC = () => {
    const { t } = useI18n();
    const {
        detectedTools, isScanning, scanTools,
        activeToolCategory, setActiveToolCategory,
        selectedTool, setSelectedTool,
        onGoToMother,
        aiInstallableIds,
    } = useAppManager();

    return (
        <div className="flex-1 flex flex-col overflow-hidden">
            {/* Toolbar - Fixed */}
            {/* Category tabs + Action buttons */}
            <div className="flex items-center justify-between flex-shrink-0 pb-4 mb-4">
                <div className="flex gap-1">
                    {toolCategories.map(cat => (
                        <button
                            key={cat}
                            onClick={() => setActiveToolCategory(cat)}
                            className={`px-4 py-2 text-xs transition-colors outline-none ${activeToolCategory === cat
                                ? 'text-cyber-accent font-bold border-b-2 border-cyber-accent'
                                : 'text-cyber-text-secondary hover:text-cyber-accent'
                                }`}
                        >
                            {(() => {
                                const catMap: Record<string, string> = {
                                    'ALL': 'toolCat.all', 'AgentOS': 'toolCat.agentOS',
                                    'IDE': 'toolCat.ide', 'CLI': 'toolCat.cli',
                                    'AutoTrading': 'toolCat.autoTrading', 'Game': 'toolCat.game',
                                    'Utility': 'toolCat.utility'
                                };
                                return t((catMap[cat] || cat) as any);
                            })()}
                        </button>
                    ))}
                </div>
                <div className="flex items-center gap-2">
                    <button
                        onClick={scanTools}
                        disabled={isScanning}
                        className="text-[10px] border border-cyber-accent text-cyber-accent px-3 py-1 hover:bg-cyber-accent/10 transition-colors rounded disabled:opacity-50 outline-none"
                    >
                        {isScanning ? t('status.scanning') : t('btn.refresh')}
                    </button>
                </div>
            </div>
            {/* Tool cards - Scrolling */}
            <div className="flex-1 overflow-y-auto">
                <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4">
                    {isScanning && detectedTools.length === 0 ? (
                        // Skeleton cards while scanning
                        <>
                            {Array.from({ length: 6 }).map((_, i) => (
                                <div key={i} className="p-5 border border-cyber-border rounded-card bg-black/80 flex flex-col animate-pulse">
                                    <div className="absolute top-4 right-4 w-10 h-10 rounded-lg bg-cyber-border/30" />
                                    <div className="h-5 w-2/3 bg-cyber-border/40 rounded mb-4" />
                                    <div className="space-y-2">
                                        <div className="h-3 w-4/5 bg-cyber-border/30 rounded" />
                                        <div className="h-3 w-3/5 bg-cyber-border/30 rounded" />
                                        <div className="h-3 w-4/5 bg-cyber-border/30 rounded" />
                                        <div className="h-3 w-2/5 bg-cyber-border/30 rounded" />
                                    </div>
                                </div>
                            ))}
                        </>
                    ) : (
                        detectedTools
                            .filter(tool => activeToolCategory === 'ALL' || tool.category === activeToolCategory)
                            .sort((a, b) => {
                                // 1. Installed first
                                if (a.installed !== b.installed) return a.installed ? -1 : 1;
                                // 2. Within same install status: AI auto-installable (remote index) first
                                const aHasRemote = aiInstallableIds.includes(a.id);
                                const bHasRemote = aiInstallableIds.includes(b.id);
                                if (aHasRemote !== bHasRemote) return aHasRemote ? -1 : 1;
                                // 3. Then by category
                                const categoryOrder: Record<string, number> = { 'AgentOS': 0, 'IDE': 1, 'CLI': 2, 'AutoTrading': 3, 'Game': 4, 'Utility': 5 };
                                return (categoryOrder[a.category || ''] ?? 99) - (categoryOrder[b.category || ''] ?? 99);
                            })
                            .map(tool => (
                                <ToolCard
                                    key={tool.id}
                                    {...tool}
                                    selected={selectedTool === tool.id}
                                    onClick={() => setSelectedTool(tool.id)}
                                    hasRemoteInstall={aiInstallableIds.includes(tool.id)}
                                    onMotherAgentInstall={() => onGoToMother(tool.id, tool.displayName || tool.name)}
                                />
                            ))
                    )}
                </div>
            </div>
        </div>
    );
};

// ===== Model List Section (extracted from IIFE) =====

interface ModelListSectionProps {
    selectedToolData: LocalTool;
    userModels: ModelConfig[];
    toolModelConfig: Record<string, string | null>;
    selectedTool: string | null;
    handleSelectModel: (toolId: string, modelId: string) => void;
    modelProtocolSelection: Record<string, 'openai' | 'anthropic'>;
    setModelProtocolSelection: React.Dispatch<React.SetStateAction<Record<string, 'openai' | 'anthropic'>>>;
    t: (key: any) => string;
}

const ModelListSection: React.FC<ModelListSectionProps> = ({
    selectedToolData, userModels, toolModelConfig, selectedTool,
    handleSelectModel, modelProtocolSelection, setModelProtocolSelection, t,
}) => {
    const toolProtocols = selectedToolData.apiProtocol || ['openai', 'anthropic'];

    const { localModels, cloudModels } = useMemo(() => {
        const compatible = userModels.filter(model => {
            const hasOpenAI = toolProtocols.includes('openai') && !!model.baseUrl;
            const hasAnthropic = toolProtocols.includes('anthropic') && !!model.anthropicUrl;
            return hasOpenAI || hasAnthropic;
        });
        return {
            localModels: compatible.filter(m => m.internalId === 'local-server'),
            cloudModels: compatible.filter(m => m.internalId !== 'local-server'),
        };
    }, [userModels, toolProtocols]);

    const renderModelCard = (model: typeof userModels[0]) => {
        const isSelected = selectedTool ? toolModelConfig[selectedTool] === model.internalId : false;
        const isLocalServer = model.internalId === 'local-server';

        const modelHasBoth = !!(model.baseUrl && model.anthropicUrl);
        const toolSupportsBoth = toolProtocols.includes('openai') && toolProtocols.includes('anthropic');
        const showSwitcher = modelHasBoth && toolSupportsBoth;

        let currentProtocol = 'openai';
        if (toolSupportsBoth) {
            currentProtocol = modelProtocolSelection[model.modelId || ''] ||
                (toolProtocols[0] === 'anthropic' ? 'anthropic' : 'openai');
        } else {
            currentProtocol = toolProtocols[0];
        }

        const displayUrl = currentProtocol === 'anthropic'
            ? (model.anthropicUrl || model.baseUrl)
            : (model.baseUrl || model.anthropicUrl);
        const apiPath = (() => {
            try {
                const url = new URL(displayUrl || '');
                const path = url.pathname === '/' ? '' : url.pathname;
                return url.hostname + path;
            } catch {
                return displayUrl || 'No URL Configured';
            }
        })();

        const iconSrc = isLocalServer ? null : getModelIcon(model.name, model.modelId || '');

        return (
            <div
                key={model.internalId}
                className={`p-3 border rounded cursor-pointer transition-all mb-2 flex items-center gap-3 ${isSelected
                    ? isLocalServer
                        ? 'border-cyan-400 bg-cyan-400/10'
                        : 'border-cyber-accent bg-cyber-accent/10'
                    : isLocalServer
                        ? 'border-cyan-400/50 hover:border-cyan-400 bg-cyan-400/5'
                        : 'border-cyber-border hover:border-cyber-accent/50'
                    }`}
                onClick={() => selectedTool && handleSelectModel(selectedTool, model.internalId)}
            >
                {/* Left: Radio + Icon */}
                <div className="flex items-center gap-3 flex-shrink-0">
                    <div className={`w-4 h-4 rounded-full border-2 flex items-center justify-center ${isSelected
                        ? isLocalServer ? 'border-cyan-400' : 'border-cyber-accent'
                        : 'border-cyber-border'
                        }`}>
                        {isSelected && <div className={`w-2 h-2 rounded-full ${isLocalServer ? 'bg-cyan-400' : 'bg-cyber-accent'}`} />}
                    </div>
                    {isLocalServer ? (
                        <div className="w-6 h-6 rounded bg-cyan-400/15 flex items-center justify-center text-cyan-400">
                            <ServerIcon size={14} />
                        </div>
                    ) : (
                        iconSrc && <img src={iconSrc} alt="" className="w-6 h-6" />
                    )}
                </div>

                {/* Right: Two-row layout */}
                <div className="flex-1 min-w-0 flex flex-col justify-center h-10">
                    <div className="flex items-center gap-2">
                        <div className={`text-sm font-bold truncate leading-none flex-1 min-w-0 ${isLocalServer ? 'text-cyan-400' : ''}`}>{model.name || 'Untitled Model'}</div>
                        {showSwitcher && (
                            <span
                                className={`text-[10px] font-mono cursor-pointer select-none flex-shrink-0 transition-colors ${isLocalServer
                                    ? 'text-cyan-400/60 hover:text-cyan-400'
                                    : 'text-cyber-text-muted/60 hover:text-cyber-accent'
                                    }`}
                                onClick={(e) => {
                                    e.stopPropagation();
                                    const newProtocol = currentProtocol === 'openai' ? 'anthropic' : 'openai';
                                    setModelProtocolSelection(prev => ({ ...prev, [model.modelId || '']: newProtocol }));
                                }}
                            >
                                {currentProtocol === 'openai' ? 'OpenAI' : 'Anthropic'} <span className="text-[8px]">⇄</span>
                            </span>
                        )}
                    </div>
                    <div className="text-[10px] text-cyber-text-secondary truncate leading-tight mt-1 opacity-70">
                        {apiPath}
                    </div>
                </div>
            </div>
        );
    };

    return (
        <>
            {/* Local models area */}
            {localModels.length > 0 && (
                <div className="mb-4">
                    <div className="text-xs text-cyan-400/80 mb-2">
                        {t('agent.myLocalModel')}:
                    </div>
                    {localModels.map(renderModelCard)}
                </div>
            )}
            {/* Cloud models area */}
            <div className="text-xs text-cyber-text-secondary mb-3">
                {t('agent.selectModelFor')} {selectedToolData.name}:
            </div>
            {cloudModels.length > 0 ? (
                <div className="space-y-2">
                    {cloudModels.map(renderModelCard)}
                </div>
            ) : localModels.length === 0 ? (
                <div className="py-10 flex flex-col items-center gap-3 text-center">
                    <BoxIcon size={28} className="text-cyber-accent opacity-25" />
                    <p className="text-[12px] text-cyber-text-secondary font-mono leading-relaxed">
                        {t('agent.noModelsTitle')}<br />
                        {t('agent.noModelsHintPre')} <span className="text-cyber-accent font-bold">{t('nav.modelNexus')}</span> {t('agent.noModelsHintPost')}
                    </p>
                </div>
            ) : null}
        </>
    );
};

// ===== Right Panel (config panel with tabs) =====

export const AppManagerPanel: React.FC = () => {
    const { t } = useI18n();
    const {
        selectedToolData, selectedTool,
        userModels, toolModelConfig, handleSelectModel,
        modelProtocolSelection, setModelProtocolSelection,
    } = useAppManager();




    return (
        <>
            {/* Header */}
            <div className="p-2 flex items-center justify-between bg-transparent">
                <div className="flex gap-1">
                    <span className="px-3 py-1.5 text-xs font-bold text-cyber-accent">
                        {t('agent.modelsTab')}
                    </span>
                </div>
                {selectedToolData && (
                    <span className="text-[10px] text-cyber-accent">
                        {selectedToolData.name}
                    </span>
                )}
            </div>

            <div className="flex-1 p-2 overflow-y-auto">
                {selectedToolData ? (
                    <div className="space-y-2">
                        <ModelListSection
                            selectedToolData={selectedToolData}
                            userModels={userModels}
                            toolModelConfig={toolModelConfig}
                            selectedTool={selectedTool}
                            handleSelectModel={handleSelectModel}
                            modelProtocolSelection={modelProtocolSelection}
                            setModelProtocolSelection={setModelProtocolSelection}
                            t={t}
                        />
                    </div>
                ) : (
                    <p className="text-cyber-text-secondary text-center py-10">
                        {t('agent.selectTool')}
                    </p>
                )}
            </div>
        </>
    );
};

// ===== Bottom Bar (launch area) =====

export const AppManagerBottom: React.FC = () => {
    const { t } = useI18n();
    const {
        selectedTool, toolModelConfig,
        launchAfterApply, setLaunchAfterApply,
        isLaunching, agreedConfigPolicy, setAgreedConfigPolicy,
        handleLaunch,
    } = useAppManager();

    const hasModel = !!(selectedTool && toolModelConfig[selectedTool]);

    return (
        <div className="flex-shrink-0 flex flex-col mt-2">
            <div className="mx-2 border-t border-cyber-border"></div>
            <div className="flex items-center justify-end gap-8 px-6 py-5">
                {/* Developer invite hint */}
                <div className="flex-1 text-[13px] text-cyber-text-muted/40">{t('hint.devInvite')}</div>
                {/* Launch button */}
                <button
                    onClick={handleLaunch}
                    disabled={!hasModel || !agreedConfigPolicy || isLaunching}
                    className={`w-64 h-14 text-lg font-bold font-mono tracking-widest transition-all flex-shrink-0 rounded-lg cjk-btn ${(!hasModel || !agreedConfigPolicy || isLaunching)
                        ? 'bg-cyber-border text-cyber-text-secondary cursor-not-allowed'
                        : 'bg-cyber-accent text-black hover:bg-cyber-accent/90 hover:shadow-[0_0_15px_rgba(0,255,157,0.35)] shadow-[0_0_8px_rgba(0,255,157,0.15)]'
                        }`}
                >
                    {launchAfterApply ? t('btn.launchApp') : t('btn.modifyOnly')}
                </button>
                {/* Checkboxes */}
                <div className="flex flex-col gap-2">
                    {/* Apply & Launch checkbox */}
                    <label className="flex items-center gap-2 cursor-pointer select-none" onClick={() => setLaunchAfterApply(!launchAfterApply)}>
                        <div className={`w-3.5 h-3.5 border flex items-center justify-center transition-all flex-shrink-0 ${launchAfterApply ? 'border-cyber-accent bg-cyber-accent/20' : 'border-cyber-border hover:border-cyber-text-muted'
                            }`}>
                            {launchAfterApply && (
                                <svg width="8" height="8" viewBox="0 0 10 10" fill="none">
                                    <path d="M2 5L4 7L8 3" stroke="#00FF9D" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
                                </svg>
                            )}
                        </div>
                        <span className={`text-xs font-mono transition-colors ${launchAfterApply ? 'text-cyber-accent' : 'text-cyber-text-secondary'}`}>
                            {t('agent.applyAndLaunch')}
                        </span>
                    </label>
                    {/* Config policy agreement */}
                    <label className="flex items-center gap-2 cursor-pointer select-none" onClick={() => setAgreedConfigPolicy(!agreedConfigPolicy)}>
                        <div className={`w-3.5 h-3.5 border flex items-center justify-center transition-all flex-shrink-0 ${agreedConfigPolicy ? 'border-cyber-accent bg-cyber-accent/20' : 'border-cyber-border hover:border-cyber-text-muted'
                            }`}>
                            {agreedConfigPolicy && (
                                <svg width="8" height="8" viewBox="0 0 10 10" fill="none">
                                    <path d="M2 5L4 7L8 3" stroke="#00FF9D" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
                                </svg>
                            )}
                        </div>
                        <span className={`text-xs font-mono transition-colors ${agreedConfigPolicy ? 'text-cyber-accent' : 'text-cyber-text-secondary'}`}>
                            {t('agent.appliedVia')}
                        </span>
                    </label>
                </div>
            </div>
        </div>
    );
};

// ===== Apply Error Modal =====

export const AppManagerErrorModal: React.FC = () => {
    const { t } = useI18n();
    const { applyError, setApplyError } = useAppManager();

    if (!applyError) return null;

    return (
        <div className="fixed inset-0 z-[9998] flex items-center justify-center">
            <div className="absolute inset-0 bg-black/60 backdrop-blur-sm" onClick={() => setApplyError(null)} />
            <div className="relative w-[360px] max-w-[90vw] border border-red-500/40 bg-cyber-bg shadow-lg shadow-[0_0_20px_rgba(255,60,60,0.1)] rounded-xl overflow-hidden">
                <div className="h-[2px] w-full bg-red-500/60" />
                <div className="px-5 pt-4 pb-2 flex items-center gap-2">
                    <svg className="w-4 h-4 text-red-400 flex-shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z" /><line x1="12" y1="9" x2="12" y2="13" /><line x1="12" y1="17" x2="12.01" y2="17" /></svg>
                    <span className="text-sm font-mono font-bold tracking-wider text-red-400">API Key Warning</span>
                </div>
                <div className="px-5 pb-5">
                    <p className="text-xs text-cyber-text-secondary leading-relaxed font-mono">{applyError}</p>
                </div>
                <div className="flex border-t border-cyber-border">
                    <button
                        onClick={() => setApplyError(null)}
                        className="flex-1 px-4 py-2.5 text-xs font-mono font-bold tracking-wider text-red-400 hover:bg-red-500/10 hover:text-red-300 transition-all"
                    >
                        {t('common.confirm')}
                    </button>
                </div>
            </div>
        </div>
    );
};
