import React from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { LocalTool } from '../../api/types';
import { AppManagerContext, type AppManagerContextType } from './context';
import { AppManagerMain } from './AppManagerComponents';
import { DndContext } from '@dnd-kit/core';
import { useSortable } from '@dnd-kit/sortable';

vi.mock('../../hooks/useDocumentVisible', () => ({ useDocumentVisible: () => true }));
vi.mock('../../components', () => ({ getModelIcon: () => null }));
vi.mock('@dnd-kit/core', () => ({
  DndContext: ({ children }: React.PropsWithChildren) => <>{children}</>,
  DragOverlay: ({ children }: React.PropsWithChildren) => <>{children}</>,
  PointerSensor: vi.fn(),
  KeyboardSensor: vi.fn(),
  closestCenter: vi.fn(),
  useSensor: vi.fn(),
  useSensors: vi.fn(),
}));
vi.mock('@dnd-kit/sortable', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@dnd-kit/sortable')>()),
  SortableContext: ({ children }: React.PropsWithChildren) => <>{children}</>,
  useSortable: vi.fn(() => ({ attributes: {}, listeners: {}, setNodeRef: vi.fn() })),
}));

const tools: LocalTool[] = [
  { id: 'my-a', name: 'My A', category: 'Desktop', installed: true },
  { id: 'my-b', name: 'My B', category: 'Desktop', installed: true },
  { id: 'store-a', name: 'Store A', category: 'CLI Code', installed: false },
  { id: 'store-b', name: 'Store B', category: 'Science', installed: false },
  { id: 'store-c', name: 'Store C', category: 'CLI Code', installed: false },
];

