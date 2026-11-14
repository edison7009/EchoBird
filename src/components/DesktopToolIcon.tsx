import React, { useEffect, useRef, useState } from 'react';
import {
  cachedSystemApplicationIcon,
  loadSystemApplicationIcon,
} from '../utils/systemApplicationIcons';
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
export const DesktopToolImage: React.FC<{ tool: LocalTool; className?: string }> = ({
  tool,
  className = 'h-14 w-14',
}) => {
  const imageContainer = useRef<HTMLSpanElement>(null);
  const [systemIcon, setSystemIcon] = useState<{
    id: string;
    src: string | null;
    preloaded: boolean;
  } | null>(() => {
    const src = tool.systemAppId ? cachedSystemApplicationIcon(tool.systemAppId) : undefined;
    return src !== undefined ? { id: tool.systemAppId!, src, preloaded: true } : null;
  });
  const [visibility, setVisibility] = useState({ id: tool.systemAppId, visible: true });
  const visible = visibility.id !== tool.systemAppId || visibility.visible;
  const cachedIcon = tool.systemAppId ? cachedSystemApplicationIcon(tool.systemAppId) : undefined;
  const currentIcon =
    systemIcon?.id === tool.systemAppId
      ? systemIcon
      : cachedIcon !== undefined
        ? { id: tool.systemAppId, src: cachedIcon, preloaded: true }
        : null;
  const iconSrc = tool.icon || currentIcon?.src;
  const [loadedSrc, setLoadedSrc] = useState<string | null>(null);
  const [failedSrc, setFailedSrc] = useState<string | null>(null);
  const hasImage = visible && !!iconSrc && failedSrc !== iconSrc;
  const pending =
    visible &&
    !!tool.systemAppId &&
    ((!tool.icon && !currentIcon) ||
      (hasImage && loadedSrc !== iconSrc && !(!tool.icon && currentIcon?.preloaded)));
  useEffect(() => {
    const id = tool.systemAppId;
    const container = imageContainer.current;
    if (!id || tool.icon || !container) return;
    let active = true;
    let inViewport = true;
    const load = () => {
      const cached = cachedSystemApplicationIcon(id);
      if (cached !== undefined) {
        setSystemIcon({ id, src: cached, preloaded: true });
        return;
      }
      void loadSystemApplicationIcon(id).then((src) => {
        if (active && inViewport) setSystemIcon({ id, src, preloaded: false });
      });
    };
    if (typeof IntersectionObserver === 'undefined') {
      load();
      return () => {
        active = false;
      };
    }
    const observer = new IntersectionObserver(
      (entries) => {
        inViewport = entries.some((entry) => entry.isIntersecting);
        setVisibility({ id, visible: inViewport });
        if (inViewport) {
          load();
        } else {
          setSystemIcon(null);
          setLoadedSrc(null);
          setFailedSrc(null);
        }
      },
      {
        root: container.closest?.('.overflow-y-auto') ?? null,
        rootMargin: '200px',
      }
    );
    observer.observe(container);
    return () => {
      active = false;
      observer.disconnect();
    };
  }, [tool]);

  return (
    <span
      ref={imageContainer}
      aria-busy={pending || undefined}
      className={`relative flex shrink-0 items-center justify-center ${className}`}
    >
      {tool.systemAppId && (
        <span
          aria-hidden="true"
          data-icon-skeleton=""
          className={`pointer-events-none absolute inset-0 transition-opacity duration-200 motion-reduce:transition-none ${pending ? 'opacity-100' : 'opacity-0'}`}
        >
          <span
            className={`block h-full w-full rounded-xl bg-cyber-border/30 ${pending ? 'animate-pulse motion-reduce:animate-none' : ''}`}
          />
        </span>
      )}
      {hasImage ? (
        <img
          src={iconSrc}
          alt=""
          draggable={false}
          decoding={tool.systemAppId ? 'async' : 'sync'}
          onLoad={() => setLoadedSrc(iconSrc)}
          onError={() => setFailedSrc(iconSrc)}
          className={`h-full w-full object-contain transition-opacity duration-200 motion-reduce:transition-none ${pending ? 'opacity-0' : 'opacity-100'}`}
        />
      ) : visible && !pending ? (
        <BoxIcon size={44} className="text-cyber-text-secondary" />
      ) : null}
    </span>
  );
};

export const DesktopIcon: React.FC<DesktopIconProps> = ({ tool, selected, onClick, dragProps }) => {
  const { locale } = useI18n();
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
      <DesktopToolImage tool={tool} />
      {/* Reserve exactly two lines for every name. Single-line labels center
          vertically in the same area occupied by wrapped two-line labels. */}
      <span className="flex h-8 w-full items-center justify-center text-center text-xs !leading-tight text-cyber-text">
        <span className="line-clamp-2 break-words break-keep">{displayName}</span>
      </span>
    </button>
  );
};
