import { describe, expect, it } from 'vitest';
import { sessionProject, sessionResumeCommand } from './sessionActions';

describe('career session actions', () => {
  it.each([
    ['claude', 'claude --resume'],
    ['codex', 'codex resume'],
    ['opencode', 'opencode --session'],
    ['hermes', 'hermes --resume'],
    ['mimo', 'mimo --session'],
    ['deepseek', 'dsh --profile tui --resume'],
    ['antigravity', 'agy --conversation'],
  ])('uses the native resume command for %s', (tool, prefix) => {
    expect(sessionResumeCommand(tool, 'native_id-123')).toBe(`${prefix} native_id-123`);
  });

  it.each([null, '', 'id --flag', 'id;echo', 'id&echo', '$(echo)', 'id\nother', '-option'])(
    'does not generate an unsafe resume command for %j',
    (token) => {
      expect(sessionResumeCommand('codex', token)).toBeNull();
    }
  );

  it.each(['unknown', 'antigravitydesktop', 'workbuddy', 'workbuddyai'])(
    'does not invent a terminal command for %s',
    (tool) => expect(sessionResumeCommand(tool, 'id')).toBeNull()
  );

  it.each([
    ['C:\\Projects\\中文项目\\', '中文项目'],
    ['/home/user/project/', 'project'],
    ['\\\\server\\share\\project', 'project'],
    ['', '—'],
  ])('uses the native working directory basename %s', (cwd, name) => {
    expect(sessionProject(cwd)).toBe(name);
  });
});
