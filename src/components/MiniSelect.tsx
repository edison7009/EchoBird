import React, { useState, useRef, useEffect, useLayoutEffect } from 'react';
import { Check, ChevronDown } from 'lucide-react';

interface MiniSelectProps {
  value: string;
  onChange: (value: string) => void;
  options: Array<{ id: string; label: string }>;
  className?: string;
  disabled?: boolean;
  dropUp?: boolean;
}

/** Compact custom select menu for small spaces */
export const MiniSelect: React.FC<MiniSelectProps> = ({
  value,
  onChange,
  options,
  className = '',
  disabled = false,
  dropUp = false,
}) => {
  const [isOpen, setIsOpen] = useState(false);
  const [menuMaxHeight, setMenuMaxHeight] = useState(360);
  const containerRef = useRef<HTMLDivElement>(null);
  const menuOpen = isOpen && !disabled;

  useLayoutEffect(() => {
    if (!menuOpen || !dropUp) return;
    const resizeMenu = () => {
      const container = containerRef.current;
      if (!container) return;
      const anchor = container.getBoundingClientRect();
      const boundary = container
        .closest('.overflow-y-auto, .overflow-hidden')
        ?.getBoundingClientRect();
      const top = Math.max(0, boundary?.top ?? 0) + 8;
      setMenuMaxHeight(Math.max(0, Math.min(360, anchor.top - top - 1)));
    };
    resizeMenu();
    window.addEventListener('resize', resizeMenu);
    window.addEventListener('scroll', resizeMenu, true);
    return () => {
      window.removeEventListener('resize', resizeMenu);
      window.removeEventListener('scroll', resizeMenu, true);
    };
  }, [menuOpen, dropUp]);

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

  const selectedOption = options.find((opt) => opt.id === value);

  return (
    <div ref={containerRef} className={`relative cursor-default ${className}`}>
      <button
        type="button"
        onClick={() => !disabled && setIsOpen(!isOpen)}
        disabled={disabled}
        aria-expanded={menuOpen}
        className={`w-full min-w-[90px] bg-cyber-surface/60 border border-cyber-border px-3 py-2 outline-none flex items-center justify-center transition-colors text-[14px] rounded-button ${
          disabled ? 'opacity-40' : 'hover:border-cyber-border/50'
        }`}
      >
        <span className="truncate text-cyber-text">{selectedOption?.label || '...'}</span>
        <ChevronDown
          size={14}
          className={`flex-shrink-0 ml-1.5 text-cyber-text transition-transform ${menuOpen ? 'rotate-180' : ''}`}
        />
      </button>

      {menuOpen && (
        <div
          style={dropUp ? { maxHeight: menuMaxHeight, overscrollBehavior: 'contain' } : undefined}
          className={`app-tooltip selection-popover model-selection-menu absolute ${dropUp ? 'bottom-full mb-px' : 'top-full mt-px'} inset-x-0 z-50 max-h-80 space-y-1 overflow-y-auto p-1.5`}
        >
          {options.map((option) => (
            <div
              key={option.id}
              data-selected={option.id === value || undefined}
              onClick={() => {
                onChange(option.id);
                setIsOpen(false);
              }}
              className="selection-menu-row flex items-center gap-2 px-2 py-2 text-xs transition-colors"
            >
              <span className="min-w-0 flex-1 break-words">{option.label}</span>
              {option.id === value && <Check size={12} className="shrink-0" aria-hidden="true" />}
            </div>
          ))}
        </div>
      )}
    </div>
  );
};
