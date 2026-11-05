import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { TitleBar } from './TitleBar';
import { useSidebarStore } from '../stores/sidebarStore';
import { useNavigationStore as navigation } from '../stores/navigationStore';

const windowApi = vi.hoisted(() => ({
  isMaximized: vi.fn(async () => false),
  onResized: vi.fn(async () => vi.fn()),
  startDragging: vi.fn(async () => {}),
  minimize: vi.fn(async () => {}),
  maximize: vi.fn(async () => {}),
  destroy: vi.fn(async () => {}),
}));
const lifecycleApi = vi.hoisted(() => ({
  hideMainWindow: vi.fn(async () => {}),
  getSettings: vi.fn(),
}));
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => windowApi }));
vi.mock('../api/tauri', () => lifecycleApi);
const platform = vi.hoisted(() => ({ IS_MACOS: false }));
vi.mock('../utils/platform', () => platform);
vi.mock('../hooks/useI18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }));
vi.mock('./CloseWindowDialog', () => ({ CloseWindowDialog: () => null }));

let view: ReactTestRenderer;
beforeEach(() => {
  vi.clearAllMocks();
  platform.IS_MACOS = false;
  useSidebarStore.setState({ collapsed: false });
  navigation.setState(navigation.getInitialState(), true);
  lifecycleApi.getSettings.mockResolvedValue({
    closeWindowBehaviorSet: true,
    closeToTray: false,
  });
});
afterEach(() => act(() => view?.unmount()));

it('toggles the sidebar independently of settings, window dragging and window controls', async () => {
  await act(async () => {
    view = create(<TitleBar />);
  });
  const control = () => view.root.findByProps({ 'aria-controls': 'app-sidebar' });
  expect(control().props['aria-expanded']).toBe(true);
  expect(control().props['aria-label']).toBe('nav.collapseSidebar');
  act(() => control().props.onClick());
  expect(useSidebarStore.getState().collapsed).toBe(true);
  expect(control().props['aria-expanded']).toBe(false);
  expect(control().props['aria-label']).toBe('nav.expandSidebar');
  act(() => control().props.onClick());
  expect(useSidebarStore.getState().collapsed).toBe(false);
  expect(windowApi.startDragging).not.toHaveBeenCalled();
  expect(windowApi.minimize).not.toHaveBeenCalled();
  expect(windowApi.maximize).not.toHaveBeenCalled();
  expect(lifecycleApi.getSettings).not.toHaveBeenCalled();
});

it('leaves space for native macOS traffic lights and retains dragging on blank title areas', async () => {
  platform.IS_MACOS = true;
  await act(async () => {
    view = create(<TitleBar />);
  });
  expect(
    view.root.findByProps({ 'aria-controls': 'app-sidebar' }).parent!.props.className
  ).toContain('ml-20');
  expect(view.root.findAllByType('button')).toHaveLength(6);
  const down = view.root.findAllByType('div')[0].props.onMouseDown;
  down({ button: 0, target: { closest: () => null }, preventDefault: vi.fn() });
  expect(windowApi.startDragging).toHaveBeenCalledOnce();
});

it('keeps history buttons grey and disabled at their boundaries and syncs new visits immediately', async () => {
  await act(async () => {
    view = create(<TitleBar />);
  });
  const back = () => view.root.findByProps({ 'aria-label': 'nav.back' });
  const forward = () => view.root.findByProps({ 'aria-label': 'nav.forward' });
  expect(back().props.disabled).toBe(true);
  expect(forward().props.disabled).toBe(true);
  act(() => {
    navigation.getState().setActivePage('models');
    navigation.getState().setActivePage('voice');
  });
  expect(back().props.disabled).toBe(false);
  expect(forward().props.disabled).toBe(true);
  act(() => back().props.onClick());
  expect(navigation.getState().activePage).toBe('models');
  expect(forward().props.disabled).toBe(false);
  act(() => back().props.onClick());
  expect(navigation.getState().activePage).toBe('apps');
  expect(back().props.disabled).toBe(true);
  act(() => forward().props.onClick());
  expect(navigation.getState().activePage).toBe('models');
  act(() => navigation.getState().setActivePage('accounts'));
  expect(forward().props.disabled).toBe(true);
  const history = navigation.getState().backHistory;
  act(() => view.root.findByProps({ 'aria-controls': 'app-sidebar' }).props.onClick());
  expect(navigation.getState().backHistory).toBe(history);
  expect(navigation.getState().activePage).toBe('accounts');
  expect(windowApi.startDragging).not.toHaveBeenCalled();
  expect(lifecycleApi.getSettings).not.toHaveBeenCalled();
});

it('keeps left-button dragging on the blank title area and excludes nested controls', async () => {
  await act(async () => {
    view = create(<TitleBar />);
  });
  const down = view.root.findAllByType('div')[0].props.onMouseDown;
  const preventDefault = vi.fn();
  down({ button: 0, target: { closest: () => null }, preventDefault });
  expect(windowApi.startDragging).toHaveBeenCalledTimes(1);
  expect(preventDefault).toHaveBeenCalledTimes(1);
  down({ button: 0, target: { closest: () => ({}) }, preventDefault });
  down({ button: 2, target: { closest: () => null }, preventDefault });
  expect(windowApi.startDragging).toHaveBeenCalledTimes(1);
  expect(preventDefault).toHaveBeenCalledTimes(1);
});

it('keeps header actions and native window controls independent of dragging', async () => {
  const changelog = vi.fn(),
    feedback = vi.fn();
  await act(async () => {
    view = create(<TitleBar onChangelogClick={changelog} onFeedbackClick={feedback} />);
  });
  expect(view.root.findAllByProps({ 'aria-label': 'settings.title' })).toHaveLength(0);
  for (const button of view.root.findAllByType('button')) {
    await act(async () => {
      await button.props.onClick();
    });
  }
  for (const action of [
    changelog,
    feedback,
    lifecycleApi.hideMainWindow,
    windowApi.minimize,
    windowApi.maximize,
    windowApi.destroy,
  ]) {
    expect(action).toHaveBeenCalledTimes(1);
  }
  expect(windowApi.startDragging).not.toHaveBeenCalled();
});

it('uses the native visibility bridge when closing to the tray', async () => {
  lifecycleApi.getSettings.mockResolvedValue({
    closeWindowBehaviorSet: true,
    closeToTray: true,
  });
  await act(async () => {
    view = create(<TitleBar />);
  });
  const buttons = view.root.findAllByType('button');
  await act(async () => {
    await buttons[buttons.length - 1].props.onClick();
  });
  expect(lifecycleApi.hideMainWindow).toHaveBeenCalledTimes(1);
  expect(windowApi.destroy).not.toHaveBeenCalled();
  expect(windowApi.startDragging).not.toHaveBeenCalled();
});
