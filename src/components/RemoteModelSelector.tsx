// RemoteModelSelector — Minimal model dropdown for Mother Agent
// Text + arrow, no background/border, hover shows soft bg, dropdown opens upward
import React, { useState, useRef, useEffect } from 'react';
import { ChevronDown, Loader2, Check } from 'lucide-react';

export interface ModelOption {
    id: string;
    name: string;
    icon?: string | null;  // icon path from getModelIcon()
}

interface RemoteModelSelectorProps {
    models: ModelOption[];
    currentModelId: string | null;
    loading: boolean;
    onSelect: (modelId: string) => void;
    placeholder?: string;
}

export const RemoteModelSelector: React.FC<RemoteModelSelectorProps> = ({
    models,
    currentModelId,
    loading,
    onSelect,
    placeholder = 'Select model',
}) => {
    const [isOpen, setIsOpen] = useState(false);
    const containerRef = useRef<HTMLDivElement>(null);

    // Close on click outside
    useEffect(() => {
        const handleClickOutside = (event: MouseEvent) => {
            if (containerRef.current && !containerRef.current.contains(event.target as Node)) {
                setIsOpen(false);
            }
        };
        document.addEventListener('mousedown', handleClickOutside);
        return () => document.removeEventListener('mousedown', handleClickOutside);
    }, []);

    // Close on Escape
    useEffect(() => {
        const handleEsc = (e: KeyboardEvent) => {
            if (e.key === 'Escape') setIsOpen(false);
        };
        if (isOpen) {
            document.addEventListener('keydown', handleEsc);
            return () => document.removeEventListener('keydown', handleEsc);
        }
    }, [isOpen]);

    const currentModel = models.find(m => m.id === currentModelId);
    const displayText = currentModel?.name || placeholder;
    const displayIcon = currentModel?.icon;

    // Tailwind JIT-friendly static class strings (Mother Agent secondary accent)
    const triggerClass = 'flex items-center gap-1.5 px-2 py-1 text-xs font-mono text-cyber-accent transition-colors rounded hover:bg-white/8 active:bg-white/12 disabled:cursor-default';
    const spinClass = 'animate-spin text-cyber-accent/70';
    const selectedItemClass = 'text-cyber-accent bg-cyber-accent/10';
    const unselectedItemClass = 'text-cyber-text hover:bg-white/8 hover:text-cyber-accent';
    const checkClass = 'flex-shrink-0 ml-1 text-cyber-accent';

    return (
        <div ref={containerRef} className="relative">
            {/* Trigger button — icon + text + arrow, no bg/border */}
            <button
                type="button"
                onClick={() => !loading && setIsOpen(!isOpen)}
                disabled={loading}
                className={triggerClass}
            >
                {loading ? (
                    <Loader2 size={12} className={spinClass} />
                ) : (
                    <>
                        {displayIcon && (
                            <img
                                src={displayIcon}
                                alt=""
                                className="w-3.5 h-3.5 flex-shrink-0"
                                onError={(e) => { (e.target as HTMLImageElement).style.display = 'none'; }}
                            />
                        )}
                        <span className="truncate max-w-[140px]">{displayText}</span>
                        <ChevronDown
                            size={11}
                            className={`flex-shrink-0 opacity-60 transition-transform ${isOpen ? 'rotate-180' : ''}`}
                        />
                    </>
                )}
            </button>

            {/* Dropdown — opens upward */}
            {isOpen && models.length > 0 && (
                <div className="absolute bottom-full mb-1 right-0 min-w-[200px] max-w-[300px] max-h-60 overflow-y-auto
                    bg-[#1a1a2e]/95 backdrop-blur-md rounded-lg shadow-xl
                    animate-in fade-in slide-in-from-bottom-2 duration-150
                    z-50"
                >
                    {models.map((model) => (
                        <div
                            key={model.id}
                            onClick={() => {
                                if (model.id !== currentModelId) {
                                    onSelect(model.id);
                                }
                                setIsOpen(false);
                            }}
                            className={`flex items-center gap-2 px-3 py-2 cursor-pointer text-xs font-mono transition-colors
                                ${model.id === currentModelId ? selectedItemClass : unselectedItemClass}`}
                        >
                            {model.icon && (
                                <img
                                    src={model.icon}
                                    alt=""
                                    className="w-4 h-4 flex-shrink-0"
                                    onError={(e) => { (e.target as HTMLImageElement).style.display = 'none'; }}
                                />
                            )}
                            <span className="truncate flex-1">{model.name}</span>
                            {model.id === currentModelId && (
                                <Check size={12} className={checkClass} />
                            )}
                        </div>
                    ))}
                    {models.length === 0 && (
                        <div className="px-3 py-2 text-xs text-cyber-text-secondary font-mono">
                            No models configured
                        </div>
                    )}
                </div>
            )}
        </div>
    );
};
