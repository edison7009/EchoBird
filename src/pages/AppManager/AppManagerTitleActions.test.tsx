import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import * as api from '../../api/tauri';
import { useNavigationStore } from '../../stores/navigationStore';
import { AppManagerTitleActions } from './AppManagerComponents';

const { scanTools, setViewMode } = vi.hoisted(() => ({
  scanTools: vi.fn(),
  setViewMode: vi.fn(),
}));
vi.mock('./context', () => ({
  useAppManager: () => ({ scanTools, setViewMode, isScanning: false, viewMode: 'desktop' }),
}));
vi.mock('../../hooks/useI18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }));
vi.mock('../../api/tauri', () => ({ openToolPathsConfig: vi.fn().mockResolvedValue(undefined) }));
vi.mock('../../components', () => ({ getModelIcon: () => null }));

let renderer: ReactTestRenderer;
const buttons = () => renderer.root.findAllByType('button');
const button = (label: string) => buttons().find((node) => node.children.includes(label))!;
beforeEach(() => {
  vi.clearAllMocks();
  useNavigationStore.setState(useNavigationStore.getInitialState(), true);
});
afterEach(() => {
  act(() => renderer?.unmount());
  useNavigationStore.setState(useNavigationStore.getInitialState(), true);
});

it.each(['pets', 'plugins', 'usageIsland'] as const)(
  'selects the Apps %s tab without scanning or writing configuration',
  (tab) => {
    act(() => {
      renderer = create(<AppManagerTitleActions />);
    });
    expect(buttons().some((node) => node.children.includes(`developer.${tab}`))).toBe(true);
    act(() => useNavigationStore.getState().setDeveloperMode(true));
    expect(
      buttons().map((node) => node.children.filter((child) => typeof child === 'string').join(''))
    ).toEqual([
      'btn.editPaths',
      'btn.refresh',
      'aiDesktop.desktopView',
      'aiDesktop.installView',
      'developer.pets',
      'developer.plugins',
      'developer.usageIsland',
    ]);
    expect(buttons().flatMap((node) => node.findAllByType('svg'))).toHaveLength(2);
    expect(button('btn.refresh').findAllByType('svg')).toHaveLength(1);
    expect(button('btn.editPaths').findAllByType('svg')).toHaveLength(1);
    act(() => button(`developer.${tab}`).props.onClick());
    expect(
      buttons()
        .filter((node) => node.props['aria-pressed'] === true)
        .map((node) => node.children.join(''))
    ).toEqual([`developer.${tab}`]);
    expect(button('btn.refresh')).toBeUndefined();
    expect(button('btn.editPaths')).toBeUndefined();
    expect(useNavigationStore.getState()).toMatchObject({
      activePage: 'apps',
      appsTab: tab,
      developerMode: true,
      backHistory: ['apps'],
    });
    expect(scanTools).not.toHaveBeenCalled();
    expect(setViewMode).not.toHaveBeenCalled();
    expect(api.openToolPathsConfig).not.toHaveBeenCalled();
    act(() => useNavigationStore.getState().goBack());
    expect(useNavigationStore.getState().activePage).toBe('apps');
    expect(button(`developer.${tab}`).props['aria-pressed']).toBe(false);
    act(() => useNavigationStore.getState().setAppsTab('desktop'));
    act(() => button('btn.refresh').props.onClick());
    expect(scanTools).toHaveBeenCalledOnce();
  }
);
