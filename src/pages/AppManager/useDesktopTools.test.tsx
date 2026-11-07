import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { useLayoutEffect } from 'react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { LocalTool } from '../../api/types';
import { useDesktopTools } from './useDesktopTools';

class PendingImage {
  static instances: PendingImage[] = [];
  src = '';
  resolve!: () => void;
  reject!: () => void;
  decode = () =>
    new Promise<void>((resolve, reject) => {
      this.resolve = resolve;
      this.reject = () => reject(new Error('Broken icon'));
    });
  constructor() {
    PendingImage.instances.push(this);
  }
}
let renderer: ReactTestRenderer | undefined;
let result: ReturnType<typeof useDesktopTools>;
const tool = (id: string): LocalTool => ({
  id,
  name: id,
  installed: true,
  category: 'Desktop',
  icon: `./${id}.png`,
});
function Harness({ tools, scope = 'desktop' }: { tools: LocalTool[]; scope?: string }) {
  const value = useDesktopTools(tools, scope);
  useLayoutEffect(() => {
    result = value;
  });
  return null;
}
async function render(tools: LocalTool[], scope = 'desktop') {
  await act(async () => {
    if (renderer) renderer.update(<Harness tools={tools} scope={scope} />);
    else renderer = create(<Harness tools={tools} scope={scope} />);
  });
}
beforeEach(() => {
  PendingImage.instances = [];
  vi.stubGlobal('Image', PendingImage);
});
afterEach(() => {
  act(() => renderer?.unmount());
  renderer = undefined;
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

it('publishes the complete grid together and immediately reuses warm images on entry', async () => {
  const tools = [tool('first-a'), tool('first-b')];
  await render(tools);
  expect(result.tools).toEqual([]);
  expect(result.pending).toBe(true);
  await act(async () => PendingImage.instances[0].resolve());
  expect(result.tools).toEqual([]);
  await act(async () => PendingImage.instances[1].resolve());
  expect(result.tools).toEqual(tools);
  expect(result.pending).toBe(false);
  act(() => renderer!.unmount());
  renderer = undefined;
  await render(tools);
  expect(result.tools).toEqual(tools);
  expect(result.pending).toBe(false);
  expect(PendingImage.instances).toHaveLength(2);
});

it('keeps the previous grid during refresh and rejects obsolete plugin sources', async () => {
  const tools = [tool('refresh-old')];
  await render(tools);
  await act(async () => PendingImage.instances[0].resolve());
  await render([{ ...tools[0], icon: './plugin-old.png' }]);
  expect(result.tools).toEqual(tools);
  await render([{ ...tools[0], icon: './plugin-new.png' }]);
  await act(async () => PendingImage.instances[1].resolve());
  expect(result.tools).toEqual(tools);
  await act(async () => PendingImage.instances[2].resolve());
  expect(result.tools[0].icon).toBe('./plugin-new.png');
  await render([tool('store-only')], 'store');
  expect(result.tools).toEqual([]);
  await act(async () => PendingImage.instances[3].resolve());
  expect(result.tools[0].id).toBe('store-only');
});

it('uses a stable fallback for errors and timeouts and ignores late decode completion', async () => {
  vi.useFakeTimers();
  await render([tool('broken-icon'), tool('stalled-icon')]);
  await act(async () => PendingImage.instances[0].reject());
  expect(result.tools).toEqual([]);
  await act(async () => {
    vi.advanceTimersByTime(3000);
  });
  expect(result.pending).toBe(false);
  expect(result.tools.map((entry) => entry.icon)).toEqual(['', '']);
  await act(async () => PendingImage.instances[1].resolve());
  expect(result.tools.map((entry) => entry.icon)).toEqual(['', '']);
});
