import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { VoiceInputGuide } from './VoiceInputGuide';
import { useNavigationStore } from '../stores/navigationStore';
import { useSidebarStore } from '../stores/sidebarStore';
import { loadLocale, translate, type TKey } from '../i18n';
import * as voiceApi from '../api/voiceInput';

import { useAutoImportStore, type ImportProgress } from '../stores/autoImportStore';
import { useConfigBackupStore } from '../stores/configBackupStore';
import { DesktopPrimaryButton } from './DesktopPrimaryButton';
const importMocks = vi.hoisted(() => ({ invoke: vi.fn(), invalidate: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({
  invoke: importMocks.invoke,
  Channel: class {
    onmessage?: (value: ImportProgress) => void;
  },
}));
vi.mock('../stores/modelRepository', () => ({
  modelRepository: { invalidate: importMocks.invalidate },
}));
const localization = vi.hoisted(() => ({ locale: 'en' }));
vi.mock('../hooks/useI18n', () => ({
  useI18n: () => ({ t: (key: TKey) => translate(key, localization.locale) }),
}));
vi.mock('../pages/VoiceInput/VoiceFlow', () => ({ VoiceFlow: () => <div data-flow /> }));
vi.mock('../api/voiceInput', () => ({
  recordVoice: vi.fn(),
  startVoice: vi.fn(),
  stopVoice: vi.fn(),
  saveVoiceConfig: vi.fn(),
}));

let view: ReactTestRenderer;
const dismissed = vi.fn();
let rect: { left: number; right: number; top: number; width: number; height: number };
let panelHeight = 380;
let resized: () => void;
let keydown: (event: {
  key: string;
  shiftKey?: boolean;
  preventDefault: () => void;
  stopPropagation: () => void;
}) => void;
let doc: { hidden: boolean; body: object; activeElement: object; getElementById: () => object };
let anchor: {
  focus: ReturnType<typeof vi.fn>;
  scrollIntoView: ReturnType<typeof vi.fn>;
  getBoundingClientRect: () => typeof rect;
};
let previous: {
  focus: ReturnType<typeof vi.fn>;
  isConnected: boolean;
  getClientRects: () => object[];
};
let close: { focus: ReturnType<typeof vi.fn> };
let first: { focus: ReturnType<typeof vi.fn> };
const disconnected = vi.fn();

beforeEach(async () => {
  vi.useFakeTimers();
  vi.clearAllMocks();
  useAutoImportStore.setState({ running: false, progress: null, error: false, revision: 0 });
  useConfigBackupStore.setState({ phase: 'idle' });
  localization.locale = 'en';
  await loadLocale('en');
  useSidebarStore.setState({ collapsed: false });
  useNavigationStore.setState(useNavigationStore.getInitialState(), true);
  rect = { left: 40, right: 252, top: 320, width: 212, height: 40 };
  panelHeight = 380;
  anchor = { focus: vi.fn(), scrollIntoView: vi.fn(), getBoundingClientRect: () => rect };
  previous = { focus: vi.fn(), isConnected: true, getClientRects: () => [{}] };
  close = {
    focus: vi.fn(() => {
      doc.activeElement = close;
    }),
  };
  first = {
    focus: vi.fn(() => {
      doc.activeElement = first;
    }),
  };
  doc = { hidden: false, body: {}, activeElement: previous, getElementById: () => anchor };
  vi.stubGlobal('document', doc);
  vi.stubGlobal('window', {
    innerWidth: 1000,
    innerHeight: 700,
    addEventListener: vi.fn((event, handler) => {
      if (event === 'resize') resized = handler;
      if (event === 'keydown') keydown = handler;
    }),
    removeEventListener: vi.fn(),
  });
  vi.stubGlobal('fetch', vi.fn());
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe = vi.fn();
      disconnect = disconnected;
    }
  );
});
afterEach(() => {
  act(() => view?.unmount());
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

function mount() {
  act(() => {
    view = create(<VoiceInputGuide onClose={dismissed} />, {
      createNodeMock: (element) => {
        if (element.props.className === 'voice-guide-panel page-scroll')
          return { getBoundingClientRect: () => ({ height: panelHeight }) };
        if (element.props.className === 'voice-guide-root')
          return {
            querySelectorAll: () => [first, close],
            contains: (value: object) => [first, close].includes(value as typeof first),
          };
        if (element.props['aria-label'] === translate('btn.close', localization.locale))
          return close;
        return null;
      },
    });
  });
}

function openVoiceGuidePage() {
  act(() =>
    view.root
      .findByProps({ 'aria-label': translate('guide.next', localization.locale) })
      .props.onClick()
  );
}

it.each(['zh-Hans', 'zh-Hant', 'en', 'ja'])(
  'demonstrates speech and whole-text fill locally in %s, then clears every timer',
  async (locale) => {
    await loadLocale(locale);
    localization.locale = locale;
    mount();
    openVoiceGuidePage();
    const desktop = () => view.root.findByProps({ className: 'voice-guide-desktop' });
    const editor = () => view.root.findByProps({ className: 'voice-guide-editor' });
    expect(desktop().props['data-phase']).toBe('idle');
    act(() => {
      vi.advanceTimersByTime(900);
    });
    expect(desktop().props['data-phase']).toBe('recording');
    act(() => {
      vi.advanceTimersByTime(3200);
    });
    expect(desktop().props['data-phase']).toBe('processing');
    act(() => {
      vi.advanceTimersByTime(600);
    });
    expect(editor().props['data-filled']).toBe(true);
    expect(view.root.findByProps({ className: 'voice-guide-example' }).children).toEqual([
      translate('voice.guide.example', locale),
    ]);
    expect(translate('voice.guide.example', locale)).not.toContain('voice.guide.');
    act(() => {
      vi.advanceTimersByTime(3300);
    });
    expect(desktop().props['data-phase']).toBe('idle');
    expect(editor().props['data-filled']).toBe(false);
    expect(fetch).not.toHaveBeenCalled();
    for (const call of Object.values(voiceApi)) expect(call).not.toHaveBeenCalled();
    act(() => view.unmount());
    expect(vi.getTimerCount()).toBe(0);
    expect(disconnected).toHaveBeenCalledTimes(2);
  }
);

it('keeps its arrow on the expanded/collapsed navigation and within a smaller viewport', () => {
  mount();
  openVoiceGuidePage();
  const bubble = () => view.root.findByProps({ className: 'voice-guide-position' });
  expect(bubble().props.style).toMatchObject({ left: 270, top: 150, width: 420 });
  rect = { left: 8, right: 56, top: 320, width: 48, height: 40 };
  act(() => useSidebarStore.setState({ collapsed: true }));
  expect(bubble().props.style.left).toBe(74);
  expect(view.root.findByProps({ className: 'voice-guide-target' }).props.style.width).toBe(56);
  window.innerWidth = 640;
  window.innerHeight = 480;
  panelHeight = 340;
  act(() => resized());
  expect(bubble().props.style.top + panelHeight).toBeLessThanOrEqual(window.innerHeight - 16);
  expect(bubble().props.style.left + bubble().props.style.width).toBeLessThanOrEqual(
    window.innerWidth - 16
  );
});

it('traps keyboard focus, closes with Escape and restores the previous focus without starting audio', () => {
  mount();
  expect(close.focus).toHaveBeenCalledOnce();
  const event = { key: 'Tab', preventDefault: vi.fn(), stopPropagation: vi.fn() };
  doc.activeElement = close;
  act(() => keydown(event));
  expect(first.focus).toHaveBeenCalledOnce();
  act(() => keydown({ ...event, shiftKey: true }));
  expect(close.focus).toHaveBeenCalledTimes(2);
  act(() => keydown({ ...event, key: 'Escape' }));
  expect(dismissed).toHaveBeenCalledOnce();
  act(() => view.unmount());
  expect(previous.focus).toHaveBeenCalledOnce();
});

it('opens only the voice page from the guide action', () => {
  mount();
  openVoiceGuidePage();
  act(() => view.root.findByProps({ className: 'voice-guide-target' }).props.onClick());
  expect(useNavigationStore.getState().activePage).toBe('voice');
  expect(dismissed).toHaveBeenCalledOnce();
  expect(voiceApi.recordVoice).not.toHaveBeenCalled();
  expect(voiceApi.saveVoiceConfig).not.toHaveBeenCalled();
  expect(fetch).not.toHaveBeenCalled();
});

it('always starts with import and flips pages without scanning or starting audio', () => {
  mount();
  expect(view.root.findByProps({ role: 'dialog' }).props['aria-label']).toBe(
    translate('settings.autoImport', 'en')
  );
  expect(view.root.findAllByProps({ className: 'voice-guide-target' })).toHaveLength(0);
  expect(JSON.stringify(view.toJSON())).toContain('1/2');
  act(() => view.root.findByProps({ 'aria-label': translate('guide.next', 'en') }).props.onClick());
  expect(JSON.stringify(view.toJSON())).toContain('2/2');
  expect(view.root.findByProps({ className: 'voice-guide-desktop' })).toBeTruthy();
  act(() =>
    view.root.findByProps({ 'aria-label': translate('guide.previous', 'en') }).props.onClick()
  );
  expect(JSON.stringify(view.toJSON())).toContain('1/2');
  expect(fetch).not.toHaveBeenCalled();
  expect(importMocks.invoke).not.toHaveBeenCalled();
  for (const call of Object.values(voiceApi)) expect(call).not.toHaveBeenCalled();
});

it('only imports on click, guards duplicate clicks, and completes after the guide closes', async () => {
  let finish!: (value: ImportProgress) => void;
  importMocks.invoke.mockReturnValue(
    new Promise<ImportProgress>((resolve) => {
      finish = resolve;
    })
  );
  mount();
  const button = () => view.root.findByType(DesktopPrimaryButton);
  expect(importMocks.invoke).not.toHaveBeenCalled();
  act(() => {
    button().props.onClick();
    button().props.onClick();
  });
  expect(importMocks.invoke).toHaveBeenCalledOnce();
  expect(importMocks.invoke.mock.calls[0][0]).toBe('auto_import');
  expect(useNavigationStore.getState().activePage).not.toBe('settings');
  act(() => view.root.findByProps({ 'aria-label': translate('guide.next', 'en') }).props.onClick());
  act(() =>
    view.root.findByProps({ 'aria-label': translate('guide.previous', 'en') }).props.onClick()
  );
  expect(view.root.findByType(DesktopPrimaryButton).props['aria-busy']).toBe(true);
  act(() => view.unmount());
  expect(useAutoImportStore.getState().running).toBe(true);
  await act(async () =>
    finish({
      completed: 1,
      total: 1,
      source: 'fixture',
      models: 2,
      accounts: 1,
      duplicates: 0,
      issues: [],
    })
  );
  expect(useAutoImportStore.getState()).toMatchObject({
    running: false,
    revision: 1,
    error: false,
  });
  expect(importMocks.invalidate).toHaveBeenCalledOnce();
});

it('keeps a failed import retryable inline without opening another dialog', async () => {
  importMocks.invoke.mockRejectedValueOnce(new Error('fixture failure'));
  mount();
  await act(async () => view.root.findByType(DesktopPrimaryButton).props.onClick());
  expect(JSON.stringify(view.toJSON())).toContain(translate('guide.retryImport', 'en'));
  expect(view.root.findAllByProps({ role: 'dialog' })).toHaveLength(1);
  importMocks.invoke.mockResolvedValueOnce({
    completed: 1,
    total: 1,
    source: '',
    models: 0,
    accounts: 0,
    duplicates: 0,
    issues: [],
  });
  await act(async () => view.root.findByType(DesktopPrimaryButton).props.onClick());
  expect(JSON.stringify(view.toJSON())).toContain(translate('guide.importDone', 'en'));
});

it('does not start import while backup or restore is pending', () => {
  mount();
  useConfigBackupStore.setState({ phase: 'working' });
  act(() => view.root.findByType(DesktopPrimaryButton).props.onClick());
  expect(importMocks.invoke).not.toHaveBeenCalled();
});

it.each([0, 1])(
  'skips the entire guide from page %s without importing or opening voice',
  (page) => {
    mount();
    if (page === 1)
      act(() =>
        view.root.findByProps({ 'aria-label': translate('guide.next', 'en') }).props.onClick()
      );
    act(() => view.root.findByProps({ className: 'voice-guide-skip' }).props.onClick());
    expect(dismissed).toHaveBeenCalledOnce();
    expect(importMocks.invoke).not.toHaveBeenCalled();
    for (const call of Object.values(voiceApi)) expect(call).not.toHaveBeenCalled();
  }
);
