import { invoke } from '@tauri-apps/api/core';

export interface CommandCodeAccount {
  id: string;
  email: string;
  name?: string;
  plan?: string | null;
  remaining?: number | null;
  total?: number | null;
  active: boolean;
}
export interface CommandCodeLogin {
  loginId: string;
  authorizationUrl: string;
  expiresAt: number;
}
export const listCommandCodeAccounts = () =>
  invoke<CommandCodeAccount[]>('list_commandcode_accounts');
export const startCommandCodeLogin = () => invoke<CommandCodeLogin>('start_commandcode_login');
export const pollCommandCodeLogin = (loginId: string) =>
  invoke<CommandCodeAccount | null>('poll_commandcode_login', { loginId });
export const cancelCommandCodeLogin = (loginId: string) =>
  invoke<void>('cancel_commandcode_login', { loginId });
export const refreshCommandCodeAccount = (accountId: string) =>
  invoke<CommandCodeAccount>('refresh_commandcode_account', { accountId });
export const switchCommandCodeAccount = (accountId: string) =>
  invoke<CommandCodeAccount>('switch_commandcode_account', { accountId });
export const deleteCommandCodeAccount = (accountId: string) =>
  invoke<void>('delete_commandcode_account', { accountId });
