// Tauri IPC API layer — replaces window.electron.* calls
// All frontend↔backend communication goes through this module.
//
// Domain modules have been split out for better organisation.
// This file keeps common/misc functions and re-exports all domain modules
// so consumers can continue to use  `import * as api from '../api/tauri'`.

import { invoke } from '@tauri-apps/api/core';
export { switchDevinAccount } from './devin';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { open as openShell } from '@tauri-apps/plugin-shell';
import type { DetectedTool, ApplyModelInput, AppSettings } from './types';

// ─── Re-export domain modules ───

export * from './models';
export * from './accountProxy';
export * from './clientAccounts';
export * from './cline';
export * from './minimax';
export * from './qoder';
export * from './qwenwork';
export * from './dumate';
export * from './doubao';
export * from './localServer';
export * from './agent';
export * from './parasite';
export * from './ssh';
export * from './secret';
export * from './bundled';
export * from './aiCareer';
export * from './freeModels';
export * from './smartRouter';
export * from './routerStatistics';

// ─── Tool APIs ───

export async function scanTools(): Promise<DetectedTool[]> {
  return invoke('scan_tools');
}

export async function applyModelToTool(
  toolId: string,
  modelInfo: ApplyModelInput
): Promise<{ success: boolean; message: string }> {
  return invoke('apply_model_to_tool', { toolId, modelInfo });
}

export async function restoreToolToOfficial(
  toolId: string
): Promise<{ success: boolean; message: string }> {
  return invoke('restore_tool_to_official', { toolId });
}

export interface CodexAccount {
  id: string;
  email: string;
  plan?: string;
  subscriptionEndAt?: number | null;
  quotaWindows?: { label?: string | null; remainingPercent: number; resetAt?: number | null }[];
  quotaPercent?: number | null;
  quotaResetAt?: number | null;
  active: boolean;
}

export async function listCodexAccounts(): Promise<CodexAccount[]> {
  return invoke('list_codex_accounts');
}

export async function captureCurrentCodexAccount(): Promise<CodexAccount> {
  return invoke('capture_current_codex_account');
}

export function startCodexLogin(): Promise<string> {
  return invoke('start_codex_login');
}

export function cancelCodexLogin(loginId: string): Promise<void> {
  return invoke('cancel_codex_login', { loginId });
}

export async function addCodexAccountViaOAuth(
  loginId: string,
  callbackMessages: {
    complete: string;
    closeWindow: string;
    failed: string;
  }
): Promise<CodexAccount> {
  return invoke('add_codex_account_via_oauth', { loginId, callbackMessages });
}

export async function switchCodexAccount(accountId: string): Promise<CodexAccount> {
  return invoke('switch_codex_account', { accountId });
}

export async function refreshCodexAccountQuota(accountId: string): Promise<CodexAccount> {
  return invoke('refresh_codex_account_quota', { accountId });
}

export async function deleteCodexAccount(accountId: string): Promise<void> {
  return invoke('delete_codex_account', { accountId });
}

// ─── Process APIs ───

export async function startTool(
  toolId: string,
  startCommand?: string,
  cwd?: string
): Promise<void> {
  return invoke('start_tool', {
    toolId,
    startCommand: startCommand || null,
    cwd: cwd ?? null,
  });
}

export interface CliLaunchSettings {
  preset: string;
  directoryMode: 'ask' | 'fixed';
  directory: string;
  terminal: string;
}

export interface CliLaunchOptions {
  terminals: string[];
  presets: Record<string, { id: string; command: string }[]>;
  coffeeTools: string[];
}

export async function getCliLaunchSettings(): Promise<Record<string, CliLaunchSettings>> {
  return invoke('get_cli_launch_settings');
}

export async function saveCliLaunchSettings(
  settings: Record<string, CliLaunchSettings>
): Promise<void> {
  return invoke('save_cli_launch_settings', { settings });
}

export async function getCliLaunchOptions(): Promise<CliLaunchOptions> {
  return invoke('get_cli_launch_options');
}

