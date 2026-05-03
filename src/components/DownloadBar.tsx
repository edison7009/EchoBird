// Global download progress bar — bottom status bar
// Ported from Electron v1.1.0
import React from 'react';
import { Download, Package, X, Check, AlertTriangle, Pause, Play } from 'lucide-react';
import { useDownload, DownloadItem } from './DownloadContext';
import { useI18n } from '../hooks/useI18n';

// Map runtime/engine names to display labels
const ENGINE_LABELS: Record<string, string> = {
    'llama-server': 'llama.cpp',
    'vllm': 'vLLM',
    'sglang': 'SGLang',
};

function getDisplayName(fileName: string): string {
    return ENGINE_LABELS[fileName] ?? fileName;
}

// Format file size
function formatSize(bytes: number): string {
    if (bytes >= 1e9) return (bytes / 1e9).toFixed(1) + ' GB';
    if (bytes >= 1e6) return (bytes / 1e6).toFixed(0) + ' MB';
    if (bytes >= 1e3) return (bytes / 1e3).toFixed(0) + ' KB';
    return bytes + ' B';
}

// Shorten file name for display
function shortenFileName(name: string, maxLen = 32): string {
    if (name.length <= maxLen) return name;
    const ext = name.lastIndexOf('.');
    if (ext > 0) {
        const base = name.slice(0, ext);
        const extension = name.slice(ext);
        const keep = maxLen - extension.length - 3;
        return base.slice(0, keep) + '...' + extension;
    }
    return name.slice(0, maxLen - 3) + '...';
}

// Single download item row
const DownloadItemRow: React.FC<{
    item: DownloadItem;
    onPause?: () => void;
    onResume?: () => void;
    onCancel?: (fileName?: string) => void;
}> = ({ item, onPause, onResume, onCancel }) => {
    const { t } = useI18n();
    const isInstalling = item.status === 'installing';
    const isActive = item.status === 'downloading' || item.status === 'speed_test' || isInstalling;
    const isDone = item.status === 'completed';
    const isError = item.status === 'error';
    const isPaused = item.status === 'paused';
    // Engine install: no real file size, just show progress
    const hasSize = item.total > 0;

    return (
        <div className="flex items-center gap-3 h-full min-w-0">
            {/* Status icon */}
            {isInstalling && <Package className="w-3.5 h-3.5 text-cyan-400 flex-shrink-0 animate-pulse" />}
            {isActive && !isInstalling && <Download className="w-3.5 h-3.5 text-cyber-text flex-shrink-0 animate-pulse" />}
            {isPaused && <Pause className="w-3.5 h-3.5 text-yellow-400 flex-shrink-0" />}
            {isDone && <Check className="w-3.5 h-3.5 text-green-400 flex-shrink-0" />}
            {isError && <AlertTriangle className="w-3.5 h-3.5 text-red-400 flex-shrink-0" />}

            {/* Display name (resolved from runtime key if applicable) */}
            <span className={`text-[11px] font-mono truncate min-w-0 ${isDone ? 'text-green-400' : isError ? 'text-red-400' : isPaused ? 'text-yellow-400'
                    : isInstalling ? 'text-cyan-400' : 'text-cyber-text'
                }`}>
                {shortenFileName(getDisplayName(item.fileName))}
            </span>

            {/* Progress bar (shown when downloading / installing / paused) */}
            {(isActive || isPaused) && (
                <div className="flex items-center gap-2 flex-shrink-0">
                    {/* Progress bar track */}
                    <div className="w-24 h-1.5 bg-cyber-border/50 rounded-full overflow-hidden">
                        <div
                            className={`h-full rounded-full transition-all duration-300 ${isPaused ? 'bg-yellow-400' : isInstalling ? 'bg-cyan-400' : 'bg-cyber-accent'
                                }`}
                            style={{ width: `${item.progress}%` }}
                        />
                    </div>
                    {/* Percentage */}
                    <span className={`text-[10px] font-mono w-8 text-right ${isPaused ? 'text-yellow-400' : isInstalling ? 'text-cyan-400' : 'text-cyber-text'
                        }`}>
                        {item.progress}%
                    </span>
                    {/* File size: only shown for real file downloads, not engine installs */}
                    {hasSize && !isInstalling && (
                        <span className="text-[10px] font-mono text-cyber-text-secondary">
                            {formatSize(item.downloaded)}/{formatSize(item.total)}
                        </span>
                    )}
                    {/* Engine install: show installing label instead of sizes */}
                    {isInstalling && (
                        <span className="text-[10px] font-mono text-cyan-400/70">
                            Installing…
                        </span>
                    )}
                </div>
            )}

            {/* Complete status text */}
            {isDone && (
                <span className="text-[10px] font-mono text-green-400/70 flex-shrink-0">
                    {t('status.complete')}
                </span>
            )}

            {/* Paused status text */}
            {isPaused && (
                <span className="text-[10px] font-mono text-yellow-400/70 flex-shrink-0">
                    {t('download.pause')}
                </span>
            )}

            {/* Error status text */}
            {isError && (
                <span className="text-[10px] font-mono text-red-400/70 flex-shrink-0">
                    {t('status.failed')}
                </span>
            )}

            {/* Action buttons — not shown during engine install (pip cannot be paused) */}
            {!isInstalling && (
                <div className="flex items-center gap-1 flex-shrink-0 ml-auto">
                    {/* Downloading: Pause button */}
                    {isActive && onPause && (
                        <button
                            onClick={onPause}
                            className="text-cyber-text-secondary/50 hover:text-yellow-400 transition-colors"
                        >
                            <Pause className="w-3.5 h-3.5" />
                        </button>
                    )}
                    {/* Paused: Resume button */}
                    {isPaused && onResume && (
                        <button
                            onClick={onResume}
                            className="text-cyber-text-secondary/50 hover:text-green-400 transition-colors"
                        >
                            <Play className="w-3.5 h-3.5" />
                        </button>
                    )}
                    {/* Downloading or paused: Cancel button */}
                    {(isActive || isPaused) && onCancel && (
                        <button
                            onClick={() => onCancel(item.fileName)}
                            className="text-cyber-text-secondary/50 hover:text-red-400 transition-colors"
                        >
                            <X className="w-3.5 h-3.5" />
                        </button>
                    )}
                </div>
            )}
        </div>
    );
};

