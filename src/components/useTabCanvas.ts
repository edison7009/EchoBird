import type { MutableRefObject } from 'react';
import type { ViewModeTabPosition } from './ViewModeTabGroup';
import { paintTabCanvas } from './tabCanvas';
import { requestFlowFrame, cancelFlowFrame } from './flowFrames';

const positionFields = ['x', 'y', 'width', 'height'] as const;

function positionChanged(previous: ViewModeTabPosition, next: ViewModeTabPosition) {
  return positionFields.some((key) => Math.abs(previous[key] - next[key]) > 0.01);
}

// Returns one owner for the canvas, observers, animation and layout restoration.
export function mountTabCanvas(
  group: HTMLDivElement,
  remembered?: MutableRefObject<ViewModeTabPosition | null>
) {
  const selector = '.view-mode-tab';
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
  const palette = { surface: '', keyBottom: '', border, well: '', shadow: '', radius };
  const colors = document.createElement('span');
  colors.className = 'view-mode-canvas-colors';
  group.append(colors);
  const readColors = () => {
    const theme = getComputedStyle(colors);
    palette.surface = theme.backgroundColor;
    palette.keyBottom = theme.color;
    palette.well = theme.borderTopColor;
    palette.shadow = theme.outlineColor;
    palette.border = bordered ? getComputedStyle(group).borderTopColor : 'transparent';
  };
  const visible = () => !document.hidden && width > 0 && height > 0;
  const paint = (now: number) => {
    frame = 0;
    if (disposed || !target || !visible()) return;
    // A shared frame can have a timestamp just before this control's transition started.
    const progress = !from || motion.matches ? 1 : Math.max(0, Math.min(1, (now - started) / 220));
    current = { ...target };
    if (from && progress < 1) {
      const ease = 1 - (1 - progress) ** 3;
      for (const key of positionFields) current[key] = from[key] + (target[key] - from[key]) * ease;
    } else {
      from = null;
    }
    // Preserve the painted position for controls that share it across remounts.
    if (remembered) remembered.current = current;
    paintTabCanvas(canvas, width, height, devicePixelRatio || 1, current, palette);
    // The key is static; keep painting only while it slides to a new tab.
    if (progress < 1) frame = requestFlowFrame(paint);
  };
  const requestPaint = () => {
    if (!frame && !disposed && target && visible()) frame = requestFlowFrame(paint);
  };
  const update = () => {
    const selected = group.querySelector<HTMLElement>(
      `${selector}[aria-pressed="true"], ${selector}[aria-selected="true"]`
    );
    canvas.style.left = `${group.scrollLeft}px`;
    canvas.style.top = `${group.scrollTop}px`;
    const bounds = group.getBoundingClientRect();
    width = bounds.width;
    height = bounds.height;
    if (!selected || !selected.offsetWidth || !selected.offsetHeight || !width || !height) {
      target = null;
      if (frame) cancelFlowFrame(frame);
      frame = 0;
      // Keep the segmented control's well visible with no selected tab.
      if (visible()) {
        readColors();
        paintTabCanvas(canvas, width, height, devicePixelRatio || 1, null, palette);
      }
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
    if (!target || positionChanged(target, next)) {
      const previous = current ?? remembered?.current;
      from = previous && positionChanged(previous, next) ? previous : null;
      target = next;
      started = performance.now();
    }
    readColors();
    requestPaint();
  };
  const resize = new ResizeObserver(update);
  resize.observe(group);
  let observedTabs = new Set<HTMLElement>();
  const observeTabs = () => {
    const tabs = new Set(group.querySelectorAll<HTMLElement>(selector));
    observedTabs.forEach((tab) => {
      if (!tabs.has(tab)) resize.unobserve(tab);
    });
    tabs.forEach((tab) => {
      if (!observedTabs.has(tab)) resize.observe(tab);
    });
    observedTabs = tabs;
  };
  observeTabs();
  const attributes = new MutationObserver((records) => {
    if (records.some((record) => record.type === 'childList')) observeTabs();
    update();
  });
  attributes.observe(group, {
    subtree: true,
    childList: true,
    attributes: true,
    attributeFilter: ['aria-pressed', 'aria-selected'],
  });
  const theme = new MutationObserver(() => {
    if (!target) {
      update();
      return;
    }
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
    if (frame) cancelFlowFrame(frame);
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
