import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { Box } from 'lucide-react';
import { getSystemApplicationIcon } from '../api/tauri';
import { DesktopToolImage } from './DesktopToolIcon';
import type { LocalTool } from '../api/types';
import { loadSystemApplicationIcon } from '../utils/systemApplicationIcons';
import { Profiler } from 'react';

vi.mock('../api/tauri', () => ({ getSystemApplicationIcon: vi.fn() }));

let renderer: ReactTestRenderer;
let visible: (entries: { isIntersecting: boolean }[]) => void;
const disconnect = vi.fn();
beforeEach(() => {
  vi.clearAllMocks();
  vi.stubGlobal(
    'IntersectionObserver',
    class {
      constructor(callback: typeof visible) {
        visible = callback;
      }
      observe() {}
      disconnect = disconnect;
    }
  );
});
afterEach(() => {
  act(() => renderer?.unmount());
  vi.unstubAllGlobals();
});

const app = (id: string): LocalTool => ({
  id,
  systemAppId: id,
  name: 'Application',
  category: 'Desktop',
  installed: true,
});
const render = (tool: LocalTool) => {
  act(() => {
    renderer = create(<DesktopToolImage tool={tool} />, { createNodeMock: () => ({}) });
  });
};
const skeleton = () => renderer.root.findByProps({ 'data-icon-skeleton': '' });

it('keeps the skeleton through native loading and image decoding, then crossfades', async () => {
  let complete!: (src: string) => void;
  vi.mocked(getSystemApplicationIcon).mockImplementation(
    () =>
      new Promise((resolve) => {
        complete = resolve;
      })
  );
  render(app('system:loading'));
  expect(getSystemApplicationIcon).not.toHaveBeenCalled();
  expect(skeleton().props.className).toContain('opacity-100');
  expect(renderer.root.findAllByType(Box)).toHaveLength(0);
  await act(async () => visible([{ isIntersecting: true }]));
  expect(getSystemApplicationIcon).toHaveBeenCalledExactlyOnceWith('system:loading');
  await act(async () => complete('data:image/png;base64,loading'));
  const image = renderer.root.findByType('img');
  expect(image.props.className).toContain('opacity-0');
  expect(skeleton().props.className).toContain('opacity-100');
  act(() => image.props.onLoad());
  expect(image.props.className).toContain('opacity-100');
  expect(skeleton().props.className).toContain('opacity-0');
  expect(skeleton().props.className).toContain('motion-reduce:transition-none');
  expect(renderer.root.findAllByType(Box)).toHaveLength(0);
});

it.each(['missing', 'rejected', 'decode-failed'])(
  'finishes the skeleton for %s icons',
  async (failure) => {
    if (failure === 'rejected')
      vi.mocked(getSystemApplicationIcon).mockRejectedValue(new Error('Unavailable'));
    else
      vi.mocked(getSystemApplicationIcon).mockResolvedValue(
        failure === 'missing' ? null : 'broken'
      );
    render(app(`system:${failure}`));
    await act(async () => visible([{ isIntersecting: true }]));
    if (failure === 'decode-failed') act(() => renderer.root.findByType('img').props.onError());
    expect(skeleton().props.className).toContain('opacity-0');
    expect(renderer.root.findAllByType(Box)).toHaveLength(1);
  }
);

it('ignores old responses when a tile changes application and reuses cached icons', async () => {
  let complete!: (src: string) => void;
  vi.mocked(getSystemApplicationIcon)
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          complete = resolve;
        })
    )
    .mockResolvedValue('new-image');
  render(app('system:old'));
  act(() => visible([{ isIntersecting: true }]));
  act(() => renderer.update(<DesktopToolImage tool={app('system:new')} />));
  await act(async () => visible([{ isIntersecting: true }]));
  await act(async () => complete('old-image'));
  expect(renderer.root.findByType('img').props.src).toBe('new-image');
  act(() => renderer.unmount());
  render(app('system:new'));
  await act(async () => visible([{ isIntersecting: true }]));
  expect(getSystemApplicationIcon).toHaveBeenCalledTimes(2);
  expect(renderer.root.findByType('img').props.src).toBe('new-image');
  expect(renderer.root.findByType('img').props.className).toContain('opacity-100');
  expect(skeleton().props.className).toContain('opacity-0');
});

it('loads a provided icon with the same decode transition', () => {
  render({ ...app('system:provided'), icon: 'provided-image' });
  expect(getSystemApplicationIcon).not.toHaveBeenCalled();
  act(() => renderer.root.findByType('img').props.onLoad());
  expect(skeleton().props.className).toContain('opacity-0');
});

it('shows an already cached icon on the first commit when the same detail image changes app', async () => {
  vi.mocked(getSystemApplicationIcon)
    .mockResolvedValueOnce('cached-first')
    .mockResolvedValueOnce('cached-second');
  await loadSystemApplicationIcon('system:switch-first');
  await loadSystemApplicationIcon('system:switch-second');
  act(() => {
    renderer = create(
      <Profiler id="detail-icon" onRender={() => {}}>
        <DesktopToolImage tool={app('system:switch-first')} />
      </Profiler>,
      { createNodeMock: () => ({}) }
    );
  });
  const committed: Array<{ src: string; skeleton: string }> = [];
  act(() => {
    renderer.update(
      <Profiler
        id="detail-icon"
        onRender={() =>
          committed.push({
            src: renderer.root.findByType('img').props.src,
            skeleton: skeleton().props.className,
          })
        }
      >
        <DesktopToolImage tool={app('system:switch-second')} />
      </Profiler>
    );
  });
  expect(committed[0].src).toBe('cached-second');
  expect(committed[0].skeleton).toContain('opacity-0');
  expect(renderer.root.findAllByType(Box)).toHaveLength(0);
  expect(getSystemApplicationIcon).toHaveBeenCalledTimes(2);
});

it('unmounts offscreen images and restores a recent decoded icon on return', async () => {
  vi.mocked(getSystemApplicationIcon).mockResolvedValueOnce('visible-image');
  render(app('system:viewport'));
  await act(async () => visible([{ isIntersecting: true }]));
  act(() => renderer.root.findByType('img').props.onLoad());
  act(() => visible([{ isIntersecting: false }]));
  expect(renderer.root.findAllByType('img')).toHaveLength(0);
  expect(renderer.root.findAllByType(Box)).toHaveLength(0);
  await act(async () => visible([{ isIntersecting: true }]));
  expect(renderer.root.findByType('img').props.src).toBe('visible-image');
  expect(skeleton().props.className).toContain('opacity-0');
  expect(getSystemApplicationIcon).toHaveBeenCalledOnce();
});

it('keeps a late native response out of an offscreen tile', async () => {
  let finish!: (src: string) => void;
  vi.mocked(getSystemApplicationIcon).mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      })
  );
  render(app('system:offscreen-pending'));
  await act(async () => visible([{ isIntersecting: true }]));
  act(() => visible([{ isIntersecting: false }]));
  await act(async () => finish('late-viewport-image'));
  expect(renderer.root.findAllByType('img')).toHaveLength(0);
  await act(async () => visible([{ isIntersecting: true }]));
  expect(renderer.root.findByType('img').props.src).toBe('late-viewport-image');
  expect(getSystemApplicationIcon).toHaveBeenCalledOnce();
});
