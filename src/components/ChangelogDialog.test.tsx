import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { useLayoutEffect, useState } from 'react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { ChangelogDialog } from './ChangelogDialog';

vi.mock('../hooks/useI18n', () => ({ useI18n: () => ({ t: (key: string) => key, locale: 'en' }) }));
vi.mock('../pages/MotherAgent/mdComponents', () => ({ mdComponents: {} }));
vi.mock('./VoiceInputGuide', () => ({
  VoiceInputGuide: ({ onClose }: { onClose: () => void }) => (
    <button data-guide onClick={onClose}>
      guide
    </button>
  ),
}));
let view: ReactTestRenderer;
let open: (value: boolean) => void;
const closed = vi.fn();
function Harness({ automatic = false }: { automatic?: boolean }) {
  const [isOpen, setOpen] = useState(true);
  useLayoutEffect(() => {
    open = setOpen;
  }, []);
  return (
    <ChangelogDialog
      isOpen={isOpen}
      showVoiceGuideOnClose={automatic}
      onClose={() => {
        closed();
        setOpen(false);
      }}
    />
  );
}
beforeEach(() => {
  vi.useFakeTimers();
  closed.mockClear();
  vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('offline')));
  vi.stubGlobal('window', { addEventListener: vi.fn(), removeEventListener: vi.fn() });
});
afterEach(() => {
  act(() => view?.unmount());
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

it.each([false, true])(
  'shows the guide only after closing an automatic=%s changelog, including offline notes',
  async (automatic) => {
    await act(async () => {
      view = create(<Harness automatic={automatic} />);
    });
    expect(view.root.findAllByProps({ 'data-guide': true })).toHaveLength(0);
    expect(fetch).toHaveBeenCalledTimes(3);
    const dismiss = view.root.findByType('button').props.onClick;
    act(() => {
      dismiss();
      dismiss();
    });
    act(() => {
      vi.advanceTimersByTime(199);
    });
    expect(closed).not.toHaveBeenCalled();
    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(closed).toHaveBeenCalledOnce();
    expect(view.root.findAllByProps({ 'data-guide': true })).toHaveLength(automatic ? 1 : 0);
    expect(fetch).toHaveBeenCalledTimes(3);
    if (automatic) act(() => view.root.findByProps({ 'data-guide': true }).props.onClick());
    await act(async () => {
      view.update(<Harness automatic={false} />);
      open(true);
    });
    act(() => view.root.findByType('button').props.onClick());
    act(() => {
      vi.advanceTimersByTime(200);
    });
    expect(view.root.findAllByProps({ 'data-guide': true })).toHaveLength(0);
  }
);

it('cleans up a pending close when the application unmounts', async () => {
  await act(async () => {
    view = create(<Harness automatic />);
  });
  act(() => view.root.findByType('button').props.onClick());
  act(() => view.unmount());
  act(() => {
    vi.advanceTimersByTime(500);
  });
  expect(closed).not.toHaveBeenCalled();
  expect(vi.getTimerCount()).toBe(0);
});
