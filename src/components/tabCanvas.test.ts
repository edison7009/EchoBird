import { describe, expect, it, vi } from 'vitest';
import { paintTabCanvas, type TabCanvasPalette } from './tabCanvas';

// Records what the painter draws: fills (with their colour and the shadow in
// force) and the rounded key shapes.
function paint(selection: Parameters<typeof paintTabCanvas>[4], border = 'rgb(80,80,80)') {
  const fills: { style: unknown; shadow: string; rect?: number[] }[] = [];
  const keys: number[][] = [];
  const gradients: { stops: [number, string][] }[] = [];
  const context = {
    fillStyle: '' as unknown,
    shadowColor: '',
    shadowBlur: 0,
    shadowOffsetY: 0,
    setTransform: vi.fn(),
    clearRect: vi.fn(),
    save: vi.fn(),
    restore: vi.fn(() => {
      context.shadowColor = '';
    }),
    beginPath: vi.fn(),
    moveTo: vi.fn((x: number, y: number) => keys.push([x, y])),
    lineTo: vi.fn(),
    quadraticCurveTo: vi.fn(),
    closePath: vi.fn(),
    clip: vi.fn(),
    createLinearGradient: () => {
      const gradient = {
        stops: [] as [number, string][],
        addColorStop: (o: number, c: string) => gradient.stops.push([o, c]),
      };
      gradients.push(gradient);
      return gradient;
    },
    fill: vi.fn(() => fills.push({ style: context.fillStyle, shadow: context.shadowColor })),
    fillRect: vi.fn((...rect: number[]) =>
      fills.push({ style: context.fillStyle, shadow: context.shadowColor, rect })
    ),
  };
  const palette: TabCanvasPalette = {
    surface: '#f7f2ea',
    keyBottom: '#e6dfd3',
    border,
    well: 'rgb(42,39,35)',
    shadow: 'rgb(0 0 0 / 0.4)',
    radius: 8,
  };
  const canvas = { width: 120, height: 32, getContext: () => context };
  paintTabCanvas(canvas as unknown as HTMLCanvasElement, 120, 32, 1, selection, palette);
  return { fills, keys, gradients, context };
}

describe('tab canvas', () => {
  it('draws a bordered group as a well with the chosen tab as a raised cream key', () => {
    const { fills, gradients, keys } = paint({ x: 60, y: 0, width: 60, height: 32 });
    // The well fills the whole group first.
    expect(fills[0]).toMatchObject({ style: 'rgb(42,39,35)', rect: [0, 0, 120, 32] });
    // The key: cream from top to foot, filled under its drop shadow.
    expect(gradients[0].stops).toEqual([
      [0, '#f7f2ea'],
      [1, '#e6dfd3'],
    ]);
    const key = fills.find((fill) => fill.style === gradients[0]);
    expect(key?.shadow).toBe('rgb(0 0 0 / 0.4)');
    // It sits 3px inside its tab, so the well shows round it.
    expect(keys.some(([x, y]) => y === 3 && x > 63 && x < 70)).toBe(true);
  });

  it('draws no well for a borderless group, only its keys', () => {
    const { fills, gradients } = paint(
      [
        { x: 0, y: 0, width: 50, height: 24 },
        { x: 60, y: 0, width: 50, height: 24 },
      ],
      'transparent'
    );
    expect(fills.some((fill) => fill.style === 'rgb(42,39,35)')).toBe(false);
    expect(gradients).toHaveLength(2);
  });

  it('paints the same image every time: the key does not flow or flicker', () => {
    const first = paint({ x: 0, y: 0, width: 60, height: 32 });
    const second = paint({ x: 0, y: 0, width: 60, height: 32 });
    expect(second.keys).toEqual(first.keys);
    expect(second.fills.map((fill) => fill.rect)).toEqual(first.fills.map((fill) => fill.rect));
  });
});
