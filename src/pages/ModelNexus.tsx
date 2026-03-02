// Model Nexus Page — Model cards, debug console, add/edit modal
// Extracted from App.tsx with Provider pattern for shared state

import { useState, useEffect, useRef, useCallback, createContext, useContext } from 'react';
import { X } from 'lucide-react';
import { MiniSelect } from '../components/MiniSelect';
import { ModelCard, ModelCardSkeleton, getModelIcon } from '../components';
import { useI18n } from '../hooks/useI18n';
import * as api from '../api/tauri';
import type { SSNodeConfig, ModelConfig } from '../api/types';

// ===== Context =====

interface ModelNexusCtx {
    // Models
    userModels: ModelConfig[];
    setUserModels: React.Dispatch<React.SetStateAction<ModelConfig[]>>;
    isLoadingModels: boolean;
    selectedModel: string | null;
    setSelectedModel: (id: string | null) => void;
    selectedModelData: ModelConfig | undefined;
    // Test
    testInput: string;
    setTestInput: (v: string) => void;
    testOutput: string[];
    setTestOutput: React.Dispatch<React.SetStateAction<string[]>>;
    isTesting: boolean;
    arrowIndex: number;
    testProtocol: 'openai' | 'anthropic';
    setTestProtocol: (v: 'openai' | 'anthropic') => void;
    modelLatencies: Record<string, number>;
    pingingModelIds: Set<string>;
    modelTerminals: Record<string, { input: string; output: string[] }>;
    setModelTerminals: React.Dispatch<React.SetStateAction<Record<string, { input: string; output: string[] }>>>;
    testInputRef: React.RefObject<HTMLInputElement>;
    inputFocused: boolean;
    setInputFocused: (v: boolean) => void;
    cursorPos: number;
    setCursorPos: (v: number) => void;
    // Modal
    showAddModelModal: boolean;
    setShowAddModelModal: (v: boolean) => void;
    modelModalAnimatingOut: boolean;
    editingModelId: string | null;
    setEditingModelId: (v: string | null) => void;
    newModelForm: NewModelForm;
    setNewModelForm: React.Dispatch<React.SetStateAction<NewModelForm>>;
    showApiKey: boolean;
    setShowApiKey: (v: boolean) => void;
    keyDestroyed: boolean;
    setKeyDestroyed: (v: boolean) => void;
    closeModelModal: () => void;
    // Actions
    pingAllModels: () => Promise<void>;
    handleTestModel: () => Promise<void>;
}

interface NewModelForm {
    name: string;
    baseUrl: string;
    anthropicUrl: string;
    apiKey: string;
    modelId: string;
    useProxy: boolean;
    ssServer: string;
    ssPort: string;
    ssCipher: string;
    ssPassword: string;
}

const ModelNexusContext = createContext<ModelNexusCtx | null>(null);
const useModelNexus = () => {
    const ctx = useContext(ModelNexusContext);
    if (!ctx) throw new Error('useModelNexus must be used within ModelNexusProvider');
    return ctx;
};

// ===== Provider =====

