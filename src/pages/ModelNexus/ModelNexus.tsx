// Model Nexus Page — Model cards, debug console, add/edit modal
// Extracted from App.tsx with Provider pattern for shared state

import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { open as shellOpen } from '@tauri-apps/plugin-shell';
import { X, Box, ExternalLink, Plus } from 'lucide-react';
import { MiniSelect } from '../../components/MiniSelect';
import { ModelCard, ModelCardSkeleton, getModelIcon } from '../../components';
import { useI18n } from '../../hooks/useI18n';
import * as api from '../../api/tauri';
import type { SSNodeConfig, ModelConfig } from '../../api/types';
import { ModelNexusContext, useModelNexus } from './context';
import type { NewModelForm } from './context';
import modelDirectory from '../../data/modelDirectory.json';


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
                    ? 'border-cyber-border/50 text-cyber-text hover:bg-cyber-text/10'
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
        if (selectedModel === model.internalId) {
            // Click again to deselect
            setSelectedModel(null);
            return;
        }
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
                            className="h-48 border border-dashed border-cyber-border flex flex-col items-center justify-center hover:border-cyber-border cursor-pointer transition-all rounded-card text-cyber-text-secondary hover:text-cyber-text"
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

// ===== Right Panel (Provider / Relay tabs) =====

// Provider / relay lists are loaded from src/data/modelDirectory.json so
// users can edit baseUrl / anthropicUrl / modelId per vendor without touching
// component code. Provider order is curated (roughly end-user MAU); names are
// picked to match keyword rules in getModelIcon so the SVG resolves.
type DirectoryEntry = {
    name: string;
    url: string;
    baseUrl: string;
    anthropicUrl: string;
    modelId: string;
    region: 'cn' | 'global';
};

const PROVIDERS: DirectoryEntry[] = modelDirectory.providers as DirectoryEntry[];
const RELAYS: DirectoryEntry[] = modelDirectory.relays as DirectoryEntry[];

// Locale-aware reorder: zh* surfaces 'cn' entries first; everything else
// surfaces 'global' first. Within each region we keep the curated JSON order
// (Array.prototype.sort is stable in modern engines).
function sortByLocale(list: DirectoryEntry[], locale: string): DirectoryEntry[] {
    const cnFirst = locale.toLowerCase().startsWith('zh');
    const weight = (e: DirectoryEntry) =>
        cnFirst ? (e.region === 'cn' ? 0 : 1) : (e.region === 'global' ? 0 : 1);
    return [...list].sort((a, b) => weight(a) - weight(b));
}

