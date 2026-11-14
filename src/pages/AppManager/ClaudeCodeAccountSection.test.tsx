import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it, vi } from 'vitest';
import { ClaudeCodeAccountSection } from './ClaudeCodeAccountSection';
import { AppManagerContext, type AppManagerContextType } from './context';
import type { ClaudeCodeAccount } from '../../api/tauri';

const plugin = vi.hoisted(() => ({ installed: true }));
vi.mock('../../hooks/useAccountPlugin', () => ({ useAccountPlugin: () => plugin.installed }));

function renderAccount(
  quotaPercent: number | null,
  busy = false,
  desktop = false,
  awaitingExit = true,
  overrides: Partial<ClaudeCodeAccount> = {}
) {
  const context = {
    claudeCodeAccounts: {
      accounts: [
        {
          id: 'test',
          email: 'test@example.com',
          plan: 'Max 5X',
          fiveHour: quotaPercent == null ? null : { remainingPercent: quotaPercent },
          sevenDay: { remainingPercent: 82 },
          ...overrides,
        },
      ],
      selectedId: 'test',
      busy,
      remainingSeconds: 60,
      refreshing: new Set(),
      authorizationFailedIds: new Set(),
      select: () => {},
      add: async () => {},
      refresh: async () => {},
      remove: async () => {},
    },
  } as unknown as AppManagerContextType;
  context.claudeDesktopAccounts = {
    ...context.claudeCodeAccounts,
    awaitingClientExit: desktop && busy && awaitingExit,
    addNext: async () => {},
  } as AppManagerContextType['claudeDesktopAccounts'];
  return renderToStaticMarkup(
    <AppManagerContext.Provider value={context}>
      <ClaudeCodeAccountSection showDivider={false} desktop={desktop} />
    </AppManagerContext.Provider>
  );
}

describe('Claude Code account card', () => {
  it('keeps the compact account layout without tooltips or cursor overrides', () => {
    const markup = renderAccount(37);
    expect(markup).toContain('test@example.com');
    expect(markup).toContain('Max 5X');
    expect(markup).toContain('37%');
    expect(markup).toContain('82%');
    expect(markup).toContain('items-center gap-0.5 text-green-500');
    expect(markup).not.toContain('5h:');
    expect(markup).not.toContain('7d:');
    expect(markup).not.toContain('w-[64px]');
    expect(markup).not.toContain('width:');
    expect(markup).not.toContain('h-1.5');
    expect(markup).toContain('aria-checked="true"');
    expect(markup).toContain('grid h-14');
    expect(markup).not.toContain('title=');
    expect(markup).not.toContain('tooltip');
    expect(markup).not.toContain('cursor-');
    expect(markup).not.toContain('dialog');
    expect(markup).not.toContain('waitingForBrowser');
  });

  it('distinguishes unavailable quota from an exhausted account', () => {
    expect(renderAccount(null)).not.toContain('—');
    expect(renderAccount(null)).toContain('82%');
    expect(renderAccount(null)).not.toContain('0%');
    expect(renderAccount(0)).toContain('0%');
    expect(renderAccount(0)).toContain('5h');
    expect(renderAccount(0)).toContain('7d');
    expect(renderAccount(0)).toContain('·');
  });
  it('shows Free and one unavailable-data status instead of three placeholders', () => {
    const markup = renderAccount(null, false, true, true, { plan: 'Free', sevenDay: null });
    expect(markup).toContain('Free');
    expect(markup).toContain('model.noUsageData');
    expect(markup).not.toContain('—');
    expect(markup).not.toContain('0%');
    const text = markup.replace(/<[^>]*>/g, '');
    expect(text).not.toContain('5h');
    expect(text).not.toContain('7d');
  });

  it('disables adding and shows the existing waiting label during OAuth', () => {
    const markup = renderAccount(37, true);
    expect(markup).toContain('aria-label="btn.cancel"');
    expect(markup).toContain('agent.waitingForBrowser');
    expect(markup).not.toContain('agent.addCurrentAccount');
  });
  it('reuses the Claude layout while saving a desktop snapshot', () => {
    const markup = renderAccount(null, true, true);
    expect(markup).toContain('/icons/tools/claudedesktop.svg');
    expect(markup).toContain('account-pill');
    expect(markup).not.toContain('claude-account-pill');
    expect(markup).toContain('agent.accountSnapshotSaving');
    expect(markup).not.toContain('agent.waitingForBrowser');
    expect(markup).not.toContain('agent.claudeDesktopExitClient');
    expect(markup).not.toContain('—');
    expect(markup).toContain('82%');
    expect(markup).not.toContain('5h:');
    expect(markup).toContain('grid h-14');
    expect(markup.match(/Max 5X/g)).toHaveLength(1);
    expect(markup).not.toContain('cursor-');
    expect(markup).not.toContain('title=');
  });
  it('labels Claude desktop as account snapshots and explains them with the shared hint', () => {
    const desktop = renderAccount(37, false, true);
    expect(desktop).toContain('agent.accountSnapshot');
    expect(desktop).toContain('aria-label="agent.accountSnapshotHint"');
    expect(desktop).not.toContain('plugins.category.account');
    expect(desktop).not.toContain('title=');
    expect(desktop).not.toContain('cursor-');
    // The hint sits beside, not inside, the button that saves the snapshot.
    expect(desktop).not.toMatch(/<button[^>]*>(?:(?!<\/button>).)*agent\.accountSnapshotHint/s);
    expect(desktop).toContain('agent.accountSnapshotNext');
    expect(renderAccount(37, true, true)).not.toContain('agent.accountSnapshotNext');
    const code = renderAccount(37);
    expect(code).not.toContain('agent.accountSnapshotNext');
    expect(code).toContain('plugins.category.account');
    expect(code).not.toContain('agent.accountSnapshot');
  });
  it('keeps the snapshot label but drops its explanation while the plugin is missing', () => {
    plugin.installed = false;
    try {
      const markup = renderAccount(37, false, true);
      expect(markup).toContain('agent.accountSnapshot');
      expect(markup).not.toContain('agent.accountSnapshotHint');
    } finally {
      plugin.installed = true;
    }
  });
});
