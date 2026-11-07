import {
  useLayoutEffect,
  useRef,
  type ComponentPropsWithoutRef,
  type MutableRefObject,
} from 'react';
import { IS_MACOS } from '../utils/platform';
import { mountTabCanvas } from './useTabCanvas';

export interface ViewModeTabPosition {
  x: number;
  y: number;
  width: number;
  height: number;
}

export function ViewModeTabGroup({
  children,
  positionRef,
  ...props
}: ComponentPropsWithoutRef<'div'> & {
  positionRef?: MutableRefObject<ViewModeTabPosition | null>;
}) {
  const groupRef = useRef<HTMLDivElement>(null);
  const localPosition = useRef<ViewModeTabPosition | null>(null);

  useLayoutEffect(() => {
    const group = groupRef.current;
    if (!group) return;
    if (IS_MACOS) return mountTabCanvas(group, positionRef ?? localPosition);
    let frame = 0;
    const place = (value: ViewModeTabPosition) => {
      group.style.setProperty('--view-mode-x', `${value.x}px`);
      group.style.setProperty('--view-mode-y', `${value.y}px`);
      group.style.setProperty('--view-mode-width', `${value.width}px`);
      group.style.setProperty('--view-mode-height', `${value.height}px`);
    };
    const update = () => {
      const selected = group.querySelector<HTMLElement>(
        '.view-mode-tab[aria-pressed="true"], .view-mode-tab[aria-selected="true"]'
      );
      if (!selected) {
        delete group.dataset.sliding;
        return;
      }
      if (!selected.offsetWidth || !selected.offsetHeight) return;
      const next = {
        x: selected.offsetLeft,
        y: selected.offsetTop,
        width: selected.offsetWidth,
        height: selected.offsetHeight,
      };

      const previous = positionRef?.current ?? localPosition.current;
      if (!group.dataset.sliding && previous) {
        place(previous);
        group.dataset.sliding = 'true';
        // Commit the previous position when a keyed panel has recreated the tabs.
        group.getBoundingClientRect();
      }
      place(next);
      group.dataset.sliding = 'true';
      localPosition.current = next;
      if (positionRef) positionRef.current = next;
    };
    update();
    // ResizeObserver covers first paint and responsive width changes. The
    // attribute observer covers a same-size tab switch, which does not resize
    // anything in WebView2 and previously left the selected tab on its flat
    // fallback background until another layout pass occurred.
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(update);
    observer?.observe(group);
    group.querySelectorAll('.view-mode-tab').forEach((tab) => observer?.observe(tab));
    const attributes =
      typeof MutationObserver === 'undefined'
        ? null
        : new MutationObserver(() => {
            if (typeof requestAnimationFrame === 'undefined') {
              update();
              return;
            }
            if (frame) cancelAnimationFrame(frame);
            frame = requestAnimationFrame(() => {
              frame = 0;
              update();
            });
          });
    attributes?.observe(group, {
      subtree: true,
      attributes: true,
      attributeFilter: ['aria-pressed', 'aria-selected'],
    });
    if (typeof requestAnimationFrame !== 'undefined') {
      frame = requestAnimationFrame(() => {
        frame = 0;
        update();
      });
    }
    return () => {
      if (frame) cancelAnimationFrame(frame);
      observer?.disconnect();
      attributes?.disconnect();
    };
  }, [children, positionRef]);

  return (
    <div {...props} ref={groupRef}>
      {children}
    </div>
  );
}
