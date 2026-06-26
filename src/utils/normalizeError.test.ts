import { describe, expect, it } from 'vitest';
import { errorToKey } from './normalizeError';

describe('errorToKey', () => {
  it('classifies HTML responses rejected during SSE setup', () => {
    expect(
      errorToKey('SSE setup error: Invalid header value: "text/html; charset=utf-8"')
    ).toBe('error.providerReturnedHtml');
  });

  it('keeps provider errors verbatim when no category matches', () => {
    expect(errorToKey('Invalid API Key')).toBeNull();
  });
});