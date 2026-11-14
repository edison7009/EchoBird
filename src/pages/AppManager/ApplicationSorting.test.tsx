import React from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { DndContext } from '@dnd-kit/core';
import type { LocalTool } from '../../api/types';
import { DesktopIcon } from '../../components/DesktopToolIcon';
import { MiniSelect } from '../../components/MiniSelect';
import { AppManagerContext, type AppManagerContextType } from './context';
import { AppManagerMain } from './AppManagerComponents';
import { useNavigationStore } from '../../stores/navigationStore';
import * as systemIcons from '../../utils/systemApplicationIcons';

let documentVisible = true;
vi.mock('../../hooks/useDocumentVisible', () => ({ useDocumentVisible: () => documentVisible }));
vi.mock('../../components', () => ({ getModelIcon: () => null }));
vi.mock('../../utils/platform', () => ({ IS_WINDOWS: false, IS_LINUX: true, IS_MACOS: false }));
vi.mock('@dnd-kit/core', async (original) => ({
  ...(await original<typeof import('@dnd-kit/core')>()),
  DndContext: ({ children }: { children: React.ReactNode }) => children,
}));

const sortKey = 'echobird_appmgr_all_sort';
const orderKey = 'echobird_appmgr_all_order';
const tools: LocalTool[] = [
  { id: 'z', name: 'Zulu', installed: true, category: 'Desktop', createdAt: 200 },
  { id: 'a10', name: 'App 10', installed: true, category: 'Desktop', createdAt: 100 },
  { id: 'missing', name: 'Missing date', installed: true, category: 'Desktop' },
  { id: 'a2', name: 'app 2', installed: true, category: 'Desktop', createdAt: 200 },
  { id: 'zero', name: 'Zero', installed: true, category: 'Desktop', createdAt: 0 },
  { id: 'null', name: 'Null date', installed: true, category: 'Desktop', createdAt: null },
];

