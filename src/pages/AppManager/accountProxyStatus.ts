import type { AccountProvider } from '../../api/types';
import type { AppManagerContextType } from './context';

export interface AccountProxyStatus {
  accounts: Array<{ id: string }>;
  authorizationFailedIds: Set<string>;
  add: () => unknown;
  busy: boolean;
  remainingSeconds: number;
}

/**
 * Resolve the shared account lifecycle owned by AppManager for a Smart Access
 * account source. Keep this mapping in one place so reauthorization and failure
 * state cannot drift between Account Center and the router.
 */
export function accountProxyStatus(
  context: AppManagerContextType,
  provider: AccountProvider
): AccountProxyStatus {
  switch (provider) {
    case 'cursor':
      return context.cursorAccounts;
    case 'grokbot':
      return context.grokBotAccounts;
    case 'doubaowork':
    case 'dola':
      return context.doubaoAccountGroups[provider];
    case 'dumate':
      return context.dumateAccounts;
    case 'commandcode':
      return context.commandCodeAccounts;
    case 'claudeCode':
      return context.claudeCodeAccounts;
    case 'codex':
      return {
        accounts: context.codexAccounts,
        authorizationFailedIds: context.codexAuthorizationFailedIds,
        add: context.addCodexAccount,
        busy: context.isAddingCodexAccount,
        remainingSeconds: context.codexOAuthRemainingSeconds,
      };
    case 'workBuddy':
      return context.workBuddyAccountGroups.workbuddy;
    case 'workBuddyAi':
      return context.workBuddyAccountGroups.workbuddyai;
    case 'antigravity':
      return context.antigravityAccounts;
    case 'kiro':
      return context.kiroAccounts;
    case 'copilot':
      return context.copilotAccounts;
    case 'kilo':
      return context.kiloAccounts;
    case 'trae':
      return context.traeAccountGroups.trae;
    case 'traecn':
      return context.traeAccountGroups.traecn;
    case 'minimax':
      return context.minimaxAccounts;
    case 'qoder':
      return context.qoderAccountGroups.qoder;
    case 'qoderCn':
      return context.qoderAccountGroups.qodercn;
    case 'qwenwork':
    case 'qwenworkcn':
      return context.qwenWorkAccountGroups[provider];
    case 'grok':
      return context.grokAccounts;
    case 'cline':
      return context.clineAccounts;
    case 'zcode':
      return context.zcodeAccounts;
    case 'kimi':
      return context.kimiAccounts;
    case 'openDesign':
      return context.openDesignAccounts;
    case 'codeArts':
      return context.codeArtsAccounts;
    case 'devin':
      return context.devinAccounts;
    default:
      throw new Error('Unsupported account source');
  }
}