export function ModelNexusProvider({ children }: { children: React.ReactNode }) {
    // Models state
    const [userModels, setUserModels] = useState<ModelConfig[]>([]);
    const [isLoadingModels, setIsLoadingModels] = useState(true);
    const [selectedModel, setSelectedModel] = useState<string | null>('gpt4o');
    const [pingingModelIds, setPingingModelIds] = useState<Set<string>>(new Set());

    // Modal state
    const [showAddModelModal, setShowAddModelModal] = useState(false);
    const [modelModalAnimatingOut, setModelModalAnimatingOut] = useState(false);
    const [editingModelId, setEditingModelId] = useState<string | null>(null);
    const [showApiKey, setShowApiKey] = useState(false);
    const [keyDestroyed, setKeyDestroyed] = useState(false);
    const [newModelForm, setNewModelForm] = useState<NewModelForm>({
        name: '',
        baseUrl: '',
        anthropicUrl: '',
        apiKey: '',
        modelId: '',
        useProxy: false,
        ssServer: '',
        ssPort: '',
        ssCipher: 'aes-128-gcm',
        ssPassword: ''
    });

    const closeModelModal = useCallback(() => {
        setModelModalAnimatingOut(true);
        setTimeout(() => {
            setModelModalAnimatingOut(false);
            setShowAddModelModal(false);
            setEditingModelId(null);
        }, 200);
    }, []);

    // Test state
    const [testInput, setTestInput] = useState('');
    const [testOutput, setTestOutput] = useState<string[]>([]);
    const [isTesting, setIsTesting] = useState(false);
    const [arrowIndex, setArrowIndex] = useState(0);
    const [modelLatencies, setModelLatencies] = useState<Record<string, number>>({});
    const [modelTerminals, setModelTerminals] = useState<Record<string, { input: string; output: string[] }>>({});
    const [testProtocol, setTestProtocol] = useState<'openai' | 'anthropic'>('openai');
    const testInputRef = useRef<HTMLInputElement>(null!);
    const [inputFocused, setInputFocused] = useState(false);
    const [cursorPos, setCursorPos] = useState(0);

    // Derived
    const selectedModelData = userModels.find(m => m.internalId === selectedModel);

    // Load models from config
    useEffect(() => {
        const loadModels = async () => {
            setIsLoadingModels(true);
            if (api.getModels) {
                try {
                    const models = await api.getModels();
                    setUserModels(models);
                } catch (error) {
                    console.error('Load models failed:', error);
                }
            }
            setIsLoadingModels(false);
        };
        loadModels();
    }, []);

    // Auto-fill Model ID and API Key for local models
    useEffect(() => {
        const isLocal = (url: string) => url.includes('localhost') || url.includes('127.0.0.1');
        const hasLocalUrl = isLocal(newModelForm.baseUrl) || isLocal(newModelForm.anthropicUrl);

        if (hasLocalUrl) {
            setNewModelForm(prev => {
                const updates: any = {};
                if (!prev.modelId) updates.modelId = 'local-model';
                if (!prev.apiKey) updates.apiKey = 'not-needed';
                return Object.keys(updates).length > 0 ? { ...prev, ...updates } : prev;
            });
        }
    }, [newModelForm.baseUrl, newModelForm.anthropicUrl]);

    // Marquee animation
    useEffect(() => {
        if (!isTesting) return;
        const timer = setInterval(() => {
            setArrowIndex(prev => (prev + 1) % 4);
        }, 200);
        return () => clearInterval(timer);
    }, [isTesting]);

    // Listen for model selection change - auto restore terminal history and focus
    useEffect(() => {
        if (selectedModel && modelTerminals[selectedModel]) {
            const saved = modelTerminals[selectedModel];
            setTestInput(saved?.input || '');
            setTestOutput(saved?.output || []);
        } else {
            setTestInput('');
            setTestOutput([]);
        }
        testInputRef.current?.focus();
    }, [selectedModel]);

    // Listen for protocol change - focus input
    useEffect(() => {
        testInputRef.current?.focus();
    }, [testProtocol]);

    // ping --all
    const pingAllModels = async () => {
        if (isTesting) return;
        setIsTesting(true);
        const allModels = userModels;
        setPingingModelIds(new Set(allModels.map(m => m.internalId)));
        for (const model of allModels) {
            try {
                const result = await api.pingModel(model.internalId);
                setPingingModelIds(prev => {
                    const next = new Set(prev);
                    next.delete(model.internalId);
                    return next;
                });
                if (result?.success) {
                    setModelLatencies(prev => ({ ...prev, [model.internalId]: result.latency }));
                }
            } catch {
                setPingingModelIds(prev => {
                    const next = new Set(prev);
                    next.delete(model.internalId);
                    return next;
                });
            }
        }
        setPingingModelIds(new Set());
        setIsTesting(false);
    };

    // Model test function
    const handleTestModel = async () => {
        if (!testInput.trim() || !selectedModel || isTesting) return;

        const prompt = testInput.trim();
        setTestInput('');
        setIsTesting(true);
        testInputRef.current?.blur();

        // Smart protocol selection
        let effectiveProtocol = testProtocol;
        if (selectedModelData) {
            if (!selectedModelData.baseUrl && selectedModelData.anthropicUrl) {
                effectiveProtocol = 'anthropic';
            } else if (selectedModelData.baseUrl && !selectedModelData.anthropicUrl) {
                effectiveProtocol = 'openai';
            }
        }

        setTestOutput(prev => [...prev, `> ${prompt}`, `Sending request via ${effectiveProtocol === 'openai' ? 'OpenAI' : 'Anthropic'}...`]);

        try {
            if (!api.testModel) {
                setTestOutput(prev => [...prev, 'Test API not available']);
                return;
            }

            const result = await api.testModel(selectedModel, prompt, effectiveProtocol);

            if (result.success) {
                setModelLatencies(prev => ({ ...prev, [selectedModel]: result.latency }));
                setTestOutput(prev => [
                    ...prev,
                    `Response in ${result.latency}ms`,
                    result.response || 'No response'
                ]);
                // Reload model list to refresh test status
                if (api.getModels) {
                    const updatedModels = await api.getModels();
                    setUserModels(updatedModels);
                }
            } else {
                setTestOutput(prev => [
                    ...prev,
                    result.error || 'Unknown error',
                    result.latency > 0 ? `(failed after ${result.latency}ms)` : ''
                ].filter(Boolean));
            }
        } catch (error) {
            setTestOutput(prev => [...prev, String(error)]);
        } finally {
            setIsTesting(false);
        }
    };

    return (
        <ModelNexusContext.Provider value={{
            userModels, setUserModels, isLoadingModels,
            selectedModel, setSelectedModel, selectedModelData,
            testInput, setTestInput, testOutput, setTestOutput,
            isTesting, arrowIndex, testProtocol, setTestProtocol,
            modelLatencies, pingingModelIds,
            modelTerminals, setModelTerminals,
            testInputRef, inputFocused, setInputFocused,
            cursorPos, setCursorPos,
            showAddModelModal, setShowAddModelModal,
            modelModalAnimatingOut, editingModelId, setEditingModelId,
            newModelForm, setNewModelForm,
            showApiKey, setShowApiKey, keyDestroyed, setKeyDestroyed,
            closeModelModal, pingAllModels, handleTestModel,
        }}>
            {children}
        </ModelNexusContext.Provider>
    );
}

