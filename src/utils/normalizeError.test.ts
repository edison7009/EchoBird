import { describe, expect, it } from 'vitest';
import { errorToKey } from './normalizeError';

describe('errorToKey', () => {
  it('classifies HTML responses rejected during SSE setup', () => {
    expect(
      errorToKey('SSE setup error: Invalid header value: "text/html; charset=utf-8"')
    ).toBe('error.providerReturnedHtml');
  });

  it('classifies provider authentication failures', () => {
    expect(errorToKey('Invalid API Key')).toBe('error.providerAuthFailed');
    expect(errorToKey('401 Unauthorized')).toBe('error.providerAuthFailed');
  });

  it('classifies provider quota and rate-limit failures', () => {
    expect(errorToKey('429 Too Many Requests')).toBe('error.providerRateLimited');
    expect(errorToKey('You exceeded your current quota')).toBe(
      'error.providerRateLimited'
    );
  });

  it('classifies invalid provider endpoint failures', () => {
    expect(errorToKey('404 Not Found')).toBe('error.providerEndpointInvalid');
    expect(errorToKey('getaddrinfo ENOTFOUND api.example.com')).toBe(
      'error.providerEndpointInvalid'
    );
  });

  it('keeps unknown provider errors verbatim when no category matches', () => {
    expect(errorToKey('Provider exploded unexpectedly')).toBeNull();
  });
});
