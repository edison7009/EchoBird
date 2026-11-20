import { useLayoutEffect, useRef, useState } from 'react';
import { useI18n } from '../../hooks/useI18n';
import type { ChatMessage } from './types';

interface MessageNavigationProps {
  prompts: { index: number; message: Extract<ChatMessage, { type: 'user' }> }[];
  activeIndex?: number;
  active: boolean;
  onJump: (message: ChatMessage, index: number) => void;
}

export function MessageNavigation({
  prompts,
  activeIndex,
  active,
  onJump,
}: MessageNavigationProps) {
  const { t } = useI18n();
  const listRef = useRef<HTMLDivElement>(null);
  const [preview, setPreview] = useState<{ index: number; top: number; text: string }>();
  const clearPreview = () => setPreview(undefined);

  useLayoutEffect(() => {
    const list = listRef.current;
    const current = list?.querySelector<HTMLElement>('[aria-current="location"]');
    if (!active || !list || !current) return;
    const bottom = current.offsetTop + current.offsetHeight;
    if (current.offsetTop < list.scrollTop) list.scrollTop = current.offsetTop;
    if (bottom > list.scrollTop + list.clientHeight) list.scrollTop = bottom - list.clientHeight;
  }, [active, activeIndex, prompts.length]);

  const showPreview = (button: HTMLButtonElement, index: number, text: string) => {
    const list = listRef.current;
    if (!list) return;
    setPreview({
      index,
      text,
      top: button.offsetTop + button.offsetHeight / 2 - list.scrollTop,
    });
  };

  return (
    <nav
      aria-label={t('mother.messageNavigation')}
      className="mother-message-nav"
      onPointerLeave={clearPreview}
      onKeyDown={(event) => {
        if (event.key === 'Escape') clearPreview();
      }}
    >
      <div
        ref={listRef}
        onScroll={clearPreview}
        className="relative overflow-y-auto overscroll-contain slim-scroll"
      >
        {prompts.map(({ index, message }, turn) => {
          const text = message.text.trim().replace(/\s+/g, ' ').slice(0, 160);
          const distance = preview ? Math.abs(preview.index - turn) : undefined;
          return (
            <button
              key={index}
              type="button"
              aria-label={`${turn + 1}. ${text}`}
              aria-current={index === activeIndex ? 'location' : undefined}
              data-preview-distance={distance !== undefined && distance <= 2 ? distance : undefined}
              onPointerEnter={(event) => {
                if (event.pointerType !== 'touch') showPreview(event.currentTarget, turn, text);
              }}
              onFocus={(event) => {
                if (event.currentTarget.matches(':focus-visible')) {
                  showPreview(event.currentTarget, turn, text);
                }
              }}
              onBlur={clearPreview}
              onClick={() => onJump(message, index)}
            >
              <span aria-hidden="true" />
            </button>
          );
        })}
      </div>
      {active && preview && (
        <div
          aria-hidden="true"
          className="app-tooltip mother-message-preview pointer-events-none absolute -translate-y-1/2 truncate rounded-md border px-2 py-1 text-xs shadow-lg"
          style={{ top: preview.top }}
        >
          {preview.text}
        </div>
      )}
    </nav>
  );
}
