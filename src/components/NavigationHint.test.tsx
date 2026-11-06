import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, expect, it, vi } from 'vitest';
import { RoutingHint } from './RoutingToggle';

vi.mock('react-dom', async (importOriginal) => ({
  ...(await importOriginal<typeof import('react-dom')>()),
  createPortal: (node: React.ReactNode) => node,
}));

let renderer: ReactTestRenderer;
afterEach(() => {
  act(() => renderer?.unmount());
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

it('replaces navigation labels immediately without a delayed tooltip or border', () => {
  vi.useFakeTimers();
  vi.stubGlobal('document', { body: {} });
  vi.stubGlobal('window', { addEventListener: vi.fn(), removeEventListener: vi.fn() });
  act(() => {
    renderer = create(
      <>
        <RoutingHint hint="Desktop" navigation>
          <button>Home</button>
        </RoutingHint>
        <RoutingHint hint="Models" navigation>
          <button>Models</button>
        </RoutingHint>
      </>
    );
  });
  const anchors = renderer.root.findAll(
    (node) => node.type === 'span' && !!node.props.onMouseEnter && !!node.props.onMouseLeave
  );
  act(() => anchors[0].props.onMouseEnter());
  let tip = renderer.root.findByProps({ role: 'tooltip' });
  expect(tip.props.className).not.toContain('border');
  expect(tip.props.className.split(' ')).toContain('app-tooltip');
  expect(tip.props.className).not.toContain('bg-cyber-elevated');
  act(() => anchors[0].props.onMouseLeave());
  expect(renderer.root.findAllByProps({ role: 'tooltip' })).toHaveLength(0);
  act(() => anchors[1].props.onMouseEnter());
  expect(renderer.root.findAllByProps({ role: 'tooltip' })).toHaveLength(1);
  tip = renderer.root.findByProps({ role: 'tooltip' });
  expect(tip.findByProps({ className: 'block overflow-y-auto' }).children).toEqual(['Models']);
  act(() => anchors[1].props.onMouseLeave());
  expect(renderer.root.findAllByProps({ role: 'tooltip' })).toHaveLength(0);
  expect(vi.getTimerCount()).toBe(0);
});
