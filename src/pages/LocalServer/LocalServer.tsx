// Local Server Page — llama-server management UI
// Layout restored from Electron v1.0.8 LocalModelPlayer.tsx
// Architecture: Provider + Main + Panel (consistent with other pages)
import { useState, useEffect, useRef } from 'react';
import {
  Play,
  Square,
  Terminal,
  ChevronDown,
  Download,
  Loader2,
  HardDrive,
  FolderOpen,
  Settings,
} from 'lucide-react';
import { MiniSelect } from '../../components/MiniSelect';
import { ViewModeTabs } from '../../components/ViewModeTabs';
import { useI18n } from '../../hooks/useI18n';
import { useConfirm } from '../../components/ConfirmDialog';
import { useDownload } from '../../components/DownloadContext';
import * as api from '../../api/tauri';
import type { SystemInfo } from '../../api/tauri';
import { normalizeStoreModels, type StoreModel } from '../../api/types';
import bundledStoreModels from '../../data/localModelStore.json';
import { useNavigationStore } from '../../stores/navigationStore';
import { useDocumentVisible } from '../../hooks/useDocumentVisible';
import { useLocalServer } from './context';
import type { EngineStatus, GgufFileEntry } from './context';
export { LocalServerProvider } from './provider';

// ─── Helper: Parse model info from file path ───

