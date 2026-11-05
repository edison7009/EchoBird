import { describe, expect, it, vi } from 'vitest';
import { paintTabCanvas } from './tabCanvas';

// Sample the radial fills emitted by the real painter. This catches a jump
// in the image at a wrap boundary rather than just checking the phase clock.
function sample(phase: number, independent: boolean) {
  const lights: { x: number; y: number; sx: number; sy: number; alpha: number }[] = [];
  let transform = { x: 0, y: 0, sx: 1, sy: 1 };
  const stack: (typeof transform)[] = [];
  const context = {
    globalAlpha: 1,
    fillStyle: '' as unknown,
    setTransform: vi.fn(),
    clearRect: vi.fn(),
    beginPath: vi.fn(),
    moveTo: vi.fn(),
    lineTo: vi.fn(),
    quadraticCurveTo: vi.fn(),
    closePath: vi.fn(),
    clip: vi.fn(),
    rect: vi.fn(),
    save: () => stack.push({ ...transform }),
    restore: () => {
      transform = stack.pop()!;
    },
    translate: (x: number, y: number) => {
      transform.x += x;
      transform.y += y;
    },
    scale: (sx: number, sy: number) => {
      transform.sx *= sx;
      transform.sy *= sy;
    },
    createRadialGradient: () => ({ ...transform, addColorStop: vi.fn() }),
    fillRect: () => {
      if (typeof context.fillStyle === 'object') {
        lights.push({ ...(context.fillStyle as typeof transform), alpha: context.globalAlpha });
      }
    },
  };
  const canvas = { width: 120, height: 32, getContext: () => context };
  const region = { x: 0, y: 0, width: 120, height: 32 };
  paintTabCanvas(
    canvas as unknown as HTMLCanvasElement,
    120,
    32,
    1,
    independent ? [region] : region,
    { surface: 'black', wave: 'white', border: 'transparent', radius: 0 },
    phase
  );
  return Array.from({ length: 65 }, (_, index) => {
    const x = (((index % 13) + 0.5) * 120) / 13;
    const y = ((Math.floor(index / 13) + 0.5) * 32) / 5;
    return lights.reduce((pixel, light) => {
      const alpha =
        light.alpha *
        Math.max(0, 1 - Math.hypot((x - light.x) / light.sx, (y - light.y) / light.sy));
      return alpha + pixel * (1 - alpha);
    }, 0);
  });
}
const difference = (a: number[], b: number[]) =>
  Math.max(...a.map((value, i) => Math.abs(value - b[i])));

describe.each([false, true])('flow continuity (independent=%s)', (independent) => {
  it.each([0.5, 1])('keeps the image continuous when a light wraps at phase %s', (boundary) => {
    expect(
      difference(
        sample(boundary - 0.0001, independent),
        sample((boundary + 0.0001) % 1, independent)
      )
    ).toBeLessThan(0.001);
  });
  it('still moves between wrap boundaries', () => {
    expect(difference(sample(0, independent), sample(0.25, independent))).toBeGreaterThan(0.01);
  });
});
