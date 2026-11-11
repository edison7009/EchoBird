import React, { useState } from 'react';
import { Box as BoxIcon } from 'lucide-react';
import type { LocalTool } from '../api/types';
import { useI18n } from '../hooks/useI18n';

// Resolve the same localized catalog label in the app and website demo.
export const toolDisplayName = (tool: LocalTool, locale: string): string => {
  if (tool.displayName) return tool.displayName;
  if (tool.names && locale !== 'en') {
    return (
      tool.names[locale] ||
      tool.names[locale.split('-')[0]] ||
      Object.entries(tool.names).find(([k]) => k.startsWith(locale.split('-')[0]))?.[1] ||
      tool.name
    );
  }
  return tool.name;
};

export interface DesktopIconProps {
  tool: LocalTool;
  selected: boolean;
  onClick: () => void;
  /** dnd-kit drag attributes/listeners (sortable tiles only). Applied to the
      button itself so the tile stays a single focusable control instead of
      nesting a button inside a role="button" wrapper. */
  dragProps?: React.HTMLAttributes<HTMLElement>;
}

// A desktop-style launcher tile: icon on top, name beneath. Clicking selects;
// the bottom bar holds the launch / install action. All icons render
// uniformly — which section an app sits in (已安装 / 未安装) tells the state.
export const DesktopIcon: React.FC<DesktopIconProps> = ({ tool, selected, onClick, dragProps }) => {
  const { locale } = useI18n();
  const iconSrc = tool.icon;
  const [failedSrc, setFailedSrc] = useState<string | null>(null);
  const displayName = toolDisplayName(tool, locale);

  return (
    <button
      {...dragProps}
      onClick={onClick}
      aria-label={displayName}
      data-desktop-tool-id={tool.id}
      className={`flex w-full flex-col items-center gap-1.5 rounded-xl border-2 px-1.5 py-3 outline-none transition-colors select-none focus-visible:ring-2 focus-visible:ring-cyber-accent ${
        selected
          ? 'border-cyber-accent/70 bg-cyber-accent/10'
          : 'border-transparent hover:bg-cyber-elevated/60'
      }`}
    >
      {/* The icon alone is the graphic — no tile background behind it; the
          icon itself renders at the tile size. */}
      <span className="relative flex h-14 w-14 shrink-0 items-center justify-center">
        {iconSrc && failedSrc !== iconSrc ? (
          <img
            src={iconSrc}
            alt=""
            draggable={false}
            decoding="sync"
            onError={() => setFailedSrc(iconSrc)}
            className="w-14 h-14 object-contain"
          />
        ) : (
          <BoxIcon size={44} className="text-cyber-text-secondary" />
        )}
      </span>
      {/* Reserve exactly two lines for every name. Single-line labels center
          vertically in the same area occupied by wrapped two-line labels. */}
      <span className="flex h-8 w-full items-center justify-center text-center text-xs !leading-tight text-cyber-text">
        <span className="line-clamp-2 break-words break-keep">{displayName}</span>
      </span>
    </button>
  );
};
