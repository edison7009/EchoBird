import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { ClaudeCodeLoginDialog } from './ClaudeCodeLoginDialog';

describe('Claude Code authorization dialog', () => {
  it('uses the existing centered input dialog without extra copy or cursor overrides', () => {
    const markup = renderToStaticMarkup(
      <ClaudeCodeLoginDialog
        submitting={false}
        error={null}
        onClose={() => {}}
        onSubmit={async () => {}}
      />
    );
    expect(markup).toContain('items-center justify-center');
    expect(markup).toContain('w-[450px]');
    expect(markup).toContain('role="dialog"');
    expect(markup).toContain('agent.authorizationCode');
    expect(markup).toContain('model.paste');
    expect(markup).toContain('common.confirm');
    expect(markup.match(/<input /g)).toHaveLength(1);
    expect(markup).not.toContain('title=');
    expect(markup).not.toContain('tooltip');
    expect(markup).not.toContain('cursor-');
    expect(markup).not.toMatch(/<p[ >]/);
    expect(markup).toMatch(/type="submit" disabled=""/);
  });
});
