import React from 'react';
import { act, create } from 'react-test-renderer';
import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  AccountSectionButton,
  AccountSectionRewardButton,
  AccountSectionRow,
  AccountSectionWarmUpButton,
} from './AccountSectionPrimitives';
import { AccountPrivacyButton } from '../../components/AccountPrivacyButton';
import { useAccountPrivacyStore } from '../../stores/accountPrivacyStore';

vi.mock('../../hooks/useI18n', () => ({
  useI18n: () => ({
    t: (key: string) =>
      key === 'agent.waitingForBrowser' ? 'Waiting for browser ({seconds})' : key,
  }),
}));

describe('account warm-up button', () => {
  it('sits left of refresh, warms up once and never selects the row', () => {
    const select = vi.fn();
    const warmUp = vi.fn();
    const renderer = create(
      <AccountSectionRow
        selected={false}
        email="idle@example.test"
        onSelect={select}
        onDelete={() => {}}
        onRefresh={() => {}}
        leadingAction={<AccountSectionWarmUpButton email="idle@example.test" onWarmUp={warmUp} />}
      />
    );
    try {
      const buttons = renderer.root.findAllByType('button');
      expect(buttons.map((button) => button.props['aria-label'])).toEqual([
        'agent.warmUpAccount idle@example.test',
        'agent.refreshAccount idle@example.test',
        'btn.delete idle@example.test',
      ]);
      const guard = renderer.root.findAll(
        (node) => node.type === 'span' && typeof node.props.onClick === 'function'
      )[0];
      const stopPropagation = vi.fn();
      act(() => buttons[0].props.onClick());
      act(() => guard.props.onClick({ stopPropagation }));
      expect(warmUp).toHaveBeenCalledTimes(1);
      expect(stopPropagation).toHaveBeenCalledTimes(1);
      expect(select).not.toHaveBeenCalled();
    } finally {
      act(() => renderer.unmount());
    }
  });

  it('disables while pending and hides the account name in privacy mode', () => {
    useAccountPrivacyStore.setState({ hidden: true });
    const renderer = create(
      <AccountSectionWarmUpButton email="idle@example.test" disabled onWarmUp={() => {}} />
    );
    try {
      const button = renderer.root.findByType('button');
      expect(button.props.disabled).toBe(true);
      expect(button.props['aria-label']).not.toContain('idle@example.test');
    } finally {
      act(() => renderer.unmount());
      useAccountPrivacyStore.setState({ hidden: false });
    }
  });
});

describe('account login cancellation button', () => {
  it.each([undefined, 'Please quit the Cue app ({seconds})'])(
    'cancels %s without starting another login, even when adding is disabled',
    (waitingLabel) => {
      const add = vi.fn();
      const cancel = vi.fn();
      const renderer = create(
        <AccountSectionButton
          iconSrc="/icons/tools/cue.png"
          busy
          disabled
          remainingSeconds={58}
          waitingLabel={waitingLabel}
          onClick={add}
          onCancel={cancel}
        />
      );
      try {
        expect(JSON.stringify(renderer.toJSON())).toContain('58');
        const button = renderer.root.findByType('button');
        expect(button.props['aria-label']).toBe('btn.cancel');
        expect(button.props.disabled).not.toBe(true);
        const stopPropagation = vi.fn();
        act(() => button.props.onClick({ stopPropagation }));
        expect(cancel).toHaveBeenCalledTimes(1);
        expect(stopPropagation).toHaveBeenCalledTimes(1);
        expect(add).not.toHaveBeenCalled();
      } finally {
        act(() => renderer.unmount());
      }
    }
  );

  it('keeps the ordinary add action when idle and preserves the provider choice while waiting', () => {
    const add = vi.fn();
    const cancel = vi.fn();
    const props = {
      iconSrc: '/icons/tools/zcode.png',
      remainingSeconds: 60,
      onClick: add,
      onCancel: cancel,
    };
    const renderer = create(<AccountSectionButton {...props} busy={false} />);
    try {
      const addButton = renderer.root.findByType('button');
      expect(addButton.props.disabled).toBeFalsy();
      act(() => addButton.props.onClick());
      expect(add).toHaveBeenCalledTimes(1);
      expect(cancel).not.toHaveBeenCalled();
      act(() =>
        renderer.update(
          <AccountSectionButton {...props} busy secondary={<button disabled>Z.ai</button>} />
        )
      );
      expect(JSON.stringify(renderer.toJSON())).toContain('Z.ai');
      expect(renderer.root.findByProps({ 'aria-label': 'btn.cancel' }).props.disabled).not.toBe(
        true
      );
      expect(renderer.root.findAllByType('button')).toHaveLength(2);
    } finally {
      act(() => renderer.unmount());
    }
  });
});

