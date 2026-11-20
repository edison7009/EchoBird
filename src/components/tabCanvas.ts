import type { ViewModeTabPosition } from './ViewModeTabGroup';

export interface TabCanvasPalette {
  /** Top of the chosen key (lighter). */
  surface: string;
  /** Foot of the chosen key (deeper). */
  wave: string;
  /** The group's border colour; a bordered group is drawn as a sunken well. */
  border: string;
  /** The well behind the keys. */
  well: string;
  /** The key's drop shadow. */
  shadow: string;
  radius: number;
}

/** The chosen key sits this far inside its tab, so the well shows round it. */
const KEY_INSET = 3;

function roundedRect(
  context: CanvasRenderingContext2D,
  x: number,
  y: number,
  width: number,
  height: number,
  radius: number
) {
  const r = Math.min(radius, width / 2, height / 2);
  context.moveTo(x + r, y);
  context.lineTo(x + width - r, y);
  context.quadraticCurveTo(x + width, y, x + width, y + r);
  context.lineTo(x + width, y + height - r);
  context.quadraticCurveTo(x + width, y + height, x + width - r, y + height);
  context.lineTo(x + r, y + height);
  context.quadraticCurveTo(x, y + height, x, y + height - r);
  context.lineTo(x, y + r);
  context.quadraticCurveTo(x, y, x + r, y);
  context.closePath();
}

/**
 * Paints a segmented control as keys in a well: a bordered group becomes a
 * sunken well, and each chosen tab a raised cream key (lighter at the top,
 * a soft shadow beneath). Static: only a
 * key sliding to a new tab needs more than one frame.
 */
export function paintTabCanvas(
  canvas: HTMLCanvasElement,
  width: number,
  height: number,
  scale: number,
  selection: ViewModeTabPosition | ViewModeTabPosition[],
  palette: TabCanvasPalette
) {
  const context = canvas.getContext('2d');
  if (!context || width <= 0 || height <= 0) return;
  const pixelsWide = Math.max(1, Math.round(width * scale));
  const pixelsHigh = Math.max(1, Math.round(height * scale));
  if (canvas.width !== pixelsWide) canvas.width = pixelsWide;
  if (canvas.height !== pixelsHigh) canvas.height = pixelsHigh;
  context.setTransform(pixelsWide / width, 0, 0, pixelsHigh / height, 0, 0);
  context.clearRect(0, 0, width, height);
  context.save();
  context.beginPath();
  roundedRect(context, 0, 0, width, height, palette.radius);
  context.clip();
  if (palette.border !== 'transparent') {
    // The well: a plain, slightly deeper surface behind the keys.
    context.fillStyle = palette.well;
    context.fillRect(0, 0, width, height);
  }
  for (const region of Array.isArray(selection) ? selection : [selection]) {
    const inset = Math.min(KEY_INSET, region.width / 4, region.height / 4);
    const x = region.x + inset;
    const y = region.y + inset;
    const keyWidth = Math.max(0, region.width - inset * 2);
    const keyHeight = Math.max(0, region.height - inset * 2);
    const keyRadius = Math.max(
      2,
      Math.min(Array.isArray(selection) ? keyHeight / 2 : palette.radius - inset, keyHeight / 2)
    );
    const fill = context.createLinearGradient?.(0, y, 0, y + keyHeight);
    if (fill) {
      fill.addColorStop(0, palette.surface);
      fill.addColorStop(1, palette.wave);
    }
    context.save();
    context.shadowColor = palette.shadow;
    context.shadowBlur = 6;
    context.shadowOffsetY = 2;
    context.beginPath();
    roundedRect(context, x, y, keyWidth, keyHeight, keyRadius);
    context.fillStyle = fill ?? palette.surface;
    context.fill?.();
    context.restore();
  }
  context.restore();
}