// ─── Self-update downloads and platform-specific completion ───

export interface SelfUpdateProgress {
  status: 'speed_test' | 'downloading' | 'launching' | 'downloaded' | 'error';
  percent: number;
}

/// Download to system Downloads. Windows launches its wizard and exits;
/// macOS/Linux open the containing folder and keep running.
export async function downloadAndInstallUpdate(version: string): Promise<void> {
  return invoke('download_and_install_update', { version });
}

/// Subscribe to self-update progress while downloadAndInstallUpdate runs.
export function onSelfUpdateProgress(
  callback: (data: SelfUpdateProgress) => void
): Promise<UnlistenFn> {
  return listen<SelfUpdateProgress>('self-update-progress', (event) => {
    callback(event.payload);
  });
}

// ─── Shell APIs (uses Tauri shell plugin) ───

export async function openExternal(url: string): Promise<void> {
  await openShell(url);
}

export async function openFolder(path: string): Promise<void> {
  await invoke('open_folder', { path });
}

/// Open (creating from a template on first use) the user's tool-path overrides
/// file `~/.echobird/tool-paths.json`, where users add install paths for tools
/// detected at non-default locations. Survives app updates; the scanner merges
/// it on top of bundled defaults. Returns the file's absolute path.
export async function openToolPathsConfig(): Promise<string> {
  return invoke('open_tool_paths_config');
}

// ─── App Settings APIs ───

export async function getSettings(): Promise<AppSettings> {
  return invoke('get_settings');
}

export async function saveSettings(settings: AppSettings): Promise<void> {
  return invoke('save_settings', { settings });
}

// ─── App Lifecycle APIs ───

export async function appReady(): Promise<void> {
  return invoke('app_ready');
}

export async function hideMainWindow(): Promise<void> {
  return invoke('hide_main_window');
}

/// Read the last `lines` lines from EchoBird's backend log file — used
/// by the "问题反馈 / Feedback" page's copy-to-clipboard button so users
/// can paste recent logs into a GitHub issue.
export async function readLogTail(lines: number): Promise<string> {
  return invoke<string>('read_log_tail', { lines });
}

// ─── Window APIs (Tauri built-in) ───

export { getCurrentWindow } from '@tauri-apps/api/window';

export interface ClaudeCodeAccount {
  id: string;
  email: string;
  plan?: string | null;
  fiveHour?: { remainingPercent: number; resetAt?: number | null } | null;
  sevenDay?: { remainingPercent: number; resetAt?: number | null } | null;
  active: boolean;
}

export async function listClaudeCodeAccounts(): Promise<ClaudeCodeAccount[]> {
  return invoke('list_claude_code_accounts');
}
export interface ClaudeCodeLogin {
  loginId: string;
  authorizationUrl: string;
  expiresAt: number;
}
export async function startClaudeCodeLogin(): Promise<ClaudeCodeLogin> {
  return invoke('start_claude_code_login');
}
export async function completeClaudeCodeLogin(
  loginId: string,
  code: string
): Promise<ClaudeCodeAccount> {
  return invoke('complete_claude_code_login', { loginId, code });
}
export async function cancelClaudeCodeLogin(loginId: string): Promise<void> {
  return invoke('cancel_claude_code_login', { loginId });
}
export async function switchClaudeCodeAccount(accountId: string): Promise<ClaudeCodeAccount> {
  return invoke('switch_claude_code_account', { accountId });
}
export async function refreshClaudeCodeAccountQuota(accountId: string): Promise<ClaudeCodeAccount> {
  return invoke('refresh_claude_code_account_quota', { accountId });
}

export async function warmUpClaudeCodeAccount(accountId: string): Promise<ClaudeCodeAccount> {
  return invoke('warm_up_claude_code_account', { accountId });
}
export async function deleteClaudeCodeAccount(accountId: string): Promise<void> {
  return invoke('delete_claude_code_account', { accountId });
}