function isNewerEngineVersion(installed: string, latest: string): boolean {
  const parse = (value: string) =>
    value
      .match(/^[bv]?(\d+(?:\.\d+)*)$/)?.[1]
      .split('.')
      .map(Number);
  const current = parse(installed);
  const available = parse(latest);
  if (!current || !available) return false;
  for (let i = 0; i < Math.max(current.length, available.length); i++) {
    const difference = (available[i] || 0) - (current[i] || 0);
    if (difference !== 0) return difference > 0;
  }
  return false;
}

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
  const {
    selectedModelPath,
    setSelectedModelPath,
    serverRunning: isRunning,
    setServerRunning: setIsRunning,
    serverPort,
    setServerPort: setServerPortCtx,
    serverModelName: _serverModelName,
    setServerModelName,
    setServerApiKey,
    runtime,
    setRuntime,
  } = useLocalServer();

  // Configuration state
  const setServerPort = (v: number) => setServerPortCtx(v);
  const [gpuLayers, setGpuLayers] = useState<number>(-1);
  // Default to 32K for Mother Agent's system prompt and tool definitions.
  // Smaller contexts are available for lightweight non-agent workloads.
  const [contextSize, setContextSize] = useState<number>(32768);

  // System info: OS / arch / GPU — used for runtime filtering and COMPUTE option
  const [systemInfo, setSystemInfo] = useState<SystemInfo | null>(null);
  useEffect(() => {
    api
      .getSystemInfo()
      .then(setSystemInfo)
      .catch(() => {});
  }, []);

  // Runtime options: vLLM / SGLang only on Linux
  const isLinux = systemInfo ? systemInfo.os === 'linux' : navigator.platform.startsWith('Linux');
  const isWindows = systemInfo ? systemInfo.os === 'windows' : navigator.platform.startsWith('Win');
  // GPU-Full = full layer offload. NVIDIA → CUDA build; macOS llama.cpp is
  // Metal-enabled; other no-GPU hosts → AVX2 CPU build.
  const hasNvidiaGpu = systemInfo ? systemInfo.hasNvidiaGpu : false;

  // Hand off to Mother Agent ("一键安装" page) with a pre-filled trigger phrase.
  // Used by the CUDA detect/install helper pills below.
  const goToMother = useNavigationStore((s) => s.goToMother);
  const hasAmdGpu = systemInfo ? (systemInfo.hasAmdGpu ?? false) : false;
  // Apple Silicon has no separate VRAM number (unified memory) so it isn't
  // flagged nvidia/amd — but macOS llama.cpp uses Metal, so it IS GPU-capable.
  // Treat macOS as a usable GPU so we don't snap to -ngl 0 (CPU-only) on Metal.
  const hasMetalGpu = systemInfo?.os === 'macos';
  const runtimeOptions = [
    // Linux + GPU: vLLM / SGLang first (recommended for Linux). llama.cpp last.
    ...(isLinux && (hasNvidiaGpu || hasAmdGpu)
      ? [
          { id: 'vllm', label: 'vLLM' },
          { id: 'sglang', label: 'SGLang' },
        ]
      : []),
    { id: 'llama-server', label: 'llama.cpp' },
  ];

  // Linux + GPU: default runtime swaps from llama-server to vllm so the
  // selected card matches the first option in the recommended order.
  const runtimeDefaulted = useRef(false);
  useEffect(() => {
    if (!systemInfo || runtimeDefaulted.current) return;
    runtimeDefaulted.current = true;
    if (isLinux && (hasNvidiaGpu || hasAmdGpu) && runtime === 'llama-server') {
      setRuntime('vllm');
    }
  }, [systemInfo, isLinux, hasNvidiaGpu, hasAmdGpu, runtime, setRuntime]);

  // No-GPU + llama-server: snap gpuLayers from the -1 default (GPU-Auto)
  // down to 0 (CPU only) once systemInfo lands. Without this the COMPUTE
  // select shows '...' until the user clicks it — the -1 value has no
  // matching option when only CPU-only is offered. Also keeps the value
  // passed to startLlmServer in sync with what the user sees.
  useEffect(() => {
    if (!systemInfo) return;
    if (
      runtime === 'llama-server' &&
      !hasNvidiaGpu &&
      !hasAmdGpu &&
      !hasMetalGpu &&
      gpuLayers === -1
    ) {
      setGpuLayers(0);
    }
  }, [systemInfo, hasNvidiaGpu, hasAmdGpu, hasMetalGpu, runtime, gpuLayers]);

  // Server state
  const [logs, setLogs] = useState<string[]>([]);
  const logsEndRef = useRef<HTMLDivElement>(null);
  const logsContainerRef = useRef<HTMLDivElement>(null);

  // Engine detection
  const [engineStatus, setEngineStatus] = useState<EngineStatus>('checking');
  const [engineInstallDir, setEngineInstallDir] = useState<string>('');
  const [engineBinaryNames, setEngineBinaryNames] = useState<string[]>([]);
  const [engineVersion, setEngineVersion] = useState<string>('');
  const [engineCheckRevision, setEngineCheckRevision] = useState(0);
  const activeEngineRuntime = useRef(runtime);

  // Engine version picker (Windows + NVIDIA + llama-server only).
  // macOS/Linux + non-NVIDIA stay on the original auto-latest install
  // path; their llama.cpp builds aren't CUDA-variant-keyed anyway.
  // Picker is a modal opened ON DEMAND when the user clicks SETUP
  // ENGINE — the big button stays as-is rather than being replaced by
  // an inline dropdown (less always-on UI weight, less decision
  // pressure for users who just want the latest).
  const showVersionPicker = isWindows && hasNvidiaGpu && runtime === 'llama-server';
  const [engineOptions, setEngineOptions] = useState<api.LlamaReleaseOption[]>([]);
  const [enginePickerOpen, setEnginePickerOpen] = useState(false);
  // Gear dialog: full-command override for the llama-server launch.
  const [customCmdOpen, setCustomCmdOpen] = useState(false);
  const [customCmdText, setCustomCmdText] = useState('');
  const [defaultCmdText, setDefaultCmdText] = useState('');
  // Whether a saved custom launch command exists. It carries its own engine on
  // line 1, so START can run from it even when the bundled engine isn't set up.
  const [hasCustomCmd, setHasCustomCmd] = useState(false);
  useEffect(() => {
    api
      .getLlmCustomCommand()
      .then((c) => setHasCustomCmd(!!c))
      .catch(() => {});
  }, []);

  // Get engine download progress from global DownloadContext (single source of truth)
  // Key: use runtime name so progress matches the current engine being installed
  const { downloads } = useDownload();
  const engineDl = downloads.get(runtime);
  const downloadProgress = engineDl?.progress ?? 0;
  const downloadedSize = engineDl?.downloaded ?? 0;
  const totalSize = engineDl?.total ?? 0;

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

  // Check engine on mount AND when runtime changes
  useEffect(() => {
    activeEngineRuntime.current = runtime;
    // Guard against a slower earlier check resolving after a newer one (rapid
    // runtime switches) and overwriting engineStatus with the wrong runtime's
    // data — mirrors the cancelled guard on the version-picker effect below.
    let cancelled = false;
    const check = async () => {
      setEngineStatus('checking');
      try {
        const status = await api.getLocalEngineStatus(runtime);
        if (cancelled) return;
        const entry = status.engines.find((e) => e.name === runtime);
        if (entry?.installDir) setEngineInstallDir(entry.installDir);
        // Reset names first so stale data from previous runtime doesn't linger
        setEngineBinaryNames(entry?.binaryNames?.length ? entry.binaryNames : []);
        setEngineVersion(entry?.version || '');
        if (!entry?.installed) {
          setEngineStatus('not-installed');
        } else if (
          entry.latestVersion &&
          isNewerEngineVersion(entry.version, entry.latestVersion)
        ) {
          setEngineStatus('update-available');
        } else {
          setEngineStatus('ready');
        }
      } catch {
        if (!cancelled) setEngineStatus('error');
      }
    };
    check();
    return () => {
      cancelled = true;
    };
  }, [runtime, engineCheckRevision]);

  // Pre-fetch the version picker options on mount (Windows + NVIDIA
  // only). Done eagerly so opening the modal feels instant; empty
  // result → degrade to auto-latest install when the user clicks.
  useEffect(() => {
    if (!showVersionPicker) {
      setEngineOptions([]);
      return;
    }
    let cancelled = false;
    api
      .listEngineReleaseOptions(runtime, 10)
      .then((opts) => {
        if (!cancelled) setEngineOptions(opts);
      })
      .catch(() => {
        if (!cancelled) setEngineOptions([]);
      });
    return () => {
      cancelled = true;
    };
  }, [showVersionPicker, runtime]);

  // Sync engineStatus from DownloadContext
  useEffect(() => {
    if (!engineDl) return;
    if (
      engineDl.status === 'downloading' ||
      engineDl.status === 'speed_test' ||
      engineDl.status === 'installing'
    ) {
      setEngineStatus('downloading');
    } else if (engineDl.status === 'completed') {
      setEngineStatus('ready');
    } else if (engineDl.status === 'cancelled') {
      setEngineCheckRevision((value) => value + 1);
    } else if (engineDl.status === 'error') {
      setEngineStatus('error');
    }
    // Re-sync only when the download status changes; engineDl's identity can
    // change without its status changing.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [engineDl?.status]);

  // Install engine handler — routes by platform.
  // - Windows + NVIDIA + llama-server with options loaded: open the
  //   version picker modal. User picks; the modal calls
  //   handleInstallEngineWithPick which does the actual install.
  // - All other paths (macOS / Linux / non-NVIDIA / non-llama runtimes
  //   / empty options on network failure): auto-latest install.
  const handleDownloadEngine = async () => {
    if (showVersionPicker && engineOptions.length > 0) {
      setEnginePickerOpen(true);
      return;
    }
    setEngineStatus('downloading');
    try {
      await api.installLocalEngine(runtime);
      if (activeEngineRuntime.current === runtime) setEngineStatus('ready');
    } catch (err) {
      if (activeEngineRuntime.current === runtime) setEngineStatus('error');
      setLogs((prev) => [
        ...prev,
        `[Error] Engine install failed: ${err instanceof Error ? err.message : String(err)}`,
      ]);
    } finally {
      if (activeEngineRuntime.current === runtime) setEngineCheckRevision((value) => value + 1);
    }
  };

  const handleInstallEngineWithPick = async (opt: api.LlamaReleaseOption) => {
    setEnginePickerOpen(false);
    setEngineStatus('downloading');
    try {
      await api.installLocalEngine(runtime, {
        version: opt.tag,
        cudaVersion: opt.cudaVersion,
      });
      if (activeEngineRuntime.current === runtime) setEngineStatus('ready');
    } catch (err) {
      if (activeEngineRuntime.current === runtime) setEngineStatus('error');
      setLogs((prev) => [
        ...prev,
        `[Error] Engine install failed: ${err instanceof Error ? err.message : String(err)}`,
      ]);
    } finally {
      if (activeEngineRuntime.current === runtime) setEngineCheckRevision((value) => value + 1);
    }
  };

  const lastRunning = useRef<boolean>(isRunning);
  // Last server-log buffer we rendered, so the 1s poll only overwrites `logs`
  // when the backend buffer actually changed (otherwise a locally-appended
  // [Error] line gets clobbered on the next tick while the buffer is static).
  const lastServerLogs = useRef<string>('');

  const docVisible = useDocumentVisible();

  const localPage = useNavigationStore((state) => state.activePage === 'localLlm');
  useEffect(() => {
    if (!docVisible || !localPage) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const serverLogs = await api.getLlmServerLogs();
        if (cancelled) return;
        const joined = serverLogs.join('\n');
        if (serverLogs.length > 0 && joined !== lastServerLogs.current) {
          lastServerLogs.current = joined;
          setLogs(serverLogs);
        }
      } catch (e) {
        console.error('[LocalServer] Poll error:', e);
      } finally {
        if (!cancelled) timer = setTimeout(poll, 1000);
      }
    };
    void poll();
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [docVisible, localPage]);

  // Scroll handling
  const handleScroll = () => {
    const container = logsContainerRef.current;
    if (!container) return;
    const isAtBottom = container.scrollHeight - container.scrollTop - container.clientHeight < 40;
    autoFollowRef.current = isAtBottom;
    setShowScrollBtn(!isAtBottom && logs.length > 0);
  };

  useEffect(() => {
    if (logs.length > 0 && autoFollowRef.current && logsEndRef.current) {
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
        lastRunning.current = false;
        window.dispatchEvent(new Event('models-changed'));
      } catch (e) {
        setLogs((prev) => [...prev, `[Error] Failed to stop: ${e}`]);
      }
    } else {
      if (!selectedModelPath) return;
      setLogs([]);
      // Reset the change-detection guard too — otherwise, if the backend
      // returns a log buffer identical to the previous run's, the poll's
      // `joined !== lastServerLogs.current` check stays false and the cleared
      // panel never repopulates after a restart.
      lastServerLogs.current = '';
      autoFollowRef.current = true;
      setShowScrollBtn(false);
      try {
        await api.startLlmServer(selectedModelPath, serverPort, gpuLayers, contextSize, runtime);
        setIsRunning(true);
        lastRunning.current = true;
        window.dispatchEvent(new Event('models-changed'));
        // Fetch server info to get the generated API key
        try {
          const info = await api.getLlmServerInfo();
          if (info.apiKey) setServerApiKey(info.apiKey);
        } catch {
          /* ignore */
        }
      } catch (e) {
        setLogs((prev) => [...prev, `[Error] ${e}`]);
      }
    }
  };

  // Gear dialog handlers. The command is one token per line (line 1 = the
  // executable). Opening prefills with the stored custom command if any, else
  // the auto default; save stores it (empty = clear); reset clears + restores
  // the default text. The backend spawns a stored command verbatim, so this is
  // how an AMD user points at their own Vulkan llama-server build.
  const cmdToText = (c: { exe: string; args: string[] }) => [c.exe, ...c.args].join('\n');

  // The launch command's model is owned by the right-panel selector, not the
  // gear: EchoBird injects -m (+ --host/--port) at launch. So the dialog always
  // shows the model line as the current UI selection (or a placeholder when
  // none), and adopting a hand-typed -m on save just sets that selection.
  const MODEL_TOKEN = '<MODEL>';
  // Placeholder for line 1 (the engine exe) used when no engine is installed —
  // shown in the seeded template so the box isn't blank; START stays disabled
  // until the user replaces it with a real path (see saveCustomCmd).
  const ENGINE_TOKEN = '<ENGINE>';
  const setModelInArgs = (args: string[], model: string): string[] => {
    const out = [...args];
    const i = out.findIndex((a) => a === '-m' || a === '--model');
    if (i >= 0) out[i + 1] = model;
    else out.push('-m', model);
    return out;
  };
  const modelFromArgs = (args: string[]): string | null => {
    const i = args.findIndex((a) => a === '-m' || a === '--model');
    return i >= 0 && i + 1 < args.length ? args[i + 1] : null;
  };

  const openCustomCmd = async () => {
    const model = selectedModelPath || MODEL_TOKEN;
    // The auto default needs an installed engine (+ model) to compute. When the
    // engine isn't set up yet — which is exactly when a bring-your-own-engine
    // user opens this to point at their own build — that call fails; treat it as
    // "no default" and still open the dialog rather than blocking on it.
    let defText: string;
    try {
      const def = await api.getLlmDefaultCommand(
        selectedModelPath || '',
        serverPort,
        gpuLayers,
        contextSize
      );
      defText = cmdToText({ exe: def.exe, args: setModelInArgs(def.args, model) });
    } catch {
      // No engine installed: seed a template instead of a blank box so the user
      // can see our one-token-per-line format. Known values (host/port/ctx) are
      // filled; engine + model stay as <ENGINE>/<MODEL> for the user to replace.
      defText = [
        ENGINE_TOKEN,
        '-m',
        model,
        '--host',
        '127.0.0.1',
        '--port',
        String(serverPort),
        '--ctx-size',
        String(contextSize),
      ].join('\n');
    }
    setDefaultCmdText(defText);
    try {
      const custom = await api.getLlmCustomCommand();
      setCustomCmdText(
        custom ? cmdToText({ exe: custom.exe, args: setModelInArgs(custom.args, model) }) : defText
      );
    } catch (e) {
      setLogs((prev) => [...prev, `[Error] ${e}`]);
      setCustomCmdText(defText);
    }
    setCustomCmdOpen(true);
  };

  const saveCustomCmd = async () => {
    const lines = customCmdText
      .split('\n')
      .map((l) => l.trim())
      .filter((l) => l.length > 0);
    try {
      if (lines.length === 0 || lines[0] === ENGINE_TOKEN) {
        // Empty, or the engine placeholder was never filled in → treat as not
        // configured so START doesn't light up on a command that can't run.
        await api.clearLlmCustomCommand();
        setHasCustomCmd(false);
      } else {
        await api.setLlmCustomCommand(lines[0], lines.slice(1));
        setHasCustomCmd(true);
        // Adopt a hand-typed real model path as the selection so START enables
        // and the right panel reflects it (the placeholder token is ignored).
        const m = modelFromArgs(lines.slice(1));
        if (m && m !== MODEL_TOKEN && m !== selectedModelPath) {
          setSelectedModelPath(m);
        }
      }
      setCustomCmdOpen(false);
    } catch (e) {
      setLogs((prev) => [...prev, `[Error] ${e}`]);
    }
  };

  const resetCustomCmd = async () => {
    try {
      await api.clearLlmCustomCommand();
      setHasCustomCmd(false);
      setCustomCmdText(defaultCmdText);
    } catch (e) {
      setLogs((prev) => [...prev, `[Error] ${e}`]);
    }
  };

  // Render START button (state machine)
  const renderStartButton = () => {
    // START needs both a model and an engine. The engine can come from the
    // bundled install OR a saved custom command (whose line 1 is its own exe),
    // so a custom command lets a no-engine user run their own build. The model
    // is always selectedModelPath (a hand-typed -m is adopted into it on save),
    // so that half is unchanged.
    // A saved custom command only applies to the llama-server runtime (that's
    // the only runtime its gear is shown on and the only engine it overrides),
    // so it must not enable START on vllm/sglang where it doesn't apply.
    const customCmdUsable = hasCustomCmd && runtime === 'llama-server';
    const disabledStart =
      !isRunning &&
      (!selectedModelPath ||
        (engineStatus === 'not-installed' && !customCmdUsable) ||
        engineStatus === 'downloading' ||
        engineStatus === 'checking' ||
        (engineStatus === 'error' && !customCmdUsable));
    const btnBase =
      'font-bold text-base font-mono transition-colors flex items-center justify-center gap-2 flex-shrink-0 border rounded-button outline-none';
    const btnActive = 'border-cyber-border/50 text-cyber-text hover:bg-cyber-text/10';
    const btnDisabled = 'border-cyber-border text-cyber-text-muted';
    const btnStop = 'border-cyber-error/50 text-cyber-error hover:bg-cyber-error/10';

    const startStopBtn = (
      <button
        onClick={handleToggleServer}
        disabled={disabledStart}
        className={`py-[calc(0.75rem_-_1px)] px-[calc(1.5rem_-_1px)] tracking-[0.3em] ${btnBase} ${isRunning ? btnStop : disabledStart ? btnDisabled : btnActive}`}
      >
        {isRunning ? (
          <>
            <Square className="w-3.5 h-3.5 fill-current" /> {t('btn.stop')}
          </>
        ) : (
          <>
            <Play className="w-3.5 h-3.5 fill-current" /> {t('btn.start')}
          </>
        )}
      </button>
    );

    const folderBtn = (
      <button
        onClick={() => engineInstallDir && api.openFolder(engineInstallDir)}
        disabled={!engineInstallDir}
        className={`p-[calc(0.75rem_-_1px)] ${btnBase} ${
          engineInstallDir
            ? 'border-cyber-border/50 text-cyber-text-secondary hover:text-cyber-text hover:bg-cyber-text/10'
            : btnDisabled
        }`}
      >
        <FolderOpen className="w-4 h-4" />
      </button>
    );

    // Gear: custom launch command. Always available on the llama-server runtime
    // — even with no bundled engine installed — so a user can open it and point
    // at their own llama.cpp build instead. The auto-default prefill needs an
    // engine, but openCustomCmd degrades gracefully (blank default) without one.
    const canCustomize = runtime === 'llama-server';
    const gearBtn = canCustomize ? (
      <button
        onClick={openCustomCmd}
        className={`p-[calc(0.75rem_-_1px)] ${btnBase} border-cyber-border/50 text-cyber-text-secondary hover:text-cyber-text hover:bg-cyber-text/10`}
      >
        <Settings className="w-4 h-4" />
      </button>
    ) : null;

    // Engine not installed: show SETUP ENGINE button (always full-width).
    // On click, Windows+NVIDIA+llama-server opens the picker modal;
    // other paths go straight to auto-latest install.
    if (engineStatus === 'not-installed' || engineStatus === 'error') {
      return (
        <div className="flex gap-1.5 w-full">
          <button
            onClick={handleDownloadEngine}
            className="flex-1 py-[calc(0.75rem_-_1px)] font-bold text-base tracking-[0.3em] font-mono transition-colors flex items-center justify-center gap-2 rounded-lg border outline-none bg-cyber-accent text-white border-cyber-accent hover:bg-cyber-accent-secondary hover:border-cyber-accent-secondary shadow-lg shadow-cyber-accent/30"
          >
            <Download className="w-4 h-4" />
            {engineStatus === 'error'
              ? `\u26A0 ${t('server.setupEngine')}`
              : t('server.setupEngine')}
          </button>
          {folderBtn}
          {gearBtn}
          {startStopBtn}
        </div>
      );
    }

    // Downloading: show progress bar
    if (engineStatus === 'downloading') {
      return (
        <div className="flex gap-1.5 w-full">
          <div className="flex-1 relative overflow-hidden rounded-lg bg-cyber-border">
            <div
              className="absolute inset-0 bg-cyber-text/40 transition-all duration-300 ease-out"
              style={{ width: `${downloadProgress}%` }}
            />
            <div className="relative py-3 flex items-center justify-center gap-2 font-bold text-base tracking-[0.3em] font-mono text-cyber-text">
              <Loader2 className="w-4 h-4 animate-spin" />
              {downloadProgress === 0
                ? `${t('server.downloading')} 0%`
                : totalSize > 0
                  ? `${t('server.downloading')} ${downloadProgress}% · ${formatSize(downloadedSize)}/${formatSize(totalSize)}`
                  : `${t('server.downloading')} ${downloadProgress}%`}
            </div>
          </div>
          {folderBtn}
          {gearBtn}
          {startStopBtn}
        </div>
      );
    }

    // Checking engine
    if (engineStatus === 'checking') {
      return (
        <div className="flex gap-1.5 w-full">
          <div className="flex-1 py-3 flex items-center justify-center rounded-lg bg-cyber-border text-cyber-text-secondary">
            <Loader2 className="w-5 h-5 animate-spin" />
          </div>
          {folderBtn}
          {gearBtn}
          {startStopBtn}
        </div>
      );
    }

    // Ready and update-available states share the same engine information and actions.
    return (
      <div className="flex gap-1.5 w-full">
        {engineStatus === 'update-available' && (
          <button
            onClick={handleDownloadEngine}
            className={`py-2 px-4 font-mono text-xs flex items-center gap-1.5 rounded-button border transition-colors flex-shrink-0 outline-none ${btnActive}`}
          >
            <Download className="w-3.5 h-3.5" />
            {t('server.upgradeEngine') || 'UPGRADE ENGINE'}
          </button>
        )}
        <div className="flex-1 py-2 px-4 font-mono text-xs flex items-center gap-0 rounded-lg bg-cyber-border/60 overflow-hidden min-w-0">
          <HardDrive className="w-3.5 h-3.5 flex-shrink-0 text-cyber-text mr-2" />
          {engineBinaryNames.length > 0 ? (
            <div className="flex items-center gap-0 min-w-0 overflow-hidden">
              {engineBinaryNames.map((name, i) => (
                <span key={name} className="flex items-center gap-0 min-w-0">
                  {i > 0 && (
                    <span className="flex-shrink-0 mx-2 text-cyber-text opacity-70">+</span>
                  )}
                  <span
                    className={`truncate tracking-wide ${i === 0 ? 'text-cyber-text' : 'text-cyber-text-secondary'}`}
                    style={{ minWidth: 0 }}
                  >
                    {name}
                  </span>
                </span>
              ))}
            </div>
          ) : (
            <span className="text-cyber-text-secondary truncate">
              {runtime}
              {engineVersion ? ` v${engineVersion}` : ''}
            </span>
          )}
        </div>
        {folderBtn}
        {gearBtn}
        {startStopBtn}
      </div>
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
              <span className="text-cyber-text font-bold truncate">{modelInfo.name}</span>
              {modelInfo.quant && (
                <span className="text-cyber-text font-bold flex-shrink-0">{modelInfo.quant}</span>
              )}
            </>
          ) : (
            <span className="text-cyber-text-muted/70">{t('server.selectFromPanel')}</span>
          )}
        </div>

        {/* Parameter row */}
        <div className="grid grid-cols-4 gap-3 [&_button.opacity-40]:opacity-100 [&_button.opacity-40]:border-cyber-border/40 [&_button.opacity-40>*]:opacity-40">
          {/* Compute mode is auto-determined, never a user choice here: the list
              always resolves to a single option — GPU-Full when a supported GPU
              is present (or for vLLM/SGLang, which manage the GPU internally),
              otherwise CPU-only on a GPU-less machine. So it is always locked and
              acts purely as a status indicator. (Previously the CPU-only case was
              left interactive, showing a pointless single-item dropdown.) */}
          <div className="flex items-center gap-2">
            <label className="text-[11px] text-cyber-text-secondary font-mono font-bold flex-shrink-0">
              {t('server.compute')}
            </label>
            <MiniSelect
              value={runtime !== 'llama-server' ? '-1' : String(gpuLayers)}
              onChange={(v) => setGpuLayers(Number(v))}
              disabled
              options={[
                // GPU-Full always first; shown when any GPU present or non-llama runtime
                ...(runtime !== 'llama-server' || hasNvidiaGpu || hasAmdGpu || hasMetalGpu
                  ? [{ id: '-1', label: t('server.gpuFull') }]
                  : []),
                // CPU-only only shown when no GPU detected and using llama-server
                ...(!hasNvidiaGpu && !hasAmdGpu && !hasMetalGpu && runtime === 'llama-server'
                  ? [{ id: '0', label: t('server.cpuOnly') }]
                  : []),
              ]}
              className="flex-1"
            />
          </div>

          <div className="flex items-center gap-2">
            <label className="text-[11px] text-cyber-text-secondary font-mono font-bold flex-shrink-0">
              {t('server.context')}
            </label>
            <MiniSelect
              value={String(contextSize)}
              onChange={(v) => setContextSize(Number(v))}
              disabled={isRunning}
              options={[
                { id: '8192', label: '8K' },
                { id: '16384', label: '16K' },
                { id: '32768', label: '32K' },
                { id: '65536', label: '64K' },
                { id: '131072', label: '128K' },
                { id: '262144', label: '256K' },
                { id: '524288', label: '512K' },
                { id: '1048576', label: '1M' },
              ]}
              className="flex-1"
            />
          </div>
          <div className="flex items-center gap-2">
            <label className="text-[11px] text-cyber-text-secondary font-mono font-bold flex-shrink-0">
              {t('server.port')}
            </label>
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
            <label className="text-[11px] text-cyber-text-secondary font-mono font-bold flex-shrink-0">
              {t('server.runtime')}
            </label>
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
        <div className="flex items-center justify-between py-2 flex-shrink-0">
          <div className="flex items-center gap-2 text-sm font-mono text-cyber-text-secondary">
            <Terminal className="w-3 h-3" />
            <span>{t('server.stdout')}</span>
          </div>
        </div>

        {/* Log area */}
        <div className="relative flex-1">
          <div
            ref={logsContainerRef}
            onScroll={handleScroll}
            className="absolute inset-0 overflow-y-auto py-3 bg-cyber-surface/60 border border-transparent font-mono text-sm space-y-0.5 custom-scrollbar rounded-lg"
          >
            {logs.length === 0 && (
              <div
                className="flex flex-col items-center justify-center gap-10 py-8"
                style={{ minHeight: 'calc(100% - 24px)' }}
              >
                <div className="font-mono text-center space-y-3">
                  <div className="text-lg text-cyber-text-secondary/80">
                    {'>'} {t('server.awaitingInit')}
                  </div>
                  <div className="text-base text-cyber-text-muted/70">
                    {t('server.selectConfigStart')}
                  </div>
                </div>
                <div className="w-full max-w-3xl px-4">
                  <div className="rounded-lg border border-transparent p-5">
                    <table className="w-full text-sm border-collapse font-sans">
                      <thead>
                        <tr className="text-cyber-text-secondary">
                          <th className="px-3 pb-3 text-left border-b border-cyber-border/80 w-20 text-xs font-semibold uppercase tracking-wider"></th>
                          <th className="px-3 pb-3 text-left border-b border-cyber-border/80 text-xs font-semibold uppercase tracking-wider">
                            {isWindows ? t('server.tier.minSpec') : t('server.tier.minSpecAuto')}
                          </th>
                        </tr>
                      </thead>
                      <tbody className="text-cyber-text/90">
                        <tr className="border-b border-cyber-border/40">
                          <td className="px-3 py-4 font-semibold text-cyber-text-secondary text-center align-middle">
                            {t('server.tier.entry')}
                          </td>
                          <td className="px-3 py-4 align-top space-y-1">
                            <div>4–6GB GPU / Apple Silicon 16GB</div>
                            <div className="text-cyber-text-muted/80 text-xs">
                              Qwen3.5 2B/4B、MiniCPM5 1B/2B、Granite 4.2 3B（Q4）
                            </div>
                          </td>
                        </tr>
                        <tr className="border-b border-cyber-border/40">
                          <td className="px-3 py-4 font-semibold text-cyber-text-secondary text-center align-middle">
                            {t('server.tier.recommended')}
                          </td>
                          <td className="px-3 py-4 align-top space-y-1">
                            <div>8GB GPU / Apple Silicon 16–24GB</div>
                            <div className="text-cyber-text-muted/80 text-xs">
                              Qwen3.5 9B、Gemma 4 E4B、LFM2.5 8B-A1B（Q4）
                            </div>
                          </td>
                        </tr>
                        <tr>
                          <td className="px-3 py-4 font-semibold text-cyber-text-secondary text-center align-middle">
                            {t('server.tier.flagship')}
                          </td>
                          <td className="px-3 py-4 align-top space-y-1">
                            <div>12–16GB GPU / Apple Silicon 24–32GB</div>
                            <div className="text-cyber-text-muted/80 text-xs">
                              Qwen3.5 9B（Q6/Q8）、Gemma 4 12B（Q4/Q6）
                            </div>
                          </td>
                        </tr>
                      </tbody>
                    </table>
                  </div>
                  {/* CUDA helpers — Windows only (macOS has no CUDA; Linux users handle their own toolkit).
                      Same chip style as the Mother Agent quick-command chips: NOT bold (bold CJK at this
                      size renders cramped/blurry) and bordered for visual consistency. */}
                  {isWindows && (
                    <div className="flex justify-center gap-3 mt-4">
                      <button
                        onClick={() => goToMother(t('mother.hintDetectCuda'))}
                        className="px-3 py-1.5 text-xs rounded-button border border-cyber-border/50 text-cyber-text-secondary hover:bg-cyber-text/10 hover:text-cyber-text transition-colors outline-none"
                      >
                        {t('mother.hintDetectCuda')}
                      </button>
                      <button
                        onClick={() => goToMother(t('mother.hintInstallCuda'))}
                        className="px-3 py-1.5 text-xs rounded-button border border-cyber-border/50 text-cyber-text-secondary hover:bg-cyber-text/10 hover:text-cyber-text transition-colors outline-none"
                      >
                        {t('mother.hintInstallCuda')}
                      </button>
                    </div>
                  )}
                </div>
              </div>
            )}
            {logs.map((log, i) => (
              <div key={i} className="leading-relaxed">
                <span className="text-cyber-text-muted/60 select-none mr-2">$</span>
                <span
                  className={
                    log.includes('[Error]') || log.includes('[ERR]')
                      ? 'text-red-400'
                      : 'text-cyber-text/80'
                  }
                >
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
              className="absolute bottom-3 right-3 w-7 h-7 flex items-center justify-center bg-cyber-bg/90 border border-cyber-border/50 rounded text-cyber-text-secondary hover:text-cyber-text hover:border-cyber-border/50 transition-colors"
            >
              <ChevronDown className="w-4 h-4" />
            </button>
          )}
        </div>
      </div>

      {/* Engine version picker modal — opened only on Windows+NVIDIA */}
      {enginePickerOpen && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center"
          onClick={() => setEnginePickerOpen(false)}
        >
          <div
            className="bg-cyber-surface border border-cyber-border rounded-lg shadow-2xl w-[520px] max-w-[90vw] max-h-[80vh] flex flex-col"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="px-5 py-4 border-b border-cyber-border/60">
              <div className="text-base font-bold text-cyber-text">
                {t('server.enginePicker.title')}
              </div>
              <div className="text-xs text-cyber-text-muted mt-1">
                {t('server.enginePicker.hint')}
              </div>
            </div>
            <div className="flex-1 overflow-y-auto custom-scrollbar p-2">
              {(() => {
                // Tag of the most recent release — every CUDA variant
                // from that release gets the "latest" badge (not just
                // the first row, which was previously misleading: e.g.
                // CUDA 12.4 of b9320 happened to come first in the
                // assets array and got the badge while CUDA 13.1 of
                // the SAME release didn't).
                const latestTag = engineOptions[0]?.tag;
                return engineOptions.map((opt) => (
                  <button
                    key={opt.assetName}
                    onClick={() => handleInstallEngineWithPick(opt)}
                    className="w-full text-left px-3 py-2.5 rounded hover:bg-cyber-elevated transition-colors flex items-center justify-between gap-3 group"
                  >
                    <div className="font-mono text-sm text-cyber-text">
                      <span className="font-bold">CUDA {opt.cudaVersion}</span>
                      <span className="text-cyber-text-muted ml-2">· {opt.tag}</span>
                      {opt.tag === latestTag && (
                        <span className="ml-2 text-xs text-cyber-accent">
                          {t('server.enginePicker.latest')}
                        </span>
                      )}
                    </div>
                    <Download className="w-4 h-4 text-cyber-text-muted group-hover:text-cyber-accent flex-shrink-0" />
                  </button>
                ));
              })()}
            </div>
            <div className="px-5 py-3 border-t border-cyber-border/60 flex justify-end">
              <button
                onClick={() => setEnginePickerOpen(false)}
                className="px-[calc(1rem_-_1px)] py-[calc(0.375rem_-_1px)] text-sm font-mono border border-cyber-border/50 text-cyber-text-secondary hover:text-cyber-text hover:bg-cyber-text/10 rounded-button transition-colors outline-none"
              >
                {t('btn.cancel')}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Custom launch command modal (gear) — full-command override + reset */}
      {customCmdOpen && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center"
          onClick={() => setCustomCmdOpen(false)}
        >
          <div
            className="bg-cyber-surface border border-cyber-border rounded-lg shadow-2xl w-[600px] max-w-[92vw] max-h-[80vh] flex flex-col"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="px-5 py-4 border-b border-cyber-border/60">
              <div className="text-base font-bold text-cyber-text">
                {t('server.customCmdTitle')}
              </div>
              <div className="text-xs text-cyber-text-muted mt-1 leading-relaxed">
                {t('server.customCmdDesc')}
              </div>
            </div>
            <div className="flex-1 overflow-y-auto custom-scrollbar p-4">
              <textarea
                value={customCmdText}
                onChange={(e) => setCustomCmdText(e.target.value)}
                spellCheck={false}
                rows={10}
                className="w-full bg-cyber-input border border-cyber-border rounded-button px-3 py-2 text-xs text-cyber-text font-mono leading-relaxed focus:border-cyber-accent focus:outline-none resize-none"
              />
            </div>
            <div className="px-5 py-3 border-t border-cyber-border/60 flex justify-between gap-2">
              <button
                onClick={resetCustomCmd}
                className="px-[calc(1rem_-_1px)] py-[calc(0.375rem_-_1px)] text-sm font-mono border border-cyber-border/50 text-cyber-text-secondary hover:text-cyber-text hover:bg-cyber-text/10 rounded-button transition-colors outline-none"
              >
                {t('server.customCmdReset')}
              </button>
              <div className="flex gap-2">
                <button
                  onClick={() => setCustomCmdOpen(false)}
                  className="px-[calc(1rem_-_1px)] py-[calc(0.375rem_-_1px)] text-sm font-mono border border-cyber-border/50 text-cyber-text-secondary hover:text-cyber-text hover:bg-cyber-text/10 rounded-button transition-colors outline-none"
                >
                  {t('btn.close')}
                </button>
                <button
                  onClick={saveCustomCmd}
                  className="px-[calc(1rem_-_1px)] py-[calc(0.375rem_-_1px)] text-sm font-mono border border-cyber-border/50 text-cyber-text rounded-button hover:bg-cyber-text/10 transition-colors outline-none"
                >
                  {t('btn.save')}
                </button>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};

// Format file size
function formatSize(bytes: number): string {
  if (bytes >= 1e9) return (bytes / 1e9).toFixed(1) + ' GB';
  if (bytes >= 1e6) return (bytes / 1e6).toFixed(0) + ' MB';
  return (bytes / 1e3).toFixed(0) + ' KB';
}

// Known model names for icon detection
const KNOWN_MODELS = [
  'minicpm',
  'qwen',
  'llama',
  'deepseek',
  'mistral',
  'phi',
  'gemma',
  'yi',
  'internlm',
  'glm',
  'chatglm',
  'nemotron',
  'codestral',
  'mixtral',
  'granite',
];

function guessIconFromFileName(fileName: string): string | null {
  const lower = fileName.toLowerCase();
  if (lower.includes('lfm')) return 'liquid';
  for (const name of KNOWN_MODELS) {
    if (lower.includes(name)) return name;
  }
  return null;
}

// ─── Right Panel: Dual-Tab Model Store ───

export const LocalServerPanel: React.FC = () => {
  const { t } = useI18n();
  const {
    ggufFiles,
    isScanning,
    rescanModels,
    selectedModelPath,
    setSelectedModelPath,
    modelsDirs,
    serverRunning: _serverRunning,
    serverPort: _serverPort,
    serverModelName: _serverModelName,
    runtime,
  } = useLocalServer();
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
  const { downloads, startDownload, isDownloading } = useDownload();
  const downloadDirectoryLocked = Array.from(downloads.values()).some(
    (download) => download.repo && ['downloading', 'speed_test', 'paused'].includes(download.status)
  );

  // Rescan models after download completes
  const completedDownloads = useRef(new Set<string>());
  useEffect(() => {
    const completed = new Set(
      [...downloads].filter(([, download]) => download.status === 'completed').map(([id]) => id)
    );
    const hasNewCompletion = [...completed].some((id) => !completedDownloads.current.has(id));
    completedDownloads.current = completed;
    if (hasNewCompletion) rescanModels();
  }, [downloads, rescanModels]);

  // Local model dirs (mutable copy for add/remove)
  const [localDirs, setLocalDirs] = useState<string[]>(modelsDirs);
  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setLocalDirs(modelsDirs);
  }, [modelsDirs]);

  // Load download directory on mount
  useEffect(() => {
    api
      .getDownloadDir()
      .then(setDownloadDir)
      .catch(() => {});
  }, []);

  // Fetch store models: backend (remote→cache) then the bundled catalog.
  useEffect(() => {
    let cancelled = false;
    if (activeTab === 'store' && storeModels.length === 0) {
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setIsLoadingStore(true);
      api
        .getStoreModels()
        .then((data: unknown) => {
          const normalized = normalizeStoreModels(data);
          if (!cancelled) {
            setStoreModels(
              normalized.length ? normalized : normalizeStoreModels(bundledStoreModels)
            );
            setIsLoadingStore(false);
          }
        })
        .catch(() => {
          if (!cancelled) {
            setStoreModels(normalizeStoreModels(bundledStoreModels));
            setIsLoadingStore(false);
          }
        });
    }
    return () => {
      cancelled = true;
    };
  }, [activeTab, storeModels.length]);

  // Change download directory
  const handleChangeDownloadDir = async () => {
    if (downloadDirectoryLocked) return;
    try {
      const newDir = await api.setDownloadDir();
      setDownloadDir(newDir);
      rescanModels();
    } catch (e) {
      console.error('[ModelStore] Set download dir failed:', e);
    }
  };

  // Collapse llama.cpp split-GGUF shards (`foo-00001-of-00008.gguf` …
  // `foo-00008-of-00008.gguf`) into a single virtual entry: keep the first
  // shard's path (llama-server auto-loads the rest from the same dir when
  // given the first shard) but sum every shard's size so the displayed size
  // and VRAM estimate reflect the whole model, not just one shard.
  const consolidatedFiles = (() => {
    const shardGroups: Record<string, GgufFileEntry[]> = {};
    const single: GgufFileEntry[] = [];
    for (const f of ggufFiles) {
      const m = f.fileName.match(/^(.+)-\d{5}-of-(\d{5})\.gguf$/i);
      if (!m) {
        single.push(f);
        continue;
      }
      const path = f.filePath.replace(/\\/g, '/');
      const key = `${path.slice(0, path.lastIndexOf('/'))}/${m[1]}-of-${m[2]}`;
      (shardGroups[key] ||= []).push(f);
    }
    const out: GgufFileEntry[] = [...single];
    for (const shards of Object.values(shardGroups)) {
      shards.sort((a, b) => a.fileName.localeCompare(b.fileName));
      const expected = Number(shards[0].fileName.match(/-of-(\d{5})\.gguf$/i)?.[1]);
      if (
        shards.length !== expected ||
        !shards.every(
          (file, index) =>
            Number(file.fileName.match(/-(\d{5})-of-\d{5}\.gguf$/i)?.[1]) === index + 1
        )
      )
        continue;
      const first = shards[0];
      const totalSize = shards.reduce((s, x) => s + x.fileSize, 0);
      out.push({ ...first, fileSize: totalSize });
    }
    return out;
  })();

  // Group local files by model name, with sourceDir
  const localGroups = (() => {
    const catalog = [...storeModels, ...bundledStoreModels];
    const map: Record<
      string,
      {
        id: string;
        modelName: string;
        icon: string | null;
        sourceDir: string;
        variants: GgufFileEntry[];
      }
    > = {};
    for (const f of consolidatedFiles) {
      const base = f.fileName
        .replace(/\.gguf$/i, '')
        // Strip the shard suffix (`-00001-of-00008`) before stripping the
        // quant suffix, so the resulting model name groups multi-shard +
        // single-file variants of the same model under one card.
        .replace(/-\d{5}-of-\d{5}$/i, '')
        .replace(/[-.](?:q[0-9_]+[a-z_]*|f16|f32|fp16|bf16)$/i, '');
      const catalogModel = catalog.find((model) =>
        model.variants.some((variant) =>
          variant.files.some((file) => file.split('/').pop() === f.fileName)
        )
      );
      const key = catalogModel?.id || base;
      if (!map[key]) {
        const displayName =
          catalogModel?.name || base.replace(/[-_]/g, ' ').replace(/\b\w/g, (c) => c.toUpperCase());
        // Find which directory this file belongs to
        const sourceDir =
          localDirs.find((d) => f.filePath.replace(/\\/g, '/').startsWith(d.replace(/\\/g, '/'))) ||
          '';
        map[key] = {
          id: key,
          modelName: displayName,
          icon: catalogModel?.icon || guessIconFromFileName(f.fileName),
          sourceDir,
          variants: [],
        };
      }
      map[key].variants.push(f);
    }
    return Object.values(map);
  })();

  // Keep small downloads first, including catalogs returned by older servers.
  const filteredStoreModels = storeModels
    .filter((model) => (model.runtimes || ['llama-server']).includes(runtime))
    .sort((a, b) => {
      const smallestA = Math.min(
        ...a.variants.map((v) =>
          Number.isFinite(v.fileSize) && v.fileSize > 0 ? v.fileSize : Infinity
        )
      );
      const smallestB = Math.min(
        ...b.variants.map((v) =>
          Number.isFinite(v.fileSize) && v.fileSize > 0 ? v.fileSize : Infinity
        )
      );
      return smallestA - smallestB;
    });

  // Strip any HF subdir prefix from a catalog filename (sharded unsloth GGUFs
  // are listed as "UD-Q4_K_XL/foo-00001-of-00010.gguf" but stored flat).
  const basename = (path: string) => {
    const i = path.lastIndexOf('/');
    return i === -1 ? path : path.slice(i + 1);
  };

  // Check if every file in a variant exists locally. Single-file variants pass
  // a one-element array; multi-shard variants pass the full shard list.
  const isDownloaded = (files: string[]) => {
    const first = basename(files[0] || '');
    return (
      files.length > 0 &&
      ggufFiles.some((candidate) => {
        if (candidate.fileName !== first) return false;
        const path = candidate.filePath.replace(/\\/g, '/');
        const dir = path.slice(0, path.lastIndexOf('/') + 1);
        return files.every((file) =>
          ggufFiles.some((entry) => entry.filePath.replace(/\\/g, '/') === dir + basename(file))
        );
      })
    );
  };

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
      <div className="h-10 px-2 flex items-center justify-between bg-transparent">
        <ViewModeTabs
          fullWidth
          value={activeTab}
          onChange={setActiveTab}
          options={[
            { value: 'local', label: t('server.local') },
            { value: 'store', label: t('server.store') },
          ]}
        />
      </div>

      {/* ===== Content Area ===== */}
      <div className="flex-1 overflow-y-auto p-2">
        {/* ── LOCAL Tab ── */}
        {activeTab === 'local' && (
          <div className="h-full flex flex-col">
            {/* Directory Management Toolbar */}
            <div className="flex items-center gap-2 mb-3 flex-shrink-0">
              {!isDeleteMode ? (
                <>
                  <button
                    onClick={handleAddDir}
                    className="text-xs font-mono font-bold text-cyber-text-secondary hover:text-cyber-text transition-colors"
                  >
                    {t('store.add')}
                  </button>
                  {localGroups.length > 0 && (
                    <button
                      onClick={() => {
                        setIsDeleteMode(true);
                        setDeleteSelection(new Set());
                      }}
                      className="text-xs font-mono font-bold text-red-500/50 hover:text-red-400 transition-colors ml-auto"
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
                    className="text-xs font-mono font-bold text-cyber-text-secondary hover:text-cyber-text transition-colors"
                  >
                    {t('store.cancel')}
                  </button>
                  <button
                    onClick={handleRemoveDirs}
                    disabled={deleteSelection.size === 0}
                    className={`text-xs font-mono font-bold transition-colors ml-auto ${
                      deleteSelection.size > 0
                        ? 'text-red-400 hover:text-red-300'
                        : 'text-cyber-text-secondary/50'
                    }`}
                  >
                    [{t('store.remove')}({deleteSelection.size})]
                  </button>
                </>
              )}
            </div>

            {isScanning ? (
              <div className="flex items-center justify-center py-10">
                <Loader2 className="w-5 h-5 text-cyber-text animate-spin" />
              </div>
            ) : localGroups.length > 0 ? (
              <div className="space-y-2">
                {localGroups.map((group) => {
                  const groupKey = group.id;
                  const isExpanded = expandedLocalGroup === groupKey;
                  const selected = group.variants.find((v) => v.filePath === selectedModelPath);
                  const isGroupSelected = isDeleteMode && deleteSelection.has(group.sourceDir);

                  return (
                    <div
                      key={groupKey}
                      className={`p-3 rounded transition-colors border bg-cyber-surface/60 ${
                        isDeleteMode
                          ? isGroupSelected
                            ? 'border-red-500'
                            : 'border-transparent hover:bg-cyber-elevated/70'
                          : selected
                            ? 'border-cyber-accent'
                            : 'border-transparent hover:bg-cyber-elevated/70'
                      }`}
                    >
                      {/* Card Header */}
                      <div
                        className="flex items-center gap-3"
                        onClick={() => {
                          if (isDeleteMode) {
                            // Delete mode: Toggle directory selection
                            if (group.sourceDir) {
                              setDeleteSelection((prev) => {
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
                          <div
                            className={`w-4 h-4 rounded-sm border-2 flex items-center justify-center flex-shrink-0 transition-colors ${
                              isGroupSelected
                                ? 'border-red-400 bg-red-400'
                                : 'border-cyber-border hover:border-red-400/50'
                            }`}
                          >
                            {isGroupSelected && (
                              <svg
                                className="w-3 h-3 text-white"
                                fill="none"
                                viewBox="0 0 24 24"
                                stroke="currentColor"
                                strokeWidth={3}
                              >
                                <path
                                  strokeLinecap="round"
                                  strokeLinejoin="round"
                                  d="M5 13l4 4L19 7"
                                />
                              </svg>
                            )}
                          </div>
                        ) : (
                          <div
                            className={`w-4 h-4 rounded-full border-2 relative flex-shrink-0 transition-colors ${selected ? 'border-cyber-accent' : 'border-cyber-border'}`}
                          >
                            {selected && (
                              <div className="w-2 h-2 rounded-full bg-cyber-accent absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2" />
                            )}
                          </div>
                        )}

                        {/* Icon */}
                        {group.icon ? (
                          <img
                            src={`./icons/models/${group.icon}.svg`}
                            alt={group.modelName}
                            className="w-8 h-8 flex-shrink-0"
                            onError={(e) => {
                              (e.target as HTMLImageElement).style.display = 'none';
                            }}
                          />
                        ) : (
                          <HardDrive className="w-8 h-8 text-cyber-text-secondary flex-shrink-0" />
                        )}

                        {/* Name + Description */}
                        <div className="flex-1 min-w-0 flex flex-col justify-center min-h-[2.5rem] py-0.5">
                          <div className="flex items-center justify-between gap-2">
                            <div
                              className={`text-sm font-bold truncate leading-none ${isDeleteMode ? 'text-red-400/80' : ''}`}
                            >
                              {group.modelName}
                            </div>
                            {!isDeleteMode &&
                              (selected ? (
                                <span className="text-[10px] text-cyber-accent flex-shrink-0 font-mono font-bold">
                                  {(() => {
                                    const m = selected.fileName.match(
                                      /[-.]([Qq][0-9_]+[A-Za-z_]*)/
                                    );
                                    return m ? m[1].toUpperCase() : '';
                                  })()}
                                </span>
                              ) : (
                                <span className="text-[10px] text-cyber-text-secondary flex-shrink-0">
                                  {group.variants.length} {t('store.ver')}
                                </span>
                              ))}
                          </div>
                          <div className="text-[10px] text-cyber-text-secondary truncate leading-tight mt-1 opacity-70">
                            {group.sourceDir}
                          </div>
                        </div>
                      </div>

                      {/* Expanded: Variant list (Normal Mode Only) */}
                      {isExpanded && !isDeleteMode && (
                        <div className="mt-3 pt-3 border-t border-cyber-border/30 space-y-1">
                          {group.variants.map((v) => {
                            const isSelected = selectedModelPath === v.filePath;
                            const qMatch = v.fileName.match(/[-.]([Qq][0-9_]+[A-Za-z_]*)/);
                            const quant = qMatch ? qMatch[1].toUpperCase() : 'Default';

                            return (
                              <div
                                key={v.filePath}
                                className={`flex items-center gap-3 p-2 rounded transition-colors ${isSelected ? 'bg-cyber-elevated/75' : 'hover:bg-cyber-elevated/70'}`}
                                onClick={() => setSelectedModelPath(v.filePath)}
                              >
                                <span
                                  className={`text-xs font-mono font-bold w-14 flex-shrink-0 ${isSelected ? 'text-cyber-accent' : 'text-cyber-text'}`}
                                >
                                  {quant}
                                </span>
                                <span className="text-[10px] text-cyber-text-secondary flex-1 whitespace-nowrap">
                                  {formatSize(v.fileSize)}
                                </span>
                                <div className="flex-shrink-0 w-8 flex items-center justify-center">
                                  <div
                                    className={`w-4 h-4 rounded-full border-2 relative transition-colors ${isSelected ? 'border-cyber-accent' : 'border-cyber-border hover:border-cyber-accent/50'}`}
                                  >
                                    {isSelected && (
                                      <div className="w-2 h-2 rounded-full bg-cyber-accent absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2" />
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
            ) : (
              <div className="flex-1 flex flex-col items-center justify-center text-center">
                <HardDrive className="w-8 h-8 text-cyber-text-secondary mb-3 opacity-50" />
                <p className="text-sm text-cyber-text-secondary">{t('server.selectModelDir')}</p>
                <p className="text-xs text-cyber-text-secondary mt-1 opacity-70">
                  {t('server.downloadFromStore')}
                </p>
                {localDirs.length > 0 && (
                  <div className="mt-4 text-[10px] text-cyber-text-muted space-y-1">
                    {localDirs.map((dir, i) => (
                      <div key={i} className="truncate">
                        {dir}
                      </div>
                    ))}
                  </div>
                )}
              </div>
            )}
          </div>
        )}

        {/* ── STORE Tab ── */}
        {activeTab === 'store' && (
          <>
            {/* Download directory — clickable to change */}
            {downloadDir && (
              <div
                className="mb-3 text-[10px] text-cyber-text-secondary p-2 bg-cyber-surface/60 border border-cyber-border rounded truncate hover:bg-cyber-elevated/70 transition-colors"
                aria-disabled={downloadDirectoryLocked}
                onClick={handleChangeDownloadDir}
              >
                <FolderOpen className="w-3 h-3 inline mr-1" />
                {t('download.location')} {downloadDir}
              </div>
            )}

            {isLoadingStore ? (
              <div className="flex items-center justify-center py-10">
                <Loader2 className="w-5 h-5 text-cyber-accent animate-spin" />
              </div>
            ) : (
              <div className="space-y-2">
                {/* Runtime filter badge */}
                {runtime !== 'llama-server' && (
                  <div className="text-[10px] font-mono text-cyber-text-muted px-1 pb-1">
                    {runtime}
                  </div>
                )}
                {filteredStoreModels.map((model) => {
                  const isExpanded = expandedModelId === model.id;
                  const hasDownloaded = model.variants.some((v) => isDownloaded(v.files));
                  const icon = model.icon || guessIconFromFileName(model.name);

                  return (
                    <div
                      key={model.id}
                      className={`p-[11px] rounded transition-colors border-2 ${
                        isExpanded
                          ? 'border-cyber-accent/70 bg-cyber-accent/10'
                          : 'border-transparent bg-cyber-surface/60 hover:bg-cyber-elevated/70'
                      }`}
                      onClick={() => setExpandedModelId(isExpanded ? null : model.id)}
                    >
                      {/* Card Header */}
                      <div className="flex items-center gap-3">
                        {icon ? (
                          <img
                            src={`./icons/models/${icon}.svg`}
                            alt={model.name}
                            className="w-8 h-8 flex-shrink-0"
                            onError={(e) => {
                              const target = e.target as HTMLImageElement;
                              target.style.display = 'none';
                            }}
                          />
                        ) : (
                          <HardDrive className="w-8 h-8 flex-shrink-0 text-cyber-text-secondary" />
                        )}
                        <div className="flex-1 min-w-0 flex flex-col justify-center">
                          <div className="flex items-center justify-between gap-2">
                            <div className="text-sm font-bold truncate leading-none text-cyber-text flex-1 min-w-0">
                              {model.name}
                            </div>
                            {hasDownloaded && (
                              <span className="text-[10px] text-cyber-text-secondary flex-shrink-0">
                                {t('store.ready')}
                              </span>
                            )}
                          </div>
                          <div className="text-[10px] text-cyber-text-secondary truncate leading-tight mt-1 opacity-70 flex gap-1">
                            {(model.runtimes || ['llama-server']).map((r) => (
                              <span key={r} className="px-1 rounded bg-cyber-surface/50">
                                {r === 'llama-server' ? 'llama.cpp' : r}
                              </span>
                            ))}
                          </div>
                        </div>
                      </div>

                      {/* Expanded: Variants with download controls */}
                      {isExpanded && (
                        <div className="mt-3 pt-3 border-t border-cyber-border/30 space-y-1">
                          {model.variants.map((variant) => {
                            const variantDownloaded = isDownloaded(variant.files);
                            // Progress map is keyed by the first shard's basename — backend
                            // emits that as DownloadProgressEvent.fileName.
                            const primaryKey = basename(variant.files[0] ?? '');
                            const dlItem = downloads.get(primaryKey);
                            const isActiveDownload =
                              dlItem?.status === 'downloading' || dlItem?.status === 'speed_test';
                            const isPaused = dlItem?.status === 'paused';
                            return (
                              <div
                                key={variant.quantization}
                                className={`p-2 rounded transition-colors ${variantDownloaded ? 'bg-cyber-elevated/60' : 'hover:bg-cyber-elevated'}`}
                              >
                                <div className="flex items-center gap-3">
                                  {/* Quantization — strip the unsloth-Dynamic "UD-" prefix
                                      cosmetically so labels stay short enough to fit the fixed
                                      w-14 column without wrapping (e.g. "UD-IQ2_M" → "IQ2_M").
                                      The catalog still stores the full unsloth name. */}
                                  <span className="text-xs font-mono font-bold text-cyber-text w-14 flex-shrink-0 whitespace-nowrap">
                                    {variant.quantization.replace(/^UD-/, '')}
                                  </span>
                                  {/* File size */}
                                  <span className="text-[10px] text-cyber-text-secondary flex-1 min-w-0 truncate">
                                    {formatSize(variant.fileSize)}
                                  </span>

                                  {/* Action: fixed width to prevent layout shift */}
                                  <div className="flex-shrink-0 w-8 flex items-center justify-center">
                                    {variantDownloaded ? (
                                      <span className="text-cyber-text-muted text-sm">✓</span>
                                    ) : isActiveDownload || isPaused ? (
                                      <span
                                        className={`text-[10px] font-mono ${isPaused ? 'text-yellow-400' : 'text-cyber-accent'}`}
                                      >
                                        {dlItem?.progress ?? 0}%
                                      </span>
                                    ) : (
                                      <button
                                        disabled={isDownloading}
                                        onClick={(e) => {
                                          e.stopPropagation();
                                          startDownload(model.huggingfaceRepo, variant.files);
                                        }}
                                        className={`${dlItem?.status === 'error' ? 'text-red-400 hover:text-red-300' : 'text-cyber-text-secondary hover:text-cyber-text'} transition-colors`}
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
                {filteredStoreModels.length === 0 && !isLoadingStore && (
                  <div className="text-center py-8 text-cyber-text-secondary text-xs font-mono">
                    No models for {runtime} in store
                  </div>
                )}
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
  const { serverRunning, serverPort } = useLocalServer();
  const [copied, setCopied] = useState('');

  if (!serverRunning) return null;

  const handleCopy = (label: string, value: string) => {
    navigator.clipboard.writeText(value);
    setCopied(label);
    setTimeout(() => setCopied(''), 2000);
  };

  return (
    <div className="flex-shrink-0 border-t border-cyber-border/30">
      <div className="flex items-center gap-4 py-2 text-xs font-mono">
        <div
          className="flex items-center gap-1.5 hover:text-cyber-text transition-colors"
          onClick={() => handleCopy('openai', `http://127.0.0.1:${serverPort}/v1`)}
        >
          <span className="text-cyber-text-secondary/80">OpenAI:</span>
          <code className="text-cyber-text/80">127.0.0.1:{serverPort}/v1</code>
          <span className="text-cyber-text ml-0.5">
            {copied === 'openai' ? t('btn.copied') : t('btn.copy')}
          </span>
        </div>
        <span className="text-cyber-border">|</span>
        <div
          className="flex items-center gap-1.5 hover:text-cyber-text transition-colors"
          onClick={() => handleCopy('anthropic', `http://127.0.0.1:${serverPort}/anthropic`)}
        >
          <span className="text-cyber-text-secondary/80">Anthropic:</span>
          <code className="text-cyber-text/80">127.0.0.1:{serverPort}/anthropic</code>
          <span className="text-cyber-text ml-0.5">
            {copied === 'anthropic' ? t('btn.copied') : t('btn.copy')}
          </span>
        </div>
      </div>
    </div>
  );
};
