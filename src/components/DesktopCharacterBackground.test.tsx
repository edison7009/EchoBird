import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { AppBackground } from './AppBackground';
import { useBackgroundStore } from '../stores/backgroundStore';
import { useNavigationStore } from '../stores/navigationStore';
import { useEasterEggStore } from '../stores/easterEggStore';

const mocks = vi.hoisted(() => ({
  selectedTool: null as string | null,
  select: vi.fn(),
  dispose: vi.fn(),
  create: vi.fn(),
}));
vi.mock('../pages/AppManager/context', () => ({
  useAppManager: () => ({ selectedTool: mocks.selectedTool }),
}));
vi.mock('./desktopCharacterRenderer', () => ({
  createCharacterRenderer: mocks.create,
}));

let view: ReactTestRenderer | undefined;
let doc: EventTarget & { visibilityState: string };
let preferences: Map<string, EventTarget & { matches: boolean }>;
let attached: object[];
const host = { appendChild: (canvas: object) => attached.push(canvas) };
const currentCanvas = () => attached[0];

beforeEach(() => {
  vi.clearAllMocks();
  mocks.selectedTool = null;
  attached = [];
  mocks.create.mockReturnValue({ select: mocks.select, dispose: mocks.dispose });
  doc = Object.assign(new EventTarget(), {
    visibilityState: 'visible',
    createElement: () => {
      const canvas = {
        dataset: {},
        className: '',
        getBoundingClientRect: () => ({ left: 10, top: 30, width: 1000, height: 600 }),
        remove: () => {
          attached = attached.filter((element) => element !== canvas);
        },
      };
      return canvas;
    },
    querySelectorAll: () => [
      {
        dataset: { desktopToolId: 'claude' },
        getClientRects: () => [1],
        getBoundingClientRect: () => ({ left: 260, top: 150, width: 100, height: 120 }),
      },
    ],
  });
  preferences = new Map();
  vi.stubGlobal('document', doc);
  vi.stubGlobal('window', {
    matchMedia: (query: string) => {
      if (!preferences.has(query))
        preferences.set(query, Object.assign(new EventTarget(), { matches: false }));
      return preferences.get(query);
    },
  });
  vi.stubGlobal('localStorage', { getItem: vi.fn(), setItem: vi.fn() });
  useNavigationStore.setState({ activePage: 'apps' });
  useEasterEggStore.setState({ enabled: true });
  useBackgroundStore.setState({ mode: 'characters' });
});

afterEach(() => {
  act(() => view?.unmount());
  view = undefined;
  vi.unstubAllGlobals();
});

function mount(selected: string | null = null) {
  mocks.selectedTool = selected;
  act(() => {
    view = create(<AppBackground />, { createNodeMock: () => host });
  });
}
function select(id: string | null) {
  act(() => {
    mocks.selectedTool = id;
    view!.update(<AppBackground />);
  });
}