// ===== Title Actions (ping --all button, rendered in page header) =====

export function ModelNexusTitleActions() {
    const { pingAllModels, isTesting } = useModelNexus();
    return (
        <div className="ml-auto flex-shrink-0 flex items-center gap-3">
            <button
                onClick={pingAllModels}
                disabled={isTesting}
                className={`text-xs font-mono px-2 py-1 border rounded transition-colors ${!isTesting
                    ? 'border-cyber-accent/50 text-cyber-accent hover:bg-cyber-accent/10'
                    : 'border-cyber-border text-cyber-text-muted cursor-not-allowed'
                    }`}
            >
                $ ping --all
            </button>
        </div>
    );
}

// ===== Main Content (model card grid) =====

export function ModelNexusMain() {
    const { t } = useI18n();
    const {
        userModels, isLoadingModels,
        selectedModel, setSelectedModel,
        testInput, setTestOutput,
        testProtocol, setTestProtocol,
        modelLatencies, pingingModelIds,
        modelTerminals, setModelTerminals,
        isTesting,
        editingModelId, setEditingModelId,
        setNewModelForm, setShowAddModelModal,
        setUserModels, keyDestroyed, setKeyDestroyed,
    } = useModelNexus();

    // Stable handlers for model card interactions
    const handleCardClick = useCallback((model: typeof userModels[0]) => {
        if (selectedModel) {
            setModelTerminals(prev => ({
                ...prev,
                [selectedModel]: { input: testInput, output: [] }
            }));
        }
        setSelectedModel(model.internalId);
        if (model.baseUrl) {
            setTestProtocol('openai');
        } else if (model.anthropicUrl) {
            setTestProtocol('anthropic');
        }
    }, [selectedModel, testInput, setModelTerminals, setSelectedModel, setTestProtocol]);

    const handleCardProtocolClick = useCallback((model: typeof userModels[0], protocol: 'openai' | 'anthropic') => {
        setTestProtocol(protocol);
        if (selectedModel !== model.internalId) {
            if (selectedModel) {
                setModelTerminals(prev => ({
                    ...prev,
                    [selectedModel]: { input: testInput, output: [] }
                }));
            }
            setSelectedModel(model.internalId);
        }
    }, [selectedModel, testInput, setModelTerminals, setSelectedModel, setTestProtocol]);

    const handleCardEdit = useCallback(async (model: typeof userModels[0]) => {
        // Reload fresh model data from disk to get latest apiKey state
        let freshModel = model;
        try {
            const freshModels = await api.getModels();
            const found = freshModels.find(m => m.internalId === model.internalId);
            if (found) {
                freshModel = found;
                // Also update the models list with fresh data
                setUserModels(freshModels);
            }
        } catch { /* fallback to stale model */ }

        setEditingModelId(freshModel.internalId);
        if (freshModel.apiKey?.startsWith('enc:v1:') && api.isKeyDestroyed) {
            api.isKeyDestroyed(freshModel.internalId).then(destroyed => setKeyDestroyed(destroyed));
        } else {
            setKeyDestroyed(false);
        }
        setNewModelForm({
            name: freshModel.name,
            baseUrl: freshModel.baseUrl,
            anthropicUrl: freshModel.anthropicUrl || '',
            apiKey: freshModel.apiKey,
            modelId: freshModel.modelId || '',
            useProxy: !!freshModel.ssNode,
            ssServer: freshModel.ssNode?.server || '',
            ssPort: freshModel.ssNode?.port?.toString() || '',
            ssCipher: freshModel.ssNode?.cipher || 'aes-128-gcm',
            ssPassword: freshModel.ssNode?.password || ''
        });
        setShowAddModelModal(true);
    }, [setEditingModelId, setKeyDestroyed, setNewModelForm, setShowAddModelModal, setUserModels]);

    const handleCardDelete = useCallback(async (modelId: string) => {
        await api.deleteModel(modelId);
        setUserModels(prev => prev.filter(m => m.internalId !== modelId));
    }, [setUserModels]);

    return (
        <div className="flex-1 overflow-y-auto">
            <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4">
                {/* Show skeleton when loading */}
                {isLoadingModels ? (
                    <>
                        <ModelCardSkeleton />
                        <ModelCardSkeleton />
                        <ModelCardSkeleton />
                        <ModelCardSkeleton />
                    </>
                ) : (
                    <>
                        {/* User custom models */}
                        {userModels.map(model => {
                            const protocols: ('openai' | 'anthropic')[] = [];
                            if (model.baseUrl) protocols.push('openai');
                            if (model.anthropicUrl) protocols.push('anthropic');
                            const isDemo = model.modelType === 'DEMO';
                            return (
                                <ModelCard
                                    key={model.internalId}
                                    id={model.internalId}
                                    name={model.name}
                                    type={model.modelType || ''}
                                    baseUrl={model.baseUrl}
                                    anthropicUrl={model.anthropicUrl}
                                    modelId={model.modelId || ''}
                                    hasProxy={!!model.proxyUrl}
                                    protocols={protocols}
                                    latency={modelLatencies[model.internalId] ?? model.openaiLatency}
                                    openaiTested={model.openaiTested}
                                    anthropicTested={model.anthropicTested}
                                    isPinging={pingingModelIds.has(model.internalId)}
                                    selected={selectedModel === model.internalId}
                                    isActive={selectedModel === model.internalId}
                                    onClick={() => handleCardClick(model)}
                                    onProtocolClick={(protocol) => handleCardProtocolClick(model, protocol)}
                                    onEdit={isDemo ? undefined : () => handleCardEdit(model)}
                                    onDelete={isDemo ? undefined : () => handleCardDelete(model.internalId)}
                                />
                            );
                        })}

                        {/* Add new model button */}
                        <div
                            className="h-48 border border-dashed border-cyber-border flex flex-col items-center justify-center hover:border-cyber-accent cursor-pointer transition-all rounded-card text-cyber-text-secondary hover:text-cyber-accent"
                            onClick={() => {
                                setNewModelForm({
                                    name: '',
                                    baseUrl: '',
                                    anthropicUrl: '',
                                    apiKey: '',
                                    modelId: '',
                                    useProxy: false,
                                    ssServer: '',
                                    ssPort: '',
                                    ssCipher: 'aes-256-gcm',
                                    ssPassword: ''
                                });
                                setEditingModelId(null);
                                setShowAddModelModal(true);
                            }}
                        >
                            <span className="font-bold tracking-wider">{t('btn.addModel')}</span>
                            <span className="text-[10px] opacity-60 mt-1">OpenAI / Anthropic API</span>
                        </div>
                    </>
                )}
            </div>
        </div>
    );
}