export const DownloadBar: React.FC = () => {
    const { downloads, startDownload, pauseDownload, cancelDownload } = useDownload();
    const { t } = useI18n();

    // Completely hidden when no downloads
    if (downloads.size === 0) return null;

    // Get all download items, sorted by status: installing > downloading > paused > error > completed
    const items = Array.from(downloads.values()).sort((a, b) => {
        const order: Record<string, number> = { installing: 0, downloading: 0, speed_test: 0, paused: 1, error: 2, completed: 3, cancelled: 4 };
        return (order[a.status] ?? 9) - (order[b.status] ?? 9);
    });

    // Show the first item (most important)
    const primary = items[0];
    // Count active (downloading or installing)
    const queueCount = items.filter(i =>
        i.status === 'downloading' || i.status === 'speed_test' || i.status === 'installing'
    ).length;

    // Resume download (use stored repo + fileName to re-call startDownload)
    const handleResume = () => {
        if (primary?.repo) {
            startDownload(primary.repo, primary.fileName);
        }
    };

    return (
        <div className="
            h-7 flex items-center px-4 
            bg-cyber-bg/80 backdrop-blur-sm
            border-t border-cyber-border/30
            transition-all duration-300
        ">
            {/* Primary download item */}
            <div className="flex-1 min-w-0">
                <DownloadItemRow
                    item={primary}
                    onPause={pauseDownload}
                    onResume={handleResume}
                    onCancel={cancelDownload}
                />
            </div>

            {/* Queue count (shown when multiple downloads) */}
            {queueCount > 1 && (
                <span className="text-[10px] font-mono text-cyber-text-secondary ml-3 flex-shrink-0">
                    +{queueCount - 1} {t('download.inQueue')}
                </span>
            )}
        </div>
    );
};
