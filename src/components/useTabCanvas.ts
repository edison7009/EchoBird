import type { MutableRefObject } from 'react';
import type { ViewModeTabPosition } from './ViewModeTabGroup';
import { paintTabCanvas } from './tabCanvas';

// Returns one owner for the canvas, observers, animation and layout restoration.
export function mountTabCanvas(
  group: HTMLDivElement,
  remembered?: MutableRefObject<ViewModeTabPosition | null>
) {
  const canvas = document.createElement('canvas');
  canvas.className = 'view-mode-canvas';
  canvas.setAttribute('aria-hidden', 'true');
  const style = getComputedStyle(group);
  const original = {
    borderWidth: group.style.borderWidth,
    padding: group.style.padding,
  };
  const bordered = Number.parseFloat(style.borderTopWidth) > 0;
  const border = bordered ? style.borderTopColor : 'transparent';
  const radius = Number.parseFloat(style.borderTopLeftRadius) || 0;
  const padding = ['Top', 'Right', 'Bottom', 'Left'].map(
    (side) =>
      Number.parseFloat(style.getPropertyValue(`padding-${side.toLowerCase()}`)) +
      Number.parseFloat(style.getPropertyValue(`border-${side.toLowerCase()}-width`))
  );
  // Keep the original layout insets, but put the border and fill in one surface.
  group.style.borderWidth = '0';
  group.style.padding = padding.map((value) => `${value}px`).join(' ');
  group.dataset.tabRenderer = 'canvas';
  group.prepend(canvas);
  let frame = 0;
  let disposed = false;
  let from: ViewModeTabPosition | null = null;
  let target: ViewModeTabPosition | null = null;
  let current: ViewModeTabPosition | null = null;
  let started = 0;
  let width = 0;
  let height = 0;
  const motion = matchMedia('(prefers-reduced-motion: reduce)');
  const palette = { surface: '', wave: '', border, radius };
  const colors = document.createElement('span');
  colors.className = 'view-mode-canvas-colors';
  group.append(colors);
  const readColors = () => {
    const theme = getComputedStyle(colors);
    palette.surface = theme.backgroundColor;
    palette.wave = theme.color;
    palette.border = bordered ? getComputedStyle(group).borderTopColor : 'transparent';
  };
  const visible = () => !document.hidden && width > 0 && height > 0 && target !== null;
  const paint = (now: number) => {
    frame = 0;
    if (disposed || !visible() || !target) return;
    const progress = motion.matches ? 1 : Math.min(1, (now - started) / 220);
    const ease = 1 - (1 - progress) ** 3;
    current = { ...target };
    if (from && progress < 1) {
      for (const key of ['x', 'y', 'width', 'height'] as const)
        current[key] = from[key] + (target[key] - from[key]) * ease;
    }
    paintTabCanvas(
      canvas,
      width,
      height,
      devicePixelRatio || 1,
      current,
      palette,
      motion.matches ? 0 : (now % 3000) / 3000
    );
    if (!motion.matches) frame = requestAnimationFrame(paint);
  };
  const requestPaint = () => {
    if (!frame && !disposed && visible()) frame = requestAnimationFrame(paint);
  };
  const update = () => {
    const selected = group.querySelector<HTMLElement>(
      '.view-mode-tab[aria-pressed="true"], .view-mode-tab[aria-selected="true"]'
    );
    canvas.style.left = `${group.scrollLeft}px`;
    canvas.style.top = `${group.scrollTop}px`;
    const bounds = group.getBoundingClientRect();
    width = bounds.width;
    height = bounds.height;
    if (!selected || !selected.offsetWidth || !selected.offsetHeight || !width || !height) {
      target = null;
      if (frame) cancelAnimationFrame(frame);
      frame = 0;
      canvas.getContext('2d')?.clearRect(0, 0, canvas.width, canvas.height);
      return;
    }
    const rect = selected.getBoundingClientRect();
    const next = {
      x: rect.left - bounds.left,
      y: rect.top - bounds.top,
      width: rect.width,
      height: rect.height,
    };
    // Extend a row's fill to its outer edge; border and fill are clipped together.
    const inset = padding[0];
    if (next.y <= inset + 0.5) {
      next.height += next.y;
      next.y = 0;
    }
    if (height - (next.y + next.height) <= padding[2] + 0.5) next.height = height - next.y;
    if (next.x <= padding[3] + 0.5) {
      next.width += next.x;
      next.x = 0;
    }
    if (width - (next.x + next.width) <= padding[1] + 0.5) next.width = width - next.x;
    if (
      !target ||
      (['x', 'y', 'width', 'height'] as const).some(
        (key) => Math.abs(next[key] - target![key]) > 0.01
      )
    ) {
      from = current ?? remembered?.current ?? next;
      target = next;
      started = performance.now();
    }
    if (remembered) remembered.current = next;
    readColors();
    requestPaint();
  };
  const resize = new ResizeObserver(update);
  resize.observe(group);
  group.querySelectorAll('.view-mode-tab').forEach((tab) => resize.observe(tab));
  const attributes = new MutationObserver(update);
  attributes.observe(group, {
    subtree: true,
    attributes: true,
    attributeFilter: ['aria-pressed', 'aria-selected'],
  });
  const theme = new MutationObserver(() => {
    readColors();
    requestPaint();
  });
  theme.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ['data-theme', 'style', 'class'],
  });
  group.addEventListener('scroll', update);
  document.addEventListener('visibilitychange', update);
  motion.addEventListener('change', update);
  update();
  return () => {
    disposed = true;
    if (frame) cancelAnimationFrame(frame);
    resize.disconnect();
    attributes.disconnect();
    theme.disconnect();
    group.removeEventListener('scroll', update);
    document.removeEventListener('visibilitychange', update);
    motion.removeEventListener('change', update);
    canvas.remove();
    colors.remove();
    delete group.dataset.tabRenderer;
    group.style.borderWidth = original.borderWidth;
    group.style.padding = original.padding;
  };
}
