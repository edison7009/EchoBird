import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { ViewModeTabGroup, type ViewModeTabPosition } from './ViewModeTabGroup';

const canvas = vi.hoisted(() => ({ mount: vi.fn(), cleanup: vi.fn() }));
vi.mock('./useTabCanvas', () => ({ mountTabCanvas: canvas.mount }));

let renderer: ReactTestRenderer;
const root = { dataset: {} as Record<string, string> };
const onSelect = vi.fn();
function render(positionRef?: { current: ViewModeTabPosition | null }) {
  act(() => {
    renderer = create(
      <ViewModeTabGroup className="view-mode-tabs" role="tablist" positionRef={positionRef}>
        <button className="view-mode-tab" role="tab" aria-selected onClick={onSelect}>
          Selected
        </button>
      </ViewModeTabGroup>,
      { createNodeMock: (node) => (node.type === 'div' ? root : null) }
    );
  });
}
beforeEach(() => canvas.mount.mockReturnValue(canvas.cleanup));
afterEach(() => vi.clearAllMocks());

it('delegates paint to one canvas owner on every platform and releases it on exit', () => {
  const positionRef = { current: null as ViewModeTabPosition | null };
  render(positionRef);
  expect(canvas.mount).toHaveBeenCalledWith(root, positionRef);
  expect(onSelect).not.toHaveBeenCalled();
  act(() => renderer.unmount());
  expect(canvas.cleanup).toHaveBeenCalledOnce();
});

it('remembers geometry locally when no shared position is supplied', () => {
  render();
  expect(canvas.mount).toHaveBeenCalledWith(root, { current: null });
  act(() => renderer.unmount());
});
