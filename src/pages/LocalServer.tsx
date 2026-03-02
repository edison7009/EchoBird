// Local Server Page — llama-server management UI
// Layout restored from Electron v1.0.8 LocalModelPlayer.tsx
// Architecture: Provider + Main + Panel (consistent with other pages)

import { useState, useEffect, useRef, useCallback, createContext, useContext } from 'react';
import { Play, Square, Terminal, ChevronDown, Download, Loader2, HardDrive, FolderOpen } from 'lucide-react';
import { MiniSelect } from '../components/MiniSelect';
import { useI18n } from '../hooks/useI18n';
import { useConfirm } from '../components/ConfirmDialog';
import { useDownload } from '../components/DownloadContext';
import * as api from '../api/tauri';
import type { StoreModel, StoreModelVariant } from '../api/types';

// ─── Types ───

type EngineStatus = 'checking' | 'ready' | 'not-installed' | 'downloading' | 'error';

interface GgufFileEntry {
    fileName: string;
    filePath: string;
    fileSize: number;
}

// ─── Context ───

interface LocalServerContextValue {
    // Model selection
    selectedModelPath: string | null;
    setSelectedModelPath: (path: string | null) => void;
    // GGUF / HF model files
    ggufFiles: GgufFileEntry[];
    isScanning: boolean;
    rescanModels: (runtime?: string) => void;
    // Model dirs
    modelsDirs: string[];
    // Server runtime state (for bottom bar)
    serverRunning: boolean;
    setServerRunning: (v: boolean) => void;
    serverPort: number;
    setServerPort: (v: number) => void;
    serverModelName: string;
    setServerModelName: (v: string) => void;
    serverApiKey: string;
    setServerApiKey: (v: string) => void;
}

const LocalServerContext = createContext<LocalServerContextValue | null>(null);

const useLocalServer = () => {
    const ctx = useContext(LocalServerContext);
    if (!ctx) throw new Error('useLocalServer must be used within LocalServerProvider');
    return ctx;
};

// ─── Provider ───

export const LocalServerProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
    const [selectedModelPath, setSelectedModelPath] = useState<string | null>(null);
    const [ggufFiles, setGgufFiles] = useState<GgufFileEntry[]>([]);
    const [isScanning, setIsScanning] = useState(false);
    const [modelsDirs, setModelsDirs] = useState<string[]>([]);
    // Server runtime state (shared with bottom bar)
    const [serverRunning, setServerRunning] = useState(false);
    const [serverPort, setServerPort] = useState(11434);
    const [serverModelName, setServerModelName] = useState('');
    const [serverApiKey, setServerApiKey] = useState('');

    const rescanModels = useCallback(async (runtime?: string) => {
        setIsScanning(true);
        try {
            const dirs = await api.getModelsDirs();
            setModelsDirs(dirs);
            const allFiles: GgufFileEntry[] = [];
            const isHfRuntime = runtime === 'vllm' || runtime === 'sglang';
            for (const dir of dirs) {
                if (isHfRuntime) {
                    // Scan HuggingFace model directories
                    const models = await api.scanHfModels(dir);
                    for (const m of models) {
                        allFiles.push({
                            fileName: m.modelName,
                            filePath: m.modelPath,
                            fileSize: m.totalSize,
                        });
                    }
                } else {
                    // Scan GGUF files (default)
                    const files = await api.scanGgufFiles(dir);
                    for (const f of files) {
                        allFiles.push({
                            fileName: f.fileName,
                            filePath: f.filePath,
                            fileSize: f.fileSize,
                        });
                    }
                }
            }
            setGgufFiles(allFiles);
        } catch (e) {
            console.error('[LocalServer] Failed to scan models:', e);
        }
        setIsScanning(false);
    }, []);

    // Initial scan
    useEffect(() => { rescanModels(); }, [rescanModels]);

    return (
        <LocalServerContext.Provider value={{
            selectedModelPath, setSelectedModelPath,
            ggufFiles, isScanning, rescanModels,
            modelsDirs,
            serverRunning, setServerRunning,
            serverPort, setServerPort,
            serverModelName, setServerModelName,
            serverApiKey, setServerApiKey,
        }}>
            {children}
        </LocalServerContext.Provider>
    );
};

// ─── Helper: Parse model info from file path ───

function parseModelInfo(filePath: string) {
    if (!filePath) return { name: 'NO MODEL SELECTED', quant: '', shortPath: '' };
    const fileName = filePath.replace(/\\/g, '/').split('/').pop() || '';
    const base = fileName.replace(/\.gguf$/i, '');
    const quantMatch = base.match(/[-_](q\d[_a-z0-9]*|f16|f32|fp16|fp32|bf16)$/i);
    const quant = quantMatch ? quantMatch[1].toUpperCase() : '';
    const name = quantMatch ? base.slice(0, quantMatch.index) : base;
    return { name, quant, shortPath: fileName };
}

// ─── Main Content ───

