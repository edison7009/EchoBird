import React, { useState, useRef, useEffect } from 'react';
import { ChevronDown } from 'lucide-react';

interface MiniSelectProps {
    value: string;
    onChange: (value: string) => void;
    options: Array<{ id: string; label: string }>;
    className?: string;
    disabled?: boolean;
    dropUp?: boolean;
    accent?: 'green' | 'blue';
}

/** Compact custom select menu for small spaces */
export const MiniSelect: React.FC<MiniSelectProps> = ({ value, onChange, options, className = '', disabled = false, dropUp = false, accent = 'green' }) => {
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

    const selectedOption = options.find(opt => opt.id === value);

    // Accent color classes
    const accentBorderHover = accent === 'blue' ? 'hover:border-cyber-accent-secondary/50' : 'hover:border-cyber-accent/50';
    const accentBorderOpen = accent === 'blue' ? 'border-cyber-accent-secondary' : 'border-cyber-accent';
    const accentChevron = accent === 'blue' ? 'text-cyber-accent-secondary' : 'text-cyber-accent';
    const accentDropdownBorder = accent === 'blue' ? 'border-cyber-accent-secondary/60' : 'border-cyber-accent/60';
    const accentItemActive = accent === 'blue' ? 'bg-cyber-accent-secondary/15 text-cyber-accent-secondary' : 'bg-cyber-accent/15 text-cyber-accent';
    const accentItemHover = accent === 'blue' ? 'hover:bg-cyber-accent-secondary/10 hover:text-cyber-accent-secondary' : 'hover:bg-cyber-accent/10 hover:text-cyber-accent';

    return (
        <div ref={containerRef} className={`relative ${className}`}>
            <button
                type="button"
                onClick={() => !disabled && setIsOpen(!isOpen)}
                className={`w-full min-w-[90px] bg-black border border-cyber-border px-3 py-1.5 outline-none cursor-pointer flex items-center justify-center transition-colors text-xs font-mono rounded-button ${disabled
                    ? 'opacity-40 cursor-not-allowed'
                    : accentBorderHover
                    } ${isOpen ? accentBorderOpen : ''}`}
            >
                <span className="truncate text-cyber-text">{selectedOption?.label || '...'}</span>
                <ChevronDown
                    size={12}
                    className={`flex-shrink-0 ml-1 ${accentChevron} transition-transform ${isOpen ? 'rotate-180' : ''}`}
                />
            </button>

            {isOpen && (
                <div className={`absolute ${dropUp ? 'bottom-full mb-px' : 'top-full mt-px'} left-0 right-0 bg-black border ${accentDropdownBorder} max-h-52 overflow-y-auto z-50 rounded-button`}>
                    {options.map((option) => (
                        <div
                            key={option.id}
                            onClick={() => {
                                onChange(option.id);
                                setIsOpen(false);
                            }}
                            className={`px-2 py-1.5 cursor-pointer transition-colors text-xs font-mono truncate text-center ${option.id === value
                                ? accentItemActive
                                : `text-cyber-text ${accentItemHover}`
                                }`}
                        >
                            {option.label}
                        </div>
                    ))}
                </div>
            )}
        </div>
    );
};
