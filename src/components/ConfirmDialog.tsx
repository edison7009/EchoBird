// ConfirmDialog — Global confirm dialog with cyber theme
// Usage: const confirm = useConfirm(); const ok = await confirm({ title, message });

import React, { useState, useCallback, createContext, useContext, useEffect, useRef } from 'react';
import { AlertTriangle } from 'lucide-react';
import { useI18n } from '../hooks/useI18n';

// Dialog options
export interface ConfirmOptions {
    title?: string;
    message?: string;
    confirmText?: string;
    cancelText?: string;
    type?: 'danger' | 'warning' | 'normal' | 'info';  // danger=red, warning=yellow, normal=green, info=blue (Mother Agent)
}

// Context type — single function returning Promise<boolean>
interface ConfirmContextType {
    confirm: (options: ConfirmOptions) => Promise<boolean>;
}

const ConfirmContext = createContext<ConfirmContextType | undefined>(undefined);

export const useConfirm = () => {
    const context = useContext(ConfirmContext);
    if (!context) {
        throw new Error('useConfirm must be used within a ConfirmDialogProvider');
    }
    return context.confirm;
};

// Provider + Dialog UI
export const ConfirmDialogProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
    const { t } = useI18n();
    const [isOpen, setIsOpen] = useState(false);
    const [isAnimatingOut, setIsAnimatingOut] = useState(false);
    const [options, setOptions] = useState<ConfirmOptions>({});
    const resolveRef = useRef<((value: boolean) => void) | null>(null);

    const confirm = useCallback((opts: ConfirmOptions): Promise<boolean> => {
        setOptions(opts);
        setIsOpen(true);
        setIsAnimatingOut(false);
        return new Promise<boolean>((resolve) => {
            resolveRef.current = resolve;
        });
    }, []);

    // Animate out then close
    const closeWith = useCallback((result: boolean) => {
        setIsAnimatingOut(true);
        setTimeout(() => {
            setIsOpen(false);
            setIsAnimatingOut(false);
            resolveRef.current?.(result);
            resolveRef.current = null;
        }, 200);
    }, []);

    // ESC key support
    useEffect(() => {
        if (!isOpen) return;
        const handler = (e: KeyboardEvent) => {
            if (e.key === 'Escape') closeWith(false);
            if (e.key === 'Enter') closeWith(true);
        };
        window.addEventListener('keydown', handler);
        return () => window.removeEventListener('keydown', handler);
    }, [isOpen, closeWith]);

    const isDanger = options.type === 'danger';
    const isWarning = options.type === 'warning';
    const isInfo = options.type === 'info';

    const accentClass = isDanger
        ? 'border-red-500/40 shadow-[0_0_20px_rgba(255,60,60,0.1)]'
        : isWarning
            ? 'border-cyber-warning/40 shadow-[0_0_20px_rgba(255,204,0,0.12)]'
            : isInfo
                ? 'border-cyber-border-secondary/40 shadow-[0_0_20px_rgba(0,212,255,0.12)]'
                : 'border-cyber-border/40 shadow-2xl';
    const lineClass = isDanger ? 'bg-red-500/60' : isWarning ? 'bg-cyber-warning/60' : 'bg-cyber-border';
    const titleClass = isDanger ? 'text-red-400' : isWarning ? 'text-cyber-warning' : isInfo ? 'text-cyber-text-secondary' : 'text-cyber-text';
    const confirmClass = isDanger
        ? 'text-red-400 hover:bg-red-500/10 hover:text-red-300'
        : isWarning
            ? 'text-cyber-warning hover:bg-cyber-warning/10 hover:text-cyber-warning/80'
            : isInfo
                ? 'text-cyber-text-secondary hover:bg-cyber-accent-secondary/10'
                : 'text-cyber-text hover:bg-cyber-text/10';

    return (
        <ConfirmContext.Provider value={{ confirm }}>
            {children}
            {isOpen && (
                <div
                    className={`fixed inset-0 z-[9998] flex items-center justify-center transition-all duration-200 ${isAnimatingOut ? 'opacity-0' : 'opacity-100'
                        }`}
                >
                    {/* Backdrop */}
                    <div
                        className="absolute inset-0 bg-black/60 backdrop-blur-sm"
                        onClick={() => closeWith(false)}
                    />

                    {/* Dialog box */}
                    <div
                        className={`relative w-[360px] max-w-[90vw] border bg-cyber-surface shadow-2xl rounded-xl overflow-hidden transition-all duration-200 ${isAnimatingOut
                            ? 'scale-95 opacity-0'
                            : 'scale-100 opacity-100'
                            } ${accentClass}`}
                        onClick={e => e.stopPropagation()}
                    >
                        {/* Top accent line */}
                        <div className={`h-[2px] w-full ${lineClass}`} />

                        {/* Header */}
                        <div className="px-5 pt-4 pb-2 flex items-center gap-2">
                            {(isDanger || isWarning) && (
                                <AlertTriangle className={`w-4 h-4 flex-shrink-0 ${isDanger ? 'text-red-400' : 'text-cyber-warning'}`} />
                            )}
                            {isInfo && (
                                <svg className="w-4 h-4 flex-shrink-0 text-cyber-text-secondary" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><circle cx="12" cy="12" r="10" /><line x1="12" y1="8" x2="12" y2="12" /><line x1="12" y1="16" x2="12.01" y2="16" /></svg>
                            )}
                            <span className={`text-sm font-mono font-bold tracking-wider ${titleClass}`}>
                                {options.title || t('common.confirm')}
                            </span>
                        </div>

                        {/* Message */}
                        <div className="px-5 pb-5">
                            <p className="text-xs text-cyber-text-secondary leading-relaxed font-mono">
                                {options.message || t('common.areYouSure')}
                            </p>
                        </div>

                        {/* Actions */}
                        <div className="flex border-t border-cyber-border">
                            {options.cancelText !== '' && (
                                <button
                                    onClick={() => closeWith(false)}
                                    className="flex-1 px-4 py-2.5 text-xs font-mono font-bold tracking-wider text-cyber-text-secondary hover:text-cyber-text hover:bg-cyber-elevated transition-all border-r border-cyber-border"
                                >
                                    {options.cancelText || t('btn.cancel')}
                                </button>
                            )}
                            <button
                                onClick={() => closeWith(true)}
                                className={`flex-1 px-4 py-2.5 text-xs font-mono font-bold tracking-wider transition-all ${confirmClass}`}
                            >
                                {options.confirmText || t('common.confirm')}
                            </button>
                        </div>
                    </div>
                </div>
            )}
        </ConfirmContext.Provider>
    );
};