export const LocalServerMain: React.FC = () => {
    const { t } = useI18n();
    const { selectedModelPath, rescanModels, serverRunning: isRunning, setServerRunning: setIsRunning, serverPort, setServerPort: setServerPortCtx, serverModelName, setServerModelName, setServerApiKey } = useLocalServer();

    // Configuration state
    const setServerPort = (v: number) => setServerPortCtx(v);
    const [gpuLayers, setGpuLayers] = useState<number>(-1);
    const [contextSize, setContextSize] = useState<number>(4096);
    const [runtime, setRuntime] = useState('llama-server');

    // Rescan models when runtime changes (GGUF vs HuggingFace)
    useEffect(() => { rescanModels(runtime); }, [runtime, rescanModels]);

    // OS detection: vLLM / SGLang only available on Linux
    const isLinux = navigator.platform.startsWith('Linux');
    const runtimeOptions = [
        { id: 'llama-server', label: 'llama.cpp' },
        ...(isLinux ? [
            { id: 'vllm', label: 'vLLM' },
            { id: 'sglang', label: 'SGLang' },
        ] : []),
    ];

    // Server state
    const [logs, setLogs] = useState<string[]>([]);
    const logsEndRef = useRef<HTMLDivElement>(null);
    const logsContainerRef = useRef<HTMLDivElement>(null);

    // Engine detection
    const [engineStatus, setEngineStatus] = useState<EngineStatus>('checking');
    const [downloadProgress, setDownloadProgress] = useState<number>(0);

    // Auto-follow scroll
    const autoFollowRef = useRef(true);
    const [showScrollBtn, setShowScrollBtn] = useState(false);

    const modelInfo = parseModelInfo(selectedModelPath || '');

    // Sync model name to context when model changes
    useEffect(() => {
        if (selectedModelPath) {
            setServerModelName(modelInfo.name + (modelInfo.quant ? '-' + modelInfo.quant : ''));
        }
    }, [selectedModelPath, modelInfo.name, modelInfo.quant, setServerModelName]);

    // Check engine on mount
    useEffect(() => {
        const check = async () => {
            setEngineStatus('checking');
            try {
                const path = await api.findLlamaServer();
                setEngineStatus(path ? 'ready' : 'not-installed');
            } catch {
                setEngineStatus('error');
            }
        };
        check();
    }, []);

    // Listen for engine download progress (fileName === 'llama-server')
    useEffect(() => {
        let unlisten: (() => void) | undefined;
        (async () => {
            const { listen } = await import('@tauri-apps/api/event');
            unlisten = await listen<{ file_name: string; progress: number; status: string }>('download-progress', (event) => {
                if (event.payload.file_name !== 'llama-server') return;
                const { progress, status } = event.payload;
                if (status === 'downloading' || status === 'speed_test') {
                    setEngineStatus('downloading');
                    setDownloadProgress(progress);
                } else if (status === 'completed') {
                    setEngineStatus('ready');
                    setDownloadProgress(100);
                } else if (status === 'cancelled') {
                    setEngineStatus('not-installed');
                    setDownloadProgress(0);
                } else if (status === 'error') {
                    setEngineStatus('error');
                }
            });
        })();
        return () => { unlisten?.(); };
    }, []);

    // Download engine handler
    const handleDownloadEngine = async () => {
        setEngineStatus('downloading');
        setDownloadProgress(0);
        try {
            await api.downloadLlamaServer();
            setEngineStatus('ready');
        } catch (err: any) {
            setEngineStatus('error');
            setLogs(prev => [...prev, `[Error] Engine download failed: ${err?.message || err}`]);
        }
    };

    // Polling: server status + logs
    useEffect(() => {
        const poll = async () => {
            try {
                const info = await api.getLlmServerInfo();
                setIsRunning(info.running);
                const serverLogs = await api.getLlmServerLogs();
                if (serverLogs.length > 0) {
                    setLogs(serverLogs);
                }
            } catch (e) {
                console.error('[LocalServer] Poll error:', e);
            }
        };
        poll();
        const interval = setInterval(poll, 1000);
        return () => clearInterval(interval);
    }, []);

    // Scroll handling
    const handleScroll = () => {
        const container = logsContainerRef.current;
        if (!container) return;
        const isAtBottom = container.scrollHeight - container.scrollTop - container.clientHeight < 40;
        autoFollowRef.current = isAtBottom;
        setShowScrollBtn(!isAtBottom && logs.length > 0);
    };

    useEffect(() => {
        if (autoFollowRef.current && logsEndRef.current) {
            logsEndRef.current.scrollIntoView({ behavior: 'auto' });
        }
    }, [logs]);

    const scrollToBottom = () => {
        autoFollowRef.current = true;
        setShowScrollBtn(false);
        logsEndRef.current?.scrollIntoView({ behavior: 'smooth' });
    };

    // Start/Stop server
    const handleToggleServer = async () => {
        if (isRunning) {
            try {
                await api.stopLlmServer();
                setIsRunning(false);
                setServerApiKey('');
            } catch (e) {
                setLogs(prev => [...prev, `[Error] Failed to stop: ${e}`]);
            }
        } else {
            if (!selectedModelPath) return;
            setLogs([]);
            autoFollowRef.current = true;
            setShowScrollBtn(false);
            try {
                await api.startLlmServer(selectedModelPath, serverPort, gpuLayers, contextSize, runtime);
                setIsRunning(true);
                // Fetch server info to get the generated API key
                try {
                    const info = await api.getLlmServerInfo();
                    if (info.apiKey) setServerApiKey(info.apiKey);
                } catch { /* ignore */ }
            } catch (e) {
                setLogs(prev => [...prev, `[Error] ${e}`]);
            }
        }
    };



    // Render START button (state machine)
    const renderStartButton = () => {
        // Engine not installed: show SETUP ENGINE button
        if (engineStatus === 'not-installed' || engineStatus === 'error') {
            return (
                <button
                    onClick={handleDownloadEngine}
                    className="w-full py-3 font-bold text-base tracking-[0.3em] font-mono transition-all flex items-center justify-center gap-2
                        bg-cyber-accent/10 text-cyber-accent border border-cyber-accent/50 hover:bg-cyber-accent/20 shadow-[0_0_15px_rgba(0,255,157,0.15)]"
                >
                    <Download className="w-4 h-4" />
                    {engineStatus === 'error' ? `\u26A0 ${t('server.setupEngine')}` : t('server.setupEngine')}
                </button>
            );
        }

        // Downloading: show progress bar
        if (engineStatus === 'downloading') {
            return (
                <div className="w-full relative overflow-hidden border border-cyber-accent/50 bg-cyber-accent/5">
                    <div
                        className="absolute inset-0 bg-cyber-accent/15 transition-all duration-300 ease-out"
                        style={{ width: `${downloadProgress}%` }}
                    />
                    <div className="relative py-3 flex items-center justify-center gap-2 font-bold text-base tracking-[0.3em] font-mono text-cyber-accent">
                        <Loader2 className="w-4 h-4 animate-spin" />
                        {downloadProgress === 0 ? `${t('server.downloading')} 0%` : `${t('server.downloading')} ${downloadProgress}%`}
                    </div>
                </div>
            );
        }

        // Checking engine
        if (engineStatus === 'checking') {
            return (
                <div className="w-full py-3 font-bold text-base tracking-[0.3em] font-mono flex items-center justify-center gap-2
                    bg-cyber-accent/10 text-cyber-accent/50 border border-cyber-accent/30">
                    <Loader2 className="w-4 h-4 animate-spin" />
                    CHECKING…
                </div>
            );
        }

        // Normal: START / STOP
        return (
            <button
                onClick={handleToggleServer}
                disabled={!isRunning && !selectedModelPath}
                className={`w-full py-3 font-bold text-base tracking-[0.3em] font-mono transition-all flex items-center justify-center gap-2 ${isRunning
                    ? 'bg-red-500/10 text-red-400 border border-red-500/50 hover:bg-red-500/20 shadow-[0_0_15px_rgba(239,68,68,0.15)]'
                    : !selectedModelPath
                        ? 'bg-cyber-border/30 text-cyber-text-muted/50 cursor-not-allowed border border-cyber-border/30'
                        : 'bg-cyber-accent/10 text-cyber-accent border border-cyber-accent/50 hover:bg-cyber-accent/20 shadow-[0_0_15px_rgba(0,255,157,0.15)]'
                    }`}
            >
                {isRunning ? (
                    <><Square className="w-3.5 h-3.5 fill-current" /> {t('btn.stop')}</>
                ) : (
                    <><Play className="w-3.5 h-3.5 fill-current" /> {t('btn.start')}</>
                )}
            </button>
        );
    };

    return (
        <div className="flex flex-col flex-1 overflow-hidden">
            {/* ===== Control Area ===== */}
            <div className="py-4 space-y-4 flex-shrink-0">
                {/* Current model display */}
                <div className="flex items-center gap-2 font-mono text-base">
                    <span className="text-cyber-text-secondary">{t('server.selectModel')}</span>
                    {selectedModelPath ? (
                        <>
                            <span className="text-cyber-accent font-bold truncate">{modelInfo.name}</span>
                            {modelInfo.quant && (
                                <span className="text-cyber-accent font-bold flex-shrink-0">{modelInfo.quant}</span>
                            )}
                        </>
                    ) : (
                        <span className="text-cyber-text-muted/70">{t('server.selectFromPanel')}</span>
                    )}
                </div>

                {/* Parameter row */}
                <div className="grid grid-cols-4 gap-3">
                    <div className="flex items-center gap-2">
                        <label className="text-[11px] text-cyber-text-secondary font-mono font-bold flex-shrink-0">{t('server.compute')}</label>
                        <MiniSelect
                            value={String(gpuLayers)}
                            onChange={(v) => setGpuLayers(Number(v))}
                            disabled={isRunning}
                            options={[
                                { id: '-1', label: t('server.gpuFull') },
                                { id: '0', label: t('server.cpuOnly') },
                            ]}
                            className="flex-1"
                        />
                    </div>
                    <div className="flex items-center gap-2">
                        <label className="text-[11px] text-cyber-text-secondary font-mono font-bold flex-shrink-0">{t('server.context')}</label>
                        <MiniSelect
                            value={String(contextSize)}
                            onChange={(v) => setContextSize(Number(v))}
                            disabled={isRunning}
                            options={[
                                { id: '2048', label: '2K' },
                                { id: '4096', label: '4K' },
                                { id: '8192', label: '8K' },
                                { id: '16384', label: '16K' },
                                { id: '32768', label: '32K' },
                            ]}
                            className="flex-1"
                        />
                    </div>
                    <div className="flex items-center gap-2">
                        <label className="text-[11px] text-cyber-text-secondary font-mono font-bold flex-shrink-0">{t('server.port')}</label>
                        <MiniSelect
                            value={String(serverPort)}
                            onChange={(v) => {
                                if (v === 'random') {
                                    setServerPort(10000 + Math.floor(Math.random() * 50000));
                                } else {
                                    setServerPort(Number(v));
                                }
                            }}
                            disabled={isRunning}
                            options={[
                                { id: String(serverPort), label: String(serverPort) },
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
                            options={runtimeOptions}
                            className="flex-1"
                        />
                    </div>
                </div>

                {/* Start button */}
                {renderStartButton()}
            </div>

            {/* ===== Terminal Output ===== */}
            <div className="flex-1 flex flex-col overflow-hidden">
                {/* Terminal header */}
                <div className="flex items-center justify-between py-2 border-b border-cyber-border/30 flex-shrink-0">
                    <div className="flex items-center gap-2 text-sm font-mono text-cyber-text-secondary">
                        <Terminal className="w-3 h-3" />
                        <span>{t('server.stdout')}</span>
                    </div>
                </div>

                {/* Log area */}
                <div className="relative flex-1">
                    <div ref={logsContainerRef} onScroll={handleScroll} className="absolute inset-0 overflow-y-auto py-3 bg-cyber-terminal font-mono text-sm space-y-0.5 custom-scrollbar">
                        {logs.length === 0 && (
                            <div className="flex items-center justify-center" style={{ minHeight: 'calc(100% - 24px)' }}>
                                <div className="font-mono text-center space-y-3">
                                    <div className="text-lg text-cyber-text-secondary/80">{'>'} {t('server.awaitingInit')}</div>
                                    <div className="text-base text-cyber-text-muted/70">{t('server.selectConfigStart')}</div>
                                </div>
                            </div>
                        )}
                        {logs.map((log, i) => (
                            <div key={i} className="leading-relaxed">
                                <span className="text-cyber-text-muted/60 select-none mr-2">$</span>
                                <span className={log.includes('[Error]') || log.includes('[ERR]') ? 'text-red-400' : 'text-cyber-text/80'}>
                                    {log}
                                </span>
                            </div>
                        ))}
                        <div ref={logsEndRef} />
                    </div>
                    {/* Scroll to bottom button */}
                    {showScrollBtn && (
                        <button
                            onClick={scrollToBottom}
                            className="absolute bottom-3 right-3 w-7 h-7 flex items-center justify-center bg-cyber-bg/90 border border-cyber-border/50 rounded text-cyber-text-secondary hover:text-cyber-accent hover:border-cyber-accent/50 transition-colors"
                        >
                            <ChevronDown className="w-4 h-4" />
                        </button>
                    )}
                </div>
            </div>

        </div>
    );
};


// Format file size
function formatSize(bytes: number): string {
    if (bytes >= 1e9) return (bytes / 1e9).toFixed(1) + ' GB';
    if (bytes >= 1e6) return (bytes / 1e6).toFixed(0) + ' MB';
    return (bytes / 1e3).toFixed(0) + ' KB';
}

// Estimate VRAM needed from file size (rough: fileSize * 1.2)
function estimateVramGb(fileSize: number): number {
    return Math.round((fileSize / 1e9) * 1.2 * 10) / 10;
}

// Parse VRAM string like "24 GB" to number
function parseVramString(vramStr: string): number {
    const match = vramStr.match(/(\d+\.?\d*)/);
    return match ? parseFloat(match[1]) : 0;
}

// Get VRAM fitness label and color
function getVramFitness(requiredGb: number, userVramGb: number, t: ReturnType<typeof useI18n>['t']): { label: string; color: string } | null {
    if (userVramGb <= 0) return null;
    const ratio = requiredGb / userVramGb;
    if (ratio <= 0.7) return { label: t('vram.easy'), color: 'text-green-400' };
    if (ratio <= 1.0) return { label: t('vram.good'), color: 'text-cyan-400' };
    if (ratio <= 1.3) return { label: t('vram.tight'), color: 'text-yellow-400' };
    return { label: t('vram.heavy'), color: 'text-red-400' };
}

// Known model names for icon detection
const KNOWN_MODELS = ['qwen', 'llama', 'deepseek', 'mistral', 'phi', 'gemma', 'yi', 'internlm', 'glm', 'chatglm', 'nemotron', 'codestral', 'mixtral'];

function guessIconFromFileName(fileName: string): string | null {
    const lower = fileName.toLowerCase();
    for (const name of KNOWN_MODELS) {
        if (lower.includes(name)) return name;
    }
    return null;
}

// ─── Right Panel: Dual-Tab Model Store ───

export const LocalServerPanel: React.FC = () => {
    const { t } = useI18n();
    const { ggufFiles, isScanning, rescanModels, selectedModelPath, setSelectedModelPath, modelsDirs, serverRunning, serverPort, serverModelName } = useLocalServer();
    const confirm = useConfirm();

    const [activeTab, setActiveTab] = useState<'local' | 'store'>('local');
    const [storeModels, setStoreModels] = useState<StoreModel[]>([]);
    const [isLoadingStore, setIsLoadingStore] = useState(false);
    const [expandedModelId, setExpandedModelId] = useState<string | null>(null);
    const [expandedLocalGroup, setExpandedLocalGroup] = useState<string | null>(null);

    // Delete mode state
    const [isDeleteMode, setIsDeleteMode] = useState(false);
    const [deleteSelection, setDeleteSelection] = useState<Set<string>>(new Set());

    // Download directory
    const [downloadDir, setDownloadDir] = useState('');

    // Global download state from context
    const { downloads, startDownload } = useDownload();

    // Rescan models after download completes
    useEffect(() => {
        const hasCompleted = Array.from(downloads.values()).some(d => d.status === 'completed');
        if (hasCompleted) rescanModels();
    }, [downloads, rescanModels]);

    // GPU info
    const [gpuName, setGpuName] = useState<string | null>(null);
    const [gpuVramGb, setGpuVramGb] = useState(0);

    // Local model dirs (mutable copy for add/remove)
    const [localDirs, setLocalDirs] = useState<string[]>(modelsDirs);
    useEffect(() => { setLocalDirs(modelsDirs); }, [modelsDirs]);

    // Load download dir + GPU info on mount
    useEffect(() => {
        api.getDownloadDir().then(setDownloadDir).catch(() => { });
        // Try cached GPU info first, then detect
        api.getGpuInfo().then(info => {
            if (info) {
                setGpuName(info.gpuName);
                setGpuVramGb(info.gpuVramGb);
            } else {
                // Auto-detect on first visit
                api.detectGpu().then(detected => {
                    if (detected) {
                        setGpuName(detected.gpuName);
                        setGpuVramGb(detected.gpuVramGb);
                    }
                }).catch(() => { });
            }
        }).catch(() => { });
    }, []);

    // Fetch store models: backend (remote→cache) then fallback to static JSON
    useEffect(() => {
        if (activeTab === 'store' && storeModels.length === 0) {
            setIsLoadingStore(true);
            api.getStoreModels()
                .then((data: StoreModel[]) => {
                    if (data && data.length > 0) {
                        setStoreModels(data);
                    } else {
                        // Fallback to static JSON
                        return fetch('./api/store/models.json')
                            .then(r => r.json())
                            .then((fallback: StoreModel[]) => setStoreModels(fallback));
                    }
                })
                .catch(() => {
                    // Double fallback
                    fetch('./api/store/models.json')
                        .then(r => r.json())
                        .then((fallback: StoreModel[]) => setStoreModels(fallback))
                        .catch(e => console.error('[ModelStore] All sources failed:', e));
                })
                .finally(() => setIsLoadingStore(false));
        }
    }, [activeTab, storeModels.length]);

    // Change download directory
    const handleChangeDownloadDir = async () => {
        try {
            const newDir = await api.setDownloadDir();
            setDownloadDir(newDir);
        } catch (e) {
            console.error('[ModelStore] Set download dir failed:', e);
        }
    };

    // Group local files by model name, with sourceDir
    const localGroups = (() => {
        const map: Record<string, { modelName: string; icon: string | null; sourceDir: string; variants: GgufFileEntry[] }> = {};
        for (const f of ggufFiles) {
            const base = f.fileName
                .replace(/\.gguf$/i, '')
                .replace(/[-.](?:q[0-9_]+[a-z_]*|f16|f32|fp16|bf16)$/i, '');
            if (!map[base]) {
                const displayName = base
                    .replace(/[-_]/g, ' ')
                    .replace(/\b\w/g, c => c.toUpperCase());
                // Find which directory this file belongs to
                const sourceDir = localDirs.find(d => f.filePath.replace(/\\/g, '/').startsWith(d.replace(/\\/g, '/'))) || '';
                map[base] = { modelName: displayName, icon: guessIconFromFileName(f.fileName), sourceDir, variants: [] };
            }
            map[base].variants.push(f);
        }
        return Object.values(map);
    })();

    // Check if a file exists locally
    const isDownloaded = (fileName: string) => ggufFiles.some(f => f.fileName === fileName);

    // ─── Handlers ───

    const handleAddDir = async () => {
        try {
            const dirs = await api.addModelsDir();
            setLocalDirs(dirs);
            rescanModels();
        } catch (e) {
            console.error('[ModelStore] Add dir failed:', e);
        }
    };

    const handleRemoveDirs = async () => {
        if (deleteSelection.size === 0) return;
        const ok = await confirm({
            title: t('server.removeDirectories'),
            message: t('server.removeDirectoryConfirm'),
            confirmText: t('btn.remove'),
            cancelText: t('btn.cancel'),
            type: 'danger',
        });
        if (!ok) return;
        for (const dir of deleteSelection) {
            const dirs = await api.removeModelsDir(dir);
            setLocalDirs(dirs);
        }
        setIsDeleteMode(false);
        setDeleteSelection(new Set());
        rescanModels();
    };

    return (
        <>
            {/* ===== Tab Header ===== */}
            <div className="p-2 flex items-center justify-between bg-transparent">
                <div className="flex gap-1">
                    <button
                        onClick={() => setActiveTab('local')}
                        className={`px-3 py-1.5 text-xs font-bold rounded transition-colors ${activeTab === 'local'
                            ? 'bg-cyber-accent text-black'
                            : 'text-cyber-text-secondary hover:text-cyber-text'
                            }`}
                    >
                        {t('server.local')}
                    </button>
                    <button
                        onClick={() => setActiveTab('store')}
                        className={`px-3 py-1.5 text-xs font-bold rounded transition-colors ${activeTab === 'store'
                            ? 'bg-cyan-400 text-black'
                            : 'text-cyber-text-secondary hover:text-cyber-text'
                            }`}
                    >
                        {t('server.store')}
                    </button>
                </div>
                {/* GPU info badge */}
                {gpuName && (
                    <span className="text-[10px] text-cyber-text-muted font-mono truncate max-w-[120px]">
                        {gpuName}{gpuVramGb > 0 ? ` ${gpuVramGb}G` : ''}
                    </span>
                )}
            </div>

            {/* ===== Content Area ===== */}
            <div className="flex-1 overflow-y-auto p-2">

                {/* ── LOCAL Tab ── */}
                {activeTab === 'local' && (
                    <>
                        {/* Directory Management Toolbar */}
                        <div className="flex items-center gap-2 mb-3">
                            {!isDeleteMode ? (
                                <>
                                    <button
                                        onClick={handleAddDir}
                                        className="text-[10px] font-mono font-bold text-green-500/70 hover:text-green-400 transition-colors"
                                    >
                                        {t('store.add')}
                                    </button>
                                    {localGroups.length > 0 && (
                                        <button
                                            onClick={() => {
                                                setIsDeleteMode(true);
                                                setDeleteSelection(new Set());
                                            }}
                                            className="text-[10px] font-mono font-bold text-red-500/50 hover:text-red-400 transition-colors ml-auto"
                                        >
                                            {t('store.del')}
                                        </button>
                                    )}
                                </>
                            ) : (
                                <>
                                    <button
                                        onClick={() => {
                                            setIsDeleteMode(false);
                                            setDeleteSelection(new Set());
                                        }}
                                        className="text-[10px] font-mono font-bold text-cyber-text-secondary hover:text-cyber-text transition-colors"
                                    >
                                        {t('store.cancel')}
                                    </button>
                                    <button
                                        onClick={handleRemoveDirs}
                                        disabled={deleteSelection.size === 0}
                                        className={`text-[10px] font-mono font-bold transition-colors ml-auto ${deleteSelection.size > 0
                                            ? 'text-red-400 hover:text-red-300'
                                            : 'text-cyber-text-secondary/50 cursor-not-allowed'
                                            }`}
                                    >
                                        [{t('store.remove')}({deleteSelection.size})]
                                    </button>
                                </>
                            )}
                        </div>

                        {isScanning ? (
                            <div className="flex items-center justify-center py-10">
                                <Loader2 className="w-5 h-5 text-cyber-accent animate-spin" />
                            </div>
                        ) : localGroups.length > 0 ? (
                            <div className="space-y-2">
                                {localGroups.map(group => {
                                    const groupKey = group.modelName;
                                    const isExpanded = expandedLocalGroup === groupKey;
                                    const selected = group.variants.find(v => v.filePath === selectedModelPath);
                                    const isGroupSelected = isDeleteMode && deleteSelection.has(group.sourceDir);

                                    return (
                                        <div
                                            key={groupKey}
                                            className={`p-3 border rounded transition-all ${isDeleteMode
                                                ? (isGroupSelected
                                                    ? 'border-red-500/50 bg-red-500/5'
                                                    : 'border-cyber-border hover:border-red-500/30')
                                                : (selected
                                                    ? 'border-green-500/50 bg-green-500/5'
                                                    : 'border-cyber-border hover:border-green-500/30')
                                                }`}
                                        >
                                            {/* Card Header */}
                                            <div
                                                className="flex items-center gap-3 cursor-pointer"
                                                onClick={() => {
                                                    if (isDeleteMode) {
                                                        // Delete mode: Toggle directory selection
                                                        if (group.sourceDir) {
                                                            setDeleteSelection(prev => {
                                                                const next = new Set(prev);
                                                                if (next.has(group.sourceDir)) next.delete(group.sourceDir);
                                                                else next.add(group.sourceDir);
                                                                return next;
                                                            });
                                                        }
                                                    } else {
                                                        // Normal mode: Expand/Collapse
                                                        setExpandedLocalGroup(isExpanded ? null : groupKey);
                                                    }
                                                }}
                                            >
                                                {/* Card selector: Normal=Green Circle / Delete=Red Square */}
                                                {isDeleteMode ? (
                                                    <div className={`w-4 h-4 rounded-sm border-2 flex items-center justify-center flex-shrink-0 transition-colors ${isGroupSelected ? 'border-red-400 bg-red-400' : 'border-cyber-border hover:border-red-400/50'
                                                        }`}>
                                                        {isGroupSelected && (
                                                            <svg className="w-3 h-3 text-white" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={3}>
                                                                <path strokeLinecap="round" strokeLinejoin="round" d="M5 13l4 4L19 7" />
                                                            </svg>
                                                        )}
                                                    </div>
                                                ) : (
                                                    <div className={`w-4 h-4 rounded-full border-2 flex items-center justify-center flex-shrink-0 transition-colors ${selected ? 'border-green-400' : 'border-cyber-border'}`}>
                                                        {selected && <div className="w-2 h-2 rounded-full bg-green-400" />}
                                                    </div>
                                                )}

                                                {/* Icon */}
                                                {group.icon ? (
                                                    <img
                                                        src={`./icons/models/${group.icon}.svg`}
                                                        alt={group.modelName}
                                                        className="w-6 h-6 flex-shrink-0"
                                                        onError={(e) => { (e.target as HTMLImageElement).style.display = 'none'; }}
                                                    />
                                                ) : (
                                                    <HardDrive className="w-6 h-6 text-cyber-text-secondary flex-shrink-0" />
                                                )}

                                                {/* Name + Description */}
                                                <div className="flex-1 min-w-0 flex flex-col justify-center h-10">
                                                    <div className="flex items-center justify-between gap-2">
                                                        <div className={`text-sm font-bold truncate leading-none ${isDeleteMode ? 'text-red-400/80' : ''}`}>
                                                            {group.modelName}
                                                        </div>
                                                        {!isDeleteMode && (
                                                            selected ? (
                                                                <span className="text-[10px] text-green-400 flex-shrink-0 font-mono font-bold">
                                                                    {(() => { const m = selected.fileName.match(/[-.]([Qq][0-9_]+[A-Za-z_]*)/); return m ? m[1].toUpperCase() : ''; })()}
                                                                </span>
                                                            ) : (
                                                                <span className="text-[10px] text-cyber-text-secondary flex-shrink-0">
                                                                    {group.variants.length} {t('store.ver')}
                                                                </span>
                                                            )
                                                        )}
                                                    </div>
                                                    <div className="text-[10px] text-cyber-text-secondary truncate leading-tight mt-1 opacity-70">
                                                        {group.sourceDir}
                                                    </div>
                                                </div>
                                            </div>

                                            {/* Expanded: Variant list (Normal Mode Only) */}
                                            {isExpanded && !isDeleteMode && (
                                                <div className="mt-3 pt-3 border-t border-cyber-border/30 space-y-1">
                                                    {group.variants.map(v => {
                                                        const isSelected = selectedModelPath === v.filePath;
                                                        const qMatch = v.fileName.match(/[-.]([Qq][0-9_]+[A-Za-z_]*)/);
                                                        const quant = qMatch ? qMatch[1].toUpperCase() : 'Default';

                                                        return (
                                                            <div
                                                                key={v.filePath}
                                                                className={`flex items-center gap-3 p-2 rounded cursor-pointer transition-colors ${isSelected ? 'bg-green-500/10' : 'hover:bg-cyber-surface/30'}`}
                                                                onClick={() => setSelectedModelPath(v.filePath)}
                                                            >
                                                                <span className={`text-xs font-mono font-bold w-14 flex-shrink-0 ${isSelected ? 'text-green-400' : 'text-cyber-accent'}`}>
                                                                    {quant}
                                                                </span>
                                                                <span className="text-[10px] text-cyber-text-secondary flex-1 whitespace-nowrap">
                                                                    {estimateVramGb(v.fileSize)} GB · {formatSize(v.fileSize)}
                                                                </span>
                                                                <span className="text-[10px] w-10 text-center flex-shrink-0">
                                                                    {(() => {
                                                                        const fit = getVramFitness(estimateVramGb(v.fileSize), gpuVramGb, t);
                                                                        return fit ? <span className={`font-bold ${fit.color}`}>{fit.label}</span> : null;
                                                                    })()}
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
                                })}
                            </div>
                        ) : (
                            <div className="text-center py-10">
                                <HardDrive className="w-8 h-8 text-cyber-text-secondary mx-auto mb-3 opacity-50" />
                                <p className="text-sm text-cyber-text-secondary">{t('server.selectModelDir')}</p>
                                <p className="text-[10px] text-cyber-text-secondary mt-1 opacity-70">{t('server.downloadFromStore')}</p>
                                {localDirs.length > 0 && (
                                    <div className="mt-4 text-[10px] text-cyber-text-muted space-y-1">
                                        {localDirs.map((dir, i) => (
                                            <div key={i} className="truncate">{dir}</div>
                                        ))}
                                    </div>
                                )}
                            </div>
                        )}
                    </>
                )}

                {/* ── STORE Tab ── */}
                {activeTab === 'store' && (
                    <>
                        {/* Download directory — clickable to change */}
                        {downloadDir && (
                            <div
                                className="mb-3 text-[10px] text-cyan-400 p-2 bg-cyan-400/10 rounded truncate cursor-pointer hover:bg-cyan-400/15 transition-colors"
                                onClick={handleChangeDownloadDir}
                            >
                                <FolderOpen className="w-3 h-3 inline mr-1" />{t('download.location')} {downloadDir}
                            </div>
                        )}

                        {isLoadingStore ? (
                            <div className="flex items-center justify-center py-10">
                                <Loader2 className="w-5 h-5 text-cyan-400 animate-spin" />
                            </div>
                        ) : (
                            <div className="space-y-2">
                                {storeModels.map(model => {
                                    const isExpanded = expandedModelId === model.id;
                                    const hasDownloaded = model.variants.some(v => isDownloaded(v.fileName));

                                    return (
                                        <div
                                            key={model.id}
                                            className={`p-3 border rounded cursor-pointer transition-all ${isExpanded
                                                ? 'border-cyan-400/50 bg-cyan-400/5'
                                                : hasDownloaded
                                                    ? 'border-cyan-400/30 hover:border-cyan-400/50'
                                                    : 'border-cyber-border hover:border-cyan-400/50'
                                                }`}
                                            onClick={() => setExpandedModelId(isExpanded ? null : model.id)}
                                        >
                                            {/* Card Header */}
                                            <div className="flex items-center gap-3">
                                                <img
                                                    src={`./icons/models/${model.icon}.svg`}
                                                    alt={model.name}
                                                    className="w-6 h-6 flex-shrink-0"
                                                    onError={(e) => {
                                                        const target = e.target as HTMLImageElement;
                                                        target.style.display = 'none';
                                                    }}
                                                />
                                                <div className="flex-1 min-w-0 flex flex-col justify-center">
                                                    <div className="flex items-center justify-between gap-2">
                                                        <div className="text-sm font-bold truncate leading-none text-cyan-400 flex-1 min-w-0">{model.name}</div>
                                                        {hasDownloaded && (
                                                            <span className="text-[10px] text-cyan-400 flex-shrink-0">{t('store.ready')}</span>
                                                        )}
                                                    </div>
                                                    <div className="text-[10px] text-cyber-text-secondary truncate leading-tight mt-1 opacity-70">
                                                        {model.description}
                                                    </div>
                                                </div>
                                            </div>

                                            {/* Expanded: Variants with download controls */}
                                            {isExpanded && (
                                                <div className="mt-3 pt-3 border-t border-cyber-border/30 space-y-1">
                                                    {model.variants.map(variant => {
                                                        const variantDownloaded = isDownloaded(variant.fileName);
                                                        const dlItem = downloads.get(variant.fileName);
                                                        const isActiveDownload = dlItem?.status === 'downloading' || dlItem?.status === 'speed_test';
                                                        const isPaused = dlItem?.status === 'paused';
                                                        return (
                                                            <div
                                                                key={variant.quantization}
                                                                className={`p-2 rounded transition-colors ${variantDownloaded ? 'bg-cyan-400/5' : 'hover:bg-cyber-surface/30'}`}
                                                            >
                                                                <div className="flex items-center gap-3">
                                                                    {/* Quantization */}
                                                                    <span className="text-xs font-mono font-bold text-cyan-400 w-14 flex-shrink-0">
                                                                        {variant.quantization}
                                                                    </span>
                                                                    {/* VRAM + Size */}
                                                                    <span className="text-[10px] text-cyber-text-secondary flex-1 whitespace-nowrap">
                                                                        {variant.recommendedVRAM} · {formatSize(variant.fileSize)}
                                                                    </span>
                                                                    {/* Fitness label */}
                                                                    <span className="text-[10px] w-10 text-center flex-shrink-0">
                                                                        {(() => {
                                                                            const reqGb = parseVramString(variant.recommendedVRAM);
                                                                            const fit = getVramFitness(reqGb, gpuVramGb, t);
                                                                            return fit ? <span className={`font-bold ${fit.color}`}>{fit.label}</span> : null;
                                                                        })()}
                                                                    </span>
                                                                    {/* Action: fixed width to prevent layout shift */}
                                                                    <div className="flex-shrink-0 w-8 flex items-center justify-center">
                                                                        {variantDownloaded ? (
                                                                            <span className="text-cyan-400 text-sm">✓</span>
                                                                        ) : (isActiveDownload || isPaused) ? (
                                                                            <span className={`text-[10px] font-mono ${isPaused ? 'text-yellow-400' : 'text-cyan-400'}`}>
                                                                                {dlItem?.progress ?? 0}%
                                                                            </span>
                                                                        ) : (
                                                                            <button
                                                                                onClick={(e) => { e.stopPropagation(); startDownload(model.huggingfaceRepo, variant.fileName); }}
                                                                                className={`${dlItem?.status === 'error' ? 'text-red-400 hover:text-red-300' : 'text-cyber-text-secondary hover:text-cyan-400'} transition-colors`}
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
                                })}
                            </div>
                        )}
                    </>
                )}
            </div>
        </>
    );
};

export const LocalServerBottom: React.FC = () => {
    const { t } = useI18n();
    const { serverRunning, serverPort, serverModelName, serverApiKey } = useLocalServer();
    const [copied, setCopied] = useState('');

    if (!serverRunning) return null;

    const handleCopy = (label: string, value: string) => {
        navigator.clipboard.writeText(value);
        setCopied(label);
        setTimeout(() => setCopied(''), 2000);
    };

    return (
        <div className="flex-shrink-0 border-t border-cyber-border/30">
            <div className="flex items-center gap-4 px-6 py-2 text-xs font-mono">
                <div
                    className="flex items-center gap-1.5 cursor-pointer hover:text-cyber-accent transition-colors"
                    onClick={() => handleCopy('model', serverModelName || 'local-server')}
                >
                    <span className="text-cyber-text-secondary/80">Model ID:</span>
                    <span className="text-cyber-accent">{copied === 'model' ? t('btn.copied') : t('btn.copy')}</span>
                </div>
                <span className="text-cyber-border">|</span>
                <div
                    className="flex items-center gap-1.5 cursor-pointer hover:text-cyber-accent transition-colors"
                    onClick={() => handleCopy('key', serverApiKey || 'not-needed')}
                >
                    <span className="text-cyber-text-secondary/80">API Key:</span>
                    <span className="text-cyber-accent">{copied === 'key' ? t('btn.copied') : t('btn.copy')}</span>
                </div>
                <span className="text-cyber-border">|</span>
                <div
                    className="flex items-center gap-1.5 cursor-pointer hover:text-cyber-accent transition-colors"
                    onClick={() => handleCopy('openai', `http://127.0.0.1:${serverPort}/v1`)}
                >
                    <span className="text-cyber-text-secondary/80">OpenAI:</span>
                    <code className="text-cyber-accent/80">127.0.0.1:{serverPort}/v1</code>
                    <span className="text-cyber-accent ml-0.5">{copied === 'openai' ? t('btn.copied') : t('btn.copy')}</span>
                </div>
                <span className="text-cyber-border">|</span>
                <div
                    className="flex items-center gap-1.5 cursor-pointer hover:text-cyber-accent transition-colors"
                    onClick={() => handleCopy('anthropic', `http://127.0.0.1:${serverPort}/anthropic`)}
                >
                    <span className="text-cyber-text-secondary/80">Anthropic:</span>
                    <code className="text-cyber-accent/80">127.0.0.1:{serverPort}/anthropic</code>
                    <span className="text-cyber-accent ml-0.5">{copied === 'anthropic' ? t('btn.copied') : t('btn.copy')}</span>
                </div>
            </div>
        </div>
    );
};
