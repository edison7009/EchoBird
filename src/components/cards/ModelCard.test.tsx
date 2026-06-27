import { describe, expect, it } from 'vitest';
import type { TKey } from '../../i18n/types';
import { getModelIcon } from './ModelCard';
import { en } from '../../i18n/en';
import zhHans from '../../i18n/zh-Hans';
import zhHant from '../../i18n/zh-Hant';
import ja from '../../i18n/ja';

// The Model Center "duplicate config" action (issue #337) renders a
// [btn.duplicate] button. These tests pin the i18n contract for that key and
// the icon-resolution helper the card relies on.

describe('btn.duplicate i18n key', () => {
  // Forcing the union member here makes `tsc --noEmit` fail if the key was
  // ever dropped from TKey, so the type and the dictionaries stay in lockstep.
  const key: TKey = 'btn.duplicate';

  it('resolves to a non-empty string in every locale (no missing-key fallback)', () => {
    for (const [locale, dict] of [
      ['en', en],
      ['zh-Hans', zhHans],
      ['zh-Hant', zhHant],
      ['ja', ja],
    ] as const) {
      const value = dict[key];
      expect(value, `missing btn.duplicate in ${locale}`).toBeTruthy();
      expect(typeof value).toBe('string');
      expect(String(value).trim().length).toBeGreaterThan(0);
    }
  });
});

describe('getModelIcon', () => {
  it('matches a provider keyword to its icon asset', () => {
    expect(getModelIcon('Claude Sonnet', 'claude-3-5-sonnet')).toBe('./icons/models/claude.svg');
    expect(getModelIcon('My GPT', 'gpt-4o')).toBe('./icons/models/chatgpt.svg');
  });

  it('returns null when no keyword matches', () => {
    expect(getModelIcon('totally-unknown-model', 'xyz-001')).toBeNull();
  });
});
