import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { RotateCw } from 'lucide-react';
import type { SelfUpdateProgress } from '../api/tauri';
import { RoutingHint } from './RoutingToggle';
import { UpdateDownloadButton } from './UpdateDownloadButton';

let renderer: ReactTestRenderer;
afterEach(() => act(() => renderer?.unmount()));

function render(progress: SelfUpdateProgress | null, collapsed = true) {
  const onClick = vi.fn();
  act(() => {
    renderer = create(
      <UpdateDownloadButton
        version="4.2.0"
        progress={progress}
        collapsed={collapsed}
        onClick={onClick}
      />
    );
  });
  return { onClick, button: renderer.root.findByType('button') };
}

describe('UpdateDownloadButton', () => {
  it('labels each of the four states with a tip and no restart icon before launch', () => {
    const cases: [SelfUpdateProgress | null, string][] = [
      [null, 'updates.download v4.2.0'],
      [{ status: 'speed_test', percent: 0 }, 'updates.connecting'],
      [{ status: 'downloading', percent: 61 }, 'updates.downloading 61%'],
      [{ status: 'launching', percent: 100 }, 'updates.restarting'],
    ];
    for (const [progress, label] of cases) {
      const { button } = render(progress);
      expect(button.props['aria-label']).toBe(label);
      expect(renderer.root.findByType(RoutingHint).props.hint).toBe(label);
      expect(renderer.root.findAllByType(RotateCw)).toHaveLength(
        progress?.status === 'launching' ? 1 : 0
      );
      act(() => renderer.unmount());
    }
  });

  it('places the tip right when collapsed and above when expanded', () => {
    render(null, true);
    expect(renderer.root.findByType(RoutingHint).props).toMatchObject({
      placement: 'right',
      navigation: true,
    });
    act(() => renderer.unmount());
    render(null, false);
    expect(renderer.root.findByType(RoutingHint).props.placement).toBe('top');
  });

  it('only starts a download from the idle state', () => {
    const idle = render(null);
    expect(idle.button.props.disabled).toBe(false);
    act(() => idle.button.props.onClick());
    expect(idle.onClick).toHaveBeenCalledOnce();
    act(() => renderer.unmount());
    const busy = render({ status: 'launching', percent: 100 });
    expect(busy.button.props.disabled).toBe(true);
  });
});