export interface ClaudeDesktopLogin {
  loginId: string;
  expiresAt: number;
}
export interface ClaudeDesktopLoginPoll {
  account: ClaudeCodeAccount | null;
  awaitingClientExit: boolean;
  expiresAt: number | null;
  /** Reported by plugin 0.1.0-preview.10 and later once the sign-in is cleared. */
  cleared?: boolean;
}
// The plugin reports the shared "close the client" error; Claude Desktop must be
// quit from its tray because closing the window only hides it.
function withClaudeDesktopQuitError<T>(result: Promise<T>): Promise<T> {
  return result.catch((error: unknown) => {
    const raw = error instanceof Error ? error.message : String(error);
    throw raw.split('|', 1)[0] === 'accountError.closeClient'
      ? new Error('accountError.quitClaudeDesktop')
      : error;
  });
}
export function listClaudeDesktopAccounts(): Promise<ClaudeCodeAccount[]> {
  return invoke('list_claude_desktop_accounts');
}
export function startClaudeDesktopLogin(next = false): Promise<ClaudeDesktopLogin> {
  return withClaudeDesktopQuitError(invoke('start_claude_desktop_login', { next }));
}
export function pollClaudeDesktopLogin(loginId: string): Promise<ClaudeDesktopLoginPoll> {
  return withClaudeDesktopQuitError(invoke('poll_claude_desktop_login', { loginId }));
}
export function cancelClaudeDesktopLogin(loginId: string): Promise<void> {
  return invoke('cancel_claude_desktop_login', { loginId });
}
export function switchClaudeDesktopAccount(accountId: string): Promise<ClaudeCodeAccount> {
  return withClaudeDesktopQuitError(invoke('switch_claude_desktop_account', { accountId }));
}
export function refreshClaudeDesktopAccount(accountId: string): Promise<ClaudeCodeAccount> {
  return withClaudeDesktopQuitError(invoke('refresh_claude_desktop_account', { accountId }));
}

export function warmUpClaudeDesktopAccount(accountId: string): Promise<ClaudeCodeAccount> {
  return withClaudeDesktopQuitError(invoke('warm_up_claude_desktop_account', { accountId }));
}
export function deleteClaudeDesktopAccount(accountId: string): Promise<void> {
  return withClaudeDesktopQuitError(invoke('delete_claude_desktop_account', { accountId }));
}

export type ZCodeProvider = 'bigmodel' | 'zai';
export interface OpenDesignAccount {
  id: string;
  email: string;
  active: boolean;
  plan?: string | null;
  quotaWindows: { label: string; remainingPercent: number; resetAt?: number | null }[];
  balanceUsd?: number | null;
  subscriptionEndAt?: number | null;
}
export interface OpenDesignLogin {
  loginId: string;
  verificationUri: string;
  expiresAt: number;
  pollIntervalSeconds: number;
}
export function listOpenDesignAccounts(): Promise<OpenDesignAccount[]> {
  return invoke('list_opendesign_accounts');
}
export function startOpenDesignLogin(): Promise<OpenDesignLogin> {
  return invoke('start_opendesign_login');
}
export function pollOpenDesignLogin(loginId: string): Promise<OpenDesignAccount | null> {
  return invoke('poll_opendesign_login', { loginId });
}
export function cancelOpenDesignLogin(loginId: string): Promise<void> {
  return invoke('cancel_opendesign_login', { loginId });
}
export function switchOpenDesignAccount(accountId: string): Promise<OpenDesignAccount> {
  return invoke('switch_opendesign_account', { accountId });
}
export function refreshOpenDesignAccount(accountId: string): Promise<OpenDesignAccount> {
  return invoke('refresh_opendesign_account', { accountId });
}
export function deleteOpenDesignAccount(accountId: string): Promise<void> {
  return invoke('delete_opendesign_account', { accountId });
}
export interface ZCodeAccount {
  id: string;
  email: string;
  provider: ZCodeProvider;
  active: boolean;
  plan: string | null;
  subscriptionEndAt?: number | null;
  quotaWindows?: { remainingPercent: number; resetAt: number | null }[];
  remainingPercent: number | null;
  resetAt: number | null;
}
export interface ZCodeLogin {
  loginId: string;
  verificationUri: string;
  expiresAt: number;
  pollIntervalSeconds: number;
}
export function listZCodeAccounts(): Promise<ZCodeAccount[]> {
  return invoke('list_zcode_accounts');
}
export function startZCodeLogin(provider: ZCodeProvider): Promise<ZCodeLogin> {
  return invoke('start_zcode_login', { provider });
}
export function pollZCodeLogin(loginId: string): Promise<ZCodeAccount | null> {
  return invoke('poll_zcode_login', { loginId });
}
export function cancelZCodeLogin(loginId: string): Promise<void> {
  return invoke('cancel_zcode_login', { loginId });
}
export function switchZCodeAccount(accountId: string): Promise<ZCodeAccount> {
  return invoke('switch_zcode_account', { accountId });
}
export function refreshZCodeAccountQuota(accountId: string): Promise<ZCodeAccount> {
  return invoke('refresh_zcode_account_quota', { accountId });
}
export function deleteZCodeAccount(accountId: string): Promise<void> {
  return invoke('delete_zcode_account', { accountId });
}