// ===== Right Panel (Debug Console) =====

export function ModelNexusPanel() {
    const { t } = useI18n();
    const {
        selectedModelData, testOutput, isTesting, arrowIndex,
        testProtocol, setTestProtocol,
        testInput, setTestInput,
        inputFocused, setInputFocused,
        cursorPos, setCursorPos,
        testInputRef, handleTestModel,
    } = useModelNexus();

    return (
        <>
            <div className="px-4 pt-0.5 pb-3 text-sm flex items-center justify-between bg-transparent">
                <span className="font-mono">{t('debug.console')}</span>
                {selectedModelData && (
                    <span className="text-[10px] text-cyber-accent font-mono">
                        {selectedModelData.name}
                    </span>
                )}
            </div>
            <div className="flex-1 p-4 overflow-y-auto text-xs font-mono space-y-1 bg-cyber-terminal">
                {selectedModelData ? (
                    <div className="space-y-1">
                        <p className="text-cyber-accent">[SYS] Model connected</p>
                        <p className="text-cyber-text-secondary">$ echo $MODEL_ID</p>
                        <p className="text-cyber-accent/80 break-all">{selectedModelData.modelId || selectedModelData.internalId}</p>
                        <p className="text-cyber-text-secondary">$ echo $ENDPOINT ({testProtocol.toUpperCase()})</p>
                        <p className="text-cyber-accent/80 break-all">
                            {testProtocol === 'openai'
                                ? (selectedModelData.baseUrl || 'not set')
                                : (selectedModelData.anthropicUrl || 'not set')}
                        </p>
                        <p className="text-cyber-text-secondary mt-2">$ test --prompt</p>
                        {/* Test output history */}
                        {testOutput.map((line, i) => (
                            <p key={i} className={`break-words ${line.startsWith('Response in') ? 'text-green-400' :
                                line.includes('HTTP 4') || line.includes('HTTP 5') || line.includes('error') || line.includes('Error') || line.includes('failed') ? 'text-red-400' :
                                    line.startsWith('Sending') ? 'text-cyber-accent' :
                                        line.startsWith('>') ? 'text-white' :
                                            'text-cyber-text-muted/80'
                                }`}>{line}</p>
                        ))}
                        {isTesting ? (
                            <p className="text-cyber-accent font-mono">[EXEC] <span className="inline-block w-8 text-left">{['>', '>>', '>>>', ''][arrowIndex]}</span> transmitting...</p>
                        ) : (
                            <p className="text-cyber-accent">_ ready</p>
                        )}
                    </div>
                ) : (
                    <p className="text-cyber-text-secondary text-center py-10">
                        {t('model.selectToTest')}
                    </p>
                )}
            </div>
            <div className="py-3">
                <div
                    className="flex items-center gap-2 bg-cyber-terminal p-2 cursor-text"
                    onClick={() => testInputRef.current?.focus()}
                >
                    {/* Clickable protocol selector */}
                    <button
                        onClick={(e) => {
                            e.stopPropagation();
                            if (testProtocol === 'openai' && selectedModelData?.anthropicUrl) {
                                setTestProtocol('anthropic');
                            } else if (testProtocol === 'anthropic' && selectedModelData?.baseUrl) {
                                setTestProtocol('openai');
                            }
                        }}
                        className="text-xs font-mono select-none whitespace-nowrap text-cyber-accent cursor-pointer"
                    >
                        ~\{(selectedModelData?.baseUrl || selectedModelData?.anthropicUrl)
                            ? (testProtocol === 'openai' ? 'OpenAI' : 'Anthropic')
                            : ''} {'>'}
                    </button>
                    <div className="flex-1 relative flex items-center">
                        <input
                            ref={testInputRef}
                            type="text"
                            placeholder=""
                            value={testInput}
                            onChange={(e) => {
                                setTestInput(e.target.value);
                                setCursorPos(e.target.selectionStart || 0);
                            }}
                            onFocus={() => setInputFocused(true)}
                            onBlur={() => setInputFocused(false)}
                            onSelect={(e) => setCursorPos(e.currentTarget.selectionStart || 0)}
                            onClick={(e) => setCursorPos(e.currentTarget.selectionStart || 0)}
                            onKeyUp={(e) => {
                                setCursorPos(e.currentTarget.selectionStart || 0);
                                if (e.key === 'Enter' && !isTesting) {
                                    handleTestModel();
                                }
                            }}
                            className="w-full bg-transparent text-xs font-mono focus:outline-none text-cyber-text"
                            disabled={!selectedModelData || isTesting}
                            style={{ caretColor: 'transparent' }}
                        />
                        {/* Custom underscore cursor */}
                        <div className="absolute inset-0 flex items-end pb-[2px] pointer-events-none text-xs font-mono text-cyber-text overflow-hidden whitespace-pre">
                            <span className="invisible">{testInput.slice(0, cursorPos)}</span>
                            {inputFocused && (
                                <span
                                    className="inline-block w-[0.6em] h-[2px] bg-cyber-accent shadow-[0_0_8px_rgba(0,255,157,0.8)]"
                                    style={{ animation: 'blink 1s step-end infinite' }}
                                ></span>
                            )}
                        </div>
                    </div>
                </div>
            </div>
        </>
    );
}