describe('application character background lifecycle', () => {
  it('starts without an icon selection and updates the existing renderer on selection', () => {
    mount();
    const canvas = currentCanvas();
    expect(mocks.create).toHaveBeenCalledExactlyOnceWith(canvas);
    expect(mocks.select).not.toHaveBeenCalled();
    select('claude');
    expect(mocks.create).toHaveBeenCalledExactlyOnceWith(canvas);
    expect(mocks.select).toHaveBeenLastCalledWith('claude', { x: 0.3, y: 0.3 });
    select('codex');
    expect(mocks.create).toHaveBeenCalledTimes(1);
    expect(mocks.select).toHaveBeenLastCalledWith('codex', { x: 0.5, y: 0.5 });
    expect(mocks.dispose).not.toHaveBeenCalled();
    expect(view!.root.findByType('div').props['aria-hidden']).toBe('true');
  });

  it('keeps the same canvas and renderer across every page, including without an app selection', () => {
    mount('claude');
    const original = currentCanvas();
    for (const activePage of [
      'accounts',
      'models',
      'freeModels',
      'mother',
      'developer',
      'aiCareer',
      'aiNews',
      'localLlm',
      'feedback',
      'apps',
    ] as const) {
      act(() => {
        useNavigationStore.setState({ activePage });
        view!.update(<AppBackground />);
      });
      expect(currentCanvas()).toBe(original);
    }
    expect(mocks.create).toHaveBeenCalledTimes(1);
    expect(mocks.select).toHaveBeenCalledTimes(1);
    expect(mocks.dispose).not.toHaveBeenCalled();
    select(null);
    expect(currentCanvas()).toBe(original);
    expect(mocks.create).toHaveBeenCalledTimes(1);
    expect(mocks.dispose).not.toHaveBeenCalled();
  });

  it('selects backgrounds independently of the Easter Egg and releases character resources', () => {
    mount('claude');
    act(() => useEasterEggStore.getState().setEnabled(false));
    expect(mocks.dispose).not.toHaveBeenCalled();
    expect(attached).toHaveLength(1);
    act(() => useBackgroundStore.getState().setMode('carbon'));
    expect(mocks.dispose).toHaveBeenCalledTimes(1);
    expect(attached).toHaveLength(0);
    expect(view!.root.findByType('div').props['data-app-background']).toBe('carbon');
    expect(localStorage.setItem).toHaveBeenLastCalledWith('echobird_background', 'carbon');
    act(() => useBackgroundStore.getState().setMode('none'));
    expect(view!.toJSON()).toBeNull();
    act(() => useBackgroundStore.getState().setMode('characters'));
    expect(mocks.create).toHaveBeenCalledTimes(2);
    expect(useEasterEggStore.getState().enabled).toBe(false);
    expect(mocks.selectedTool).toBe('claude');
  });

  it.each(['none', 'carbon', 'basalt', 'monogram'] as const)(
    'does not create graphics for %s',
    (mode) => {
      useBackgroundStore.setState({ mode });
      mount();
      expect(mocks.create).not.toHaveBeenCalled();
      expect(attached).toHaveLength(0);
      if (mode === 'none') {
        expect(view!.toJSON()).toBeNull();
        return;
      }
      const node = view!.root.findByType('div');
      expect(node.props['data-app-background']).toBe(mode);
      const className = String(node.props.className);
      if (mode === 'basalt') {
        expect(className).toContain('app-background-basalt');
        expect(className).not.toContain('app-background-material');
      } else {
        expect(className).toContain('app-background-material');
        expect(className).toContain(`app-background-${mode}`);
      }
    }
  );

  it('stops while the window is hidden and resumes when visible', () => {
    mount('claude');
    act(() => {
      doc.visibilityState = 'hidden';
      doc.dispatchEvent(new Event('visibilitychange'));
    });
    expect(mocks.dispose).toHaveBeenCalledTimes(1);
    act(() => {
      doc.visibilityState = 'visible';
      doc.dispatchEvent(new Event('visibilitychange'));
    });
    expect(mocks.create).toHaveBeenCalledTimes(2);
  });

  it.each([
    '(prefers-reduced-motion: reduce)',
    '(prefers-contrast: more)',
    '(forced-colors: active)',
  ])('respects live accessibility preference %s', (query) => {
    mount('claude');
    act(() => {
      const preference = preferences.get(query)!;
      preference.matches = true;
      preference.dispatchEvent(new Event('change'));
    });
    expect(mocks.dispose).toHaveBeenCalledTimes(1);
    expect(attached).toHaveLength(0);
    act(() => {
      const preference = preferences.get(query)!;
      preference.matches = false;
      preference.dispatchEvent(new Event('change'));
    });
    expect(mocks.create).toHaveBeenCalledTimes(2);
  });

  it('tolerates unavailable graphics and removes media listeners on unmount', () => {
    mocks.create.mockReturnValue(null);
    mount('claude');
    expect(attached).toHaveLength(0);
    select('codex');
    expect(mocks.select).not.toHaveBeenCalled();
    const removals = [...preferences.values()].map((preference) =>
      vi.spyOn(preference, 'removeEventListener')
    );
    act(() => view!.unmount());
    view = undefined;
    removals.forEach((remove) =>
      expect(remove).toHaveBeenCalledWith('change', expect.any(Function))
    );
    expect(mocks.dispose).not.toHaveBeenCalled();
  });

  it('detaches before disposing and gives repeated effects on the same host a fresh canvas', () => {
    mount('claude');
    const first = currentCanvas();
    mocks.dispose.mockImplementationOnce(() => expect(attached).not.toContain(first));
    act(() => view!.update(<></>));
    expect(attached).toHaveLength(0);
    act(() => view!.update(<AppBackground />));
    expect(attached).toHaveLength(1);
    expect(currentCanvas()).not.toBe(first);
    expect(mocks.create.mock.calls[1][0]).toBe(currentCanvas());
    expect(mocks.dispose).toHaveBeenCalledTimes(1);
  });
});