function ProviderRow({ entry, onAdd }: { entry: DirectoryEntry; onAdd: () => void }) {
    const iconSrc = getModelIcon(entry.name, '');
    const hostname = (() => {
        try { return new URL(entry.url).hostname; }
        catch { return entry.url; }
    })();
    const openSite = () => shellOpen(entry.url).catch(() => window.open(entry.url, '_blank'));
    // Two click+hover zones (50/50). Buttons sit underneath; the visual content
    // floats on top with pointer-events-none so clicks pass through to whichever
    // half they land on. Named groups (group/left, group/right) let the icons
    // brighten in sync with their half's hover state.
    return (
        <div className="relative flex items-stretch rounded overflow-hidden bg-cyber-surface">
            {/* Click + hover layer (two equal halves) */}
            <button
                type="button"
                onClick={onAdd}
                aria-label={`Add model: ${entry.name}`}
                className="group/left flex-1 min-h-[64px] bg-gradient-to-r from-transparent to-transparent hover:from-cyber-text/15 hover:to-transparent transition-[background-image] duration-200"
            />
            <button
                type="button"
                onClick={openSite}
                aria-label={`Open ${entry.name} website`}
                className="group/right flex-1 min-h-[64px] bg-gradient-to-l from-transparent to-transparent hover:from-cyber-text/15 hover:to-transparent transition-[background-image] duration-200"
            />

            {/* Visual content overlay (does not capture clicks) */}
            <div className="pointer-events-none absolute inset-0 flex items-center gap-3 px-3">
                <Plus
                    size={22}
                    strokeWidth={2.5}
                    className="flex-shrink-0 text-cyber-text-muted group-hover/left:text-cyber-text group-hover/left:scale-110 transition-all"
                />
                <div className="flex-shrink-0">
                    {iconSrc ? (
                        <img
                            src={iconSrc}
                            alt=""
                            className="w-6 h-6"
                            onError={(e) => { (e.target as HTMLImageElement).style.display = 'none'; }}
                        />
                    ) : (
                        <div className="w-6 h-6 rounded bg-cyber-text/15 flex items-center justify-center text-cyber-text">
                            <Box size={14} />
                        </div>
                    )}
                </div>
                <div className="flex-1 min-w-0 flex flex-col justify-center">
                    <div className="text-sm font-bold truncate leading-none">{entry.name}</div>
                    <div className="text-[10px] text-cyber-text-secondary truncate leading-tight mt-1 opacity-70">
                        {hostname}
                    </div>
                </div>
                <ExternalLink
                    size={18}
                    strokeWidth={2.25}
                    className="flex-shrink-0 text-cyber-text-muted group-hover/right:text-cyber-text group-hover/right:scale-110 transition-all"
                />
            </div>
        </div>
    );
}

