import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { expect, it, vi } from 'vitest';
import { DirectoryRow } from './DirectoryRow';

it('keeps directory add and website actions separate', () => {
  const add = vi.fn();
  const open = vi.fn();
  let renderer!: ReactTestRenderer;
  act(() => {
    renderer = create(
      <DirectoryRow
        name="Provider"
        url="https://example.test"
        onOpen={open}
        openLabel="Open website"
        add={{ onClick: add, label: 'Add account' }}
      />
    );
  });
  try {
    const [addButton, websiteButton] = renderer.root.findAllByType('button');
    act(() => websiteButton.props.onClick());
    expect(open).toHaveBeenCalledTimes(1);
    expect(add).not.toHaveBeenCalled();
    act(() => addButton.props.onClick());
    expect(add).toHaveBeenCalledTimes(1);
    expect(open).toHaveBeenCalledTimes(1);
  } finally {
    act(() => renderer.unmount());
  }
});

it.each(['https://example.test', undefined])(
  'only the close button cancels a pending login (website=%s)',
  (url) => {
    const add = vi.fn();
    const cancel = vi.fn();
    const open = vi.fn();
    const row = (busy: boolean) => (
      <DirectoryRow
        name="Provider"
        url={url}
        onOpen={open}
        openLabel="Open website"
        secondary={busy ? 'Waiting for browser (58)' : undefined}
        add={{ onClick: add, label: 'Add account', busy, onCancel: cancel, cancelLabel: 'Cancel' }}
      />
    );
    const renderer = create(row(true));
    try {
      const [addButton, websiteButton, cancelButton] = renderer.root.findAllByType('button');
      expect(addButton.props.disabled).toBe(true);
      expect(websiteButton.props.disabled).toBe(!url);
      expect(cancelButton.props.disabled).not.toBe(true);
      expect(cancelButton.props['aria-label']).toBe('Cancel');
      expect(cancelButton.parent!.findByType('span').children).toEqual([
        'Waiting for browser (58)',
      ]);
      act(() => cancelButton.props.onClick({ stopPropagation: vi.fn() }));
      expect(cancel).toHaveBeenCalledTimes(1);
      expect(add).not.toHaveBeenCalled();
      expect(open).not.toHaveBeenCalled();
      if (url) {
        act(() => websiteButton.props.onClick());
        expect(open).toHaveBeenCalledTimes(1);
        expect(cancel).toHaveBeenCalledTimes(1);
        expect(add).not.toHaveBeenCalled();
      }
      act(() => renderer.update(row(false)));
      expect(renderer.root.findAllByType('button')).toHaveLength(2);
      expect(renderer.root.findAllByType('button')[1].props['aria-label']).toBe('Open website');
      act(() => renderer.root.findAllByType('button')[0].props.onClick());
      expect(add).toHaveBeenCalledTimes(1);
      expect(cancel).toHaveBeenCalledTimes(1);
    } finally {
      act(() => renderer.unmount());
    }
  }
);

it('a subscription-service row has only its website action', () => {
  const open = vi.fn();
  let renderer!: ReactTestRenderer;
  act(() => {
    renderer = create(
      <DirectoryRow
        name="Service fixture"
        url="https://example.test"
        onOpen={open}
        openLabel="Open service"
      />
    );
  });
  try {
    const buttons = renderer.root.findAllByType('button');
    expect(buttons).toHaveLength(1);
    expect(buttons[0].props['aria-label']).toBe('Open service');
    act(() => buttons[0].props.onClick());
    expect(open).toHaveBeenCalledTimes(1);
  } finally {
    act(() => renderer.unmount());
  }
});
