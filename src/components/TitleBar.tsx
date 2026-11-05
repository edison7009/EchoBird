// Custom frameless window title bar
import React, { useState, useEffect } from 'react';
import { Minus, Maximize2, Minimize2, X, PanelLeft, ArrowLeft, ArrowRight } from 'lucide-react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { getSettings, hideMainWindow, saveSettings } from '../api/tauri';
import { CloseWindowDialog } from './CloseWindowDialog';
import { useI18n } from '../hooks/useI18n';
import { IS_MACOS } from '../utils/platform';
import { useSidebarStore } from '../stores/sidebarStore';
import { useNavigationStore } from '../stores/navigationStore';

const navigationButtonClass =
  'h-full w-8 flex items-center justify-center rounded text-cyber-text-secondary enabled:hover:bg-cyber-elevated/50 enabled:hover:text-cyber-text disabled:text-cyber-text-muted disabled:opacity-40 transition-colors';

interface TitleBarProps {
  onFeedbackClick?: () => void;
  onChangelogClick?: () => void;
}

export const TitleBar: React.FC<TitleBarProps> = ({ onFeedbackClick, onChangelogClick }) => {
  const { t } = useI18n();
  const collapsed = useSidebarStore((state) => state.collapsed);
  const setCollapsed = useSidebarStore((state) => state.setCollapsed);
  const canGoBack = useNavigationStore((state) => state.backHistory.length > 0);
  const canGoForward = useNavigationStore((state) => state.forwardHistory.length > 0);
  const goBack = useNavigationStore((state) => state.goBack);
  const goForward = useNavigationStore((state) => state.goForward);
  const handleMinimize = () => getCurrentWindow().minimize();
  // One-click hide to tray. Reuses the same native hide command as
  // the X-button's "tray" choice in handleClose, so tray-restore is identical.
  const handleMinimizeToTray = () => hideMainWindow();
  const [isMaximized, setIsMaximized] = useState(false);
  const [showCloseDialog, setShowCloseDialog] = useState(false);

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
      await win.unmaximize();
    } else {
      await win.maximize();
    }
  };

  const handleClose = async () => {
    const win = getCurrentWindow();

    // Check user settings for close behavior
    const settings = await getSettings();

    // If user hasn't set their preference yet, or chose "always ask", show dialog
    if (!settings.closeWindowBehaviorSet || settings.closeToTray === null) {
      setShowCloseDialog(true);
      return;
    }

    // User has set a specific preference
    const closeToTray = settings.closeToTray ?? false;

    if (closeToTray) {
      // Hide window to tray instead of closing
      await hideMainWindow();
    } else {
      // Close the window normally
      win.destroy();
    }
  };

  const handleCloseDialogChoice = async (choice: 'direct' | 'tray' | null) => {
    setShowCloseDialog(false);

    if (choice === null) {
      // User cancelled
      return;
    }

    const win = getCurrentWindow();
    const settings = await getSettings();

    // Only persist this choice as the permanent preference during first-time
    // onboarding (the user has never made an explicit close-behavior choice).
    // When the user has explicitly selected "always ask" (closeToTray === null
    // with closeWindowBehaviorSet === true), this dialog is a one-time prompt and
    // must NOT overwrite their setting — otherwise picking "exit" or "tray" here
    // silently cancels "always ask".
    if (!settings.closeWindowBehaviorSet) {
      await saveSettings({
        ...settings,
        closeToTray: choice === 'tray',
        closeWindowBehaviorSet: true,
      });
    }

    // Execute the chosen action (one-time when in "always ask" mode)
    if (choice === 'tray') {
      await hideMainWindow();
    } else {
      win.destroy();
    }
  };

  return (
    <>
      <div
        className="app-titlebar h-10 flex items-center justify-between select-none flex-shrink-0 cursor-default"
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
        <div
          className={`${IS_MACOS ? 'ml-20' : 'ml-2'} flex h-full items-center gap-0.5`}
          style={{ WebkitAppRegion: 'no-drag' } as React.CSSProperties}
        >
          <button
            type="button"
            aria-label={t('nav.back')}
            disabled={!canGoBack}
            onClick={goBack}
            className={navigationButtonClass}
          >
            <ArrowLeft size={16} strokeWidth={1.75} />
          </button>
          <button
            type="button"
            aria-label={t('nav.forward')}
            disabled={!canGoForward}
            onClick={goForward}
            className={navigationButtonClass}
          >
            <ArrowRight size={16} strokeWidth={1.75} />
          </button>
          <button
            type="button"
            aria-label={t(collapsed ? 'nav.expandSidebar' : 'nav.collapseSidebar')}
            aria-controls="app-sidebar"
            aria-expanded={!collapsed}
            onClick={() => setCollapsed(!collapsed)}
            className={navigationButtonClass}
          >
            <PanelLeft size={16} strokeWidth={1.75} />
          </button>
        </div>
        {/* Window controls */}
        <div
          className="flex items-center h-full"
          style={{ WebkitAppRegion: 'no-drag' } as React.CSSProperties}
        >
          <button
            onClick={onChangelogClick}
            className="h-full px-3 flex items-center justify-center whitespace-nowrap text-[14px] leading-none font-sans text-cyber-text-secondary hover:bg-cyber-text/20 hover:text-cyber-text transition-colors"
          >
            {t('nav.changelog')}
          </button>
          <button
            onClick={onFeedbackClick}
            className="h-full px-3 flex items-center justify-center whitespace-nowrap text-[14px] leading-none font-sans text-cyber-text-secondary hover:bg-cyber-text/20 hover:text-cyber-text transition-colors"
          >
            {t('nav.feedback')}
          </button>
          <button
            onClick={handleMinimizeToTray}
            aria-label={t('settings.closeToTray')}
            className="h-full px-4 flex items-center justify-center hover:bg-cyber-text/20 transition-colors"
          >
            <img
              src="/brand/bird.png"
              alt=""
              draggable={false}
              className="h-[18px] w-[18px] select-none"
            />
          </button>
          {/* Window controls — Windows/Linux only. macOS uses native traffic
              lights on the left (titleBarStyle: Overlay), so we omit our custom
              minimize/maximize/close here and keep the remaining header actions. */}
          {!IS_MACOS && (
            <>
              <button
                onClick={handleMinimize}
                className="h-full px-4 flex items-center justify-center text-cyber-text-secondary hover:bg-cyber-text/20 hover:text-cyber-text transition-colors"
              >
                <Minus size={16} strokeWidth={1.75} />
              </button>
              <button
                onClick={handleMaximize}
                className="h-full px-4 flex items-center justify-center text-cyber-text-secondary hover:bg-cyber-text/20 hover:text-cyber-text transition-colors"
              >
                {isMaximized ? (
                  <Minimize2 size={15} strokeWidth={1.75} />
                ) : (
                  <Maximize2 size={15} strokeWidth={1.75} />
                )}
              </button>
              <button
                onClick={handleClose}
                className="h-full px-4 flex items-center justify-center text-cyber-text-secondary hover:bg-red-500/20 hover:text-red-400 transition-colors"
              >
                <X size={16} strokeWidth={1.75} />
              </button>
            </>
          )}
        </div>
      </div>

      {/* Close Window Dialog */}
      <CloseWindowDialog isOpen={showCloseDialog} onClose={handleCloseDialogChoice} />
    </>
  );
};
