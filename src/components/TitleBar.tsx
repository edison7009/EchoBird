// Custom frameless window title bar
import React, { useState, useEffect } from 'react';
import { Settings, Minus, Maximize2, Minimize2, X } from 'lucide-react';
import { useI18n } from '../hooks/useI18n';
import { getCurrentWindow, currentMonitor, LogicalSize, LogicalPosition } from '@tauri-apps/api/window';
import * as api from '../api/tauri';

interface TitleBarProps {
    onSettingsClick?: () => void;
}

export const TitleBar: React.FC<TitleBarProps> = ({ onSettingsClick }) => {
    const { t } = useI18n();
    const handleMinimize = () => getCurrentWindow().minimize();
    const [isMaximized, setIsMaximized] = useState(false);
    const [isResized, setIsResized] = useState(false);

    // Default window size (must match tauri.conf.json)
    const DEFAULT_W = 1080;
    const DEFAULT_H = 720;

    useEffect(() => {
        const win = getCurrentWindow();
        // Sync initial state and listen for resize/maximize events
        const syncState = async () => {
            const maximized = await win.isMaximized().catch(() => false);
            setIsMaximized(maximized);
            if (!maximized) {
                const factor = await win.scaleFactor().catch(() => 1.0);
                const size = await win.innerSize().catch(() => null);
                if (size) {
                    const logicalW = Math.round(size.width / factor);
                    const logicalH = Math.round(size.height / factor);
                    setIsResized(Math.abs(logicalW - DEFAULT_W) > 10 || Math.abs(logicalH - DEFAULT_H) > 10);
                }
            } else {
                setIsResized(false);
            }
        };
        syncState();
        const unlisten = win.onResized(() => syncState());
        return () => { unlisten.then(fn => fn()).catch(() => {}); };
    }, []);

    const handleMaximize = async () => {
        const win = getCurrentWindow();
        if (isMaximized) {
            // Maximized → unmaximize (restore to previous size)
            await win.unmaximize();
        } else if (isResized) {
            // Manually resized → restore to default size & center
            await win.setSize(new LogicalSize(DEFAULT_W, DEFAULT_H));
            // Center on screen
            const monitor = await currentMonitor().catch(() => null);
            if (monitor) {
                const factor = monitor.scaleFactor;
                const sw = monitor.size.width / factor;
                const sh = monitor.size.height / factor;
                await win.setPosition(new LogicalPosition(
                    Math.round((sw - DEFAULT_W) / 2),
                    Math.round((sh - DEFAULT_H) / 2),
                ));
            }
        } else {
            // Default size → maximize
            await win.maximize();
        }
    };

    // Close confirmation dialog state
    const [showCloseDialog, setShowCloseDialog] = useState(false);
    const [rememberChoice, setRememberChoice] = useState(false);
    const [isAnimatingOut, setIsAnimatingOut] = useState(false);
    const [closeBehavior, setCloseBehavior] = useState<string | null>(null);

    // Load close behavior from Rust backend
    useEffect(() => {
        api.getSettings().then(s => {
            if (s.closeBehavior && s.closeBehavior !== 'ask') setCloseBehavior(s.closeBehavior);
        }).catch(() => { });
    }, []);

    const closeDialog = () => {
        setIsAnimatingOut(true);
        setTimeout(() => {
            setShowCloseDialog(false);
            setIsAnimatingOut(false);
            setRememberChoice(false);
        }, 200);
    };

    const handleClose = () => {
        if (closeBehavior === 'minimize') {
            getCurrentWindow().hide();
            return;
        }
        if (closeBehavior === 'quit') {
            getCurrentWindow().destroy();
            return;
        }
        setShowCloseDialog(true);
    };

    const handleMinimizeToTray = () => {
        if (rememberChoice) {
            setCloseBehavior('minimize');
            api.getSettings().then(s => api.saveSettings({ ...s, closeBehavior: 'minimize' })).catch(() => { });
        }
        closeDialog();
        getCurrentWindow().hide();
    };

    const handleQuit = () => {
        if (rememberChoice) {
            setCloseBehavior('quit');
            api.getSettings().then(s => api.saveSettings({ ...s, closeBehavior: 'quit' })).catch(() => { });
        }
        closeDialog();
        getCurrentWindow().destroy();
    };

    return (
        <>
            <div
                className="h-8 bg-cyber-bg flex items-center justify-end select-none flex-shrink-0 cursor-default"
                onMouseDown={(e) => {
                    // Use startDragging for Linux (WebkitAppRegion doesn't work on Linux GTK)
                    // Also works cross-platform as a reliable fallback
                    if (e.button === 0 && !(e.target as HTMLElement).closest('button')) {
                        e.preventDefault();
                        getCurrentWindow().startDragging().catch(() => { });
                    }
                }}
            >
                {/* Window controls */}
                <div
                    className="flex items-center h-full"
                    style={{ WebkitAppRegion: 'no-drag' } as React.CSSProperties}
                >
                    <button
                        onClick={onSettingsClick}
                        className="h-full px-4 flex items-center justify-center text-cyber-text-secondary hover:bg-cyber-accent/20 hover:text-cyber-accent transition-colors"
                    >
                        <Settings size={13} />
                    </button>
                    <button
                        onClick={handleMinimize}
                        className="h-full px-4 flex items-center justify-center text-cyber-text-secondary hover:bg-cyber-accent/20 hover:text-cyber-accent transition-colors"
                    >
                        <Minus size={14} />
                    </button>
                    <button
                        onClick={handleMaximize}
                        className="h-full px-4 flex items-center justify-center text-cyber-text-secondary hover:bg-cyber-accent/20 hover:text-cyber-accent transition-colors"
                    >
                        {(isMaximized || isResized) ? <Minimize2 size={13} /> : <Maximize2 size={13} />}
                    </button>
                    <button
                        onClick={handleClose}
                        className="h-full px-4 flex items-center justify-center text-cyber-text-secondary hover:bg-red-500/20 hover:text-red-400 transition-colors"
                    >
                        <X size={14} />
                    </button>
                </div>
            </div>

            {/* Close behavior confirmation dialog */}
            {showCloseDialog && (
                <div
                    className={`fixed inset-0 z-[9999] flex items-center justify-center transition-all duration-200 ${isAnimatingOut ? 'opacity-0' : 'opacity-100'}`}
                >
                    {/* Backdrop */}
                    <div
                        className="absolute inset-0 bg-black/60 backdrop-blur-sm"
                        onClick={closeDialog}
                    />

                    {/* Dialog */}
                    <div
                        className={`relative w-[380px] max-w-[90vw] border border-cyber-accent/40 bg-cyber-bg shadow-lg rounded-xl overflow-hidden transition-all duration-200 shadow-[0_0_20px_rgba(0,255,157,0.1)] ${isAnimatingOut ? 'scale-95 opacity-0' : 'scale-100 opacity-100'}`}
                        onClick={e => e.stopPropagation()}
                    >
                        {/* Top accent line */}
                        <div className="h-[2px] w-full bg-cyber-accent/60" />

                        {/* Title */}
                        <div className="px-5 pt-4 pb-2">
                            <span className="text-sm font-mono font-bold tracking-wider text-cyber-accent">
                                {t('close.title')}
                            </span>
                        </div>

                        {/* Message */}
                        <div className="px-5 pb-3">
                            <p className="text-xs text-cyber-text-secondary leading-relaxed font-mono">
                                {t('close.message')}
                            </p>
                        </div>

                        {/* Remember choice */}
                        <div className="px-5 pb-4">
                            <label className="flex items-center gap-2 cursor-pointer group">
                                <div
                                    className={`w-3.5 h-3.5 rounded border transition-all flex items-center justify-center ${rememberChoice
                                        ? 'bg-cyber-accent/20 border-cyber-accent'
                                        : 'border-cyber-border hover:border-cyber-accent/50'
                                        }`}
                                    onClick={() => setRememberChoice(!rememberChoice)}
                                >
                                    {rememberChoice && <span className="text-[9px] text-cyber-accent">✓</span>}
                                </div>
                                <span
                                    className="text-[11px] text-cyber-text-secondary font-mono group-hover:text-cyber-text transition-colors"
                                    onClick={() => setRememberChoice(!rememberChoice)}
                                >
                                    {t('close.remember')}
                                </span>
                            </label>
                        </div>

                        {/* Action buttons */}
                        <div className="flex border-t border-cyber-border">
                            <button
                                onClick={handleMinimizeToTray}
                                className="flex-1 px-4 py-2.5 text-xs font-mono font-bold tracking-wider text-cyber-accent hover:bg-cyber-accent/10 transition-all border-r border-cyber-border"
                            >
                                {t('close.minimize')}
                            </button>
                            <button
                                onClick={handleQuit}
                                className="flex-1 px-4 py-2.5 text-xs font-mono font-bold tracking-wider text-red-400 hover:bg-red-500/10 hover:text-red-300 transition-all"
                            >
                                {t('close.quit')}
                            </button>
                        </div>
                    </div>
                </div>
            )}
        </>
    );
};
