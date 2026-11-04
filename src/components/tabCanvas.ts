import type { ViewModeTabPosition } from './ViewModeTabGroup';

export interface TabCanvasPalette {
  surface: string;
  wave: string;
  border: string;
  radius: number;
}

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

export function paintTabCanvas(
  canvas: HTMLCanvasElement,
  width: number,
  height: number,
  scale: number,
  selection: ViewModeTabPosition,
  palette: TabCanvasPalette,
  phase: number
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
  // The fill and outline share this bitmap and its pixel grid. There is no
  // separately rounded inner height or composited CSS background to line up.
  context.fillStyle = palette.surface;
  context.fillRect(selection.x, selection.y, selection.width, selection.height);
  context.save();
  context.beginPath();
  context.rect(selection.x, selection.y, selection.width, selection.height);
  context.clip();
  for (const edge of [0, 1]) {
    const x = selection.x + selection.width * ((phase + edge * 0.5) % 1);
    const y = selection.y + selection.height * edge;
    context.save();
    context.translate(x, y);
    context.scale(Math.max(1, selection.width), Math.max(1, selection.height));
    const wave = context.createRadialGradient(0, 0, 0, 0, 0, 1);
    wave.addColorStop(0, palette.wave);
    wave.addColorStop(1, 'transparent');
    context.globalAlpha = 0.38;
    context.fillStyle = wave;
    context.fillRect(-2, -2, 4, 4);
    context.restore();
  }
  context.restore();
  if (palette.border !== 'transparent') {
    context.beginPath();
    roundedRect(
      context,
      0.5,
      0.5,
      Math.max(0, width - 1),
      Math.max(0, height - 1),
      Math.max(0, palette.radius - 0.5)
    );
    context.lineWidth = 1;
    context.strokeStyle = palette.border;
    context.stroke();
  }
  context.restore();
}
