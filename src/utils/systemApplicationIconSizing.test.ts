import { afterEach, expect, it, vi } from 'vitest';
import { fitSystemApplicationIcon, iconContentBounds } from './systemApplicationIconSizing';

afterEach(() => vi.unstubAllGlobals());
const pixels = (width: number, height: number, rect: [number, number, number, number] | null) => {
  const data = new Uint8ClampedArray(width * height * 4);
  if (rect)
    for (let y = rect[1]; y < rect[1] + rect[3]; y++)
      for (let x = rect[0]; x < rect[0] + rect[2]; x++) data[(y * width + x) * 4 + 3] = 255;
  return data;
};
const image = { naturalWidth: 64, naturalHeight: 64 } as HTMLImageElement;
const src = 'data:image/png;base64,original';
const fitted = 'data:image/png;base64,fitted';
function canvasFixture(data: Uint8ClampedArray) {
  const drawImage = vi.fn();
  const input = {
    width: 0,
    height: 0,
    getContext: () => ({ drawImage: vi.fn(), getImageData: () => ({ data }) }),
  };
  const output = {
    width: 0,
    height: 0,
    getContext: () => ({ drawImage }),
    toDataURL: vi.fn(() => fitted),
  };
  const createElement = vi.fn().mockReturnValueOnce(input).mockReturnValueOnce(output);
  vi.stubGlobal('document', { createElement });
  return { input, output, drawImage, createElement };
}
it('measures opaque content independently of transparent padding and faint shadows', () => {
  const data = pixels(64, 64, [8, 10, 48, 44]);
  data[3] = 8;
  data[7] = 64;
  data[11] = 127;
  expect(iconContentBounds(data, 64, 64)).toEqual({ x: 8, y: 10, width: 48, height: 44 });
  expect(iconContentBounds(pixels(64, 64, null), 64, 64)).toBeNull();
});
it('fits inset native artwork into the same square while retaining its aspect and fringe', () => {
  const { input, output, drawImage } = canvasFixture(pixels(64, 64, [8, 10, 48, 44]));
  expect(fitSystemApplicationIcon(src, image)).toBe(fitted);
  expect(output.width).toBe(64);
  expect(output.height).toBe(64);
  const args = drawImage.mock.calls[0];
  expect(args.slice(0, 5)).toEqual([input, 7, 9, 50, 46]);
  expect(args[5]).toBe(0);
  expect(args[6]).toBeCloseTo(2.56);
  expect(args[7]).toBe(64);
  expect(args[8]).toBeCloseTo(58.88);
});
it.each([[0, 0, 64, 64], [1, 1, 62, 62], [24, 0, 16, 64], null] as const)(
  'preserves full-size, already-fitted, tall or empty artwork (%s)',
  (rect) => {
    const { output } = canvasFixture(pixels(64, 64, rect ? [...rect] : null));
    expect(fitSystemApplicationIcon(src, image)).toBe(src);
    expect(output.toDataURL).not.toHaveBeenCalled();
  }
);
it('retains the original when canvas access fails instead of losing the application icon', () => {
  vi.stubGlobal('document', {
    createElement: () => {
      throw new Error('Canvas unavailable');
    },
  });
  expect(fitSystemApplicationIcon(src, image)).toBe(src);
});
