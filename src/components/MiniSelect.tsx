import React, { useState, useRef, useEffect, useLayoutEffect } from 'react';
import { ChevronDown } from 'lucide-react';

interface MiniSelectProps {
  value: string;
  onChange: (value: string) => void;
  options: Array<{ id: string; label: string }>;
  className?: string;
  disabled?: boolean;
  dropUp?: boolean;
  opaqueMenu?: boolean;
  accent?: 'green' | 'blue';
}

/** Compact custom select menu for small spaces */
export const MiniSelect: React.FC<MiniSelectProps> = ({
  value,
  onChange,
  options,
  className = '',
  disabled = false,
  dropUp = false,
  opaqueMenu = false,
  accent = 'green',
}) => {
  const [isOpen, setIsOpen] = useState(false);
  const [menuMaxHeight, setMenuMaxHeight] = useState(360);
  const containerRef = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    if (!isOpen || !dropUp) return;
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
  }, [isOpen, dropUp]);

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

  // Accent color classes
  const accentBorderHover =
    accent === 'blue' ? 'hover:border-cyber-border-secondary/50' : 'hover:border-cyber-border/50';
  const accentBorderOpen =
    accent === 'blue' ? 'border-cyber-border-secondary' : 'border-cyber-border';
  const accentChevron = accent === 'blue' ? 'text-cyber-text-secondary' : 'text-cyber-text';
  const accentDropdownBorder =
    accent === 'blue' ? 'border-cyber-border-secondary/60' : 'border-cyber-border/60';
  const accentItemActive =
    accent === 'blue'
      ? 'bg-cyber-accent-secondary/15 text-cyber-text-secondary'
      : 'bg-cyber-text/15 text-cyber-text';
  const accentItemHover =
    accent === 'blue'
      ? 'hover:bg-cyber-accent-secondary/10 hover:text-cyber-text-secondary'
      : 'hover:bg-cyber-text/10 hover:text-cyber-text';

  return (
    <div ref={containerRef} className={`relative cursor-default ${className}`}>
      <button
        type="button"
        onClick={() => !disabled && setIsOpen(!isOpen)}
        className={`w-full min-w-[90px] bg-cyber-surface/60 border border-cyber-border px-3 py-2 outline-none flex items-center justify-center transition-colors text-[14px] rounded-button ${
          disabled ? 'opacity-40' : accentBorderHover
        } ${isOpen ? accentBorderOpen : ''}`}
      >
        <span className="truncate text-cyber-text">{selectedOption?.label || '...'}</span>
        <ChevronDown
          size={14}
          className={`flex-shrink-0 ml-1.5 ${accentChevron} transition-transform ${isOpen ? 'rotate-180' : ''}`}
        />
      </button>

      {isOpen && (
        <div
          style={dropUp ? { maxHeight: menuMaxHeight, overscrollBehavior: 'contain' } : undefined}
          className={`absolute ${dropUp ? 'bottom-full mb-px' : 'top-full mt-px'} ${dropUp || opaqueMenu ? 'bg-cyber-surface' : 'bg-cyber-surface/90'} left-0 right-0 border ${accentDropdownBorder} max-h-80 overflow-y-auto z-50 rounded-button shadow-lg`}
        >
          {options.map((option) => (
            <div
              key={option.id}
              onClick={() => {
                onChange(option.id);
                setIsOpen(false);
              }}
              className={`px-2 py-2 transition-colors text-[14px] truncate text-center ${
                option.id === value ? accentItemActive : `text-cyber-text ${accentItemHover}`
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
