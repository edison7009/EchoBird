import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { DeveloperTitleActions } from './Developer';
import { useNavigationStore as navigation } from '../stores/navigationStore';

vi.mock('../hooks/useI18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }));

let view: ReactTestRenderer;
beforeEach(() => navigation.setState(navigation.getInitialState(), true));
afterEach(() => act(() => view?.unmount()));

it('uses the shared title tabs and preserves selection when revisiting the developer page', () => {
  navigation.getState().setDeveloperMode(true);
  navigation.getState().setActivePage('developer');
  act(() => {
    view = create(<DeveloperTitleActions />);
  });
  expect(view.root.findAllByType('button').map((node) => node.props.children)).toEqual([
    'developer.pets',
    'developer.plugins',
    'developer.statistics',
    'developer.usageIsland',
  ]);
  act(() => view.root.findAllByType('button')[2].props.onClick());
  navigation.getState().setActivePage('apps');
  navigation.getState().setActivePage('developer');
  expect(view.root.findAllByType('button')[2].props['aria-pressed']).toBe(true);
});

it.each(['pets', 'plugins'] as const)(
  'switches to %s without loading or writing settings',
  (tab) => {
    const fetch = vi.fn();
    vi.stubGlobal('fetch', fetch);
    navigation.getState().setDeveloperTab(tab);
    act(() => {
      view = create(<DeveloperTitleActions />);
    });
    expect(JSON.stringify(view.toJSON())).not.toContain('developer.comingSoon');
    expect(navigation.getState().developerTab).toBe(tab);
    expect(fetch).not.toHaveBeenCalled();
    vi.unstubAllGlobals();
  }
);