export function ModelNexusPanel() {
    const { t, locale } = useI18n();
    const [panelTab, setPanelTab] = useState<'providers' | 'relays'>('providers');
    const list = useMemo(
        () => sortByLocale(panelTab === 'providers' ? PROVIDERS : RELAYS, locale),
        [panelTab, locale]
    );
    const { setNewModelForm, setEditingModelId, setShowAddModelModal } = useModelNexus();

    const handleAddFromEntry = useCallback((entry: DirectoryEntry) => {
        setNewModelForm({
            name: entry.name,
            baseUrl: entry.baseUrl,
            anthropicUrl: entry.anthropicUrl,
            apiKey: '',
            modelId: entry.modelId,
            useProxy: false,
            ssServer: '',
            ssPort: '',
            ssCipher: 'aes-128-gcm',
            ssPassword: ''
        });
        setEditingModelId(null);
        setShowAddModelModal(true);
    }, [setNewModelForm, setEditingModelId, setShowAddModelModal]);

    return (
        <>
            <div className="p-2 flex items-center justify-between bg-transparent">
                <div className="flex gap-1">
                    <button
                        onClick={() => setPanelTab('providers')}
                        className={`px-3.5 py-2 text-[14px] font-semibold rounded transition-colors ${panelTab === 'providers'
                            ? 'bg-cyber-elevated text-cyber-text'
                            : 'text-cyber-text-secondary hover:text-cyber-text'
                            }`}
                    >
                        {t('model.providers')}
                    </button>
                    <button
                        onClick={() => setPanelTab('relays')}
                        className={`px-3.5 py-2 text-[14px] font-semibold rounded transition-colors ${panelTab === 'relays'
                            ? 'bg-cyber-elevated text-cyber-text'
                            : 'text-cyber-text-secondary hover:text-cyber-text'
                            }`}
                    >
                        {t('model.relays')}
                    </button>
                </div>
            </div>
            <div className="flex-1 p-2 overflow-y-auto">
                <div className="space-y-2">
                    {list.map(entry => (
                        <ProviderRow
                            key={entry.name}
                            entry={entry}
                            onAdd={() => handleAddFromEntry(entry)}
                        />
                    ))}
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
                className={`relative w-[450px] max-w-[90vw] border border-cyber-border/30 bg-cyber-surface shadow-2xl rounded-xl overflow-hidden transition-all duration-200 ${modelModalAnimatingOut ? 'scale-95 opacity-0' : 'scale-100 opacity-100'}`}
                onClick={e => e.stopPropagation()}
            >
                {/* Top accent line */}
                <div className="h-px w-full bg-cyber-border" />

                {/* Header */}
                <div className="px-6 pt-5 pb-4 flex items-center justify-between">
                    <div className="flex items-center gap-2">
                        <span className="text-cyber-text font-mono text-sm opacity-60">&gt;_</span>
                        <span className="text-base font-bold text-cyber-text">{editingModelId ? t('model.editConfig') : t('btn.addModel')}</span>
                    </div>
                    <button
                        onClick={closeModelModal}
                        className="text-cyber-text-secondary hover:text-cyber-text transition-colors"
                    >
                        <X size={18} />
                    </button>
                </div>

                {/* Form */}
                <div className="px-5 pb-5">
                    <div className="space-y-4">
                        <div>
                            <label className="block text-xs text-cyber-text-secondary mb-1">{t('model.name')}</label>
                            <input
                                type="text"
                                placeholder="e.g. My Model"
                                value={newModelForm.name}
                                onChange={e => setNewModelForm(prev => ({ ...prev, name: e.target.value }))}
                                className="w-full bg-cyber-input border border-cyber-border px-2 py-1.5 text-xs text-cyber-text font-mono focus:border-cyber-border focus:outline-none rounded-button"
                            />
                        </div>
                        <div>
                            <label className="block text-xs text-cyber-text-secondary mb-1">{t('model.openaiUrl')}</label>
                            <input
                                type="text"
                                placeholder="https://x.x.com/v1"
                                value={newModelForm.baseUrl}
                                onChange={e => {
                                    let v = e.target.value;
                                    v = v.replace(/\/chat\/completions\/?$/i, '').replace(/\/v1\/chat\/completions\/?$/i, '/v1');
                                    setNewModelForm(prev => ({ ...prev, baseUrl: v }));
                                }}
                                className="w-full bg-cyber-input border border-cyber-border px-2 py-1.5 text-xs text-cyber-text font-mono focus:border-cyber-border focus:outline-none rounded-button"
                            />
                        </div>
                        <div>
                            <label className="block text-xs text-cyber-text-secondary mb-1">{t('model.anthropicUrl')}</label>
                            <input
                                type="text"
                                placeholder="https://x.x.com/anthropic"
                                value={newModelForm.anthropicUrl}
                                onChange={e => {
                                    let v = e.target.value;
                                    v = v.replace(/\/v1\/messages\/?$/i, '').replace(/\/messages\/?$/i, '');
                                    setNewModelForm(prev => ({ ...prev, anthropicUrl: v }));
                                }}
                                className="w-full bg-cyber-input border border-cyber-border px-2 py-1.5 text-xs text-cyber-text font-mono focus:border-cyber-border focus:outline-none rounded-button"
                            />
                        </div>
                        <div>
                            <label className="block text-xs text-cyber-text-secondary mb-1">{t('model.modelId')}</label>
                            <input
                                type="text"
                                placeholder="e.g. Qwen/Qwen-Coder"
                                value={newModelForm.modelId}
                                onChange={e => setNewModelForm(prev => ({ ...prev, modelId: e.target.value }))}
                                className="w-full bg-cyber-input border border-cyber-border px-2 py-1.5 text-xs text-cyber-text font-mono focus:border-cyber-border focus:outline-none rounded-button"
                            />
                        </div>
                        <div>
                            <label className="block text-xs text-cyber-text-secondary mb-1">{t('model.apiKey')}</label>
                            <div className="relative">
                                <input
                                    type="text"
                                    placeholder="sk-..."
                                    value={newModelForm.apiKey.startsWith('enc:v1:') ? '•••••••••••••••' : newModelForm.apiKey}
                                    onChange={e => setNewModelForm(prev => ({ ...prev, apiKey: e.target.value }))}
                                    className="w-full bg-cyber-input border border-cyber-border px-2 py-1.5 pr-8 text-xs text-cyber-text font-mono focus:border-cyber-border focus:outline-none rounded-button"
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
                                            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="text-cyber-text">
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
                                    <div className={`text-xs leading-tight ${keyDestroyed ? 'text-red-400' : 'text-cyber-text/60'}`}>
                                        {keyDestroyed ? t('key.destroyed') : t('key.encrypted')}
                                    </div>
                                )}
                            </div>
                        </div>

                        {/* SS proxy configuration area */}
                        <div className="border-t border-cyber-border/20 pt-4 mt-4">
                            <label className="flex items-center gap-3 cursor-pointer group">
                                <div className="relative">
                                    <input
                                        type="checkbox"
                                        checked={newModelForm.useProxy}
                                        onChange={e => setNewModelForm(prev => ({ ...prev, useProxy: e.target.checked }))}
                                        className="sr-only peer"
                                    />
                                    <div className="w-5 h-5 border-2 border-cyber-border/50 bg-cyber-input peer-checked:bg-cyber-accent peer-checked:border-cyber-border transition-all flex items-center justify-center">
                                        {newModelForm.useProxy && <span className="text-black text-xs font-bold">✓</span>}
                                    </div>
                                </div>
                                <span className="text-sm text-cyber-text font-mono group-hover:text-cyber-text transition-colors">{t('model.proxyTunnel')} <span className="text-cyber-text-secondary">({t('model.specificProxy')})</span></span>
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
                                            className="w-full bg-cyber-input border border-cyber-border px-2 py-1.5 text-xs text-cyber-text font-mono focus:border-cyber-border focus:outline-none rounded-button"
                                        />
                                    </div>
                                    <div>
                                        <label className="block text-xs text-cyber-text-secondary mb-1">Port *</label>
                                        <input
                                            type="number"
                                            placeholder="52324"
                                            value={newModelForm.ssPort}
                                            onChange={e => setNewModelForm(prev => ({ ...prev, ssPort: e.target.value }))}
                                            className="w-full bg-cyber-input border border-cyber-border px-2 py-1.5 text-xs text-cyber-text font-mono focus:border-cyber-border focus:outline-none rounded-button no-spinner"
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
                                            className="w-full bg-cyber-input border border-cyber-border px-2 py-1.5 text-xs text-cyber-text font-mono focus:border-cyber-border focus:outline-none rounded-button"
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
                        className="flex-1 px-4 py-3 text-[14px] font-semibold text-cyber-text-secondary hover:text-cyber-text hover:bg-cyber-elevated transition-all border-r border-cyber-border"
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

                                if (editingModelId && api.updateModel) {
                                    // Edit: always pass actual values (including empty string) — Rust handles clearing
                                    const updatedModel = await api.updateModel(editingModelId, {
                                        name: newModelForm.name,
                                        baseUrl: newModelForm.baseUrl,
                                        anthropicUrl: newModelForm.anthropicUrl,
                                        apiKey: newModelForm.apiKey,
                                        modelId: newModelForm.modelId,
                                        proxyUrl: proxyUrl,
                                        ssNode: ssNode,
                                    });
                                    if (updatedModel) {
                                        setUserModels(prev => prev.map(m => m.internalId === editingModelId ? updatedModel : m));
                                    }
                                } else {
                                    // Add: baseUrl required as string (empty string is valid)
                                    const newModel = await api.addModel({
                                        name: newModelForm.name,
                                        baseUrl: newModelForm.baseUrl,
                                        anthropicUrl: newModelForm.anthropicUrl || undefined,
                                        apiKey: newModelForm.apiKey,
                                        modelId: newModelForm.modelId,
                                        proxyUrl: proxyUrl,
                                        ssNode: ssNode,
                                    });
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
                        className="flex-1 px-4 py-3 text-[14px] font-semibold text-cyber-text hover:bg-cyber-text/10 transition-all"
                    >
                        {t('model.enterSave')}
                    </button>
                </div>
            </div>
        </div>
    );
}