export type KiroEdition = 'kirocrew' | 'kirocli';
export interface KiroAccount {
  id: string;
  edition: KiroEdition;
  name: string;
  plan?: string | null;
  active: boolean;
  quota: {
    label: string;
    remaining?: number | null;
    total?: number | null;
    resetAt?: number | null;
  }[];
}
export interface KiroLogin {
  loginId: string;
  verificationUri: string;
  expiresAt: number;
}
export async function listKiroAccounts(edition: KiroEdition): Promise<KiroAccount[]> {
  return invoke('list_kiro_accounts', { edition });
}
export async function startKiroLogin(edition: KiroEdition): Promise<KiroLogin> {
  return invoke('start_kiro_login', { edition });
}
export async function pollKiroLogin(loginId: string): Promise<KiroAccount | null> {
  return invoke('poll_kiro_login', { loginId });
}
export async function cancelKiroLogin(loginId: string): Promise<void> {
  return invoke('cancel_kiro_login', { loginId });
}
export async function switchKiroAccount(edition: KiroEdition, accountId: string): Promise<void> {
  return invoke('switch_kiro_account', { edition, accountId });
}
export async function refreshKiroAccountQuota(
  edition: KiroEdition,
  accountId: string
): Promise<KiroAccount> {
  return invoke('refresh_kiro_account_quota', { edition, accountId });
}
export async function deleteKiroAccount(edition: KiroEdition, accountId: string): Promise<void> {
  return invoke('delete_kiro_account', { edition, accountId });
}

export type TraeEdition = 'trae' | 'traecn';
export interface TraeAccount {
  id: string;
  edition: TraeEdition;
  name: string;
  plan: string | null;
  active: boolean;
  remaining?: number | null;
  workRemaining?: number | null;
  total?: number | null;
  currency?: string | null;
  expiresAt?: number | null;
  baseRemaining?: number | null;
  baseTotal?: number | null;
  baseResetAt?: number | null;
  rewardRemaining?: number | null;
  rewardTotal?: number | null;
  rewardNextExpiryAt?: number | null;
  addonRemaining?: number | null;
  dailyClaimedAt?: number | null;
  deviceClaimedAt?: number | null;
  rewardEnabled?: boolean | null;
  subscriptionEndAt?: number | null;
}
export interface TraeLogin {
  loginId: string;
  verificationUri: string;
  expiresAt: number;
}
export function listTraeAccounts(edition: TraeEdition): Promise<TraeAccount[]> {
  return invoke('list_trae_accounts', { edition });
}
export function startTraeLogin(edition: TraeEdition): Promise<TraeLogin> {
  return invoke('start_trae_login', { edition });
}
export function pollTraeLogin(loginId: string): Promise<TraeAccount | null> {
  return invoke('poll_trae_login', { loginId });
}
export function cancelTraeLogin(loginId: string): Promise<void> {
  return invoke('cancel_trae_login', { loginId });
}
export function switchTraeAccount(edition: TraeEdition, accountId: string): Promise<void> {
  return invoke('switch_trae_account', { edition, accountId });
}
export function refreshTraeAccountQuota(
  edition: TraeEdition,
  accountId: string
): Promise<TraeAccount> {
  return invoke('refresh_trae_account_quota', { edition, accountId });
}
export function claimTraeDailyCredits(
  edition: TraeEdition,
  accountId: string
): Promise<TraeAccount> {
  return invoke('claim_trae_daily_credits', { edition, accountId });
}
export function deleteTraeAccount(edition: TraeEdition, accountId: string): Promise<void> {
  return invoke('delete_trae_account', { edition, accountId });
}

