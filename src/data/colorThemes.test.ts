import { describe, expect, it } from 'vitest';
import { COLOR_THEMES, COLOR_THEME_GRID, getColorTheme, isColorThemeId } from './colorThemes';

describe('color themes', () => {
  it('keeps unique ids and valid paired palettes', () => {
    expect(new Set(COLOR_THEMES.map((theme) => theme.id)).size).toBe(COLOR_THEMES.length);
    expect(COLOR_THEMES.length).toBe(24);
    expect(COLOR_THEME_GRID).toHaveLength(4);
    expect(COLOR_THEME_GRID.every((row) => row.length === 6)).toBe(true);
    expect(COLOR_THEME_GRID.flat().slice().sort()).toEqual(
      COLOR_THEMES.map((theme) => theme.id).sort()
    );
    for (const theme of COLOR_THEMES)
      for (const variant of [theme.light, theme.dark])
        expect(Object.values(variant).every((color) => /^#[0-9A-F]{6}$/i.test(color))).toBe(true);
  });
  it('pairs the default dark palette with neutral white and gray daylight surfaces', () => {
    const { light } = getColorTheme('oatgray');
    expect(light.canvas).toBe('#FFFFFF');
    for (const color of Object.values(light)) {
      const channels = [1, 3, 5].map((start) => parseInt(color.slice(start, start + 2), 16));
      expect(new Set(channels).size).toBe(1);
    }
  });
  it('uses the oat gray artwork as the default', () => {
    expect(isColorThemeId('oatgray')).toBe(true);
    expect(getColorTheme('oatgray').labelZh).toBe('燕麦暖灰');
  });
});
