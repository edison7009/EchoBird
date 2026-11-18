import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { expect, it, vi } from 'vitest';
import { PageActionButton } from './PageActionButton';

it('keeps a pending action locked without dimming or resizing its content', () => {
  const click = vi.fn();
  let view!: ReactTestRenderer;
  act(() => {
    view = create(<PageActionButton onClick={click}>Refresh</PageActionButton>);
  });
  const before = view.root.findByType('button').props.className;
  act(() => {
    view.update(
      <PageActionButton pending onClick={click}>
        Refresh
      </PageActionButton>
    );
  });
  const button = view.root.findByType('button');
  expect(button.props.disabled).toBe(true);
  expect(button.props['aria-busy']).toBe(true);
  expect(button.props.className).toBe(before);
  expect(click).not.toHaveBeenCalled();
  act(() => {
    view.update(<PageActionButton onClick={click}>Refresh</PageActionButton>);
  });
  expect(view.root.findByType('button').props.disabled).not.toBe(true);
  act(() => view.unmount());
});

it('keeps genuine unavailability distinct from a temporary operation lock', () => {
  let view!: ReactTestRenderer;
  act(() => {
    view = create(<PageActionButton disabled>Unavailable</PageActionButton>);
  });
  const button = view.root.findByType('button');
  expect(button.props.disabled).toBe(true);
  expect(button.props.className).toContain('disabled:opacity-50');
  act(() => view.unmount());
});