describe('All application sorting', () => {
  let renderer: ReactTestRenderer | undefined;
  let storage: Map<string, string>;
  let writes: ReturnType<typeof vi.fn>;
  const select = vi.fn();
  const scan = vi.fn();

  beforeEach(() => {
    documentVisible = true;
    storage = new Map([[orderKey, JSON.stringify(['z', 'a10', 'missing', 'a2', 'zero', 'null'])]]);
    writes = vi.fn((key: string, value: string) => storage.set(key, value));
    vi.stubGlobal('__APP_EDITION__', 'full');
    vi.stubGlobal('document', { addEventListener: vi.fn(), removeEventListener: vi.fn() });
    vi.stubGlobal('window', { matchMedia: () => ({ matches: false }) });
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: writes,
    });
    select.mockClear();
    scan.mockClear();
  });

  afterEach(() => {
    act(() => renderer?.unmount());
    renderer = undefined;
    vi.unstubAllGlobals();
  });

  const renderView = (viewMode: 'all' | 'desktop' | 'install' = 'all', entries = tools) => {
    const context = {
      viewMode,
      detectedTools: entries,
      isScanning: false,
      selectedTool: null,
      setSelectedTool: select,
      scanTools: scan,
      aiInstallableIds: [],
    } as unknown as AppManagerContextType;
    const view = (
      <AppManagerContext.Provider value={context}>
        <AppManagerMain />
      </AppManagerContext.Provider>
    );
    act(() => {
      if (renderer) renderer.update(view);
      else renderer = create(view);
    });
  };
  const ids = () => renderer!.root.findAllByType(DesktopIcon).map((icon) => icon.props.tool.id);
  const chooseSort = (mode: string) => {
    act(() => renderer!.root.findByType(MiniSelect).props.onChange(mode));
  };

  it('cancels preloading on page exits and restarts for the current filtered view', () => {
    const preload = vi.spyOn(systemIcons, 'preloadSystemApplicationIcons').mockResolvedValue();
    const signals: AbortSignal[] = [];
    preload.mockImplementation(async (_ids, signal) => {
      signals.push(signal!);
    });
    useNavigationStore.setState(useNavigationStore.getInitialState(), true);
    try {
      act(() => useNavigationStore.getState().setAppsTab('all'));
      renderView(
        'all',
        tools.map((tool) => ({ ...tool, systemAppId: tool.id }))
      );
      expect(preload).toHaveBeenCalledOnce();
      expect(signals[0].aborted).toBe(false);
      act(() => useNavigationStore.getState().setActivePage('models'));
      expect(signals[0].aborted).toBe(true);
      expect(preload).toHaveBeenCalledOnce();
      act(() => useNavigationStore.getState().setActivePage('apps'));
      expect(preload).toHaveBeenCalledTimes(2);
      act(() => renderer!.root.findByType('input').props.onChange({ target: { value: 'app' } }));
      expect(signals[1].aborted).toBe(true);
      expect(preload).toHaveBeenLastCalledWith(['a10', 'a2'], signals[2]);
      act(() => renderer!.unmount());
      renderer = undefined;
      expect(signals[2].aborted).toBe(true);
    } finally {
      preload.mockRestore();
      useNavigationStore.setState(useNavigationStore.getInitialState(), true);
    }
  });

  it('pauses background icon work while its native window is hidden', () => {
    const preload = vi.spyOn(systemIcons, 'preloadSystemApplicationIcons').mockResolvedValue();
    const entries = tools.map((tool) => ({ ...tool, systemAppId: tool.id }));
    useNavigationStore.setState(useNavigationStore.getInitialState(), true);
    try {
      act(() => useNavigationStore.getState().setAppsTab('all'));
      renderView('all', entries);
      const signal = preload.mock.calls[0][1]!;
      documentVisible = false;
      renderView('all', entries);
      expect(signal.aborted).toBe(true);
      expect(preload).toHaveBeenCalledOnce();
      documentVisible = true;
      renderView('all', entries);
      expect(preload).toHaveBeenCalledTimes(2);
      expect(preload.mock.calls[1][1]!.aborted).toBe(false);
    } finally {
      preload.mockRestore();
      useNavigationStore.setState(useNavigationStore.getInitialState(), true);
    }
  });

  it('restores manual order and reuses the compact shared dropdown beside search', () => {
    renderView();
    expect(ids()).toEqual(['z', 'a10', 'missing', 'a2', 'zero', 'null']);
    const dropdown = renderer!.root.findByType(MiniSelect);
    expect(dropdown.props.value).toBe('default');
    expect(dropdown.props.options.map((option: { id: string }) => option.id)).toEqual([
      'default',
      'name',
      'date',
    ]);
    expect(dropdown.parent?.findAllByType('input')).toHaveLength(1);
    expect(dropdown.props.className).toContain('[&>button]:h-9');
    expect(writes).not.toHaveBeenCalled();
    expect(select).not.toHaveBeenCalled();
    expect(scan).not.toHaveBeenCalled();
  });

  it('sorts names naturally, preserves saved drag order, and remembers the selection', () => {
    renderView();
    chooseSort('name');
    expect(ids()).toEqual(['a2', 'a10', 'missing', 'null', 'zero', 'z']);
    expect(storage.get(sortKey)).toBe('name');
    expect(writes).toHaveBeenCalledExactlyOnceWith(sortKey, 'name');
    act(() => renderer!.unmount());
    renderer = undefined;
    renderView();
    expect(renderer!.root.findByType(MiniSelect).props.value).toBe('name');
    expect(ids()[0]).toBe('a2');
    chooseSort('default');
    expect(ids()).toEqual(['z', 'a10', 'missing', 'a2', 'zero', 'null']);
  });

  it('sorts newest creation first with name ties, zero dates, and unknown dates last', () => {
    storage.set(sortKey, 'date');
    renderView();
    expect(ids()).toEqual(['a2', 'z', 'a10', 'zero', 'missing', 'null']);
    act(() => renderer!.root.findByType('input').props.onChange({ target: { value: 'app' } }));
    expect(ids()).toEqual(['a2', 'a10']);
    expect(writes).not.toHaveBeenCalled();
    expect(select).not.toHaveBeenCalled();
    expect(scan).not.toHaveBeenCalled();
  });

  it.each(['name', 'date'])(
    'dragging in %s switches to default and saves visible slots',
    (mode) => {
      storage.set(sortKey, mode);
      renderView();
      act(() => renderer!.root.findByType('input').props.onChange({ target: { value: 'app' } }));
      expect(ids()).toEqual(['a2', 'a10']);
      act(() =>
        renderer!.root
          .findByType(DndContext)
          .props.onDragEnd({ active: { id: 'a2' }, over: { id: 'a10' } })
      );
      expect(ids()).toEqual(['a10', 'a2']);
      expect(storage.get(sortKey)).toBe('default');
      const expected =
        mode === 'date'
          ? ['a10', 'z', 'a2', 'zero', 'missing', 'null']
          : ['a10', 'a2', 'missing', 'null', 'zero', 'z'];
      expect(JSON.parse(storage.get(orderKey)!)).toEqual(expected);
      act(() => renderer!.root.findByType('input').props.onChange({ target: { value: '' } }));
      expect(ids()).toEqual(expected);
      expect(select).not.toHaveBeenCalled();
      expect(scan).not.toHaveBeenCalled();
    }
  );

  it('ignores cancelled and unchanged drags and keeps My and Store orders separate', () => {
    storage.set(sortKey, 'date');
    storage.set('echobird_appmgr_tool_order', JSON.stringify(['zero', 'z']));
    storage.set('echobird_appmgr_store_order', JSON.stringify(['a10', 'a2']));
    renderView();
    act(() => renderer!.root.findByType(DndContext).props.onDragCancel());
    act(() =>
      renderer!.root.findByType(DndContext).props.onDragEnd({ active: { id: 'z' }, over: null })
    );
    act(() =>
      renderer!.root
        .findByType(DndContext)
        .props.onDragEnd({ active: { id: 'z' }, over: { id: 'z' } })
    );
    expect(writes).not.toHaveBeenCalled();
    renderView('desktop');
    expect(renderer!.root.findAllByType(MiniSelect)).toHaveLength(0);
    expect(ids().slice(0, 2)).toEqual(['zero', 'z']);
    renderView(
      'install',
      tools.map((tool) => ({ ...tool, installed: false }))
    );
    expect(ids().slice(0, 2)).toEqual(['a10', 'a2']);
    renderView();
    expect(renderer!.root.findByType(MiniSelect).props.value).toBe('date');
    expect(ids()).toEqual(['a2', 'z', 'a10', 'zero', 'missing', 'null']);
  });

  it('tolerates invalid saved modes and blocked storage without losing in-memory sorting', () => {
    storage.set(sortKey, 'invalid');
    renderView();
    expect(renderer!.root.findByType(MiniSelect).props.value).toBe('default');
    act(() => renderer!.unmount());
    renderer = undefined;
    vi.stubGlobal('localStorage', {
      getItem: () => {
        throw new Error('blocked');
      },
      setItem: () => {
        throw new Error('blocked');
      },
    });
    renderView();
    chooseSort('name');
    expect(ids()).toEqual(['a2', 'a10', 'missing', 'null', 'zero', 'z']);
  });

  it.each(['null', '{}', '42', '"order"'])(
    'ignores a malformed saved application order %s',
    (saved) => {
      storage.set(orderKey, saved);
      renderView();
      expect(ids()).toHaveLength(tools.length);
      expect(writes).not.toHaveBeenCalled();
    }
  );

  it('keeps valid ids from a partially malformed saved order', () => {
    storage.set(orderKey, JSON.stringify(['z', null, 42]));
    renderView();
    expect(ids()[0]).toBe('z');
    expect(ids()).toHaveLength(tools.length);
    expect(writes).not.toHaveBeenCalled();
  });
});