// ===== Add/Edit Model Modal =====

export function AddModelModal() {
    const { t } = useI18n();
    const {
        showAddModelModal, modelModalAnimatingOut,
        editingModelId, setEditingModelId,
        newModelForm, setNewModelForm,
        keyDestroyed,
        closeModelModal,
        setUserModels, setShowAddModelModal,
    } = useModelNexus();

    if (!showAddModelModal) return null;

    return (
        <div
            className={`fixed inset-0 z-[9998] flex items-center justify-center transition-all duration-200 ${modelModalAnimatingOut ? 'opacity-0' : 'opacity-100'}`}
            onKeyDown={e => { if (e.key === 'Escape') closeModelModal(); }}
        >
            {/* Backdrop */}
            <div
                className="absolute inset-0 bg-black/60 backdrop-blur-sm"
                onClick={closeModelModal}
            />

            <div
                className={`relative w-[450px] max-w-[90vw] border border-cyber-accent/30 bg-cyber-bg shadow-[0_0_30px_rgba(0,255,157,0.08)] rounded-xl overflow-hidden transition-all duration-200 ${modelModalAnimatingOut ? 'scale-95 opacity-0' : 'scale-100 opacity-100'}`}
                onClick={e => e.stopPropagation()}
            >
                {/* Top accent line */}
                <div className="h-[2px] w-full bg-gradient-to-r from-cyber-accent/60 via-cyber-accent-secondary/40 to-transparent" />

                {/* Header */}
                <div className="px-5 pt-4 pb-3 flex items-center justify-between">
                    <div className="flex items-center gap-2">
                        <span className="text-cyber-accent font-mono text-xs opacity-60">&gt;_</span>
                        <span className="text-sm font-mono font-bold tracking-wider text-cyber-accent">{editingModelId ? t('model.editConfig') : t('btn.addModel')}</span>
                    </div>
                    <button
                        onClick={closeModelModal}
                        className="text-cyber-text-secondary hover:text-cyber-text transition-colors"
                    >
                        <X size={14} />
                    </button>
                </div>

                {/* Form */}
                <div className="px-5 pb-5">
                    <div className="space-y-4">
                        <div>
                            <label className="block text-xs text-cyber-text-secondary mb-1">Name</label>
                            <input
                                type="text"
                                placeholder="e.g. OpenRouter Claude"
                                value={newModelForm.name}
                                onChange={e => setNewModelForm(prev => ({ ...prev, name: e.target.value }))}
                                className="w-full bg-black border border-cyber-border px-2 py-1.5 text-xs text-cyber-text font-mono focus:border-cyber-accent focus:outline-none rounded-button"
                            />
                        </div>
                        <div>
                            <label className="block text-xs text-cyber-text-secondary mb-1">Base URL (OpenAI API)</label>
                            <input
                                type="text"
                                placeholder="https://x.x.com/v1  NOT => /chat/completions"
                                value={newModelForm.baseUrl}
                                onChange={e => setNewModelForm(prev => ({ ...prev, baseUrl: e.target.value }))}
                                className="w-full bg-black border border-cyber-border px-2 py-1.5 text-xs text-cyber-text font-mono focus:border-cyber-accent focus:outline-none rounded-button"
                            />
                        </div>
                        <div>
                            <label className="block text-xs text-cyber-text-secondary mb-1">Base URL (Anthropic API)</label>
                            <input
                                type="text"
                                placeholder="https://x.x.com/anthropic  NOT => /v1/messages"
                                value={newModelForm.anthropicUrl}
                                onChange={e => setNewModelForm(prev => ({ ...prev, anthropicUrl: e.target.value }))}
                                className="w-full bg-black border border-cyber-border px-2 py-1.5 text-xs text-cyber-text font-mono focus:border-cyber-accent focus:outline-none rounded-button"
                            />
                        </div>
                        <div>
                            <label className="block text-xs text-cyber-text-secondary mb-1">Model ID</label>
                            <input
                                type="text"
                                placeholder="e.g. anthropic/claude-opus-4.5"
                                value={newModelForm.modelId}
                                onChange={e => setNewModelForm(prev => ({ ...prev, modelId: e.target.value }))}
                                className="w-full bg-black border border-cyber-border px-2 py-1.5 text-xs text-cyber-text font-mono focus:border-cyber-accent focus:outline-none rounded-button"
                            />
                        </div>
                        <div>
                            <label className="block text-xs text-cyber-text-secondary mb-1">API Key</label>
                            <div className="relative">
                                <input
                                    type="text"
                                    placeholder="sk-..."
                                    value={newModelForm.apiKey.startsWith('enc:v1:') ? '•••••••••••••••' : newModelForm.apiKey}
                                    onChange={e => setNewModelForm(prev => ({ ...prev, apiKey: e.target.value }))}
                                    className="w-full bg-black border border-cyber-border px-2 py-1.5 pr-8 text-xs text-cyber-text font-mono focus:border-cyber-accent focus:outline-none rounded-button"
                                    readOnly={newModelForm.apiKey.startsWith('enc:v1:')}
                                />
                                {newModelForm.apiKey && newModelForm.apiKey !== 'local' && (
                                    <button
                                        type="button"
                                        onClick={async () => {
                                            if (newModelForm.apiKey.startsWith('enc:v1:')) {
                                                // Decrypt
                                                try {
                                                    const plain = await api.decryptSSHPassword(newModelForm.apiKey);
                                                    const newKey = plain || '';
                                                    setNewModelForm(prev => ({ ...prev, apiKey: newKey }));
                                                    if (editingModelId) {
                                                        setUserModels(prev => prev.map(m =>
                                                            m.internalId === editingModelId ? { ...m, apiKey: newKey } : m
                                                        ));
                                                    }
                                                } catch {
                                                    setNewModelForm(prev => ({ ...prev, apiKey: '' }));
                                                }
                                            } else {
                                                // Encrypt
                                                try {
                                                    const encrypted = await api.encryptSSHPassword(newModelForm.apiKey);
                                                    setNewModelForm(prev => ({ ...prev, apiKey: encrypted }));
                                                    if (editingModelId) {
                                                        setUserModels(prev => prev.map(m =>
                                                            m.internalId === editingModelId ? { ...m, apiKey: encrypted } : m
                                                        ));
                                                    }
                                                } catch {
                                                    // stay plaintext on failure
                                                }
                                            }
                                        }}
                                        className="absolute right-2 top-1/2 -translate-y-1/2 transition-colors hover:opacity-80"
                                    >
                                        {newModelForm.apiKey.startsWith('enc:v1:') ? (
                                            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="text-cyber-accent">
                                                <rect x="3" y="11" width="18" height="11" rx="2" />
                                                <path d="M7 11V7a5 5 0 0 1 10 0v4" />
                                            </svg>
                                        ) : (
                                            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="text-cyber-text-muted">
                                                <rect x="3" y="11" width="18" height="11" rx="2" />
                                                <path d="M7 11V7a5 5 0 0 1 9.9-1" />
                                            </svg>
                                        )}
                                    </button>
                                )}
                            </div>
                            <div className="min-h-[36px] mt-1">
                                {newModelForm.apiKey.startsWith('enc:v1:') && (
                                    <div className={`text-xs leading-tight ${keyDestroyed ? 'text-red-400' : 'text-cyber-accent/60'}`}>
                                        {keyDestroyed ? t('key.destroyed') : t('key.encrypted')}
                                    </div>
                                )}
                            </div>
                        </div>

                        {/* SS proxy configuration area */}
                        <div className="border-t border-cyber-accent/20 pt-4 mt-4">
                            <label className="flex items-center gap-3 cursor-pointer group">
                                <div className="relative">
                                    <input
                                        type="checkbox"
                                        checked={newModelForm.useProxy}
                                        onChange={e => setNewModelForm(prev => ({ ...prev, useProxy: e.target.checked }))}
                                        className="sr-only peer"
                                    />
                                    <div className="w-5 h-5 border-2 border-cyber-accent/50 bg-black peer-checked:bg-cyber-accent peer-checked:border-cyber-accent transition-all flex items-center justify-center">
                                        {newModelForm.useProxy && <span className="text-black text-xs font-bold">✓</span>}
                                    </div>
                                </div>
                                <span className="text-sm text-cyber-text font-mono group-hover:text-cyber-accent transition-colors">{t('model.proxyTunnel')} <span className="text-cyber-text-secondary">({t('model.specificProxy')})</span></span>
                            </label>

                            {newModelForm.useProxy && (
                                <div className="grid grid-cols-2 gap-3 mt-3">
                                    <div className="col-span-2">
                                        <label className="block text-xs text-cyber-text-secondary mb-1">SS Server *</label>
                                        <input
                                            type="text"
                                            placeholder="sg1.expressvpn.com"
                                            value={newModelForm.ssServer}
                                            onChange={e => setNewModelForm(prev => ({ ...prev, ssServer: e.target.value }))}
                                            className="w-full bg-black border border-cyber-border px-2 py-1.5 text-xs text-cyber-text font-mono focus:border-cyber-accent focus:outline-none rounded-button"
                                        />
                                    </div>
                                    <div>
                                        <label className="block text-xs text-cyber-text-secondary mb-1">Port *</label>
                                        <input
                                            type="number"
                                            placeholder="52324"
                                            value={newModelForm.ssPort}
                                            onChange={e => setNewModelForm(prev => ({ ...prev, ssPort: e.target.value }))}
                                            className="w-full bg-black border border-cyber-border px-2 py-1.5 text-xs text-cyber-text font-mono focus:border-cyber-accent focus:outline-none rounded-button no-spinner"
                                        />
                                    </div>
                                    <div>
                                        <label className="block text-xs text-cyber-text-secondary mb-1">Cipher</label>
                                        <MiniSelect
                                            value={newModelForm.ssCipher}
                                            onChange={value => setNewModelForm(prev => ({ ...prev, ssCipher: value }))}
                                            options={[
                                                { id: 'aes-128-gcm', label: 'aes-128-gcm' },
                                                { id: 'aes-256-gcm', label: 'aes-256-gcm' },
                                                { id: 'chacha20-ietf-poly1305', label: 'chacha20-ietf-poly1305' }
                                            ]}
                                            className="w-full"
                                        />
                                    </div>
                                    <div className="col-span-2">
                                        <label className="block text-xs text-cyber-text-secondary mb-1">Password *</label>
                                        <input
                                            type="password"
                                            placeholder="SS Password / UUID"
                                            value={newModelForm.ssPassword}
                                            onChange={e => setNewModelForm(prev => ({ ...prev, ssPassword: e.target.value }))}
                                            className="w-full bg-black border border-cyber-border px-2 py-1.5 text-xs text-cyber-text font-mono focus:border-cyber-accent focus:outline-none rounded-button"
                                        />
                                    </div>
                                </div>
                            )}
                        </div>
                    </div>
                </div>

                {/* Footer buttons */}
                <div className="flex border-t border-cyber-border">
                    <button
                        onClick={closeModelModal}
                        className="flex-1 px-4 py-2.5 text-xs font-mono font-bold tracking-wider text-cyber-text-secondary hover:text-cyber-text hover:bg-white/5 transition-all border-r border-cyber-border"
                    >
                        {t('model.escCancel')}
                    </button>
                    <button
                        onClick={async () => {
                            if (newModelForm.useProxy) {
                                if (!newModelForm.ssServer || !newModelForm.ssPort || !newModelForm.ssPassword) {
                                    console.warn('[ModelNexus] SS proxy config incomplete, saving anyway');
                                }
                            }

                            if (api.addModel) {
                                let proxyUrl: string | undefined = undefined;
                                let ssNode: SSNodeConfig | undefined = undefined;

                                if (newModelForm.useProxy && api.addSSProxyRoute) {
                                    ssNode = {
                                        name: newModelForm.name,
                                        server: newModelForm.ssServer,
                                        port: parseInt(newModelForm.ssPort),
                                        cipher: newModelForm.ssCipher,
                                        password: newModelForm.ssPassword
                                    };
                                    const tempId = editingModelId || `model_${Date.now()}`;
                                    const result = await api.addSSProxyRoute(tempId, newModelForm.baseUrl, ssNode);
                                    if (result.success) {
                                        proxyUrl = result.proxyUrl;
                                    }
                                }

                                const modelData = {
                                    name: newModelForm.name,
                                    baseUrl: newModelForm.baseUrl,
                                    anthropicUrl: newModelForm.anthropicUrl || undefined,
                                    apiKey: newModelForm.apiKey,
                                    modelId: newModelForm.modelId,
                                    proxyUrl: proxyUrl,
                                    ssNode: ssNode
                                };

                                if (editingModelId && api.updateModel) {
                                    const updatedModel = await api.updateModel(editingModelId, modelData);
                                    if (updatedModel) {
                                        setUserModels(prev => prev.map(m => m.internalId === editingModelId ? updatedModel : m));
                                    }
                                } else {
                                    const newModel = await api.addModel(modelData);
                                    setUserModels(prev => [...prev, newModel]);
                                }

                                setEditingModelId(null);
                                setNewModelForm({
                                    name: '',
                                    baseUrl: '',
                                    anthropicUrl: '',
                                    apiKey: '',
                                    modelId: '',
                                    useProxy: false,
                                    ssServer: '',
                                    ssPort: '',
                                    ssCipher: 'aes-128-gcm',
                                    ssPassword: ''
                                });
                                setShowAddModelModal(false);
                            }
                        }}
                        className="flex-1 px-4 py-2.5 text-xs font-mono font-bold tracking-wider text-cyber-accent hover:bg-cyber-accent/10 transition-all"
                    >
                        {t('model.enterSave')}
                    </button>
                </div>
            </div>
        </div>
    );
}
