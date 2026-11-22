import { useRef, type KeyboardEvent } from 'react';
import { useIsomorphicLayoutEffect } from '@dnd-kit/utilities';

interface UnderlineTabsProps<T extends string> {
  items: readonly { id: T; label: string }[];
  active: T;
  onSelect: (id: T) => void;
  label: string;
  className?: string;
  compact?: boolean;
}

export function UnderlineTabs<T extends string>({
  items,
  active,
  onSelect,
  label,
  className = '',
  compact = false,
}: UnderlineTabsProps<T>) {
  const viewportRef = useRef<HTMLDivElement>(null);
  const trackRef = useRef<HTMLDivElement>(null);
  const underlineRef = useRef<HTMLSpanElement>(null);

  useIsomorphicLayoutEffect(() => {
    const viewport = viewportRef.current;
    const track = trackRef.current;
    const underline = underlineRef.current;
    const selected = track?.querySelector<HTMLButtonElement>('[aria-selected="true"]');
    if (!viewport || !track || !underline || !selected) return;

    const placeUnderline = () => {
      const rect = selected.getBoundingClientRect();
      if (!rect.width) return;

      const firstPlacement = !underline.style.width;
      if (firstPlacement) underline.style.transition = 'none';
      underline.style.width = `${rect.width}px`;
      underline.style.transform = `translateX(${rect.left - track.getBoundingClientRect().left}px)`;
      underline.style.visibility = 'visible';
      if (firstPlacement) {
        // Commit the initial position before enabling transitions.
        underline.getBoundingClientRect();
        underline.style.removeProperty('transition');
      }
    };

    placeUnderline();
    const view = viewport.getBoundingClientRect();
    const tab = selected.getBoundingClientRect();
    if (tab.left < view.left) viewport.scrollLeft += tab.left - view.left;
    else if (tab.right > view.right) viewport.scrollLeft += tab.right - view.right;

    if (typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(placeUnderline);
    observer.observe(track);
    track.querySelectorAll('button').forEach((button) => observer.observe(button));
    return () => observer.disconnect();
  }, [active, items]);

  const move = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    let next: number;
    switch (event.key) {
      case 'ArrowRight':
        next = (index + 1) % items.length;
        break;
      case 'ArrowLeft':
        next = (index + items.length - 1) % items.length;
        break;
      case 'Home':
        next = 0;
        break;
      case 'End':
        next = items.length - 1;
        break;
      default:
        return;
    }
    event.preventDefault();
    trackRef.current?.querySelectorAll('button')[next]?.focus({ preventScroll: true });
    onSelect(items[next].id);
  };

  return (
    <div ref={viewportRef} className={`min-w-0 shrink-0 overflow-x-auto ${className}`}>
      <div
        ref={trackRef}
        role="tablist"
        aria-label={label}
        className={`relative flex w-max min-w-full border-b border-cyber-border/50 ${compact ? 'min-h-8 gap-3' : 'gap-1'}`}
      >
        {items.map((item, index) => (
          <button
            key={item.id}
            type="button"
            role="tab"
            aria-selected={active === item.id}
            tabIndex={active === item.id ? 0 : -1}
            onClick={() => onSelect(item.id)}
            onKeyDown={(event) => move(event, index)}
            className={`shrink-0 whitespace-nowrap border-b-2 border-transparent py-1.5 transition-colors focus-visible:outline focus-visible:-outline-offset-2 focus-visible:outline-cyber-accent motion-reduce:transition-none ${compact ? 'text-xs font-semibold' : 'px-3 text-[14px] font-bold'} ${active === item.id ? 'text-cyber-text' : 'text-cyber-text-secondary hover:text-cyber-text'}`}
          >
            {item.label}
          </button>
        ))}
        <span
          ref={underlineRef}
          aria-hidden="true"
          className="pointer-events-none absolute bottom-0 left-0 h-0.5 bg-[#007aff] transition-[transform,width] duration-300 ease-[cubic-bezier(0.22,1,0.36,1)] motion-reduce:transition-none"
          style={{ visibility: 'hidden' }}
        />
      </div>
    </div>
  );
}
