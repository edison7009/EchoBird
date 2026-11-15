import { describe, expect, it } from 'vitest';
import codexSource from '../../src-tauri/src/services/codex_accounts.rs?raw';
import nativeSource from '../../src-tauri/src/services/claude_code_accounts.rs?raw';
import { accountError, isAccountAuthorizationError } from './accountError';
import { en } from '../i18n/en';
import zhHans from '../i18n/zh-Hans';
import zhHant from '../i18n/zh-Hant';
import ja from '../i18n/ja';
import type { TKey } from '../i18n/types';

const t = (key: TKey) => zhHans[key] ?? en[key];

describe('account errors', () => {
  it.each([
    'accountError.loginRequired',
    'accountError.expired',
    'accountError.auth',
    'accountError.auth|HTTP 400|invalid_grant',
    'accountError.auth|HTTP 403 Forbidden',
    'accountError.quota|HTTP 401',
    new Error('accountError.network|HTTP 401 Unauthorized'),
  ])('recognizes a saved-account authorization failure (%s)', (error) => {
    expect(isAccountAuthorizationError(error)).toBe(true);
  });

  it.each([
    'accountError.network',
    'accountError.quotaTimeout',
    'accountError.authResponse',
    'accountError.read',
    'accountError.denied|HTTP 403',
    'accountError.auth|HTTP 429',
    'accountError.auth|HTTP 503 Service Unavailable',
    'accountError.quota|HTTP 40123',
    '401 Unauthorized',
    null,
  ])('does not mistake other failures for lost authorization (%s)', (error) => {
    expect(isAccountAuthorizationError(error)).toBe(false);
  });

  it('localizes both backend strings and Error objects without leaking diagnostics', () => {
    expect(accountError('accountError.unavailable', t)).toBe(zhHans['accountError.unavailable']);
    expect(accountError('accountError.expired', t)).toBe(zhHans['accountError.expired']);
    expect(accountError(new Error('accountError.write|Permission denied'), t)).toBe(
      zhHans['accountError.write']
    );
    expect(accountError('accountError.quota|HTTP 429|upstream English error', t)).toBe(
      `${zhHans['accountError.quota']} (HTTP 429)`
    );
  });

  it('uses a localized fallback for unknown system errors and invalid keys', () => {
    for (const error of ['Access is denied', 'accountError.unknown', 'toString', null]) {
      expect(accountError(error, t)).toBe(zhHans['accountError.failed']);
    }
  });

  it('has translations in every language for every backend account error', () => {
    for (const source of [codexSource, nativeSource]) {
      for (const match of source.matchAll(/accountError\.[A-Za-z]+/g)) {
        const key = match[0] as TKey;
        for (const locale of [en, zhHans, zhHant, ja]) expect(locale[key], key).toBeTruthy();
      }
    }
  });
});
