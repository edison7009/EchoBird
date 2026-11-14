import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { describe, expect, it, vi } from 'vitest';
import { WorkBuddyAccountSection } from './WorkBuddyAccountSection';
import { AppManagerContext, type AppManagerContextType } from './context';
import type { WorkBuddyAccount } from '../../api/tauri';

vi.mock('../../hooks/useI18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }));

function accountContent(overrides: Partial<WorkBuddyAccount>) {
  const account: WorkBuddyAccount = {
    id: 'reward',
    name: 'Reward',
    edition: 'workbuddy',
    plan: 'Free',
    active: true,
    remaining: 600,
    total: 600,
    baseRemaining: 500,
    baseTotal: 500,
    rewardRemaining: 100,
    rewardTotal: 100,
    expiresAt: null,
    ...overrides,
  };
  const value = {
    selectedTool: 'workbuddy',
    workBuddyAccounts: {
      accounts: [account],
      selectedId: 'reward',
      busy: false,
      remainingSeconds: 0,
      refreshing: new Set(),
      authorizationFailedIds: new Set(),
      select: vi.fn(),
      add: vi.fn(),
      refresh: vi.fn(),
      remove: vi.fn(),
      claimDaily: vi.fn(),
    },
  } as unknown as AppManagerContextType;
  return (
    <AppManagerContext.Provider value={value}>
      <WorkBuddyAccountSection showDivider={false} />
    </AppManagerContext.Provider>
  );
}

describe('WorkBuddy reward expiry', () => {
  it.each([false, true])(
    'keeps the reward expiry after the claim state (claimed=%s) and hides it at expiry',
    async (claimed) => {
      vi.useFakeTimers();
      vi.setSystemTime(new Date('2026-10-22T00:00:00Z'));
      const now = Date.now() / 1000;
      let renderer!: ReactTestRenderer;
      try {
        act(() => {
          renderer = create(
            accountContent({
              baseResetAt: now + 10 * 86400,
              rewardNextExpiryAt: now + 3600,
              dailyClaimedAt: claimed ? now : null,
            })
          );
        });
        expect(
          renderer.root.findByProps({ 'aria-label': 'agent.baseCreditsReset 10d0h' })
        ).toBeDefined();
        expect(
          renderer.root.findByProps({ 'aria-label': 'agent.rewardCreditsNextExpiry 1h0m' })
        ).toBeDefined();
        const rendered = JSON.stringify(renderer.toJSON());
        const before = claimed
          ? '"aria-label":"agent.rewardCredits"'
          : '"aria-label":"agent.dailyCreditsUnclaimed"';
        expect(rendered.indexOf(before)).toBeGreaterThan(-1);
        expect(rendered.indexOf(before)).toBeLessThan(
          rendered.indexOf('agent.rewardCreditsNextExpiry 1h0m')
        );
        await act(async () => {
          await vi.advanceTimersByTimeAsync(3600_000);
        });
        expect(
          renderer.root.findAllByProps({ 'aria-label': 'agent.rewardCreditsNextExpiry 0m' })
        ).toHaveLength(0);
        expect(JSON.stringify(renderer.toJSON())).not.toContain('agent.rewardCreditsNextExpiry');
      } finally {
        if (renderer) act(() => renderer.unmount());
        vi.useRealTimers();
      }
    }
  );
});
