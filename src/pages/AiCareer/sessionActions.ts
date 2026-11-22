const RESUME_COMMANDS: Readonly<Record<string, string>> = {
  claude: 'claude --resume',
  codex: 'codex resume',
  opencode: 'opencode --session',
  hermes: 'hermes --resume',
  mimo: 'mimo --session',
  deepseek: 'dsh --profile tui --resume',
  antigravity: 'agy --conversation',
};

export function sessionResumeCommand(tool: string, token: string | null): string | null {
  const command = RESUME_COMMANDS[tool];
  if (!command || !token || !/^[A-Za-z0-9_][A-Za-z0-9_-]*$/.test(token)) return null;
  return `${command} ${token}`;
}

export function sessionProject(cwd: string): string {
  const parts = cwd.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] || '—';
}
