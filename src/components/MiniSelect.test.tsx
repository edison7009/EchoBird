import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { MiniSelect } from './MiniSelect';

let renderer: ReactTestRenderer;
let anchorTop: number;
let boundaryTop: number;
const onChange = vi.fn();
const options = Array.from({ length: 9 }, (_, index) => ({
  id: String(index),
  label: `Language ${index}`,
}));
const open = (dropUp = true) => {
  act(() => {
    renderer = create(
      <MiniSelect value="0" options={options} onChange={onChange} dropUp={dropUp} />,
      {
        createNodeMock: () => ({
          getBoundingClientRect: () => ({ top: anchorTop }),
          closest: () => ({ getBoundingClientRect: () => ({ top: boundaryTop }) }),
        }),
      }
    );
  });
  act(() => renderer.root.findByType('button').props.onClick());
};
const menu = () =>
  renderer.root.findAllByType('div').find((node) => node.props.className.includes('absolute'))!;

beforeEach(() => {
  vi.clearAllMocks();
  anchorTop = 600;
  boundaryTop = 80;
  vi.stubGlobal('window', { addEventListener: vi.fn(), removeEventListener: vi.fn() });
  vi.stubGlobal('document', { addEventListener: vi.fn(), removeEventListener: vi.fn() });
});
afterEach(() => {
  act(() => renderer.unmount());
  vi.unstubAllGlobals();
});

it.each([
  [600, 80, 360],
  [240, 80, 151],
  [120, -20, 111],
  [80, 80, 0],
])(
  'fits an upward menu at anchor %s above boundary %s to %s pixels',
  (anchor, boundary, height) => {
    anchorTop = anchor;
    boundaryTop = boundary;
    open();
    expect(menu().props.className).toContain('bottom-full');
    expect(menu().props.className).toContain('overflow-y-auto');
    expect(menu().props.className).toContain('app-tooltip selection-popover model-selection-menu');
    expect(menu().props.style).toEqual({ maxHeight: height, overscrollBehavior: 'contain' });
    expect(menu().children).toHaveLength(9);
    expect(onChange).not.toHaveBeenCalled();
  }
);

it('updates available height on resize and page scroll, then cleans up after selecting the last option', () => {
  open();
  const listeners = vi.mocked(window.addEventListener).mock.calls;
  const resize = listeners.find(([name]) => name === 'resize')![1] as () => void;
  const scroll = listeners.find(([name]) => name === 'scroll')![1] as () => void;
  anchorTop = 260;
  act(() => resize());
  expect(menu().props.style.maxHeight).toBe(171);
  boundaryTop = 120;
  act(() => scroll());
  expect(menu().props.style.maxHeight).toBe(131);
  act(() =>
    renderer.root
      .findAllByType('div')
      .find(
        (node) =>
          node.props.onClick &&
          node.findAllByType('span').some((label) => label.children.includes('Language 8'))
      )!
      .props.onClick()
  );
  expect(onChange).toHaveBeenCalledExactlyOnceWith('8');
  expect(menu()).toBeUndefined();
  expect(window.removeEventListener).toHaveBeenCalledWith('resize', resize);
  expect(window.removeEventListener).toHaveBeenCalledWith('scroll', scroll, true);
});

it('uses the shared popover and marks the selected row in a downward menu', () => {
  open(false);
  const classes = menu().props.className.split(' ');
  expect(classes).toContain('top-full');
  expect(classes).toContain('selection-popover');
  expect(classes).toContain('app-tooltip');
  expect(menu().findByProps({ 'data-selected': true }).findByType('span').children).toEqual([
    'Language 0',
  ]);
  expect(menu().props.style).toBeUndefined();
  expect(window.addEventListener).not.toHaveBeenCalled();
});
