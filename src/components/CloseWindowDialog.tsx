// CloseWindowDialog — First-time close behavior selection dialog
import React, { useState, useEffect, useCallback } from 'react';
import { useI18n } from '../hooks/useI18n';

interface CloseWindowDialogProps {
  isOpen: boolean;
  onClose: (choice: 'direct' | 'tray' | null) => void;
}

export const CloseWindowDialog: React.FC<CloseWindowDialogProps> = ({ isOpen, onClose }) => {
  const { t } = useI18n();
  const [isAnimatingOut, setIsAnimatingOut] = useState(false);

  // Close with animation
  const handleClose = useCallback(
    (choice: 'direct' | 'tray' | null) => {
      setIsAnimatingOut(true);
      setTimeout(() => {
        setIsAnimatingOut(false);
        onClose(choice);
      }, 200);
    },
    [onClose]
  );

  // ESC to cancel
  useEffect(() => {
    if (!isOpen) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') handleClose(null);
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [isOpen, handleClose]);

  if (!isOpen) return null;

  return (
    <div
      className={`fixed inset-0 z-[9998] flex items-center justify-center transition-all duration-200 ${
        isAnimatingOut ? 'opacity-0' : 'opacity-100'
      }`}
    >
      {/* Backdrop */}
      <div
        className="absolute inset-0 bg-black/60 backdrop-blur-sm"
        onClick={() => handleClose(null)}
      />

      {/* Dialog box */}
      <div
        className={`relative w-[360px] max-w-[90vw] border border-cyber-border/40 bg-cyber-surface shadow-2xl rounded-xl overflow-hidden transition-all duration-200 ${
          isAnimatingOut ? 'scale-95 opacity-0' : 'scale-100 opacity-100'
        }`}
        onClick={(e) => e.stopPropagation()}
      >
        {/* Top accent line */}
        <div className="h-[2px] w-full bg-cyber-border" />

        {/* Header */}
        <div className="px-5 pt-4 pb-2">
          <span className="text-sm font-mono font-bold tracking-wider text-cyber-text">
            {t('closeDialog.title')}
          </span>
        </div>

        {/* Message */}
        <div className="px-5 pb-5">
          <p className="text-xs text-cyber-text-secondary leading-relaxed font-mono">
            {t('closeDialog.hint')}
          </p>
        </div>

        {/* Actions */}
        <div className="flex border-t border-cyber-border">
          <button
            onClick={() => handleClose('direct')}
            className="flex-1 px-4 py-2.5 text-xs font-mono font-bold tracking-wider text-cyber-text-secondary hover:text-cyber-text hover:bg-cyber-elevated transition-all border-r border-cyber-border"
          >
            {t('settings.closeDirectly')}
          </button>
          <button
            onClick={() => handleClose('tray')}
            className="flex-1 px-4 py-2.5 text-xs font-mono font-bold tracking-wider text-cyber-text hover:bg-cyber-text/10 transition-all"
          >
            {t('settings.closeToTray')}
          </button>
        </div>
      </div>
    </div>
  );
};
