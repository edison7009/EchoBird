import type { TKey } from '../i18n/types';

/**
 * Maps a raw backend/OS error message to a user-friendly i18n key.
 * Keeps error messages concise, non-technical, and language-neutral.
 */
export function errorToKey(msg: string): TKey {
  const lower = String(msg).toLowerCase();

  // User-initiated cancel/abort — check first
  if (
    lower === 'aborted' ||
    lower.includes('user abort') ||
    lower.includes('cancelled') ||
    lower.includes('canceled') ||
    lower.includes('agent aborted') ||
    lower.includes('abort')
  )
    return 'error.userCancelled';

  // Connection timeout / no response (includes Windows OS error 10060 and Chinese OS text)
  if (
    lower.includes('10060') ||
    lower.includes('timed out') ||
    lower.includes('timeout') ||
    lower.includes('没有回应') ||
    lower.includes('没有正确答复') ||
    lower.includes('connection timed')
  )
    return 'error.connectionTimeout';

  // SSH / host unreachable / network errors
  if (
    lower.includes('ssh') ||
    lower.includes('connection refused') ||
    lower.includes('connection failed') ||
    lower.includes('host key') ||
    lower.includes('network is unreachable') ||
    lower.includes('no route')
  )
    return 'error.serverUnreachable';

  // Agent startup failed
  if (lower.includes('agent start') || lower.includes('agent failed')) return 'error.agentFailed';

  // No server ID configured
  if (lower.includes('no server id') || lower.includes('server id')) return 'error.noServerConfig';

  // No model selected / found
  if (
    lower.includes('no model') ||
    lower.includes('model data') ||
    lower.includes('model not found')
  )
    return 'error.noModelSelected';

  // Generic fallback
  return 'error.requestFailed';
}

/**
 * Convenience: translate a raw error message using the current t() function.
 */
export function normalizeError(msg: unknown, t: (key: TKey) => string): string {
  return t(errorToKey(String(msg)));
}