export type WorkBuddyEdition = 'workbuddy' | 'workbuddyai';
export interface WorkBuddyAccount {
  id: string;
  name: string;
  edition: WorkBuddyEdition;
  plan: string | null;
  remaining: number | null;
  total: number | null;
  baseRemaining?: number | null;
  baseTotal?: number | null;
  baseResetAt?: number | null;
  rewardRemaining?: number | null;
  rewardTotal?: number | null;
  rewardNextExpiryAt?: number | null;
  addonRemaining?: number | null;
  dailyClaimedAt?: number | null;
  expiresAt: number | null;
  active: boolean;
}
export interface WorkBuddyLogin {
  loginId: string;
  verificationUri: string;
  expiresAt: number;
}
export function listWorkBuddyAccounts(edition: WorkBuddyEdition): Promise<WorkBuddyAccount[]> {
  return invoke('list_workbuddy_accounts', { edition });
}
export function startWorkBuddyLogin(edition: WorkBuddyEdition): Promise<WorkBuddyLogin> {
  return invoke('start_workbuddy_login', { edition });
}
export function pollWorkBuddyLogin(loginId: string): Promise<WorkBuddyAccount | null> {
  return invoke('poll_workbuddy_login', { loginId });
}
export function cancelWorkBuddyLogin(loginId: string): Promise<void> {
  return invoke('cancel_workbuddy_login', { loginId });
}
export function switchWorkBuddyAccount(
  edition: WorkBuddyEdition,
  accountId: string
): Promise<WorkBuddyAccount> {
  return invoke('switch_workbuddy_account', { edition, accountId });
}
export function refreshWorkBuddyAccountQuota(
  edition: WorkBuddyEdition,
  accountId: string
): Promise<WorkBuddyAccount> {
  return invoke('refresh_workbuddy_account_quota', { edition, accountId });
}
export function claimWorkBuddyDailyCredits(
  edition: WorkBuddyEdition,
  accountId: string
): Promise<WorkBuddyAccount> {
  return invoke('claim_workbuddy_daily_credits', { edition, accountId });
}
export function deleteWorkBuddyAccount(
  edition: WorkBuddyEdition,
  accountId: string
): Promise<void> {
  return invoke('delete_workbuddy_account', { edition, accountId });
}

export interface DeepSeekAccount {
  id: string;
  name: string;
  balances: { currency: 'CNY' | 'USD'; amount: number }[] | null;
  active: boolean;
}
export interface DeepSeekLogin {
  loginId: string;
  verificationUri: string;
  expiresAt: number;
}
export function listDeepSeekAccounts(): Promise<DeepSeekAccount[]> {
  return invoke('list_deepseek_accounts');
}
export function startDeepSeekLogin(locale: string): Promise<DeepSeekLogin> {
  return invoke('start_deepseek_login', { locale });
}
export function pollDeepSeekLogin(loginId: string): Promise<DeepSeekAccount | null> {
  return invoke('poll_deepseek_login', { loginId });
}
export function cancelDeepSeekLogin(loginId: string): Promise<void> {
  return invoke('cancel_deepseek_login', { loginId });
}
export function switchDeepSeekAccount(accountId: string, locale: string): Promise<DeepSeekAccount> {
  return invoke('switch_deepseek_account', { accountId, locale });
}
export function refreshDeepSeekAccountQuota(
  accountId: string,
  locale: string
): Promise<DeepSeekAccount> {
  return invoke('refresh_deepseek_account_quota', { accountId, locale });
}
export function deleteDeepSeekAccount(accountId: string): Promise<void> {
  return invoke('delete_deepseek_account', { accountId });
}

