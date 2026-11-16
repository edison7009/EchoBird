import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, expect, it, vi } from 'vitest';
import { AccountSectionButton } from './AccountSectionPrimitives';
import { ClaudeCodeAccountSection } from './ClaudeCodeAccountSection';
import { AppManagerContext, type AppManagerContextType } from './context';

vi.mock('../../hooks/useAccountCapability', () => ({ useAccountCapability: () => true }));
vi.mock('../../hooks/useI18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }));

let renderer: ReactTestRenderer;
afterEach(() => act(() => renderer?.unmount()));

function mount(snapshotBusy = false, resetting = false) {
  const accounts = {
    accounts: [],
    selectedId: null,
    busy: snapshotBusy || resetting,
    snapshotBusy,
    resetting,
    remainingSeconds: 60,
    refreshing: new Set(),
    add: vi.fn(async () => {}),
    addNext: vi.fn(async () => {}),
    cancelLogin: vi.fn(),
  };
  const context = {
    claudeDesktopAccounts: accounts,
    setApplyError: vi.fn(),
  } as unknown as AppManagerContextType;
  act(() => {
    renderer = create(
      <AppManagerContext.Provider value={context}>
        <ClaudeCodeAccountSection desktop showDivider={false} />
      </AppManagerContext.Provider>
    );
  });
  return accounts;
}

it('shares one pill and isolates the snapshot and reset click targets', async () => {
  const accounts = mount();
  expect(renderer.root.findAllByType(AccountSectionButton)).toHaveLength(1);
  expect(accounts.add).not.toHaveBeenCalled();
  expect(accounts.addNext).not.toHaveBeenCalled();
  const snapshot = renderer.root.findByProps({ 'aria-label': 'agent.accountSnapshot' });
  const reset = renderer.root.findByProps({ 'aria-label': 'agent.accountSnapshotNext' });
  expect(snapshot.type).toBe('button');
  expect(reset.type).toBe('button');
  expect(snapshot.findAllByType('button')).toHaveLength(1);
  expect(snapshot.props.className).toContain('absolute inset-0');
  const stopPropagation = vi.fn();
  await act(async () => reset.props.onClick({ stopPropagation }));
  expect(stopPropagation).toHaveBeenCalledOnce();
  expect(accounts.addNext).toHaveBeenCalledOnce();
  expect(accounts.add).not.toHaveBeenCalled();
  await act(async () => snapshot.props.onClick());
  expect(accounts.add).toHaveBeenCalledOnce();
  expect(accounts.addNext).toHaveBeenCalledOnce();
});

it('disables the second line during snapshot capture and keeps the existing cancel action', () => {
  const accounts = mount(true);
  expect(renderer.root.findAllByType(AccountSectionButton)).toHaveLength(1);
  expect(
    renderer.root.findByProps({ 'aria-label': 'agent.accountSnapshotNext' }).props.disabled
  ).toBe(true);
  const cancel = renderer.root.findByProps({ 'aria-label': 'btn.cancel' });
  act(() => cancel.props.onClick({ stopPropagation: vi.fn() }));
  expect(accounts.cancelLogin).toHaveBeenCalledOnce();
  expect(accounts.addNext).not.toHaveBeenCalled();
  expect(accounts.add).not.toHaveBeenCalled();
});

it('disables both click targets during reset and preserves its second-line progress', () => {
  mount(false, true);
  for (const label of ['agent.accountSnapshot', 'agent.accountSnapshotNext']) {
    expect(renderer.root.findByProps({ 'aria-label': label }).props.disabled).toBe(true);
  }
  expect(JSON.stringify(renderer.toJSON())).toContain('agent.accountResetting');
  expect(renderer.root.findAllByProps({ 'aria-label': 'btn.cancel' })).toHaveLength(0);
});
