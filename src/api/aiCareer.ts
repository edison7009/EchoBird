// "我的AI生涯" (My AI Career) IPC layer — cross-tool session history,
// contribution heatmap, and profile avatar. Backed by the Rust
// `ai_career` service + `set_avatar` / `get_avatar` commands.

import { invoke } from '@tauri-apps/api/core';
import type { AiCareerFamily } from '../data/aiCareerFamilies';

export type { AiCareerFamily } from '../data/aiCareerFamilies';

export interface SavedSession {
  id: string;
  name: string;
  tool: string;
  cwd: string;
  session_token: string | null;
  saved_at: string;
  file_path: string | null;
  model_id?: string | null;
  message_count?: number | null;
  turn_count: number | null;
  input_tokens: number | null;
  output_tokens: number | null;
}

export interface HeatmapEntry {
  /// File mtime, seconds since the UNIX epoch.
  ts: number;
  /// Approximate message count for the session.
  count: number;
}

export interface DailyTokenEntry {
  ts: number;
  input_tokens: number;
  output_tokens: number;
}

export interface DailyTokenResponse {
  entries: DailyTokenEntry[];
  unavailable_families: AiCareerFamily[];
}

/// One page of a single family's session history, newest first. The page is
/// scanned + parsed on the Rust side; the caller pages in more via `offset`.
export async function aiCareerFamilyHistory(
  family: AiCareerFamily,
  offset: number,
  limit: number
): Promise<SavedSession[]> {
  return invoke('ai_career_family_history', { family, offset, limit });
}

/// Contribution-heatmap entries across all families (210-day lookback).
export async function aiCareerHeatmap(
  family: AiCareerFamily | null = null
): Promise<HeatmapEntry[]> {
  return invoke('ai_career_heatmap', { family });
}

/// Total on-disk byte size across all families' session files. The
/// frontend divides this by a bytes-per-token ratio for an approximate ("≈")
/// cumulative token count — works even when a provider doesn't report real
/// usage (third-party models often log 0), since it measures content volume.
export async function aiCareerTokenBytes(family: AiCareerFamily | null = null): Promise<number> {
  return invoke('ai_career_token_bytes', { family });
}

export async function aiCareerDailyTokens(
  family: AiCareerFamily | null = null
): Promise<DailyTokenResponse> {
  return invoke('ai_career_daily_tokens', { family });
}

export async function revealAiCareerSessionFile(path: string): Promise<void> {
  return invoke('reveal_ai_career_session_file', { path });
}

/// Set the profile avatar from a user-picked image file (re-encoded to a
/// 256px PNG on the Rust side).
export async function setAvatar(sourcePath: string): Promise<void> {
  return invoke('set_avatar', { sourcePath });
}

/// Read the stored avatar as a base64 PNG data URI, or `null` if unset.
export async function getAvatar(): Promise<string | null> {
  return invoke('get_avatar');
}
