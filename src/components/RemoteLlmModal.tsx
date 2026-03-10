// Remote LLM Modal — opened from Channels "LLM Panel →"
// Connects to remote llm-server HTTP API (plugins/llm-server)

import React, { useState, useEffect, useCallback, useRef } from 'react';
import { X, Square, Terminal, ChevronDown, Download, Loader2, FolderOpen, Play, HardDrive } from 'lucide-react';
import { MiniSelect } from './MiniSelect';
import { useI18n } from '../hooks/useI18n';
import * as api from '../api/tauri';
import type { StoreModel } from '../api/types';

interface RemoteLlmModalProps {
    isOpen: boolean;
    onClose: () => void;
    remoteHost?: string; // e.g. "eben@192.168.10.39"
    displayName?: string; // e.g. "haohao" (alias)
}

// ─── Types + Helpers ───

interface ModelGroup {
    name: string;
    icon: string;
    sourceDir: string;
    variants: { fileName: string; filePath: string; fileSize: number; quant: string }[];
}

function groupModels(files: { fileName: string; filePath: string; fileSize: number }[]): ModelGroup[] {
    const groups: Record<string, ModelGroup> = {};
    for (const f of files) {
        const dir = f.filePath.replace(/[/\\][^/\\]+$/, '') || '.';
        const baseName = f.fileName.replace(/\.gguf$/i, '');
        const quantMatch = baseName.match(/[-_]((?:[qQfF]\d+[_A-Za-z0-9]*)|(?:bf16))$/);
        const quant = quantMatch ? quantMatch[1].toUpperCase() : '';
        const modelBase = quantMatch ? baseName.slice(0, quantMatch.index) : baseName;
        const key = modelBase.toLowerCase();
        if (!groups[key]) {
            const lcName = modelBase.toLowerCase();
            const icon = lcName.includes('deepseek') ? 'deepseek'
                : lcName.includes('qwen') ? 'qwen'
                    : lcName.includes('llama') ? 'llama' : 'default';
            groups[key] = { name: modelBase.replace(/[-_]/g, ' '), icon, sourceDir: dir, variants: [] };
        }
        groups[key].variants.push({ fileName: f.fileName, filePath: f.filePath, fileSize: f.fileSize, quant: quant || baseName });
    }
    for (const g of Object.values(groups)) g.variants.sort((a, b) => a.fileSize - b.fileSize);
    return Object.values(groups);
}

function formatSize(bytes: number): string {
    if (bytes >= 1e9) return (bytes / 1e9).toFixed(1) + ' GB';
    if (bytes >= 1e6) return (bytes / 1e6).toFixed(0) + ' MB';
    return (bytes / 1e3).toFixed(0) + ' KB';
}

function estimateVramGb(fileSize: number): number {
    return Math.round((fileSize / 1e9) * 1.2 * 10) / 10;
}

function parseVramString(vramStr: string): number {
    const match = vramStr.match(/(\d+\.?\d*)/);
    return match ? parseFloat(match[1]) : 0;
}

function getVramFitness(requiredGb: number, userVramGb: number, t: ReturnType<typeof useI18n>['t']): { label: string; color: string } | null {
    if (userVramGb <= 0) return null;
    const ratio = requiredGb / userVramGb;
    if (ratio <= 0.7) return { label: t('vram.easy'), color: 'text-green-400' };
    if (ratio <= 1.0) return { label: t('vram.good'), color: 'text-cyan-400' };
    if (ratio <= 1.3) return { label: t('vram.tight'), color: 'text-yellow-400' };
    return { label: t('vram.heavy'), color: 'text-red-400' };
}

const LLM_API_PORT = 8090;

// ─── Component ───

