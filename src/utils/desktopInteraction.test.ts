import { afterEach, expect, it, vi } from 'vitest';
import { handleDesktopContextMenu } from './desktopInteraction';

class ElementFixture {
  isContentEditable = false;
  field: { matches: () => boolean } | null = null;
  closest = vi.fn(() => this.field);
}

afterEach(() => vi.unstubAllGlobals());

function dispatch(target: unknown) {
  vi.stubGlobal('HTMLElement', ElementFixture);
  const event = { target, preventDefault: vi.fn() };
  handleDesktopContextMenu(event as unknown as MouseEvent);
  return event;
}

it('suppresses page menus on ordinary chrome and non-element targets', () => {
  expect(dispatch(new ElementFixture()).preventDefault).toHaveBeenCalledOnce();
  expect(dispatch(null).preventDefault).toHaveBeenCalledOnce();
});

it('retains input and textarea menus, including readonly fields', () => {
  const target = new ElementFixture();
  target.field = { matches: () => false };
  expect(dispatch(target).preventDefault).not.toHaveBeenCalled();
  expect(target.closest).toHaveBeenCalledWith('input, textarea');
});

it('suppresses menus on disabled fields and non-text input controls', () => {
  const target = new ElementFixture();
  target.field = { matches: () => true };
  expect(dispatch(target).preventDefault).toHaveBeenCalledOnce();
  expect(target.closest).toHaveBeenCalledWith('input, textarea');
});

it('retains editing menus on editable content and its editable descendants', () => {
  const target = new ElementFixture();
  target.isContentEditable = true;
  expect(dispatch(target).preventDefault).not.toHaveBeenCalled();
});