export interface GrokAccount {
  id: string;
  email: string;
  plan: string | null;
  active: boolean;
}
export interface GrokLogin {
  loginId: string;
  expiresAt: number;
}
export function startGrokLogin(): Promise<GrokLogin> {
  return invoke('start_grok_login');
}
export function pollGrokLogin(loginId: string): Promise<GrokAccount | null> {
  return invoke('poll_grok_login', { loginId });
}
export function cancelGrokLogin(loginId: string): Promise<void> {
  return invoke('cancel_grok_login', { loginId });
}
export function listGrokAccounts(): Promise<GrokAccount[]> {
  return invoke('list_grok_accounts');
}
export function switchGrokAccount(accountId: string): Promise<GrokAccount> {
  return invoke('switch_grok_account', { accountId });
}
export function deleteGrokAccount(accountId: string): Promise<void> {
  return invoke('delete_grok_account', { accountId });
}
export function refreshGrokAccount(accountId: string): Promise<GrokAccount> {
  return invoke('refresh_grok_account', { accountId });
}

export interface ManusAccount extends GrokAccount {
  subscriptionEndAt?: number | null;
  credits: {
    total: number;
    free: number | null;
    refresh: number | null;
    nextRefreshAt: number | null;
  } | null;
}
export interface ManusLogin extends GrokLogin {
  verificationUri?: string;
}
export interface ManusLoginPoll {
  account: ManusAccount | null;
  awaitingClientExit: boolean;
  expiresAt: number | null;
}
export function startManusLogin(): Promise<ManusLogin> {
  return invoke('start_manus_login');
}
export function pollManusLogin(loginId: string): Promise<ManusLoginPoll> {
  return invoke('poll_manus_login', { loginId });
}
export function cancelManusLogin(loginId: string): Promise<void> {
  return invoke('cancel_manus_login', { loginId });
}
export function listManusAccounts(): Promise<ManusAccount[]> {
  return invoke('list_manus_accounts');
}
export function switchManusAccount(accountId: string): Promise<ManusAccount> {
  return invoke('switch_manus_account', { accountId });
}
export function deleteManusAccount(accountId: string): Promise<void> {
  return invoke('delete_manus_account', { accountId });
}
export function refreshManusAccount(accountId: string): Promise<ManusAccount> {
  return invoke('refresh_manus_account', { accountId });
}

export interface CueAccount extends ManusAccount {
  weekly?: {
    remainingPercent: number | null;
    resetAt: number | null;
  } | null;
}
export interface CueLoginPoll extends Omit<ManusLoginPoll, 'account'> {
  account: CueAccount | null;
}
export function startCueLogin(): Promise<GrokLogin> {
  return invoke('start_cue_login');
}
export function pollCueLogin(loginId: string): Promise<CueLoginPoll> {
  return invoke('poll_cue_login', { loginId });
}
export function cancelCueLogin(loginId: string): Promise<void> {
  return invoke('cancel_cue_login', { loginId });
}
export function listCueAccounts(): Promise<CueAccount[]> {
  return invoke('list_cue_accounts');
}
export function switchCueAccount(accountId: string): Promise<CueAccount> {
  return invoke('switch_cue_account', { accountId });
}
export function deleteCueAccount(accountId: string): Promise<void> {
  return invoke('delete_cue_account', { accountId });
}
export function refreshCueAccount(accountId: string): Promise<CueAccount> {
  return invoke('refresh_cue_account', { accountId });
}

