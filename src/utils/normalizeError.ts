import type { TKey } from '../i18n/types';

/**
 * Maps a raw backend / OS error message to a user-friendly i18n key.
 *
 * Returns:
 *   - A TKey when the message matches a known category (timeout, ssh,
 *     cancelled, no-server, no-model, agent-failed). The caller
 *     renders the localized string.
 *   - `null` when the message is informative but uncategorized. The
 *     caller renders the message text verbatim, because v4.7.0 made
 *     the backend emit the upstream provider's real error message
 *     ("Invalid API Key" / "Rate limit exceeded" / "You exceeded
 *     your quota") — discarding that into a generic "请求失败" key
 *     would hide the actionable info from the user.
 *
 * The caller falls back to `error.requestFailed` only when the
 * message is empty (no info to show).
 */
export function errorToKey(msg: string): TKey | null {
  const raw = String(msg).trim();
  if (!raw) return 'error.requestFailed';
  const lower = raw.toLowerCase();

  // User-initiated cancel / abort — check first
  if (
    lower === 'aborted' ||
    lower.includes('user abort') ||
    lower.includes('cancelled') ||
    lower.includes('canceled') ||
    lower.includes('agent aborted') ||
    lower.includes('abort')
  )
    return 'error.userCancelled';

  // Connection timeout / no response (includes Windows OS error 10060
  // and Chinese OS text).
  if (
    lower.includes('10060') ||
    lower.includes('timed out') ||
    lower.includes('timeout') ||
    lower.includes('没有回应') ||
    lower.includes('没有正确答复') ||
    lower.includes('connection timed') ||
    lower.includes('stream stalled')
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

  // Uncategorized but informative — let the caller render `raw` verbatim
  // so users see the provider's own error message.
  return null;
}

/**
 * Convenience: translate a raw error message into a display string.
 * Falls back to the raw message itself when no category matches —
 * preserves the verbatim upstream error introduced in v4.7.0.
 */
export function normalizeError(msg: unknown, t: (key: TKey) => string): string {
  const raw = String(msg).trim();
  const key = errorToKey(raw);
  if (key) return t(key);
  // Truncate very long bodies so the chat bubble stays readable;
  // 500 chars is more than enough for any reasonable error message.
  return raw.length > 500 ? `${raw.slice(0, 500)}…` : raw;
}
