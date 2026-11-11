import cueIcon from './assets/cue.png';
import type { LocalTool, ModelConfig } from '../api/types';
import type { ModelUsageData } from '../api/models';

// Curated public catalog data only. Never import detected paths, saved accounts or keys.
export const demoTools: LocalTool[] = [
  ['claudedesktop', 'Claude Desktop', 'svg'],
  ['chatgptdesktop', 'ChatGPT', 'svg'],
  ['antigravitydesktop', 'Antigravity', 'png'],
  ['cursor', 'Cursor', 'svg'],
  ['dsh', 'DeepSeek Harness', 'svg'],
  ['workbuddyai', 'WorkBuddy', 'png'],
  ['coffeecli', 'Coffee CLI', 'png'],
  ['cue', 'Cue', 'png'],
  ['grokbot', 'Grok Bot', 'png'],
  ['vscode', 'Visual Studio Code', 'svg'],
  ['claudecode', 'Claude Code', 'svg'],
  ['codex', 'Codex CLI', 'svg'],
  ['cursorcli', 'Cursor CLI', 'svg'],
  ['opencode', 'OpenCode', 'svg'],
].map(([id, name, extension]) => ({
  id,
  name,
  category: 'Desktop',
  installed: true,
  // Use the client's tight Cue artwork; the published catalog icon has legacy transparent padding.
  icon: id === 'cue' ? cueIcon : `/icons/tools/${id}.${extension}`,
}));

export const demoModels: ModelConfig[] = [
  { internalId: 'smart-router-demo', name: '所有 Agent 通吃组', baseUrl: '', apiKey: '' },
  { internalId: 'smart-router-coding', name: 'AI 编程工作组', baseUrl: '', apiKey: '' },
  {
    internalId: 'demo-deepseek',
    name: 'DeepSeek',
    modelId: 'deepseek-v4-pro',
    baseUrl: '',
    apiKey: '',
  },
  {
    internalId: 'demo-minimax',
    name: 'MiniMax',
    modelId: 'MiniMax',
    baseUrl: '',
    apiKey: '',
  },
];

export const demoAccounts = ['work@example.com', 'personal@example.com'];

// Illustrative remaining quota and reset offsets, recreated on each page load.
export const demoQuotas: Record<string, { percent: number; resetMinutes: number }> = {
  'work@example.com': { percent: 76, resetMinutes: 154 },
  'personal@example.com': { percent: 24, resetMinutes: 87 },
};

// Public website demonstration balances, unrelated to any real API account.
export const demoModelUsage: Record<string, ModelUsageData> = {
  'demo-deepseek': { quotas: [{ percentage: 0, resetAt: 0, balance: 13.92 }] },
  'demo-minimax': { quotas: [{ percentage: 0, resetAt: 0, balance: 28.6 }] },
};