export interface CursorUsage {
  plan: string | null;
  remainingPercent: number | null;
  resetAt: number | null;
}
export interface GrokBotAccount {
  id: string;
  email: string;
  active: boolean;
  usage?: CursorUsage | null;
}
export interface GrokBotLogin {
  loginId: string;
  verificationUri: string;
  expiresAt: number;
}
export function listGrokBotAccounts(): Promise<GrokBotAccount[]> {
  return invoke('list_grok_bot_accounts');
}
export function startGrokBotLogin(): Promise<GrokBotLogin> {
  return invoke('start_grok_bot_login');
}
export function pollGrokBotLogin(loginId: string): Promise<GrokBotAccount | null> {
  return invoke('poll_grok_bot_login', { loginId });
}
export function cancelGrokBotLogin(loginId: string): Promise<void> {
  return invoke('cancel_grok_bot_login', { loginId });
}
export function switchGrokBotAccount(accountId: string): Promise<GrokBotAccount> {
  return invoke('switch_grok_bot_account', { accountId });
}
export function deleteGrokBotAccount(accountId: string): Promise<void> {
  return invoke('delete_grok_bot_account', { accountId });
}
export function refreshGrokBotAccount(accountId: string): Promise<CursorUsage> {
  return invoke('refresh_grok_bot_account', { accountId });
}

export interface AntigravityQuota {
  name: string;
  remainingPercent: number;
  resetAt: number | null;
}

export interface AntigravityAccount {
  id: string;
  email: string;
  active: boolean;
  plan: string | null;
  quotas: AntigravityQuota[];
}

export interface AntigravityLogin {
  loginId: string;
  verificationUri: string;
  expiresAt: number;
}

export function listAntigravityAccounts(): Promise<AntigravityAccount[]> {
  return invoke('list_antigravity_accounts');
}
export function startAntigravityLogin(): Promise<AntigravityLogin> {
  return invoke('start_antigravity_login');
}
export function pollAntigravityLogin(loginId: string): Promise<AntigravityAccount | null> {
  return invoke('poll_antigravity_login', { loginId });
}
export function cancelAntigravityLogin(loginId: string): Promise<void> {
  return invoke('cancel_antigravity_login', { loginId });
}
export function switchAntigravityAccount(accountId: string): Promise<AntigravityAccount> {
  return invoke('switch_antigravity_account', { accountId });
}
export function deleteAntigravityAccount(accountId: string): Promise<void> {
  return invoke('delete_antigravity_account', { accountId });
}
export function refreshAntigravityAccount(accountId: string): Promise<AntigravityAccount> {
  return invoke('refresh_antigravity_account', { accountId });
}

export interface CursorAccount {
  id: string;
  email: string;
  active: boolean;
  usage?: CursorUsage | null;
}
export interface CursorLogin {
  loginId: string;
  verificationUri: string;
  expiresAt: number;
}
export type CursorTool = 'cursor' | 'cursorcli';
export function listCursorAccounts(tool?: CursorTool): Promise<CursorAccount[]> {
  return invoke('list_cursor_accounts', tool ? { tool } : undefined);
}
export function startCursorLogin(tool?: CursorTool): Promise<CursorLogin> {
  return invoke('start_cursor_login', tool ? { tool } : undefined);
}
export function pollCursorLogin(loginId: string, tool?: CursorTool): Promise<CursorAccount | null> {
  return invoke('poll_cursor_login', { loginId, ...(tool ? { tool } : {}) });
}
export function cancelCursorLogin(loginId: string): Promise<void> {
  return invoke('cancel_cursor_login', { loginId });
}
export function switchCursorAccount(accountId: string, tool?: CursorTool): Promise<CursorAccount> {
  return invoke('switch_cursor_account', { accountId, ...(tool ? { tool } : {}) });
}
export function deleteCursorAccount(accountId: string): Promise<void> {
  return invoke('delete_cursor_account', { accountId });
}
export function refreshCursorAccount(accountId: string, tool?: CursorTool): Promise<CursorUsage> {
  return invoke('refresh_cursor_account', { accountId, ...(tool ? { tool } : {}) });
}
