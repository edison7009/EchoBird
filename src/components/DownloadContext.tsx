// Global download state management Context
// Ported from Electron v1.1.0 → Tauri API
import React, { createContext, useContext, useState, useEffect, useCallback, useRef } from 'react';
import * as api from '../api/tauri';

// Download progress data structure
export interface DownloadItem {
    fileName: string;
    repo?: string; // Repository name, used for resuming after pause
    progress: number;
    downloaded: number;
    total: number;
    status: 'downloading' | 'completed' | 'error' | 'cancelled' | 'paused' | 'speed_test' | 'installing';
}

interface DownloadContextValue {
    // All download items
    downloads: Map<string, DownloadItem>;
    // Whether there is any active downloading item
    isDownloading: boolean;
    // The currently active download (the first 'downloading' status)
    activeDownload: DownloadItem | null;
    // Trigger download (also used for resuming after pause)
    startDownload: (repo: string, fileName: string) => void;
    // Pause download (saves progress, resumable)
    pauseDownload: () => void;
    // Cancel download (deletes temp file, supports cancelling after pause, accepts fileName for pause-based cancel)
    cancelDownload: (fileName?: string) => void;
}

const DownloadContext = createContext<DownloadContextValue | null>(null);

export const useDownload = () => {
    const ctx = useContext(DownloadContext);
    if (!ctx) throw new Error('useDownload must be used within DownloadProvider');
    return ctx;
};

export const DownloadProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
    const [downloads, setDownloads] = useState<Map<string, DownloadItem>>(new Map());
    // Auto-cleanup timer refs
    const cleanupTimers = useRef<Map<string, ReturnType<typeof setTimeout>>>(new Map());

    // Listen to Tauri download progress events
    useEffect(() => {
        let unlisten: (() => void) | null = null;
        api.onDownloadProgress((data) => {
            setDownloads(prev => {
                const next = new Map(prev);
                const existing = prev.get(data.fileName);
                next.set(data.fileName, {
                    fileName: data.fileName,
                    repo: existing?.repo, // Keep existing repo info
                    progress: data.progress,
                    downloaded: data.downloaded,
                    total: data.total,
                    status: data.status as DownloadItem['status'],
                });
                return next;
            });

            // Completed, error or cancelled items auto-cleanup after 5s (paused not cleaned, status preserved)
            if (data.status === 'completed' || data.status === 'error' || data.status === 'cancelled') {
                // Clear previous timer if any (prevent duplicates)
                const existing = cleanupTimers.current.get(data.fileName);
                if (existing) clearTimeout(existing);

                const timer = setTimeout(() => {
                    setDownloads(prev => {
                        const next = new Map(prev);
                        next.delete(data.fileName);
                        return next;
                    });
                    cleanupTimers.current.delete(data.fileName);
                }, 5000);
                cleanupTimers.current.set(data.fileName, timer);
            }
        }).then(fn => { unlisten = fn; });

        return () => {
            unlisten?.();
            // Cleanup all timers
            cleanupTimers.current.forEach(t => clearTimeout(t));
            cleanupTimers.current.clear();
        };
    }, []);

    // Trigger download (also used for resuming after pause)
    const startDownload = useCallback(async (repo: string, fileName: string) => {
        // Immediately show downloading state
        setDownloads(prev => {
            const next = new Map(prev);
            next.set(fileName, {
                fileName,
                repo, // Store repo for resume after pause
                progress: 0,
                downloaded: 0,
                total: 0,
                status: 'downloading',
            });
            return next;
        });
        try {
            await api.downloadModel(repo, fileName);
        } catch (e) {
            console.error('[DownloadContext] Download failed:', e);
        }
    }, []);

    // Pause download (keeps .downloading temp file, resumable)
    const pauseDownload = useCallback(async () => {
        try {
            await api.pauseDownload();
        } catch (e) {
            console.error('[DownloadContext] Pause failed:', e);
        }
    }, []);

    // Cancel download (deletes .downloading temp file, supports cancel after pause)
    const cancelDownload = useCallback(async (fileName?: string) => {
        try {
            await api.cancelDownload(fileName);
        } catch (e) {
            console.error('[DownloadContext] Cancel failed:', e);
        }
    }, []);

    // Derived values (memoized to prevent unnecessary re-renders)
    const isDownloading = React.useMemo(
        () => Array.from(downloads.values()).some(d => d.status === 'downloading'),
        [downloads]
    );
    const activeDownload = React.useMemo(
        () => Array.from(downloads.values()).find(d => d.status === 'downloading') || null,
        [downloads]
    );

    return (
        <DownloadContext.Provider value={{ downloads, isDownloading, activeDownload, startDownload, pauseDownload, cancelDownload }}>
            {children}
        </DownloadContext.Provider>
    );
};
