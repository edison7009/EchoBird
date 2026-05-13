// Custom frameless window title bar
import React, { useState, useEffect } from 'react';
import { Settings, Minus, Maximize2, Minimize2, X } from 'lucide-react';
import { getCurrentWindow, LogicalSize } from '@tauri-apps/api/window';

interface TitleBarProps {
  onSettingsClick?: () => void;
}

export const TitleBar: React.FC<TitleBarProps> = ({ onSettingsClick }) => {
  const handleMinimize = () => getCurrentWindow().minimize();
  const [isMaximized, setIsMaximized] = useState(false);

  useEffect(() => {
    // Sync initial state and listen for resize/maximize events
    getCurrentWindow()
      .isMaximized()
      .then(setIsMaximized)
      .catch(() => {});
    const unlisten = getCurrentWindow().onResized(() => {
      getCurrentWindow()
        .isMaximized()
        .then(setIsMaximized)
        .catch(() => {});
    });
    return () => {
      unlisten.then((fn) => fn()).catch(() => {});
    };
  }, []);

  const handleMaximize = async () => {
    const win = getCurrentWindow();
    if (isMaximized) {
      // Always restore to default size (1400×900) + center
      await win.unmaximize();
      await win.setSize(new LogicalSize(1400, 900));
      await win.center();
    } else {
      await win.maximize();
    }
  };

  const handleClose = () => {
    getCurrentWindow().destroy();
  };

  return (
    <div
      className="h-8 bg-cyber-bg flex items-center justify-end select-none flex-shrink-0 cursor-default"
      onMouseDown={(e) => {
        // Use startDragging for Linux (WebkitAppRegion doesn't work on Linux GTK)
        // Also works cross-platform as a reliable fallback
        if (e.button === 0 && !(e.target as HTMLElement).closest('button')) {
          e.preventDefault();
          getCurrentWindow()
            .startDragging()
            .catch(() => {});
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
          className="h-full px-4 flex items-center justify-center text-cyber-text-secondary hover:bg-cyber-text/20 hover:text-cyber-text transition-colors"
        >
          <Settings size={13} />
        </button>
        <button
          onClick={handleMinimize}
          className="h-full px-4 flex items-center justify-center text-cyber-text-secondary hover:bg-cyber-text/20 hover:text-cyber-text transition-colors"
        >
          <Minus size={14} />
        </button>
        <button
          onClick={handleMaximize}
          className="h-full px-4 flex items-center justify-center text-cyber-text-secondary hover:bg-cyber-text/20 hover:text-cyber-text transition-colors"
        >
          {isMaximized ? <Minimize2 size={13} /> : <Maximize2 size={13} />}
        </button>
        <button
          onClick={handleClose}
          className="h-full px-4 flex items-center justify-center text-cyber-text-secondary hover:bg-red-500/20 hover:text-red-400 transition-colors"
        >
          <X size={14} />
        </button>
      </div>
    </div>
  );
};
