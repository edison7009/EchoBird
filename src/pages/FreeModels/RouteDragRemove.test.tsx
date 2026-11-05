import { useLayoutEffect, type ReactNode } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  DndContext,
  DragOverlay,
  closestCenter,
  pointerWithin,
  rectIntersection,
} from '@dnd-kit/core';
import * as api from '../../api/tauri';
import type { ModelConfig } from '../../api/types';
import { useNavigationStore } from '../../stores/navigationStore';
import { FreeModelsMain, FreeModelsProvider, useFreeModels } from './FreeModels';

const { confirm, showToast, dropState, dropRef, dropHook, measure, nexus } = vi.hoisted(() => ({
  confirm: vi.fn(),
  showToast: vi.fn(),
  dropState: { isOver: false },
  dropRef: vi.fn(),
  dropHook: vi.fn(),
  measure: vi.fn(),
  nexus: { updateUserModel: vi.fn(), setShowAddModelModal: vi.fn() },
}));
vi.mock('../../hooks/useI18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }));
vi.mock('../../components', () => ({ getModelIcon: () => null }));
vi.mock('../../components/ConfirmDialog', () => ({ useConfirm: () => confirm }));
vi.mock('../../components/Toast', () => ({ useToast: () => ({ showToast }) }));
vi.mock('../ModelNexus/context', () => ({ useModelNexus: () => nexus }));
vi.mock('react-dom', () => ({ createPortal: (children: ReactNode) => children }));
vi.mock('@dnd-kit/core', () => ({
  DndContext: ({ children }: { children: ReactNode }) => children,
  DragOverlay: ({ children }: { children: ReactNode }) => children,
  PointerSensor: vi.fn(),
  KeyboardSensor: vi.fn(),
  closestCenter: vi.fn(),
  pointerWithin: vi.fn(),
  rectIntersection: vi.fn(),
  useDroppable: (options: unknown) => {
    dropHook(options);
    return { setNodeRef: dropRef, isOver: dropState.isOver };
  },
  useDndContext: () => ({ measureDroppableContainers: measure }),
  useSensor: vi.fn(),
  useSensors: vi.fn(),
}));
vi.mock('@dnd-kit/sortable', async (original) => ({
  ...(await original<typeof import('@dnd-kit/sortable')>()),
  SortableContext: ({ children }: { children: ReactNode }) => children,
  useSortable: () => ({
    attributes: { tabIndex: 0 },
    listeners: { onPointerDown: vi.fn() },
    setNodeRef: vi.fn(),
    setActivatorNodeRef: vi.fn(),
    transform: null,
    transition: undefined,
    isDragging: false,
  }),
}));
vi.mock('../../api/tauri', () => ({
  getSmartRouterConfig: vi.fn(),
  listSmartRouters: async () => [await api.getSmartRouterConfig()],
  getSmartRouterCandidates: vi.fn(),
  getSmartRouterActivity: vi.fn(),
  setSmartRouterEnabled: vi.fn(),
  setSmartRouterCandidateEnabled: vi.fn(),
  setSmartRouterCandidates: vi.fn(),
  removeSmartRouterCandidate: vi.fn(),
  getFreeModelDirectory: vi.fn(),
  getCachedFreeModelDirectory: vi.fn(),
  deleteModel: vi.fn(),
  updateModel: vi.fn(),
  queryModelUsage: vi.fn(),
}));

const REMOVE_TARGET = 'router-remove-drop-zone';

describe('Remove router candidates by dragging', () => {
  let renderer: ReactTestRenderer;
  let context: ReturnType<typeof useFreeModels>;
  let savedModels: ModelConfig[];
  let savedOrder: string[];
  let enabled: boolean;
  let paused: string[];

  function config() {
    return {
      id: 'smart-router',
      name: 'Auto Router',
      masterEnabled: enabled,
      groupEnabled: true,
      enabled,
      candidateIds: [...savedOrder],
      disabledCandidateIds: [...paused],
      usableCandidateCount: savedOrder.length,
      baseUrl: 'http://127.0.0.1:62185/v1',
      modelId: 'smart-router',
      port: 62185,
      running: enabled,
    };
  }
  function Harness() {
    const value = useFreeModels();
    useLayoutEffect(() => {
      context = value;
    });
    return <FreeModelsMain />;
  }
  function app(showMain = true) {
    return <FreeModelsProvider>{showMain && <Harness />}</FreeModelsProvider>;
  }
  async function open() {
    await act(async () => {
      renderer = create(app());
    });
  }
  async function start(id = 'model-two') {
    await act(async () => {
      renderer.root.findByType(DndContext).props.onDragStart({ active: { id } });
    });
  }
  async function end(overId: string | null, id = 'model-two') {
    await act(async () => {
      renderer.root.findByType(DndContext).props.onDragEnd({
        active: { id },
        over: overId ? { id: overId } : null,
      });
    });
  }
  function target() {
    return renderer.root.findByProps({ id: REMOVE_TARGET });
  }
  function zone() {
    return renderer.root.find(
      (node) =>
        node.type === 'div' && String(node.props.className).includes('free-model-remove-zone')
    );
  }
  function expectNoRemoval() {
    expect(api.removeSmartRouterCandidate).not.toHaveBeenCalled();
    expect(api.setSmartRouterCandidates).not.toHaveBeenCalled();
    expect(api.setSmartRouterEnabled).not.toHaveBeenCalled();
    expect(api.deleteModel).not.toHaveBeenCalled();
    expect(api.updateModel).not.toHaveBeenCalled();
    expect(confirm).not.toHaveBeenCalled();
    expect(nexus.setShowAddModelModal).not.toHaveBeenCalled();
  }

  beforeEach(() => {
    enabled = false;
    paused = [];
    savedModels = ['one', 'two', 'three'].map((name) => ({
      internalId: `model-${name}`,
      name: `Provider ${name}`,
      modelId: `model-id-${name}`,
      baseUrl: 'https://example/v1',
      apiKey: 'stored-secret',
      scope: 'smartRouter',
    }));
    savedOrder = savedModels.map((model) => model.internalId);
    dropState.isOver = false;
    vi.stubGlobal('window', { setInterval, clearInterval, setTimeout });
    vi.stubGlobal(
      'document',
      Object.assign(new EventTarget(), { body: {}, visibilityState: 'visible' })
    );
    vi.stubGlobal('requestAnimationFrame', () => 1);
    vi.stubGlobal('cancelAnimationFrame', vi.fn());
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    useNavigationStore.setState({ activePage: 'freeModels' });
    vi.mocked(api.getSmartRouterConfig).mockImplementation(async () => config());
    vi.mocked(api.getSmartRouterCandidates).mockImplementation(async () =>
      savedOrder.map((id) => savedModels.find((model) => model.internalId === id)!)
    );
    vi.mocked(api.getCachedFreeModelDirectory).mockResolvedValue(null);
    vi.mocked(api.getSmartRouterActivity).mockResolvedValue({
      candidateId: null,
      active: false,
      sequence: 0,
      updatedAtMs: 0,
    });
    vi.mocked(api.removeSmartRouterCandidate).mockImplementation(async (id) => {
      savedOrder = savedOrder.filter((candidate) => candidate !== id);
      return config();
    });
    vi.mocked(api.setSmartRouterCandidates).mockImplementation(async (ids) => {
      savedOrder = [...ids];
      return config();
    });
    vi.mocked(api.setSmartRouterEnabled).mockImplementation(async (value) => {
      enabled = value;
      return config();
    });
    vi.mocked(api.setSmartRouterCandidateEnabled).mockImplementation(async (id, value) => {
      paused = paused.filter((candidate) => candidate !== id);
      if (!value) paused.push(id);
      return config();
    });
  });

  afterEach(() => {
    act(() => renderer?.unmount());
    vi.resetAllMocks();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it('shows no card edit or remove buttons and makes no passive model changes', async () => {
    await open();
    const buttons = renderer.root.findAllByType('button');
    expect(
      buttons.filter((button) => /^btn\.(edit|remove)/.test(button.props['aria-label'] ?? ''))
    ).toHaveLength(0);
    expect(api.getFreeModelDirectory).not.toHaveBeenCalled();
    expect(api.queryModelUsage).not.toHaveBeenCalled();
    expect(target().props['aria-hidden']).toBe(true);
    expect(zone().props.className).not.toContain('is-visible');
    expect(dropHook).toHaveBeenLastCalledWith(
      expect.objectContaining({ id: REMOVE_TARGET, disabled: true })
    );
    const hint = renderer.root
      .findAllByType('p')
      .find((node) =>
        node
          .findAllByType('span')
          .some((child) => child.children.includes('freeModels.router.reorderHint'))
      )!;
    expect(hint).toBeDefined();
    expect(hint.findByProps({ role: 'img', 'aria-label': 'btn.delete' })).toBeDefined();
    expect(hint.findByProps({ 'aria-hidden': 'true' }).children).toEqual(['|']);
    expect(hint.props.className).toContain('flex-wrap');
    expect(hint.props.className).toContain('items-center');
    expect(hint.findAllByType('button')).toHaveLength(0);
    expectNoRemoval();
  });

  it('reveals the borderless removal zone and inverts its colors only while the dragged card hits it', async () => {
    await open();
    await start();
    expect(target().props['aria-hidden']).toBe(false);
    const targetLayer = Number(target().props.className.match(/z-\[(\d+)\]/)[1]);
    expect(renderer.root.findByType(DragOverlay).props.zIndex).toBeGreaterThan(targetLayer);
    expect(zone().props.className).toContain('is-visible');
    expect(zone().props.className).not.toContain('is-over');
    const visibleClass = zone().props.className;
    expect(visibleClass.split(/\s+/)).not.toContain('border');
    expect(dropHook).toHaveBeenLastCalledWith(
      expect.objectContaining({ id: REMOVE_TARGET, disabled: false })
    );
    expect(
      zone().findAll((node) => node.children.includes('freeModels.router.dropToRemove'))
    ).toHaveLength(1);
    dropState.isOver = true;
    await act(async () => renderer.update(app()));
    expect(zone().props.className).toContain('is-over');
    expect(
      zone().findAll((node) => node.children.includes('freeModels.router.releaseToRemove'))
    ).toHaveLength(1);
    dropState.isOver = false;
    await act(async () => renderer.update(app()));
    expect(zone().props.className).toBe(visibleClass);
    expect(
      zone().findAll((node) => node.children.includes('freeModels.router.dropToRemove'))
    ).toHaveLength(1);
    dropState.isOver = true;
    await act(async () => renderer.update(app()));
    await act(async () => renderer.root.findByType(DndContext).props.onDragCancel());
    expect(target().props['aria-hidden']).toBe(true);
    expect(zone().props.className).not.toContain('is-visible');
    expect(zone().props.className).not.toContain('is-over');
    expectNoRemoval();
  });

  it('positions the removal target after returning with cached models', async () => {
    const stage = {
      getBoundingClientRect: () => ({ left: 100, width: 800, bottom: 750 }),
    };
    vi.stubGlobal('window', {
      setInterval,
      clearInterval,
      setTimeout,
      innerHeight: 900,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    });
    vi.stubGlobal(
      'ResizeObserver',
      class {
        observe = vi.fn();
        disconnect = vi.fn();
      }
    );
    await act(async () => {
      renderer = create(app(), {
        createNodeMock: (element) =>
          element.type === 'div' &&
          String(element.props.className).includes('relative overflow-hidden')
            ? stage
            : null,
      });
    });
    await act(async () => renderer.update(app(false)));
    await act(async () => renderer.update(app()));
    expectNoRemoval();
    const measuredPositions: unknown[] = [];
    measure.mockImplementation(() => measuredPositions.push({ ...target().props.style }));
    await start();
    expect(target().props.style).toEqual({ left: 100, width: 800, bottom: 162 });
    expect(measure).toHaveBeenCalledWith([REMOVE_TARGET]);
    expect(measuredPositions).toEqual([{ left: 100, width: 800, bottom: 162 }]);
    expectNoRemoval();
  });

  it('remeasures the active removal target after scroll and resize commit its new position', async () => {
    let bounds = { left: 100, width: 800, bottom: 750 };
    const stage = { getBoundingClientRect: () => bounds };
    const listeners = new Map<string, () => void>();
    vi.stubGlobal('window', {
      setInterval,
      clearInterval,
      setTimeout,
      innerHeight: 900,
      addEventListener: vi.fn((event: string, listener: () => void) => {
        listeners.set(event, listener);
      }),
      removeEventListener: vi.fn(),
    });
    vi.stubGlobal(
      'ResizeObserver',
      class {
        observe = vi.fn();
        disconnect = vi.fn();
      }
    );
    await act(async () => {
      renderer = create(app(), {
        createNodeMock: (element) =>
          element.type === 'div' &&
          String(element.props.className).includes('relative overflow-hidden')
            ? stage
            : null,
      });
    });
    const stageHost = renderer.root.find(
      (node) =>
        node.type === 'div' && String(node.props.className).includes('relative overflow-hidden')
    );
    expect(stageHost.props.className.split(/\s+/)).not.toContain('h-full');
    expect(stageHost.props.style.height).toBeUndefined();
    expect(stageHost.props.className.split(/\s+/)).toContain('flex-1');
    expect(stageHost.props.style.minHeight).toBeGreaterThanOrEqual(550);
    const initial = { left: 100, width: 800, bottom: 162 };
    expect(target().props.style).toEqual(initial);
    expect(measure).not.toHaveBeenCalled();
    const committedPositions: unknown[] = [];
    measure.mockImplementation(() => {
      committedPositions.push({ ...target().props.style });
    });
    await start();
    expect(measure).toHaveBeenCalledExactlyOnceWith([REMOVE_TARGET]);
    expect(committedPositions).toEqual([initial]);

    bounds = { left: 80, width: 680, bottom: 1100 };
    await act(async () => listeners.get('scroll')!());
    const scrolled = { left: 80, width: 680, bottom: 12 };
    expect(target().props.style).toEqual(scrolled);
    expect(measure).toHaveBeenCalledTimes(2);
    expect(measure).toHaveBeenLastCalledWith([REMOVE_TARGET]);
    expect(committedPositions).toEqual([initial, scrolled]);

    bounds = { left: 150, width: 900, bottom: 750 };
    await act(async () => listeners.get('resize')!());
    const resized = { left: 150, width: 900, bottom: 162 };
    expect(target().props.style).toEqual(resized);
    expect(measure).toHaveBeenCalledTimes(3);
    expect(committedPositions).toEqual([initial, scrolled, resized]);

    await act(async () => renderer.root.findByType(DndContext).props.onDragCancel());
    bounds = { left: 200, width: 750, bottom: 700 };
    await act(async () => listeners.get('scroll')!());
    expect(target().props['aria-hidden']).toBe(true);
    expect(measure).toHaveBeenCalledTimes(3);
    expectNoRemoval();
  });

  async function openGeometry(count: number, width = 800, measuredLength = 354) {
    savedModels = Array.from({ length: count }, (_, index) => ({
      internalId: `geometry-${index}`,
      name: `Provider ${index}`,
      modelId: `model-${index}`,
      baseUrl: 'https://example/v1',
      apiKey: '',
      scope: 'smartRouter',
    }));
    savedOrder = savedModels.map((model) => model.internalId);
    let stageWidth = width;
    let hubOffset = 0;
    const getTotalLength = vi.fn().mockReturnValue(measuredLength);
    const glowStyle = new Map<string, string>();
    const frames = new Map<number, FrameRequestCallback>();
    const observers = new Set<() => void>();
    let frameId = 0;
    vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
      frames.set(++frameId, callback);
      return frameId;
    });
    vi.stubGlobal('cancelAnimationFrame', (id: number) => frames.delete(id));
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(private callback: () => void) {}
        observe() {
          observers.add(this.callback);
        }
        disconnect() {
          observers.delete(this.callback);
        }
      }
    );
    vi.stubGlobal('window', {
      setInterval,
      clearInterval,
      setTimeout,
      innerHeight: 900,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    });
    const rect = (left: number, top: number, nodeWidth: number, height: number) => ({
      left,
      top,
      width: nodeWidth,
      height,
      bottom: top + height,
    });
    async function flushFrames() {
      for (let pass = 0; frames.size && pass < 10; pass++) {
        const pending = [...frames.values()];
        frames.clear();
        await act(async () => pending.forEach((callback) => callback(0)));
      }
      expect(frames.size).toBe(0);
    }
    await act(async () => {
      renderer = create(app(), {
        createNodeMock: (element) => {
          if (element.type === 'path' && element.props.className === 'free-model-route-glow')
            return {
              getTotalLength,
              style: { setProperty: (key: string, value: string) => glowStyle.set(key, value) },
            };
          if (element.type !== 'div') return null;
          const className = String(element.props.className);
          if (className.includes('relative overflow-hidden')) {
            return { getBoundingClientRect: () => rect(40, 50, stageWidth, 620) };
          }
          if (className.includes('free-model-router-hub')) {
            return {
              getBoundingClientRect: () =>
                rect(40 + stageWidth / 2 - 190 + hubOffset, 50, 380, 140),
            };
          }
          const model = element.props.children?.props?.children?.props?.model;
          if (className === 'min-w-0' && model) {
            const index = savedModels.findIndex((candidate) => candidate.internalId === model.id);
            return {
              getBoundingClientRect: () => {
                const columns = Math.max(1, Math.floor((stageWidth + 24) / 264));
                const x = stageWidth / 2 + ((index % columns) - (columns - 1) / 2) * 200;
                return rect(40 + x - 80, 50 + 204 + Math.floor(index / columns) * 116, 160, 80);
              },
            };
          }
          return null;
        },
      });
    });
    await flushFrames();
    return {
      flushFrames,
      getTotalLength,
      glowStyle,
      moveHub: async (offset: number) => {
        hubOffset = offset;
        await act(async () => [...observers].forEach((callback) => callback()));
        await flushFrames();
      },
      resize: async (nextWidth: number) => {
        stageWidth = nextWidth;
        await act(async () => [...observers].forEach((callback) => callback()));
        await flushFrames();
      },
    };
  }

  function visiblePaths() {
    return renderer.root
      .findAllByType('path')
      .filter((node) => node.props.className === 'free-model-route-path')
      .map((node) => node.props.d as string);
  }

  it('keeps the routing diagram fixed when the upper card moves horizontally', async () => {
    const geometry = await openGeometry(2);
    const original = visiblePaths();
    await geometry.moveHub(272);
    expect(visiblePaths()).toEqual(original);
    await geometry.moveHub(-272);
    expect(visiblePaths()).toEqual(original);
    await geometry.resize(600);
    expect(visiblePaths()).not.toEqual(original);
  });

  it('omits a paused tail and the separate lines inside paused cards', async () => {
    paused = ['geometry-4', 'geometry-5'];
    await openGeometry(8);
    expect(visiblePaths()).toHaveLength(10);
    expect(visiblePaths()).toContain('M 400 317 V 287');
    expect(visiblePaths()).toContain('M 400 433 V 403');
    expect(visiblePaths()).not.toContain('M 600 317 V 287');
    expect(visiblePaths()).not.toContain('M 400 403 V 317');
    expect(api.setSmartRouterCandidateEnabled).not.toHaveBeenCalled();
    expectNoRemoval();
  });

  it('removes an entirely paused column and its top branch', async () => {
    paused = ['geometry-2', 'geometry-5'];
    await openGeometry(8);
    expect(visiblePaths().some((path) => path.startsWith('M 600 '))).toBe(false);
    expect(visiblePaths().some((path) => path.startsWith('M 588 '))).toBe(false);
    expect(visiblePaths()).toContain('M 400 433 V 403');
    expectNoRemoval();
  });

  it('keeps a lone off-center enabled card connected to the hub', async () => {
    paused = Array.from({ length: 7 }, (_, index) => `geometry-${index + 1}`);
    await openGeometry(8);
    expect(visiblePaths()).toEqual([
      'M 400 172 V 158',
      'M 200 201 V 184 A 12 12 0 0 1 212 172',
      'M 212 172 H 400',
    ]);
    expectNoRemoval();
  });

  it.each([240, 420, 800])('draws no routes when all cards are paused at %dpx', async (width) => {
    paused = Array.from({ length: 8 }, (_, index) => `geometry-${index}`);
    await openGeometry(8, width);
    expect(visiblePaths()).toEqual([]);
    expect(renderer.root.findAllByProps({ className: 'free-model-route-arrow' })).toHaveLength(0);
    expect(api.setSmartRouterCandidateEnabled).not.toHaveBeenCalled();
    expectNoRemoval();
  });

  it('trims and restores the column when its last enabled card is toggled', async () => {
    paused = ['geometry-4', 'geometry-5'];
    const geometry = await openGeometry(8);
    await act(async () => context.setCandidateEnabled('geometry-7', false));
    await geometry.flushFrames();
    expect(visiblePaths()).not.toContain('M 400 317 V 287');
    expect(visiblePaths()).not.toContain('M 400 433 V 403');
    await act(async () => context.setCandidateEnabled('geometry-7', true));
    await geometry.flushFrames();
    expect(visiblePaths()).toContain('M 400 317 V 287');
    expect(visiblePaths()).toContain('M 400 433 V 403');
    expect(context.pausedIds.has('geometry-4')).toBe(true);
    expectNoRemoval();
  });

  it('keeps the short activity beam 20px long and remeasures after resizing without account or model changes', async () => {
    enabled = true;
    vi.mocked(api.getSmartRouterActivity).mockResolvedValue({
      candidateId: 'geometry-1',
      active: true,
      sequence: 1,
      updatedAtMs: Date.now(),
    });
    const geometry = await openGeometry(3, 800, 43);
    const span = () => Number(geometry.glowStyle.get('--free-model-route-glow-span'));
    const glow = () => renderer.root.findByProps({ className: 'free-model-route-glow' });
    expect(glow().props.d).toBe('M 400 201 V 172 V 158');
    expect(glow().props.pathLength).toBe('1');
    expect(geometry.getTotalLength).toHaveBeenCalled();
    expect(span() * 43).toBeCloseTo(20);
    const measurements = geometry.getTotalLength.mock.calls.length;
    const reads = [
      api.getSmartRouterConfig,
      api.getSmartRouterCandidates,
      api.getSmartRouterActivity,
      api.getFreeModelDirectory,
      api.getCachedFreeModelDirectory,
    ].map((read) => [read, vi.mocked(read).mock.calls.length] as const);
    geometry.getTotalLength.mockReturnValue(159);
    await geometry.resize(240);
    expect(glow().props.d).toBe('M 120 317 V 287 V 201 V 158');
    expect(geometry.getTotalLength.mock.calls.length).toBeGreaterThan(measurements);
    expect(span() * 159).toBeCloseTo(20);
    for (const [read, calls] of reads) expect(read).toHaveBeenCalledTimes(calls);
    expect(api.queryModelUsage).not.toHaveBeenCalled();
    expect(nexus.updateUserModel).not.toHaveBeenCalled();
    expectNoRemoval();
  });

  it('retains the original seven-percent activity beam on a longer route', async () => {
    enabled = true;
    vi.mocked(api.getSmartRouterActivity).mockResolvedValue({
      candidateId: 'geometry-3',
      active: true,
      sequence: 1,
      updatedAtMs: Date.now(),
    });
    const geometry = await openGeometry(6);
    expect(geometry.getTotalLength).toHaveBeenCalled();
    expect(Number(geometry.glowStyle.get('--free-model-route-glow-span'))).toBe(0.07);
    expect(api.queryModelUsage).not.toHaveBeenCalled();
    expect(nexus.updateUserModel).not.toHaveBeenCalled();
    expectNoRemoval();
  });

  it('joins the rounded outer branches to one bus while preserving the complete lower-row activity path', async () => {
    enabled = true;
    vi.mocked(api.getSmartRouterActivity).mockResolvedValue({
      candidateId: 'geometry-3',
      active: true,
      sequence: 1,
      updatedAtMs: Date.now(),
    });
    await openGeometry(6);
    const grid = renderer.root.findByProps({ className: 'free-model-node-grid relative z-10' });
    expect(grid.props.style.gridTemplateColumns).toMatch(/^repeat\(3,/);
    const paths = visiblePaths();
    const branches = paths.filter((path) => path.includes(' A '));
    expect(branches).toEqual([
      'M 200 201 V 184 A 12 12 0 0 1 212 172',
      'M 600 201 V 184 A 12 12 0 0 0 588 172',
    ]);
    expect(paths).toHaveLength(9);
    expect(paths.every((path) => !path.includes(' C '))).toBe(true);
    expect(paths.filter((path) => path.includes(' H '))).toEqual([
      'M 212 172 H 400',
      'M 588 172 H 400',
    ]);
    expect(paths).toContain('M 400 172 V 158');
    expect(paths).toContain('M 400 201 V 172');
    expect(paths).toContain('M 200 317 V 287');
    expect(paths).toContain('M 400 317 V 287');
    expect(paths).toContain('M 600 317 V 287');
    const glow = renderer.root.findByProps({ className: 'free-model-route-glow' });
    expect(glow.props.d).toMatch(/^M 200 317 V 287 V 201 /);
    expect(glow.props.d).toBe(
      `M 200 317 V 287 V 201 ${branches[0].slice('M 200 201 '.length)} H 400 V 158`
    );
    expectNoRemoval();
  });

  it('adds columns beyond three on wide stages and reconnects without changing candidates', async () => {
    const geometry = await openGeometry(8, 1400);
    const grid = () =>
      renderer.root.findByProps({ className: 'free-model-node-grid relative z-10' });
    expect(grid().props.style.gridTemplateColumns).toMatch(/^repeat\(5,/);
    expect(visiblePaths().length).toBeGreaterThan(0);
    const reads = vi.mocked(api.getSmartRouterCandidates).mock.calls.length;
    await geometry.resize(2000);
    expect(grid().props.style.gridTemplateColumns).toMatch(/^repeat\(7,/);
    expect(visiblePaths().length).toBeGreaterThan(0);
    await geometry.resize(240);
    expect(grid().props.style.gridTemplateColumns).toMatch(/^repeat\(1,/);
    expect(visiblePaths().every((path) => !/[AH]/.test(path))).toBe(true);
    expect(api.getSmartRouterCandidates).toHaveBeenCalledTimes(reads);
    expectNoRemoval();
  });

  it('keeps odd-column and single-column connections clean after a stage resize', async () => {
    const geometry = await openGeometry(4);
    await geometry.resize(790);
    const grid = renderer.root.findByProps({ className: 'free-model-node-grid relative z-10' });
    expect(grid.props.style.gridTemplateColumns).toMatch(/^repeat\(3,/);
    let paths = visiblePaths();
    expect(paths.filter((path) => path.includes(' A '))).toEqual([
      'M 195 201 V 184 A 12 12 0 0 1 207 172',
      'M 595 201 V 184 A 12 12 0 0 0 583 172',
    ]);
    expect(paths.every((path) => !path.includes(' C '))).toBe(true);
    expect(paths).toContain('M 195 317 V 287');
    await geometry.resize(240);
    expect(grid.props.style.gridTemplateColumns).toMatch(/^repeat\(1,/);
    paths = visiblePaths();
    expect(paths).toHaveLength(4);
    expect(paths.every((path) => !/[AH]/.test(path))).toBe(true);
    expect(paths).toContain('M 120 201 V 158');
    expectNoRemoval();
  });

  it('clears the active glow when its candidate leaves and clears all paths when the pool becomes empty', async () => {
    enabled = true;
    vi.mocked(api.getSmartRouterActivity).mockResolvedValue({
      candidateId: 'geometry-4',
      active: true,
      sequence: 1,
      updatedAtMs: Date.now(),
    });
    const geometry = await openGeometry(6);
    expect(renderer.root.findAllByProps({ className: 'free-model-route-glow' })).toHaveLength(1);
    await act(async () => context.removeSelectedModel('geometry-4'));
    await geometry.flushFrames();
    expect(renderer.root.findAllByProps({ className: 'free-model-route-glow' })).toHaveLength(0);
    expect(visiblePaths().length).toBeGreaterThan(0);
    for (const id of [...context.selectedIds]) {
      await act(async () => context.removeSelectedModel(id));
    }
    await geometry.flushFrames();
    expect(visiblePaths()).toEqual([]);
    expect(renderer.root.findAllByProps({ className: 'free-model-route-arrow' })).toHaveLength(0);
    expect(api.deleteModel).not.toHaveBeenCalled();
    expect(savedModels).toHaveLength(6);
  });

  it.each(['cancel', 'outside', 'same card'] as const)(
    'retains the candidates and order when a drag ends by %s',
    async (result) => {
      await open();
      await start();
      expect(target().props['aria-hidden']).toBe(false);
      if (result === 'cancel') {
        await act(async () => renderer.root.findByType(DndContext).props.onDragCancel());
      } else {
        await end(result === 'outside' ? null : 'model-two');
      }
      expect([...context.selectedIds]).toEqual(['model-one', 'model-two', 'model-three']);
      expect(context.customModels).toHaveLength(3);
      expect(target().props['aria-hidden']).toBe(true);
      expectNoRemoval();
      expect(showToast).not.toHaveBeenCalled();
    }
  );

  it('ignores a removal target without a matching active drag', async () => {
    await open();
    await end(REMOVE_TARGET);
    expectNoRemoval();
    await start('model-one');
    await end(REMOVE_TARGET, 'model-two');
    expectNoRemoval();
    expect(target().props['aria-hidden']).toBe(true);
    expect([...context.selectedIds]).toEqual(['model-one', 'model-two', 'model-three']);
  });

  it('accepts direct removal-zone collisions and excludes the zone from nearest-card fallback', async () => {
    await open();
    await start();
    const detect = renderer.root.findByType(DndContext).props.collisionDetection;
    const removal = { id: REMOVE_TARGET };
    const card = { id: 'model-one' };
    const args = {
      pointerCoordinates: { x: 100, y: 200 },
      droppableContainers: [removal, card],
    };
    vi.mocked(pointerWithin).mockReturnValue([{ id: REMOVE_TARGET }]);
    expect(detect(args)).toEqual([{ id: REMOVE_TARGET }]);
    expect(pointerWithin).toHaveBeenLastCalledWith({
      ...args,
      droppableContainers: [removal],
    });
    expect(closestCenter).not.toHaveBeenCalled();
    expect(rectIntersection).not.toHaveBeenCalled();

    vi.mocked(pointerWithin).mockReturnValue([]);
    vi.mocked(closestCenter).mockReturnValue([{ id: 'model-one' }]);
    expect(detect(args)).toEqual([{ id: 'model-one' }]);
    expect(closestCenter).toHaveBeenLastCalledWith({ ...args, droppableContainers: [card] });

    const keyboardArgs = { ...args, pointerCoordinates: null };
    vi.mocked(rectIntersection).mockReturnValue([{ id: REMOVE_TARGET }]);
    expect(detect(keyboardArgs)).toEqual([{ id: REMOVE_TARGET }]);
    expect(rectIntersection).toHaveBeenLastCalledWith({
      ...keyboardArgs,
      droppableContainers: [removal],
    });
    vi.mocked(rectIntersection).mockReturnValue([]);
    expect(detect(keyboardArgs)).toEqual([{ id: 'model-one' }]);
    expect(closestCenter).toHaveBeenLastCalledWith({
      ...keyboardArgs,
      droppableContainers: [card],
    });
  });

  it('removes only the dropped candidate, preserves the saved model and persists the remaining order', async () => {
    await open();
    await start();
    await end(REMOVE_TARGET);
    expect(api.removeSmartRouterCandidate).toHaveBeenCalledExactlyOnceWith('model-two');
    expect(api.setSmartRouterCandidates).not.toHaveBeenCalled();
    expect(api.deleteModel).not.toHaveBeenCalled();
    expect(api.updateModel).not.toHaveBeenCalled();
    expect(savedModels.map((model) => model.internalId)).toEqual([
      'model-one',
      'model-two',
      'model-three',
    ]);
    expect([...context.selectedIds]).toEqual(['model-one', 'model-three']);
    expect(context.customModels.map((model) => model.id)).toEqual(['model-one', 'model-three']);
    expect(confirm).not.toHaveBeenCalled();
    expect(nexus.updateUserModel).not.toHaveBeenCalled();
    expect(showToast).not.toHaveBeenCalled();
    act(() => renderer.unmount());
    await open();
    expect([...context.selectedIds]).toEqual(['model-one', 'model-three']);
    expect(context.customModels).toHaveLength(2);
    expect(api.removeSmartRouterCandidate).toHaveBeenCalledOnce();
  });

  it('continues to reorder when dropping over another card', async () => {
    await open();
    await start();
    await end('model-one');
    expect(api.setSmartRouterCandidates).toHaveBeenCalledExactlyOnceWith([
      'model-two',
      'model-one',
      'model-three',
    ]);
    expect([...context.selectedIds]).toEqual(['model-two', 'model-one', 'model-three']);
    expect(api.removeSmartRouterCandidate).not.toHaveBeenCalled();
    expect(api.deleteModel).not.toHaveBeenCalled();
    expect(confirm).not.toHaveBeenCalled();
  });

  it('preserves the card after a failed removal and allows a later successful drop', async () => {
    vi.mocked(api.removeSmartRouterCandidate).mockRejectedValueOnce(new Error('Disk unavailable'));
    await open();
    await start();
    await end(REMOVE_TARGET);
    expect([...context.selectedIds]).toEqual(['model-one', 'model-two', 'model-three']);
    expect(context.customModels).toHaveLength(3);
    expect(showToast).toHaveBeenCalledExactlyOnceWith('error', 'error.requestFailed');
    expect(api.deleteModel).not.toHaveBeenCalled();
    await start();
    await end(REMOVE_TARGET);
    expect([...context.selectedIds]).toEqual(['model-one', 'model-three']);
    expect(api.removeSmartRouterCandidate).toHaveBeenCalledTimes(2);
    expect(api.setSmartRouterCandidates).not.toHaveBeenCalled();
  });

  it('keeps the master switch unchanged when the current group becomes empty', async () => {
    enabled = true;
    savedOrder = ['model-two'];
    await open();
    expect(context.routerEnabled).toBe(true);
    await start();
    await end(REMOVE_TARGET);
    expect(api.removeSmartRouterCandidate).toHaveBeenCalledExactlyOnceWith('model-two');
    expect(api.setSmartRouterEnabled).not.toHaveBeenCalled();
    expect([...context.selectedIds]).toEqual([]);
    expect(context.customModels).toEqual([]);
    expect(context.routerEnabled).toBe(true);
    expect(savedModels).toHaveLength(3);
    expect(api.deleteModel).not.toHaveBeenCalled();
    expect(api.setSmartRouterCandidates).not.toHaveBeenCalled();
    expect(confirm).not.toHaveBeenCalled();
  });
});
