import { describe, expect, it } from 'vitest';
import { NEUTRAL_BRAND, brandFromPixels } from './brandColour';

// A 10×10 RGBA image: `fill(i)` gives each pixel's colour, or null for transparent.
const image = (fill: (i: number) => [number, number, number] | null) => {
  const data: number[] = [];
  for (let i = 0; i < 100; i++) {
    const c = fill(i);
    data.push(...(c ?? [0, 0, 0]), c ? 255 : 0);
  }
  return data;
};
const channels = (rgb: string) => rgb.match(/\d+/g)!.map(Number);

describe('brandFromPixels', () => {
  it('takes the brand hue and settles it into a card colour white text reads on', () => {
    const colour = brandFromPixels(image(() => [217, 119, 87]));
    expect(colour.neutral).toBe(false);
    expect(colour.from).toMatch(/^rgb\(\d+ \d+ \d+\)$/);
    const [r, g, b] = channels(colour.from);
    expect(r).toBeGreaterThan(g);
    expect(g).toBeGreaterThan(b);
    // A filled square keeps its own colours on the card.
    expect(colour.glyph).toBe(false);
  });

  it('gives black, white and grey logos the neutral slate card', () => {
    expect(brandFromPixels(image((i) => (i % 3 ? [250, 250, 250] : [20, 20, 20])))).toMatchObject({
      from: NEUTRAL_BRAND.from,
      neutral: true,
    });
  });

  it('marks a one-colour glyph on transparency to be drawn in white', () => {
    const glyph = brandFromPixels(image((i) => (i % 4 === 0 ? [40, 90, 230] : null)));
    expect(glyph.glyph).toBe(true);
    const dark = brandFromPixels(image((i) => (i % 4 === 0 ? [10, 10, 10] : null)));
    expect(dark).toMatchObject({ neutral: true, glyph: true });
  });

  it('distinguishes a white glyph from a white logo on a dark tile', () => {
    const whiteGlyph = image((i) => (i % 4 === 0 ? [255, 255, 255] : null));
    const darkTile = image((i) => (i < 5 ? null : i < 25 ? [255, 255, 255] : [20, 23, 24]));
    expect(brandFromPixels(whiteGlyph)).toMatchObject({ neutral: true, glyph: true });
    expect(brandFromPixels(darkTile)).toMatchObject({ neutral: true, glyph: false });
  });

  it('keeps several colours on transparency as they are', () => {
    const palette: [number, number, number][] = [
      [234, 67, 53],
      [52, 168, 83],
      [66, 133, 244],
      [251, 188, 5],
    ];
    expect(brandFromPixels(image((i) => (i % 2 ? palette[i % 4] : null))).glyph).toBe(false);
  });

  it('darkens pale brands until white text on the card reads', () => {
    const luminance = (rgb: string) => {
      const [r, g, b] = channels(rgb).map((v) => {
        const c = v / 255;
        return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
      });
      return 0.2126 * r + 0.7152 * g + 0.0722 * b;
    };
    for (const rgb of [
      [250, 230, 120],
      [180, 240, 200],
      [200, 220, 255],
    ] as [number, number, number][]) {
      const colour = brandFromPixels(image(() => rgb));
      expect(1.05 / (luminance(colour.from) + 0.05)).toBeGreaterThanOrEqual(2.9);
    }
  });

  it('writes no percent signs, which the cards use for quota', () => {
    expect(NEUTRAL_BRAND.from + NEUTRAL_BRAND.to).not.toContain('%');
    const colour = brandFromPixels(image(() => [40, 160, 90]));
    expect(colour.from + colour.to).not.toContain('%');
  });
});