describe('desktop account privacy', () => {
  afterEach(() => {
    useAccountPrivacyStore.setState({ hidden: false });
  });

  it.each([false, true])(
    'follows the shared button without changing layout, selection or real actions (single line: %s)',
    (singleLine) => {
      const account = { id: 'saved-account', email: 'private@example.test' };
      const select = vi.fn();
      const refresh = vi.fn();
      const remove = vi.fn();
      const claim = vi.fn();
      let renderer!: ReturnType<typeof create>;
      act(() => {
        renderer = create(
          <>
            <AccountPrivacyButton />
            <AccountSectionRow
              email={account.email}
              selected
              singleLine={singleLine}
              plan="Pro 100"
              secondary={<span>69%</span>}
              onSelect={() => select(account.id)}
              onRefresh={() => refresh(account)}
              onDelete={() => remove(account)}
              leadingAction={
                <AccountSectionRewardButton
                  email={account.email}
                  claimed={false}
                  onClaim={() => claim(account)}
                />
              }
            />
          </>
        );
      });
      try {
        const initial = JSON.stringify(renderer.toJSON());
        act(() =>
          renderer.root.findByProps({ 'aria-label': 'freeModels.hideAccountNames' }).props.onClick()
        );
        const hidden = JSON.stringify(renderer.toJSON());
        expect(hidden).not.toContain(account.email);
        expect(hidden).toContain('******');
        expect(hidden).toContain('69%');
        if (!singleLine) expect(hidden).toContain('Pro 100');
        expect(renderer.root.findByProps({ role: 'radio' }).props['aria-checked']).toBe(true);
        for (const action of [select, refresh, remove, claim])
          expect(action).not.toHaveBeenCalled();
        for (const [label, action] of [
          ['agent.refreshAccount', refresh],
          ['btn.delete', remove],
          ['agent.claimDailyCredits', claim],
        ] as const) {
          const stopPropagation = vi.fn();
          act(() =>
            renderer.root
              .findByProps({ 'aria-label': `${label} ******` })
              .props.onClick({ stopPropagation })
          );
          expect(stopPropagation).toHaveBeenCalledOnce();
          expect(action).toHaveBeenCalledExactlyOnceWith(account);
          expect(select).not.toHaveBeenCalled();
        }
        act(() => renderer.root.findByProps({ role: 'radio' }).props.onClick());
        expect(select).toHaveBeenCalledExactlyOnceWith(account.id);
        act(() =>
          renderer.root.findByProps({ 'aria-label': 'freeModels.showAccountNames' }).props.onClick()
        );
        expect(JSON.stringify(renderer.toJSON())).toBe(initial);
        expect(account).toEqual({ id: 'saved-account', email: 'private@example.test' });
      } finally {
        act(() => renderer.unmount());
      }
    }
  );

  it('keeps hidden names on remount while refresh is pending or authorization has failed', () => {
    useAccountPrivacyStore.setState({ hidden: true });
    const renderer = create(
      <AccountSectionRow
        email="private@example.test"
        selected={false}
        refreshing
        authorizationFailed
        onSelect={vi.fn()}
        onRefresh={vi.fn()}
        onDelete={vi.fn()}
      />
    );
    try {
      const hidden = JSON.stringify(renderer.toJSON());
      expect(hidden).not.toContain('private@example.test');
      expect(hidden).toContain('accountCenter.authFailed');
      expect(
        renderer.root.findByProps({ 'aria-label': 'agent.refreshAccount ******' }).props.disabled
      ).toBe(true);
      expect(renderer.root.findAllByType(AccountPrivacyButton)).toHaveLength(0);
    } finally {
      act(() => renderer.unmount());
    }
  });
});