describe('App Desktop drag sorting', () => {
  let renderer: ReactTestRenderer;
  let storage: Map<string, string>;
  const setItem = vi.fn();
  const select = vi.fn();
  let mode: 'desktop' | 'install';

  const render = (isScanning = false) => (
    <AppManagerContext.Provider
      value={
        {
          detectedTools: tools,
          viewMode: mode,
          selectedTool: null,
          setSelectedTool: select,
          isScanning,
          aiInstallableIds: ['store-a', 'store-b', 'store-c'],
        } as unknown as AppManagerContextType
      }
    >
      <AppManagerMain />
    </AppManagerContext.Provider>
  );
  const open = (
    nextMode: 'desktop' | 'install' = 'install',
    createNodeMock: NonNullable<Parameters<typeof create>[1]>['createNodeMock'] = () => null
  ) => {
    mode = nextMode;
    act(() => {
      renderer = create(render(), { createNodeMock });
    });
  };
  const visible = () =>
    renderer.root
      .findAllByType('button')
      .filter((node) => node.props['aria-label'])
      .map((node) => node.props['aria-label']);
  const drag = (active: string, over: string | null) => {
    act(() => {
      renderer.root.findByType(DndContext).props.onDragEnd({
        active: { id: active },
        over: over ? { id: over } : null,
      });
    });
  };
  const switchMode = (nextMode: 'desktop' | 'install') => {
    mode = nextMode;
    act(() => renderer.update(render()));
  };
  const category = (key: string) => {
    act(() => {
      renderer.root
        .findAllByType('button')
        .find((node) => node.children.includes(key))!
        .props.onClick();
    });
  };

  beforeEach(() => {
    storage = new Map();
    setItem.mockImplementation((key: string, value: string) => storage.set(key, value));
    vi.stubGlobal('localStorage', { getItem: (key: string) => storage.get(key), setItem });
    vi.stubGlobal('__APP_EDITION__', 'full');
    vi.mocked(useSortable).mockReturnValue({
      attributes: {},
      listeners: {},
      setNodeRef: vi.fn(),
    } as unknown as ReturnType<typeof useSortable>);
  });
  afterEach(() => {
    act(() => renderer?.unmount());
    vi.clearAllMocks();
    vi.unstubAllGlobals();
  });

  it.each(['desktop', 'install'] as const)(
    '%s keeps cached cards mounted and selectable during background scanning',
    (view) => {
      open(view);
      const first = renderer.root.findByProps({ 'aria-label': visible()[0] });
      act(() => renderer.update(render(true)));
      const cached = renderer.root.findByProps({ 'aria-label': visible()[0] });
      expect(cached).toBe(first);
      expect(cached.props.disabled).toBeFalsy();
      act(() => cached.props.onClick());
      expect(select).toHaveBeenCalledExactlyOnceWith(view === 'desktop' ? 'my-a' : 'store-a');
      expect(setItem).not.toHaveBeenCalled();
    }
  );

  it('lets store apps move and remembers their order after reopening', () => {
    open();
    drag('store-c', 'store-a');
    expect(visible()).toEqual(['Store C', 'Store A', 'Store B']);
    expect(storage.get('echobird_appmgr_store_order')).toBe(
      JSON.stringify(['store-c', 'store-a', 'store-b'])
    );
    act(() => renderer.unmount());
    open();
    expect(visible()).toEqual(['Store C', 'Store A', 'Store B']);
    expect(select).not.toHaveBeenCalled();
  });

  it('keeps category-hidden apps in place when reordering a filtered list', () => {
    open();
    category('toolCat.cli');
    expect(visible()).toEqual(['Store A', 'Store C']);
    drag('store-c', 'store-a');
    category('toolCat.all');
    expect(visible()).toEqual(['Store C', 'Store B', 'Store A']);
    expect(storage.get('echobird_appmgr_store_order')).toBe(
      JSON.stringify(['store-c', 'store-b', 'store-a'])
    );
  });

  it('preserves the saved My order and saves Store order separately', () => {
    storage.set('echobird_appmgr_tool_order', JSON.stringify(['my-b', 'my-a']));
    open('desktop');
    expect(visible()).toEqual(['My B', 'My A']);
    switchMode('install');
    drag('store-c', 'store-a');
    switchMode('desktop');
    expect(visible()).toEqual(['My B', 'My A']);
    drag('my-a', 'my-b');
    expect(storage.get('echobird_appmgr_tool_order')).toBe(JSON.stringify(['my-a', 'my-b']));
    switchMode('install');
    expect(visible()).toEqual(['Store C', 'Store A', 'Store B']);
  });

  it('does not save an unchanged, missing or invalid drop, or a cancelled drag', () => {
    open();
    drag('store-a', 'store-a');
    drag('store-a', null);
    drag('missing', 'store-a');
    act(() => renderer.root.findByType(DndContext).props.onDragCancel());
    expect(setItem).not.toHaveBeenCalled();
    expect(visible()).toEqual(['Store A', 'Store B', 'Store C']);
  });

  it('keeps passive tab/category navigation read-only and ordinary clicks selecting', () => {
    open();
    category('toolCat.cli');
    category('toolCat.all');
    switchMode('desktop');
    switchMode('install');
    expect(setItem).not.toHaveBeenCalled();
    expect(select).not.toHaveBeenCalled();
    act(() => {
      renderer.root.findByProps({ 'aria-label': 'Store A' }).props.onClick();
    });
    expect(select).toHaveBeenCalledExactlyOnceWith('store-a');
  });

  it('keeps store categories visible above the scrolling app grid', () => {
    open();
    const categories = renderer.root
      .findAllByType('div')
      .find((node) => node.props.className?.includes('shrink-0 flex-wrap gap-1'))!;
    const scroller = renderer.root
      .findAllByType('div')
      .find((node) => node.props.className?.includes('overflow-y-auto'))!;
    expect(categories.parent).toBe(scroller.parent);
    expect(scroller.findAllByProps({ 'aria-pressed': true })).toHaveLength(0);
    expect(scroller.findAllByProps({ 'aria-label': 'Store A' })).toHaveLength(1);
  });

  it('falls back to the default order for missing or malformed local state', () => {
    storage.set('echobird_appmgr_store_order', 'invalid JSON');
    open();
    expect(visible()).toEqual(['Store A', 'Store B', 'Store C']);
    expect(setItem).not.toHaveBeenCalled();
  });

  it('clears the drag overlay on category and view changes without saving', () => {
    open();
    act(() => {
      renderer.root.findByType(DndContext).props.onDragStart({ active: { id: 'store-a' } });
    });
    expect(visible()).toEqual(['Store A', 'Store B', 'Store C', 'Store A']);
    category('toolCat.cli');
    expect(visible()).toEqual(['Store A', 'Store C']);
    act(() => {
      renderer.root.findByType(DndContext).props.onDragStart({ active: { id: 'store-a' } });
    });
    switchMode('desktop');
    switchMode('install');
    expect(visible()).toEqual(['Store A', 'Store C']);
    expect(setItem).not.toHaveBeenCalled();
  });

  it('keeps a completed drag usable when saving to local storage fails', () => {
    setItem.mockImplementation(() => {
      throw new Error('Storage unavailable');
    });
    open();
    drag('store-c', 'store-a');
    expect(visible()).toEqual(['Store C', 'Store A', 'Store B']);
    switchMode('desktop');
    switchMode('install');
    expect(visible()).toEqual(['Store C', 'Store A', 'Store B']);
    expect(select).not.toHaveBeenCalled();
  });

  it.each(['desktop', 'install'] as const)(
    '%s keeps neighboring icons at their exact position and original size during sort transitions',
    (view) => {
      vi.mocked(useSortable).mockReturnValue({
        attributes: {},
        listeners: {},
        setNodeRef: vi.fn(),
        transform: { x: 120.375, y: 18.625, scaleX: 1.08, scaleY: 1.12 },
        transition: 'transform 200ms',
      } as unknown as ReturnType<typeof useSortable>);
      open(view);
      const wrappers = renderer.root
        .findAllByType('div')
        .filter((node) => node.props.style?.transform);
      expect(wrappers).toHaveLength(view === 'desktop' ? 2 : 3);
      for (const wrapper of wrappers) {
        expect(wrapper.props.style.transform).toBe('translate3d(120.375px, 18.625px, 0)');
        expect(wrapper.props.style.transition).toBe('transform 200ms');
      }
    }
  );

  it.each(['desktop', 'install'] as const)(
    '%s finishes insertion from the visible position when released mid-animation',
    (view) => {
      const names = view === 'desktop' ? ['My A', 'My B'] : ['Store A', 'Store B', 'Store C'];
      const nodes = new Map(
        names.map((name, index) => [
          name,
          {
            getBoundingClientRect: vi
              .fn()
              .mockReturnValueOnce({ left: 50 + index * 120, top: 80 })
              .mockReturnValue({ left: (index - 1) * 120, top: 0 }),
            animate: vi.fn(() => ({ cancel: vi.fn() })),
          },
        ])
      );
      const matchMedia = vi.fn(() => ({ matches: false }));
      vi.stubGlobal('window', { matchMedia });
      open(view, (element) =>
        element.props.className?.startsWith('grid ')
          ? {
              get children() {
                return visible().map((name) => nodes.get(name));
              },
            }
          : null
      );
      const prefix = view === 'desktop' ? 'my-' : 'store-';
      drag(`${prefix}a`, `${prefix}${view === 'desktop' ? 'b' : 'c'}`);
      for (const name of names.slice(1)) {
        expect(nodes.get(name)!.animate).toHaveBeenCalledExactlyOnceWith(
          [{ transform: 'translate3d(170px, 80px, 0)' }, { transform: 'translate3d(0, 0, 0)' }],
          { duration: 200, easing: 'ease' }
        );
      }
      expect(nodes.get(names[0])!.animate).not.toHaveBeenCalled();
      expect(select).not.toHaveBeenCalled();
      if (view === 'desktop') switchMode('install');
      else switchMode('desktop');
      for (const name of names.slice(1)) {
        expect(nodes.get(name)!.animate.mock.results[0].value.cancel).toHaveBeenCalledOnce();
      }
    }
  );
});
