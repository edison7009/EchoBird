import { act, create } from 'react-test-renderer';
import { expect, it, vi } from 'vitest';
import { DeepSeekAccountSection } from './DeepSeekAccountSection';
import { AppManagerContext, type AppManagerContextType } from './context';
it('keeps single-line refresh/delete actions isolated from account selection', () => {
  const account = { id: 'one', name: 'DeepSeek user', balances: null, active: true };
  const select = vi.fn();
  const refresh = vi.fn();
  const remove = vi.fn();
  const context = {
    deepSeekAccounts: {
      accounts: [account],
      selectedId: 'one',
      refreshing: new Set(['one']),
      authorizationFailedIds: new Set(),
      select,
      refresh,
      remove,
    },
  } as unknown as AppManagerContextType;
  const renderer = create(
    <AppManagerContext.Provider value={context}>
      <DeepSeekAccountSection showDivider={false} />
    </AppManagerContext.Provider>
  );
  const row = renderer.root.findByProps({ role: 'radio' });
  const [refreshButton, deleteButton] = row.findAllByType('button');
  expect(refreshButton.props.disabled).toBe(true);
  const stopPropagation = vi.fn();
  act(() => deleteButton.props.onClick({ stopPropagation }));
  expect(stopPropagation).toHaveBeenCalledOnce();
  expect(remove).toHaveBeenCalledWith(account);
  expect(refresh).not.toHaveBeenCalled();
  context.deepSeekAccounts.refreshing.clear();
  act(() => {
    renderer.update(
      <AppManagerContext.Provider value={context}>
        <DeepSeekAccountSection showDivider={false} />
      </AppManagerContext.Provider>
    );
  });
  expect(refreshButton.props.disabled).toBe(false);
  act(() => refreshButton.props.onClick({ stopPropagation }));
  expect(refresh).toHaveBeenCalledWith(account);
  expect(stopPropagation).toHaveBeenCalledTimes(2);
  const target = {};
  act(() => {
    row.props.onKeyDown({ key: 'Enter', target, currentTarget: {}, preventDefault: vi.fn() });
  });
  expect(select).not.toHaveBeenCalled();
  act(() => row.props.onClick());
  expect(select).toHaveBeenCalledWith('one');
  act(() => renderer.unmount());
});
