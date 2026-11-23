import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { PackageOpen } from 'lucide-react';
import type { SelfUpdateProgress } from '../api/tauri';
import { RoutingHint } from './RoutingToggle';
import { UpdateDownloadButton } from './UpdateDownloadButton';

const platform = vi.hoisted(() => ({ IS_LINUX: false, IS_MACOS: false }));
vi.mock('../utils/platform', () => platform);
beforeEach(() => Object.assign(platform, { IS_LINUX: false, IS_MACOS: false }));

let renderer: ReactTestRenderer;
afterEach(() => act(() => renderer?.unmount()));

function render(progress: SelfUpdateProgress | null, collapsed = true, ready = false) {
  const onClick = vi.fn();
  act(() => {
    renderer = create(
      <UpdateDownloadButton
        version="4.2.0"
        progress={progress}
        collapsed={collapsed}
        ready={ready}
        onClick={onClick}
      />
    );
  });
  return { onClick, button: renderer.root.findByType('button') };
}

describe('UpdateDownloadButton', () => {
  it('labels each of the four states with a tip and keeps the package icon during installation', () => {
    const cases: [SelfUpdateProgress | null, string][] = [
      [null, 'updates.download v4.2.0'],
      [{ status: 'speed_test', percent: 0 }, 'updates.connecting'],
      [{ status: 'downloading', percent: 61 }, 'updates.downloading 61%'],
      [{ status: 'launching', percent: 100 }, 'updates.openingInstaller'],
    ];
    for (const [progress, label] of cases) {
      const launching = progress?.status === 'launching';
      const { button } = render(progress, true, launching);
      expect(button.props['aria-label']).toBe(label);
      expect(renderer.root.findByType(RoutingHint).props.hint).toBe(label);
      expect(renderer.root.findAllByType(PackageOpen)).toHaveLength(launching ? 1 : 0);
      act(() => renderer.unmount());
    }
  });

  it('offers installation when a verified package is ready', () => {
    const { button } = render(null, false, true);
    expect(button.props['aria-label']).toBe('updates.install v4.2.0');
    expect(button.props.children).toBe('updates.install');
    expect(button.props.disabled).toBe(false);
    act(() => renderer.unmount());

    render(null, true, true);
    expect(renderer.root.findAllByType(PackageOpen)).toHaveLength(1);
  });

  it.each(['IS_MACOS', 'IS_LINUX'] as const)('opens the folder for %s packages', (os) => {
    platform[os] = true;
    const expanded = render(null, false, true);
    expect(expanded.button.props['aria-label']).toBe('aiDesktop.openLocation v4.2.0');
    expect(expanded.button.props.children).toBe('aiDesktop.openLocation');
    act(() => expanded.button.props.onClick());
    expect(expanded.onClick).toHaveBeenCalledOnce();
    act(() => renderer.unmount());

    render(null, true, true);
    expect(renderer.root.findAllByType(PackageOpen)).toHaveLength(1);
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