export const RemoteLlmModal: React.FC<RemoteLlmModalProps> = ({
    isOpen,
    onClose,
    remoteHost = 'eben@192.168.10.39',
    displayName,
}) => {
    const { t } = useI18n();
    const remoteIp = remoteHost.split('@')[1] || remoteHost;
    const apiBase = `http://${remoteIp}:${LLM_API_PORT}`;

    // Config state
    const [gpuLayers, setGpuLayers] = useState('-1');
    const [contextSize, setContextSize] = useState('4096');
    const [serverPort, setServerPort] = useState('11434');
    const [runtime, setRuntime] = useState('llama-server');
    const [copied, setCopied] = useState('');


    // Tab state
    const [activeTab, setActiveTab] = useState<'local' | 'store'>('local');
    const [expandedGroup, setExpandedGroup] = useState<string | null>(null);
    const [selectedVariant, setSelectedVariant] = useState(''); // stores filePath

    // Store models (fetched from Tauri API, same source as LocalServer)
    const [storeModels, setStoreModels] = useState<StoreModel[]>([]);
    const [isLoadingStore, setIsLoadingStore] = useState(false);
    const [expandedStoreId, setExpandedStoreId] = useState<string | null>(null);

    // Remote download state (polled from llm-server /api/download/status)
    const [remoteDownload, setRemoteDownload] = useState<{ fileName: string; progress: number; status: string }>({ fileName: '', progress: 0, status: 'idle' });
    const downloadPollRef = useRef<ReturnType<typeof setInterval> | null>(null);

    // ─── Real state from remote API ───
    const [gpu, setGpu] = useState<{ gpuName: string; gpuVramGb: number } | null>(null);
    const [localModels, setLocalModels] = useState<ModelGroup[]>([]);
    const [logs, setLogs] = useState<string[]>([]);
    const [serverInfo, setServerInfo] = useState<{ running: boolean; port: number; modelName: string; pid: number | null }>(
        { running: false, port: 0, modelName: '', pid: null }
    );
    const [runtimeStatus, setRuntimeStatus] = useState<Record<string, { installed: boolean; version?: string }>>({});
    const [remoteSystemInfo, setRemoteSystemInfo] = useState<{
        os: string; arch: string; hasNvidiaGpu: boolean; hasAmdGpu: boolean; gpuName: string | null; gpuVramGb: number | null;
    } | null>(null);
    const [remoteDirs, setRemoteDirs] = useState<string[]>([]);
    const [apiReachable, setApiReachable] = useState(false);

    // Derived state
    const gpuVramGb = gpu?.gpuVramGb || 0;
    const isRunning = serverInfo.running;
    const currentRuntimeInstalled = runtimeStatus[runtime]?.installed ?? false;
    const engineStatus: 'ready' | 'not-installed' = currentRuntimeInstalled ? 'ready' : 'not-installed';

    const handleCopy = (path: string) => {
        const port = isRunning ? serverInfo.port : parseInt(serverPort);
        navigator.clipboard.writeText(`http://${remoteIp}:${port}${path}`);
        setCopied(path);
        setTimeout(() => setCopied(''), 2000);
    };

    // ─── Fetch remote data on open ───
    useEffect(() => {
        if (!isOpen) return;
        const isHfRuntime = runtime === 'vllm' || runtime === 'sglang' || runtime === 'vllm-musa';
        const fetchAll = async () => {
            try {
                const [statusRes, gpuRes, modelsRes, hfModelsRes, logsRes, engineRes, dirsRes] = await Promise.allSettled([
                    fetch(`${apiBase}/api/status`).then(r => r.json()),
                    fetch(`${apiBase}/api/gpu`).then(r => r.json()),
                    fetch(`${apiBase}/api/models`).then(r => r.json()),
                    fetch(`${apiBase}/api/hf-models`).then(r => r.json()),
                    fetch(`${apiBase}/api/logs`).then(r => r.json()),
                    fetch(`${apiBase}/api/engine/status`).then(r => r.json()),
                    fetch(`${apiBase}/api/dirs`).then(r => r.json()),
                ]);
                setApiReachable(true);
                if (statusRes.status === 'fulfilled') setServerInfo(statusRes.value);
                if (gpuRes.status === 'fulfilled' && gpuRes.value) {
                    setGpu(gpuRes.value);
                } else {
                    // No GPU detected — auto-switch to CPU Only to prevent crash
                    setGpuLayers('0');
                }
                // Display GGUF or HF models based on runtime
                if (isHfRuntime && hfModelsRes.status === 'fulfilled') {
                    const hfFiles = (hfModelsRes.value || []).map((m: any) => ({
                        fileName: m.modelName,
                        filePath: m.modelPath,
                        fileSize: m.totalSize,
                    }));
                    setLocalModels(groupModels(hfFiles));
                } else if (modelsRes.status === 'fulfilled') {
                    setLocalModels(groupModels(modelsRes.value || []));
                }
                if (logsRes.status === 'fulfilled') setLogs(logsRes.value || []);
                if (engineRes.status === 'fulfilled') {
                    const eng = engineRes.value || {};
                    const rs: Record<string, { installed: boolean; version?: string }> = {};
                    // API returns { engines: [{ name, installed, version }] } array format
                    const engines: any[] = eng.engines || [];
                    for (const key of ['llama-server', 'vllm', 'sglang', 'vllm-musa']) {
                        const entry = engines.find((e: any) => e.name === key);
                        if (entry) rs[key] = { installed: !!entry.installed, version: entry.version };
                        else if (eng[key]) rs[key] = { installed: eng[key].installed, version: eng[key].version };
                    }
                    setRuntimeStatus(rs);
                    // Parse systemInfo from engine status
                    if (eng.systemInfo) setRemoteSystemInfo(eng.systemInfo);
                }
                if (dirsRes.status === 'fulfilled') setRemoteDirs(dirsRes.value || []);
            } catch (e) {
                console.error('[RemoteLLM] API fetch failed:', e);
                setApiReachable(false);
            }
        };
        fetchAll();
    }, [isOpen, apiBase, runtime]);

    // ─── Poll status + logs whenever modal is open (mirrors LocalServer always-on poll) ───
    useEffect(() => {
        if (!isOpen) return;
        const id = setInterval(async () => {
            try {
                const [statusRes, logsRes] = await Promise.allSettled([
                    fetch(`${apiBase}/api/status`).then(r => r.json()),
                    fetch(`${apiBase}/api/logs`).then(r => r.json()),
                ]);
                if (statusRes.status === 'fulfilled') setServerInfo(statusRes.value);
                if (logsRes.status === 'fulfilled') setLogs(logsRes.value || []);
            } catch { /* silent */ }
        }, 3000);
        return () => clearInterval(id);
    }, [isOpen, apiBase]);



    // ── Remote download: start ──
    const handleRemoteDownload = useCallback(async (repo: string, fileName: string) => {
        try {
            await fetch(`${apiBase}/api/download`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ repo, fileName }),
            });
            setRemoteDownload({ fileName, progress: 0, status: 'speed_test' });
        } catch (e) {
            console.error('[RemoteLLM] Download start failed:', e);
        }
    }, [apiBase]);

    // ── Remote download: cancel ──
    const handleCancelRemoteDownload = useCallback(async () => {
        try {
            await fetch(`${apiBase}/api/download/cancel`, { method: 'POST' });
            setRemoteDownload(d => ({ ...d, status: 'cancelled' }));
        } catch (e) {
            console.error('[RemoteLLM] Download cancel failed:', e);
        }
    }, [apiBase]);

    // ── Remote engine install: triggers same download bar ──
    const handleInstallEngine = useCallback(async () => {
        try {
            await fetch(`${apiBase}/api/engine/install`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ runtime }),
            });
            setRemoteDownload({ fileName: runtime, progress: 0, status: 'speed_test' });
        } catch (e) {
            console.error('[RemoteLLM] Engine install failed:', e);
        }
    }, [apiBase, runtime]);

    // ── Poll remote download status ──
    useEffect(() => {
        if (!isOpen) return;
        const isActive = remoteDownload.status === 'downloading' || remoteDownload.status === 'speed_test';
        if (!isActive) {
            if (downloadPollRef.current) { clearInterval(downloadPollRef.current); downloadPollRef.current = null; }
            return;
        }
        const poll = async () => {
            try {
                const res = await fetch(`${apiBase}/api/download/status`).then(r => r.json());
                setRemoteDownload({ fileName: res.fileName || '', progress: res.progress || 0, status: res.status || 'idle' });
                // Auto-refresh when download completes
                if (res.status === 'completed') {
                    // Refresh model list
                    const modelsRes = await fetch(`${apiBase}/api/models`).then(r => r.json()).catch(() => []);
                    setLocalModels(groupModels(modelsRes || []));
                    // Refresh engine status after any download completes
                    {
                        const engineRes = await fetch(`${apiBase}/api/engine/status`).then(r => r.json()).catch(() => ({}));
                        const rs: Record<string, { installed: boolean; version?: string }> = {};
                        const engines: any[] = engineRes.engines || [];
                        for (const key of ['llama-server', 'vllm', 'sglang', 'vllm-musa']) {
                            const entry = engines.find((e: any) => e.name === key);
                            if (entry) rs[key] = { installed: !!entry.installed, version: entry.version };
                            else if (engineRes[key]) rs[key] = { installed: engineRes[key].installed, version: engineRes[key].version };
                        }
                        setRuntimeStatus(rs);
                    }
                }
            } catch { /* silent */ }
        };
        downloadPollRef.current = setInterval(poll, 1000);
        return () => { if (downloadPollRef.current) { clearInterval(downloadPollRef.current); downloadPollRef.current = null; } };
    }, [isOpen, remoteDownload.status, apiBase]);

    // ── Auto-clear download status after 5s (completed/error/cancelled) ──
    useEffect(() => {
        if (remoteDownload.status === 'completed' || remoteDownload.status === 'error' || remoteDownload.status === 'cancelled') {
            const timer = setTimeout(() => setRemoteDownload({ fileName: '', progress: 0, status: 'idle' }), 30000);
            return () => clearTimeout(timer);
        }
    }, [remoteDownload.status]);

    // ─── Start / Stop handlers ───
    const handleStart = async () => {
        if (!selectedVariant) return;
        try {
            await fetch(`${apiBase}/api/start`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({
                    modelPath: selectedVariant,
                    port: parseInt(serverPort),
                    gpuLayers: gpuLayers === '-1' ? -1 : parseInt(gpuLayers),
                    contextSize: parseInt(contextSize),
                    runtime,
                }),
            });
            // Optimistically mark as running; the always-on poll will correct if it fails
            setServerInfo(prev => ({ ...prev, running: true, port: parseInt(serverPort) }));
        } catch (e) {
            console.error('[RemoteLLM] Start failed:', e);
        }
    };

    const handleStop = async () => {
        try {
            await fetch(`${apiBase}/api/stop`, { method: 'POST' });
            setServerInfo(prev => ({ ...prev, running: false, port: 0, modelName: '', pid: null }));
        } catch (e) {
            console.error('[RemoteLLM] Stop failed:', e);
        }
    };



    // ─── Fetch store models on STORE tab ───
    useEffect(() => {
        if (activeTab === 'store' && storeModels.length === 0 && isOpen) {
            setIsLoadingStore(true);
            api.getStoreModels()
                .then((data: StoreModel[]) => {
                    if (data && data.length > 0) {
                        setStoreModels(data);
                    } else {
                        return fetch('./api/store/models.json')
                            .then(r => r.json())
                            .then((fallback: StoreModel[]) => setStoreModels(fallback));
                    }
                })
                .catch(() => {
                    fetch('./api/store/models.json')
                        .then(r => r.json())
                        .then((fallback: StoreModel[]) => setStoreModels(fallback))
                        .catch(e => console.error('[RemoteLLM] Store fetch failed:', e));
                })
                .finally(() => setIsLoadingStore(false));
        }
    }, [activeTab, storeModels.length, isOpen]);

    if (!isOpen) return null;

    return (
        <div
            className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-sm"
            onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}
        >
            <div className="w-[1050px] h-[720px] bg-cyber-bg border border-cyber-accent/30 rounded-lg shadow-[0_0_60px_rgba(0,255,157,0.08)] flex flex-col overflow-hidden">
                {/* ===== Title Bar ===== */}
                <div className="flex items-center justify-between px-6 py-3 border-b border-cyber-border/30 flex-shrink-0">
                    <div className="flex items-center gap-3 font-mono">
                        <span className="text-cyber-accent font-bold text-lg tracking-wider">
                            {displayName ? `${displayName} (${remoteHost})` : remoteHost}
                        </span>
                    </div>
                    <button
                        onClick={onClose}
                        className="p-1 text-cyber-text-muted/50 hover:text-cyber-text transition-colors"
                    >
                        <X size={18} />
                    </button>
                </div>

                {/* ===== Body: Left Main + Right Panel ===== */}
                <div className="flex flex-1 overflow-hidden">
                    {/* ── Left: Main Content ── */}
                    <div className="flex-1 flex flex-col px-6 overflow-hidden">
                        {/* Control Area */}
                        <div className="py-4 space-y-4 flex-shrink-0">
                            {/* Current model */}
                            <div className="flex items-center gap-2 font-mono text-base">
                                <span className="text-cyber-text-secondary">{t('server.selectModel')}</span>
                                {(() => {
                                    const allVariants = localModels.flatMap(g => g.variants);
                                    const sel = allVariants.find(v => v.filePath === selectedVariant);
                                    if (sel) {
                                        return (
                                            <>
                                                <span className="text-cyber-accent font-bold truncate">{sel.fileName.replace('.gguf', '').replace(/-/g, ' ')}</span>
                                                <span className="text-cyber-accent font-bold flex-shrink-0">{sel.quant}</span>
                                            </>
                                        );
                                    }
                                    return <span className="text-cyber-text-muted/70">{t('server.selectFromPanel')}</span>;
                                })()}
                            </div>

                            {/* Parameter row */}
                            <div className="grid grid-cols-4 gap-3">
                                {/* Compute: locked to GPU Full when using HF runtimes (vLLM/SGLang/vLLM-MUSA manage GPU internally) */}
                                <div className="flex items-center gap-2">
                                    <label className="text-[11px] text-cyber-text-secondary font-mono font-bold flex-shrink-0">{t('server.compute')}</label>
                                    <MiniSelect
                                        value={runtime !== 'llama-server' ? '-1' : gpuLayers}
                                        onChange={setGpuLayers}
                                        disabled={isRunning || runtime !== 'llama-server'}
                                        options={[
                                            { id: '-1', label: t('server.gpuFull') },
                                            ...(runtime === 'llama-server' ? [{ id: '0', label: t('server.cpuOnly') }] : []),
                                        ]}
                                        className="flex-1"
                                    />
                                </div>
                                <div className="flex items-center gap-2">
                                    <label className="text-[11px] text-cyber-text-secondary font-mono font-bold flex-shrink-0">{t('server.context')}</label>
                                    <MiniSelect
                                        value={contextSize}
                                        onChange={setContextSize}
                                        disabled={isRunning}
                                        options={[
                                            { id: '2048', label: '2K' },
                                            { id: '4096', label: '4K' },
                                            { id: '8192', label: '8K' },
                                            { id: '16384', label: '16K' },
                                            { id: '32768', label: '32K' },
                                            { id: '65536', label: '64K' },
                                            { id: '131072', label: '128K' },
                                        ]}
                                        className="flex-1"
                                    />
                                </div>
                                <div className="flex items-center gap-2">
                                    <label className="text-[11px] text-cyber-text-secondary font-mono font-bold flex-shrink-0">{t('server.port')}</label>
                                    <MiniSelect
                                        value={serverPort}
                                        onChange={(v) => {
                                            if (v === 'random') {
                                                setServerPort(String(10000 + Math.floor(Math.random() * 50000)));
                                            } else {
                                                setServerPort(v);
                                            }
                                        }}
                                        disabled={isRunning}
                                        options={[
                                            { id: serverPort, label: serverPort },
                                            { id: 'random', label: '🎲 Random' },
                                        ]}
                                        className="flex-1"
                                    />
                                </div>
                                <div className="flex items-center gap-2">
                                    <label className="text-[11px] text-cyber-text-secondary font-mono font-bold flex-shrink-0">{t('server.runtime')}</label>
                                    <MiniSelect
                                        value={runtime}
                                        onChange={setRuntime}
                                        disabled={isRunning}
                                        options={[
                                            { id: 'llama-server', label: 'llama.cpp' },
                                            { id: 'vllm', label: 'vLLM' },
                                            { id: 'sglang', label: 'SGLang' },
                                            { id: 'vllm-musa', label: 'vLLM-MUSA' },
                                        ].filter(opt => {
                                            // All GPU runtimes: Linux only
                                            if ((opt.id === 'vllm' || opt.id === 'sglang' || opt.id === 'vllm-musa') && remoteSystemInfo && remoteSystemInfo.os !== 'linux') return false;
                                            // vLLM / SGLang: require NVIDIA or AMD GPU
                                            if ((opt.id === 'vllm' || opt.id === 'sglang') && remoteSystemInfo && !remoteSystemInfo.hasNvidiaGpu && !remoteSystemInfo.hasAmdGpu) return false;
                                            // vLLM-MUSA: require Moore Threads GPU (detected via gpuName containing 'mtt' or 'moore')
                                            if (opt.id === 'vllm-musa' && remoteSystemInfo) {
                                                const gpuName = (remoteSystemInfo.gpuName || '').toLowerCase();
                                                if (!gpuName.includes('mtt') && !gpuName.includes('moore')) return false;
                                            }
                                            // Always show hardware-compatible runtimes regardless of install status
                                            // (user needs to select a runtime to install it — filtering by installed creates a chicken-and-egg problem)
                                            return true;
                                        })}
                                        className="flex-1"
                                    />
                                </div>
                            </div>

                            {/* Start/Stop button */}
                            {engineStatus === 'not-installed' ? (() => {
                                const isInstalling = remoteDownload.status === 'downloading' || remoteDownload.status === 'speed_test' || remoteDownload.status === 'installing';
                                const isJustCompleted = remoteDownload.status === 'completed' && remoteDownload.fileName === runtime;
                                if (isInstalling) {
                                    return (
                                        <div className="w-full py-3 font-bold text-base tracking-[0.3em] font-mono flex items-center justify-center gap-2 bg-cyber-surface/30 text-cyber-accent/70 border border-cyber-accent/30">
                                            <Loader2 className="w-4 h-4 animate-spin" />
                                            INSTALLING {runtime.toUpperCase()}…
                                        </div>
                                    );
                                }
                                if (isJustCompleted) {
                                    return (
                                        <button
                                            onClick={async () => {
                                                const engineRes = await fetch(`${apiBase}/api/engine/status`).then(r => r.json()).catch(() => ({}));
                                                const rs: Record<string, { installed: boolean; version?: string }> = {};
                                                const engines: any[] = engineRes.engines || [];
                                                for (const key of ['llama-server', 'vllm', 'sglang', 'vllm-musa']) {
                                                    const entry = engines.find((e: any) => e.name === key);
                                                    if (entry) rs[key] = { installed: !!entry.installed, version: entry.version };
                                                    else if (engineRes[key]) rs[key] = { installed: engineRes[key].installed, version: engineRes[key].version };
                                                }
                                                setRuntimeStatus(rs);
                                                if (engineRes.systemInfo) setRemoteSystemInfo(engineRes.systemInfo);
                                            }}
                                            className="w-full py-3 font-bold text-base tracking-[0.3em] font-mono transition-all flex items-center justify-center gap-2 bg-cyber-accent/10 text-cyber-accent border border-cyber-accent/50 hover:bg-cyber-accent/20"
                                        >
                                            ↻ REFRESH STATUS
                                        </button>
                                    );
                                }
                                return (
                                    <button
                                        onClick={handleInstallEngine}
                                        className="w-full py-3 font-bold text-base tracking-[0.3em] font-mono transition-all flex items-center justify-center gap-2 bg-cyber-accent/10 text-cyber-accent border border-cyber-accent/50 hover:bg-cyber-accent/20 shadow-[0_0_15px_rgba(0,255,157,0.15)]"
                                    >
                                        <Download className="w-4 h-4" /> {t('server.setupEngine')}
                                    </button>
                                );
                            })() : (
                                <button
                                    onClick={isRunning ? handleStop : handleStart}
                                    disabled={!isRunning && !selectedVariant}
                                    className={`w-full py-3 font-bold text-base tracking-[0.3em] font-mono transition-all flex items-center justify-center gap-2 ${isRunning
                                        ? 'bg-red-500/10 text-red-400 border border-red-500/50 hover:bg-red-500/20 shadow-[0_0_15px_rgba(239,68,68,0.15)]'
                                        : !selectedVariant
                                            ? 'bg-cyber-surface/30 text-cyber-text-muted/50 border border-cyber-border/30 cursor-not-allowed'
                                            : 'bg-cyber-accent/10 text-cyber-accent border border-cyber-accent/50 hover:bg-cyber-accent/20 shadow-[0_0_15px_rgba(0,255,157,0.15)]'
                                        }`}
                                >
                                    {isRunning ? (
                                        <><Square className="w-3.5 h-3.5 fill-current" /> {t('btn.stop')}</>
                                    ) : (
                                        <><Play className="w-3.5 h-3.5 fill-current" /> {t('btn.start')}</>
                                    )}
                                </button>
                            )}
                        </div>

                        {/* Terminal Output */}
                        <div className="flex-1 flex flex-col overflow-hidden">
                            <div className="flex items-center gap-2 py-2 border-b border-cyber-border/30 flex-shrink-0">
                                <Terminal className="w-3 h-3 text-cyber-text-secondary" />
                                <span className="text-sm font-mono text-cyber-text-secondary">{t('server.stdout')}</span>
                            </div>
                            <div className="flex-1 overflow-y-auto py-3 bg-cyber-terminal font-mono text-sm space-y-0.5 custom-scrollbar">
                                {logs.length > 0 ? logs.map((log, i) => (
                                    <div key={i} className="leading-relaxed">
                                        <span className="text-cyber-text-muted/60 select-none mr-2">$</span>
                                        <span className="text-cyber-text/80">{log.replace(/^\$ /, '')}</span>
                                    </div>
                                )) : (
                                    <div className="flex items-center justify-center" style={{ minHeight: 'calc(100% - 24px)' }}>
                                        <div className="font-mono text-center space-y-3">
                                            <div className="text-lg text-cyber-text-secondary/80">{'>'} {t('server.awaitingInit')}</div>
                                            <div className="text-base text-cyber-text-muted/70">{t('server.selectConfigStart')}</div>
                                        </div>
                                    </div>
                                )}
                            </div>
                        </div>
                    </div>

                    {/* ── Right: Panel ── */}
                    <div className="w-[340px] border-l border-cyber-border/20 flex flex-col bg-cyber-bg">
                        {/* Tab Header */}
                        <div className="p-2 flex items-center justify-between bg-transparent flex-shrink-0">
                            <div className="flex gap-1">
                                <button
                                    onClick={() => setActiveTab('local')}
                                    className={`px-3 py-1.5 text-xs font-bold rounded transition-colors ${activeTab === 'local'
                                        ? 'bg-cyber-accent text-black'
                                        : 'text-cyber-text-secondary hover:text-cyber-text'
                                        }`}
                                >{t('server.local')}</button>
                                <button
                                    onClick={() => setActiveTab('store')}
                                    className={`px-3 py-1.5 text-xs font-bold rounded transition-colors ${activeTab === 'store'
                                        ? 'bg-cyan-400 text-black'
                                        : 'text-cyber-text-secondary hover:text-cyber-text'
                                        }`}
                                >{t('server.store')}</button>
                            </div>
                            <span className="text-[10px] text-cyber-text-muted font-mono truncate max-w-[100px]">
                                {gpu ? `${gpu.gpuName} ${gpu.gpuVramGb}G` : ''}
                            </span>
                        </div>

                        {/* Panel Content */}
                        <div className="flex-1 overflow-y-auto p-2 custom-scrollbar">
                            {activeTab === 'local' ? (
                                /* LOCAL tab */
                                <div className="space-y-2">
                                    {/* Read-only model directories */}
                                    {remoteDirs.length > 0 && (
                                        <div className="space-y-1 mb-3">
                                            {remoteDirs.map((dir, i) => (
                                                <div key={i} className="flex items-center gap-2 px-2 py-1.5 bg-cyber-bg-secondary/50 rounded border border-cyber-border/20">
                                                    <FolderOpen className="w-3.5 h-3.5 text-cyber-accent/60 flex-shrink-0" />
                                                    <span className="text-[10px] font-mono text-cyber-text-muted truncate">{dir}</span>
                                                </div>
                                            ))}
                                        </div>
                                    )}
                                    {localModels.length > 0 ? localModels.map(group => {
                                        const isExpanded = expandedGroup === group.name;
                                        const isGroupSelected = group.variants.some(v => v.filePath === selectedVariant);
                                        return (
                                            <div
                                                key={group.name}
                                                className={`p-3 border rounded transition-all ${isGroupSelected
                                                    ? 'border-green-500/50 bg-green-500/5'
                                                    : 'border-cyber-border hover:border-green-500/30'
                                                    }`}
                                            >
                                                <div
                                                    className="flex items-center gap-3 cursor-pointer"
                                                    onClick={() => setExpandedGroup(isExpanded ? null : group.name)}
                                                >
                                                    <div className={`w-4 h-4 rounded-full border-2 flex items-center justify-center flex-shrink-0 transition-colors ${isGroupSelected ? 'border-green-400' : 'border-cyber-border'}`}>
                                                        {isGroupSelected && <div className="w-2 h-2 rounded-full bg-green-400" />}
                                                    </div>
                                                    {group.icon && group.icon !== 'default' && (
                                                        <img
                                                            src={`./icons/models/${group.icon}.svg`}
                                                            alt={group.name}
                                                            className="w-6 h-6 flex-shrink-0"
                                                            onError={(e) => { (e.target as HTMLImageElement).style.display = 'none'; }}
                                                        />
                                                    )}
                                                    <div className="flex-1 min-w-0">
                                                        <div className="text-xs font-bold text-cyber-text truncate">{group.name}</div>
                                                        <div className="text-[10px] text-cyber-text-muted/60 truncate">{group.sourceDir}</div>
                                                    </div>
                                                    <span className="text-[10px] text-cyber-text-muted/50">{group.variants.length} {t('store.ver')}</span>
                                                </div>
                                                {/* Expanded variants */}
                                                {isExpanded && (
                                                    <div className="mt-3 pt-3 border-t border-cyber-border/30 space-y-1">
                                                        {group.variants.map((v, i) => {
                                                            const isSelected = v.filePath === selectedVariant;
                                                            return (
                                                                <div
                                                                    key={i}
                                                                    onClick={(e) => { e.stopPropagation(); setSelectedVariant(v.filePath); }}
                                                                    className={`flex items-center gap-3 p-2 rounded cursor-pointer transition-colors ${isSelected ? 'bg-green-500/10' : 'hover:bg-cyber-surface/30'}`}
                                                                >
                                                                    <span className={`text-xs font-mono font-bold w-14 flex-shrink-0 ${isSelected ? 'text-green-400' : 'text-cyber-accent'}`}>
                                                                        {v.quant}
                                                                    </span>
                                                                    <span className="text-[10px] text-cyber-text-secondary flex-1 whitespace-nowrap">
                                                                        {estimateVramGb(v.fileSize)} GB · {formatSize(v.fileSize)}
                                                                    </span>
                                                                    <span className="text-[10px] w-10 text-center flex-shrink-0">
                                                                        {(() => { const f = getVramFitness(estimateVramGb(v.fileSize), gpuVramGb, t); return f ? <span className={`font-bold ${f.color}`}>{f.label}</span> : null; })()}
                                                                    </span>
                                                                    <div className="flex-shrink-0 w-8 flex items-center justify-center">
                                                                        <div className={`w-4 h-4 rounded-full border-2 flex items-center justify-center transition-colors ${isSelected ? 'border-green-400' : 'border-cyber-border hover:border-green-400/50'}`}>
                                                                            {isSelected && <div className="w-2 h-2 rounded-full bg-green-400" />}
                                                                        </div>
                                                                    </div>
                                                                </div>
                                                            );
                                                        })}
                                                    </div>
                                                )}
                                            </div>
                                        );
                                    }) : (
                                        <div className="text-center py-10">
                                            <HardDrive className="w-8 h-8 text-cyber-text-secondary mx-auto mb-3 opacity-50" />
                                            <p className="text-sm text-cyber-text-secondary">{t('server.selectModelDir')}</p>
                                            <p className="text-[10px] text-cyber-text-secondary mt-1 opacity-70">{t('server.downloadFromStore')}</p>
                                        </div>
                                    )}
                                </div>
                            ) : (
                                /* STORE tab — download models to remote */
                                <div className="space-y-2">
                                    {/* Remote download directory */}
                                    <div className="flex items-center gap-2 mb-3 px-1">
                                        <FolderOpen size={10} className="text-cyber-text-muted/50 flex-shrink-0" />
                                        <span className="text-[10px] font-mono text-cyber-text-muted/70 truncate">
                                            {remoteDirs[0] || '/home/models'}
                                        </span>
                                    </div>
                                    {isLoadingStore ? (
                                        <div className="flex items-center justify-center py-10">
                                            <Loader2 className="w-5 h-5 text-cyan-400 animate-spin" />
                                        </div>
                                    ) : storeModels.length > 0 ? (
                                        storeModels.filter(m => {
                                            if (!m.runtimes) return true;
                                            if (m.runtimes.includes(runtime)) return true;
                                            // vLLM-MUSA is compatible with vLLM-format HF models
                                            if (runtime === 'vllm-musa' && m.runtimes.includes('vllm')) return true;
                                            return false;
                                        }).map(m => {
                                            const isExpanded = expandedStoreId === m.id;
                                            const hasDownloaded = m.variants?.some(v => localModels.some(g => g.variants.some(lv => lv.fileName === v.fileName)));
                                            return (
                                                <div
                                                    key={m.id}
                                                    className={`p-3 border rounded cursor-pointer transition-all ${isExpanded
                                                        ? 'border-cyan-400/50 bg-cyan-400/5'
                                                        : hasDownloaded
                                                            ? 'border-cyan-400/30 hover:border-cyan-400/50'
                                                            : 'border-cyber-border hover:border-cyan-400/50'
                                                        }`}
                                                    onClick={() => setExpandedStoreId(isExpanded ? null : m.id)}
                                                >
                                                    {/* Card Header */}
                                                    <div className="flex items-center gap-3">
                                                        <img
                                                            src={`./icons/models/${m.icon}.svg`}
                                                            alt={m.name}
                                                            className="w-6 h-6 flex-shrink-0"
                                                            onError={(e) => { (e.target as HTMLImageElement).style.display = 'none'; }}
                                                        />
                                                        <div className="flex-1 min-w-0 flex flex-col justify-center">
                                                            <div className="flex items-center justify-between gap-2">
                                                                <div className="text-sm font-bold truncate leading-none text-cyan-400 flex-1 min-w-0">{m.name}</div>
                                                                {hasDownloaded && (
                                                                    <span className="text-[10px] text-cyan-400 flex-shrink-0">{t('store.ready')}</span>
                                                                )}
                                                            </div>
                                                            <div className="text-[10px] text-cyber-text-secondary truncate leading-tight mt-1 opacity-70 flex gap-1">
                                                                {(m.runtimes || ['llama-server']).map(r => (
                                                                    <span key={r} className={`px-1 rounded ${(r === runtime || (runtime === 'vllm-musa' && r === 'vllm')) ? 'bg-cyan-400/20 text-cyan-400' : 'bg-cyber-surface/50'}`}>
                                                                        {r === 'llama-server' ? 'llama.cpp' : r}
                                                                    </span>
                                                                ))}
                                                            </div>
                                                        </div>
                                                    </div>

                                                    {/* Expanded: Variants with download */}
                                                    {isExpanded && m.variants && (
                                                        <div className="mt-3 pt-3 border-t border-cyber-border/30 space-y-1">
                                                            {m.variants.map((v, vi) => {
                                                                const variantDownloaded = localModels.some(g => g.variants.some(lv => lv.fileName === v.fileName));
                                                                const isThisDownloading = remoteDownload.fileName === v.fileName && (remoteDownload.status === 'downloading' || remoteDownload.status === 'speed_test');
                                                                const isThisError = remoteDownload.fileName === v.fileName && remoteDownload.status === 'error';
                                                                return (
                                                                    <div
                                                                        key={vi}
                                                                        className={`p-2 rounded transition-colors ${variantDownloaded ? 'bg-cyan-400/5' : 'hover:bg-cyber-surface/30'}`}
                                                                    >
                                                                        <div className="flex items-center gap-3">
                                                                            <span className="text-xs font-mono font-bold text-cyan-400 w-14 flex-shrink-0">
                                                                                {v.quantization || v.fileName?.split('-').pop()?.replace('.gguf', '').toUpperCase()}
                                                                            </span>
                                                                            <span className="text-[10px] text-cyber-text-secondary flex-1 whitespace-nowrap">
                                                                                {v.recommendedVRAM || `${estimateVramGb(v.fileSize)} GB`} · {formatSize(v.fileSize)}
                                                                            </span>
                                                                            <span className="text-[10px] w-10 text-center flex-shrink-0">
                                                                                {(() => { const vram = parseVramString(v.recommendedVRAM || `${estimateVramGb(v.fileSize)}`); const f = getVramFitness(vram, gpuVramGb, t); return f ? <span className={`font-bold ${f.color}`}>{f.label}</span> : null; })()}
                                                                            </span>
                                                                            <div className="flex-shrink-0 w-8 flex items-center justify-center">
                                                                                {variantDownloaded ? (
                                                                                    <span className="text-cyan-400 text-sm">✓</span>
                                                                                ) : isThisDownloading ? (
                                                                                    <span className="text-[10px] font-mono text-cyan-400">
                                                                                        {remoteDownload.status === 'speed_test' ? <Loader2 className="w-3 h-3 animate-spin" /> : `${remoteDownload.progress}%`}
                                                                                    </span>
                                                                                ) : (
                                                                                    <button
                                                                                        onClick={(e) => { e.stopPropagation(); handleRemoteDownload(m.huggingfaceRepo, v.fileName); }}
                                                                                        className={`${isThisError ? 'text-red-400 hover:text-red-300' : 'text-cyber-text-secondary hover:text-cyan-400'} transition-colors`}
                                                                                    >
                                                                                        <Download className="w-4 h-4" />
                                                                                    </button>
                                                                                )}
                                                                            </div>
                                                                        </div>
                                                                    </div>
                                                                );
                                                            })}
                                                        </div>
                                                    )}
                                                </div>
                                            );
                                        })
                                    ) : (
                                        <div className="text-center py-10">
                                            <HardDrive className="w-8 h-8 text-cyber-text-secondary mx-auto mb-3 opacity-50" />
                                            <p className="text-sm text-cyber-text-secondary">{t('server.selectModelDir')}</p>
                                            <p className="text-[10px] text-cyber-text-secondary mt-1 opacity-70">{t('server.downloadFromStore')}</p>
                                        </div>
                                    )}
                                </div>
                            )}
                        </div>
                    </div>
                </div>

                {/* ===== Download Progress Bar ===== */}
                {remoteDownload.status !== 'idle' && (
                    <div className="h-7 flex items-center px-4 bg-cyber-bg/80 backdrop-blur-sm border-t border-cyber-border/30 flex-shrink-0">
                        <div className="flex items-center gap-3 flex-1 min-w-0">
                            {/* Status icon */}
                            {(remoteDownload.status === 'downloading' || remoteDownload.status === 'speed_test') && <Download className="w-3.5 h-3.5 text-cyber-accent flex-shrink-0 animate-pulse" />}
                            {remoteDownload.status === 'completed' && <span className="text-green-400 text-sm flex-shrink-0">✓</span>}
                            {remoteDownload.status === 'error' && <span className="text-red-400 text-sm flex-shrink-0">✗</span>}
                            {remoteDownload.status === 'cancelled' && <X className="w-3.5 h-3.5 text-cyber-text-secondary flex-shrink-0" />}

                            {/* File name */}
                            <span className={`text-[11px] font-mono truncate min-w-0 ${remoteDownload.status === 'completed' ? 'text-green-400'
                                : remoteDownload.status === 'error' ? 'text-red-400'
                                    : 'text-cyber-text'
                                }`}>
                                {remoteDownload.fileName.length > 32
                                    ? remoteDownload.fileName.slice(0, 29) + '...'
                                    : remoteDownload.fileName}
                            </span>

                            {/* Speed test spinner */}
                            {remoteDownload.status === 'speed_test' && (
                                <Loader2 className="w-3.5 h-3.5 text-cyber-accent animate-spin flex-shrink-0" />
                            )}

                            {/* Progress bar + percentage (downloading) */}
                            {remoteDownload.status === 'downloading' && (
                                <div className="flex items-center gap-2 flex-shrink-0">
                                    <div className="w-24 h-1.5 bg-cyber-border/50 rounded-full overflow-hidden">
                                        <div
                                            className="h-full rounded-full bg-cyber-accent transition-all duration-300"
                                            style={{ width: `${remoteDownload.progress}%` }}
                                        />
                                    </div>
                                    <span className="text-[10px] font-mono w-8 text-right text-cyber-accent">
                                        {remoteDownload.progress}%
                                    </span>
                                </div>
                            )}

                            {/* Status text */}
                            {remoteDownload.status === 'completed' && (
                                <span className="text-[10px] font-mono text-green-400/70 flex-shrink-0">{t('status.complete')}</span>
                            )}
                            {remoteDownload.status === 'error' && (
                                <span className="text-[10px] font-mono text-red-400/70 flex-shrink-0">{t('status.failed')}</span>
                            )}

                            {/* Cancel button */}
                            {(remoteDownload.status === 'downloading' || remoteDownload.status === 'speed_test') && (
                                <button
                                    onClick={handleCancelRemoteDownload}
                                    className="text-cyber-text-secondary/50 hover:text-red-400 transition-colors ml-auto flex-shrink-0"
                                >
                                    <X className="w-3.5 h-3.5" />
                                </button>
                            )}
                        </div>
                    </div>
                )}

                {/* ===== Bottom: API Endpoints ===== */}
                <div className="flex items-center gap-4 px-6 py-2.5 border-t border-cyber-border/20 text-xs font-mono flex-shrink-0">
                    <div
                        className="flex items-center gap-1.5 cursor-pointer hover:text-cyber-accent transition-colors"
                        onClick={() => handleCopy('/v1')}
                    >
                        <span className="text-cyber-text-secondary/80">OpenAI:</span>
                        <code className="text-cyber-accent/80">{remoteIp}:{isRunning ? serverInfo.port : serverPort}/v1</code>
                        <span className="text-cyber-accent ml-1">{copied === '/v1' ? t('btn.copied') : t('btn.copy')}</span>
                    </div>
                    <span className="text-cyber-border">|</span>
                    <div
                        className="flex items-center gap-1.5 cursor-pointer hover:text-cyber-accent transition-colors"
                        onClick={() => handleCopy('/anthropic')}
                    >
                        <span className="text-cyber-text-secondary/80">Anthropic:</span>
                        <code className="text-cyber-accent/80">{remoteIp}:{isRunning ? serverInfo.port : serverPort}/anthropic</code>
                        <span className="text-cyber-accent ml-1">{copied === '/anthropic' ? t('btn.copied') : t('btn.copy')}</span>
                    </div>
                </div>

            </div>
        </div>
    );
};
