import { useEffect, useState } from 'react';

// A card colour for each provider, read from its own icon, so new or remote
// directory entries need no hand-kept palette.

export type BrandColour = {
  /** Top / lighter stop of the card gradient (CSS colour). */
  from: string;
  /** Bottom / deeper stop of the card gradient (CSS colour). */
  to: string;
  /** The icon has too little colour of its own: the card is neutral slate. */
  neutral: boolean;
  /**
   * The icon is a one-colour glyph on transparency (its brand colour, black
   * or white). On a card of its own colour it would vanish, so it is drawn in
   * white there. Multi-colour logos and logos on their own filled square keep
   * their colours.
   */
  glyph: boolean;
};

// Colours are written as rgb(): no percent signs in the markup, which the
// cards' own text uses for quota.
function hslToRgb(h: number, s: number, l: number): string {
  const k = (n: number) => (n + h / 30) % 12;
  const a = s * Math.min(l, 1 - l);
  const f = (n: number) => l - a * Math.max(-1, Math.min(k(n) - 3, Math.min(9 - k(n), 1)));
  return `rgb(${Math.round(f(0) * 255)} ${Math.round(f(8) * 255)} ${Math.round(f(4) * 255)})`;
}

function contrastWithWhite(h: number, s: number, l: number): number {
  const k = (n: number) => (n + h / 30) % 12;
  const a = s * Math.min(l, 1 - l);
  const channel = (n: number) => {
    const c = l - a * Math.max(-1, Math.min(k(n) - 3, Math.min(9 - k(n), 1)));
    return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  };
  const luminance = 0.2126 * channel(0) + 0.7152 * channel(8) + 0.0722 * channel(4);
  return 1.05 / (luminance + 0.05);
}

export const NEUTRAL_BRAND: BrandColour = {
  from: hslToRgb(220, 0.09, 0.3),
  to: hslToRgb(220, 0.1, 0.22),
  neutral: true,
  glyph: false,
};

function rgbToHsl(r: number, g: number, b: number): [number, number, number] {
  r /= 255;
  g /= 255;
  b /= 255;
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const l = (max + min) / 2;
  if (max === min) return [0, 0, l];
  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  const h =
    max === r ? (g - b) / d + (g < b ? 6 : 0) : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
  return [h * 60, s, l];
}

/**
 * The most common saturated hue in RGBA pixels, weighted by saturation;
 * white, black and grey pixels do not vote. Its lightness and saturation are
 * settled into a band where white text reads on every brand alike.
 */
export function brandFromPixels(data: ArrayLike<number>): BrandColour {
  const bins = Array.from({ length: 36 }, () => ({ w: 0, h: 0, s: 0, l: 0 }));
  const hues: number[] = [];
  let coloured = 0;
  let solid = 0;
  let dark = 0;
  let light = 0;
  for (let i = 0; i < data.length; i += 4) {
    if (data[i + 3] < 140) continue;
    solid++;
    const [h, s, l] = rgbToHsl(data[i], data[i + 1], data[i + 2]);
    if (s < 0.25 && l < 0.3) dark++;
    if (s < 0.25 && l > 0.7) light++;
    if (s < 0.25 || l < 0.12 || l > 0.92) continue;
    coloured++;
    hues.push(h);
    const bin = bins[Math.floor(h / 10) % 36];
    const w = s * (1 - Math.abs(l - 0.5));
    bin.w += w;
    bin.h += h * w;
    bin.s += s * w;
    bin.l += l * w;
  }
  const thin = solid < (data.length / 4) * 0.6;
  if (!solid || coloured / solid < 0.08) {
    return { ...NEUTRAL_BRAND, glyph: thin && solid > 0 && Math.max(dark, light) / solid > 0.85 };
  }
  // Pool each bin with its neighbours so a gradient icon picks its centre.
  let best = 0;
  let bestW = -1;
  for (let i = 0; i < 36; i++) {
    const w = bins[(i + 35) % 36].w * 0.5 + bins[i].w + bins[(i + 1) % 36].w * 0.5;
    if (w > bestW) {
      bestW = w;
      best = i;
    }
  }
  const bin = bins[best];
  if (!bin.w) return NEUTRAL_BRAND;
  const h = bin.h / bin.w;
  const near = hues.filter((x) => Math.min(Math.abs(x - h), 360 - Math.abs(x - h)) < 25).length;
  // Yellows and greens look lighter to the eye, so they go a little darker.
  const s = Math.min(Math.max(bin.s / bin.w, 0.5), 0.82);
  const heavy = h > 40 && h < 170 ? 0.07 : 0;
  let l = Math.min(Math.max(bin.l / bin.w, 0.36), 0.5) - heavy;
  // Then darken until white text has at least 3:1 contrast (WCAG, large bold text).
  while (contrastWithWhite(h, s, l) < 3 && l > 0.2) l -= 0.02;
  return {
    from: hslToRgb(h, s, l),
    to: hslToRgb((h + 4) % 360, Math.min(s + 0.04, 0.9), l - 0.08),
    neutral: false,
    glyph: thin && near / solid > 0.75,
  };
}

const cache = new Map<string, Promise<BrandColour>>();

/** Reads an icon once per address; failures fall back to the neutral card. */
export function brandColour(src: string | null | undefined): Promise<BrandColour> {
  if (!src) return Promise.resolve(NEUTRAL_BRAND);
  let pending = cache.get(src);
  if (!pending) {
    pending = new Promise<BrandColour>((resolve) => {
      if (typeof Image === 'undefined' || typeof document === 'undefined') {
        resolve(NEUTRAL_BRAND);
        return;
      }
      const img = new Image();
      img.onload = () => {
        try {
          const canvas = document.createElement('canvas');
          canvas.width = canvas.height = 40;
          const context = canvas.getContext('2d', { willReadFrequently: true });
          if (!context) return resolve(NEUTRAL_BRAND);
          context.drawImage(img, 0, 0, 40, 40);
          resolve(brandFromPixels(context.getImageData(0, 0, 40, 40).data));
        } catch {
          resolve(NEUTRAL_BRAND);
        }
      };
      img.onerror = () => resolve(NEUTRAL_BRAND);
      img.src = src;
    });
    cache.set(src, pending);
  }
  return pending;
}

/** The card colour for an icon: neutral slate until the icon has been read. */
export function useBrandColour(src: string | null | undefined): BrandColour {
  const [colour, setColour] = useState<{ src?: string | null; value: BrandColour }>({
    value: NEUTRAL_BRAND,
  });
  useEffect(() => {
    let cancelled = false;
    void brandColour(src).then((value) => {
      if (!cancelled) setColour({ src, value });
    });
    return () => {
      cancelled = true;
    };
  }, [src]);
  return colour.src === src ? colour.value : NEUTRAL_BRAND;
}
