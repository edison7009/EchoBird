export const CAREER_FAMILIES = [
  { id: 'claude', name: 'Claude', toolId: 'claudecode', format: 'svg' },
  { id: 'codex', name: 'Codex', toolId: 'codex', format: 'svg' },
  { id: 'opencode', name: 'OpenCode', toolId: 'opencode', format: 'svg' },
  { id: 'deepseek', name: 'DeepSeek', toolId: 'dsh', format: 'svg' },
  { id: 'hermes', name: 'Hermes', toolId: 'hermes', format: 'png' },
  { id: 'mimo', name: 'MiMo', toolId: 'mimocode', format: 'png' },
  { id: 'antigravity', name: 'Antigravity', toolId: 'antigravitydesktop', format: 'png' },
  { id: 'workbuddy', name: 'WorkBuddy', toolId: 'workbuddy', format: 'png' },
] as const;

export type AiCareerFamily = (typeof CAREER_FAMILIES)[number]['id'];
