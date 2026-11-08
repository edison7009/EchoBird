import { act, create } from 'react-test-renderer';
import { afterEach, expect, it, vi } from 'vitest';
import { CloseWindowDialog } from './CloseWindowDialog';
import type { ReactNode } from 'react';
const { portal } = vi.hoisted(() => ({
  portal: vi.fn((children: ReactNode, _target: unknown) => children),
}));
vi.mock('react-dom', () => ({ createPortal: portal }));
vi.mock('../hooks/useI18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }));
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
});

it('consumes Escape before the underlying settings dialog and cancels only the close request', () => {
  vi.useFakeTimers();
  const addEventListener = vi.fn();
  const removeEventListener = vi.fn();
  const onClose = vi.fn();
  vi.stubGlobal('document', { body: {} });
  vi.stubGlobal('window', { addEventListener, removeEventListener });
  let view!: ReturnType<typeof create>;
  act(() => {
    view = create(<CloseWindowDialog isOpen onClose={onClose} />);
  });
  const [eventName, handler, capture] = addEventListener.mock.calls[0];
  expect(eventName).toBe('keydown');
  expect(capture).toBe(true);
  const event = { key: 'Escape', preventDefault: vi.fn(), stopImmediatePropagation: vi.fn() };
  act(() => handler(event));
  expect(event.preventDefault).toHaveBeenCalledOnce();
  expect(event.stopImmediatePropagation).toHaveBeenCalledOnce();
  expect(onClose).not.toHaveBeenCalled();
  act(() => {
    vi.advanceTimersByTime(200);
  });
  expect(onClose).toHaveBeenCalledExactlyOnceWith(null);
  act(() => view.unmount());
  expect(removeEventListener).toHaveBeenCalledWith('keydown', handler, true);
});
it('shows close confirmation above settings outside the isolated app shell', () => {
  const body = {};
  vi.stubGlobal('document', { body });
  vi.stubGlobal('window', { addEventListener: vi.fn(), removeEventListener: vi.fn() });
  let view!: ReturnType<typeof create>;
  act(() => {
    view = create(<CloseWindowDialog isOpen onClose={vi.fn()} />);
  });
  expect(portal).toHaveBeenCalledWith(expect.anything(), body);
  expect(view.root.findAllByType('div')[0].props.className).toContain('z-[10000]');
  expect(view.root.findAllByType('button').map((button) => button.children)).toEqual([
    ['settings.closeDirectly'],
    ['settings.closeToTray'],
  ]);
  act(() => view.unmount());
});
