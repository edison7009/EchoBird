import type { ReactNode } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { VoiceInputMain } from './VoiceInput';
import { IslandContent, IslandTranscript, VoiceIsland } from './VoiceIsland';
import { VoiceFlow } from './VoiceFlow';
import { listen } from '@tauri-apps/api/event';
import { voiceSnapshot } from './testFixtures';
import { useVoiceInputStore as voice } from '../../stores/voiceInputStore';
import * as api from '../../api/voiceInput';
import { RemoteModelSelector } from '../../components/RemoteModelSelector';
import { MiniSelect } from '../../components/MiniSelect';
import { ViewModeTabs } from '../../components/ViewModeTabs';
import { useNavigationStore } from '../../stores/navigationStore';
import { loadLocale, translate, type TKey } from '../../i18n';
import { formatBinding } from './ShortcutRecorder';
import { copyText } from '../../utils/copyText';
import { VoiceReplay } from './VoiceReplay';

vi.mock('../../utils/copyText', () => ({ copyText: vi.fn() }));

const themeSync = vi.hoisted(() => vi.fn());
const appAppearance = vi.hoisted(() => ({ resolved: 'dark' }));
const startDragging = vi.hoisted(() => vi.fn());
const localization = vi.hoisted(() => ({ locale: null as string | null }));
const mother = vi.hoisted(() => ({
  models: [] as { internalId: string; name: string; modelId: string }[],
  setAgentModel: vi.fn(),
  setParasiteAgent: vi.fn(),
}));
vi.mock('../MotherAgent/context', () => ({ useMotherAgent: () => mother }));
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({ startDragging }),
}));
vi.mock('../../stores/themeStore', () => ({
  THEME_CHANGED_EVENT: 'echobird-theme-changed',
  useThemeStore: Object.assign(
    (selector: (state: { resolved: string }) => unknown) => selector(appAppearance),
    { getState: () => ({ sync: themeSync }) }
  ),
}));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));

vi.mock('react-dom', () => ({ createPortal: (children: ReactNode) => children }));
vi.mock('@dnd-kit/core', async (original) => ({
  ...(await original<typeof import('@dnd-kit/core')>()),
  DndContext: ({ children }: { children: ReactNode }) => children,
  DragOverlay: ({ children }: { children: ReactNode }) => children,
}));
vi.mock('@dnd-kit/sortable', async (original) => ({
  ...(await original<typeof import('@dnd-kit/sortable')>()),
  SortableContext: ({ children }: { children: ReactNode }) => children,
  useSortable: () => ({
    attributes: { tabIndex: 0 },
    listeners: { onPointerDown: vi.fn(), onKeyDown: vi.fn() },
    setNodeRef: vi.fn(),
    setActivatorNodeRef: vi.fn(),
    transform: null,
    transition: undefined,
    isDragging: false,
  }),
}));
vi.mock('../../hooks/useI18n', () => ({
  useI18n: () => ({
    t: (key: string) => (localization.locale ? translate(key as TKey, localization.locale) : key),
    locale: localization.locale ?? 'zh-Hans',
  }),
}));
vi.mock('./provider', () => ({ useVoiceAccounts: () => ({ accounts: [] }) }));
vi.mock('../ModelNexus/context', () => ({
  useModelNexus: () => ({ userModels: [], isLoadingModels: false }),
}));
vi.mock('../../api/voiceInput', () => ({
  selectionLanguages: ['zh-Hans', 'zh-Hant', 'en', 'ja', 'ko', 'fr', 'de', 'es'],
  voiceIslandScales: [70, 80, 90, 100, 110, 120, 130, 140, 150],
  openSelectionTranslation: vi.fn().mockResolvedValue(undefined),
  closeSelectionTranslation: vi.fn().mockResolvedValue(undefined),
  startVoice: vi.fn(),
  recordVoice: vi.fn(),
  stopVoice: vi.fn(),
  getVoiceReplay: vi.fn(),
  saveVoiceConfig: vi.fn(),
  cancelVoice: vi.fn(),
  suspendVoiceShortcut: vi.fn().mockResolvedValue(undefined),
  voiceIslandReady: vi.fn(),
  showMainWindow: vi.fn(),
}));

let renderer: ReactTestRenderer;
beforeEach(() => {
  vi.clearAllMocks();
  startDragging.mockReset().mockResolvedValue(undefined);
  vi.mocked(api.showMainWindow).mockReset().mockResolvedValue(undefined);
  mother.models = [];
  appAppearance.resolved = 'dark';
  localization.locale = null;
  useNavigationStore.setState({ activePage: 'voice' });
  vi.stubGlobal('document', { addEventListener: vi.fn(), removeEventListener: vi.fn() });
  vi.stubGlobal('window', {
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    matchMedia: vi.fn(() => ({
      matches: false,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    })),
  });
  vi.stubGlobal('navigator', { platform: 'Win32' });
  voice.setState(voice.getInitialState());
  voice.setState({ snapshot: voiceSnapshot() });
});
afterEach(() => {
  act(() => renderer?.unmount());
  vi.unstubAllGlobals();
});
function button(label: string) {
  return renderer.root
    .findAllByType('button')
    .find(
      (item) =>
        item.props['aria-hidden'] !== true &&
        (item.props['aria-label'] === label ||
          item.children.includes(label) ||
          item
            .findAllByType('span')
            .some((span) => span.props['aria-hidden'] !== true && span.children.includes(label)))
    )!;
}

describe('selection translation entry', () => {
  const configured = () => ({
    ...voiceSnapshot().config,
    selectionModel: 'saved-text-model',
    selectionTranslationEnabled: true,
  });
  it('requires a selection model and performs no passive translation', () => {
    act(() => {
      renderer = create(<VoiceInputMain />);
    });
    expect(renderer.root.findByProps({ 'aria-label': 'voice.selection' }).props.disabled).toBe(
      true
    );
    expect(api.openSelectionTranslation).not.toHaveBeenCalled();
    expect(api.saveVoiceConfig).not.toHaveBeenCalled();
  });
  it('enables through the existing config save without enabling voice or requiring ASR sources', async () => {
    const config = { ...configured(), selectionTranslationEnabled: false };
    voice.setState({ snapshot: voiceSnapshot({ config }) });
    vi.mocked(api.saveVoiceConfig).mockImplementation(async (saved) =>
      voiceSnapshot({ config: saved, revision: 2 })
    );
    act(() => {
      renderer = create(<VoiceInputMain />);
    });
    await act(async () =>
      renderer.root
        .findByProps({ 'aria-label': 'voice.selection' })
        .findByType(ViewModeTabs)
        .props.onChange('on')
    );
    expect(api.saveVoiceConfig).toHaveBeenCalledWith({
      ...config,
      selectionTranslationEnabled: true,
    });
    expect(voice.getState().snapshot?.config.shortcutEnabled).toBe(false);
    expect(api.openSelectionTranslation).not.toHaveBeenCalled();
  });
  it('keeps the voice waveform in idle, preview and recording instead of selection actions', () => {
    act(() => {
      renderer = create(<IslandContent snapshot={voiceSnapshot({ config: configured() })} />);
    });
    for (const props of [
      { snapshot: voiceSnapshot({ config: configured() }) },
      { snapshot: voiceSnapshot({ config: configured() }), preview: true },
      { snapshot: voiceSnapshot({ config: configured(), phase: 'recording' as const }) },
    ]) {
      act(() => renderer.update(<IslandContent {...props} />));
      expect(button('voice.selectionAction')).toBeUndefined();
      expect(button('voice.selectionAsk')).toBeUndefined();
      expect(
        renderer.root.findByProps({ className: 'voice-island-middle' }).findAllByType('button')
      ).toHaveLength(0);
    }
    expect(api.openSelectionTranslation).not.toHaveBeenCalled();
    expect(api.recordVoice).not.toHaveBeenCalled();
    expect(startDragging).not.toHaveBeenCalled();
  });
  it('uses the sliding on/off tabs and removes the manual translation entry', () => {
    voice.setState({ snapshot: voiceSnapshot({ config: configured() }) });
    act(() => {
      renderer = create(<VoiceInputMain />);
    });
    expect(button('voice.selectionManual')).toBeUndefined();
    const tabs = renderer.root
      .findByProps({ 'aria-label': 'voice.selection' })
      .findByType(ViewModeTabs);
    expect(tabs.props.options).toEqual([
      { value: 'on', label: 'voice.selectionOn' },
      { value: 'off', label: 'voice.selectionOff' },
    ]);
    expect(tabs.props.value).toBe('on');
    expect(api.openSelectionTranslation).not.toHaveBeenCalled();
    expect(api.recordVoice).not.toHaveBeenCalled();
  });
  it('does not let recording steal focus from an open translation card', () => {
    act(() => {
      renderer = create(
        <IslandContent snapshot={voiceSnapshot({ config: configured(), selectionOpen: true })} />
      );
    });
    expect(button('voice.speak').props.disabled).toBe(true);
    expect(button('voice.selectionAction')).toBeUndefined();
    expect(api.recordVoice).not.toHaveBeenCalled();
  });
});

describe('microphone card', () => {
  it('offers five colors for a single waveform and saves through the existing configuration', async () => {
    const config = {
      ...voiceSnapshot().config,
      channels: [{ kind: 'model' as const, id: 'saved-source' }],
      shortcutEnabled: true,
    };
    voice.setState({ snapshot: voiceSnapshot({ config }) });
    vi.mocked(api.saveVoiceConfig).mockImplementation(async (saved) =>
      voiceSnapshot({ config: saved, revision: 2 })
    );
    act(() => {
      renderer = create(<VoiceInputMain />);
    });
    const colors = () =>
      renderer.root
        .findByProps({ role: 'group', 'aria-label': 'voice.palette' })
        .findAllByType('button');
    expect(colors()).toHaveLength(5);
    expect(colors()[0].props['aria-pressed']).toBe(true);
    expect(JSON.stringify(renderer.toJSON())).not.toContain('voice.style');
    expect(api.saveVoiceConfig).not.toHaveBeenCalled();
    await act(async () => button('voice.palette.aurora').props.onClick());
    expect(api.saveVoiceConfig).toHaveBeenCalledExactlyOnceWith({
      ...config,
      islandPalette: 'aurora',
    });
    expect(button('voice.palette.aurora').props['aria-pressed']).toBe(true);
    expect(
      renderer.root.findByProps({ className: 'voice-island-bars' }).findAllByType('i')[0].props
        .style.backgroundColor
    ).toBe('rgb(16,185,129)');
    expect(api.startVoice).not.toHaveBeenCalled();
    expect(api.recordVoice).not.toHaveBeenCalled();
  });

  it.each(['zh-Hans', 'zh-Hant', 'en', 'ja'])(
    'localizes the compact palette controls in %s',
    (locale) => {
      localization.locale = locale;
      act(() => {
        renderer = create(<VoiceInputMain />);
      });
      const controls = renderer.root
        .findByProps({ role: 'group', 'aria-label': translate('voice.palette', locale) })
        .findAllByType('button');
      expect(controls.map((control) => control.props['aria-label'])).toEqual(
        ['white', 'aurora', 'ocean', 'sunset', 'iris'].map((palette) =>
          translate(`voice.palette.${palette}` as TKey, locale)
        )
      );
      expect(api.saveVoiceConfig).not.toHaveBeenCalled();
    }
  );

  it.each(['compact', 'wave', 'caption'])(
    'shows plain bars for a legacy %s selection without writing settings',
    (islandStyle) => {
      const config = {
        ...voiceSnapshot().config,
        islandPalette: undefined,
        islandStyle,
      } as unknown as api.VoiceConfig;
      act(() => {
        renderer = create(<IslandContent snapshot={voiceSnapshot({ config })} preview />);
      });
      expect(
        renderer.root.findByProps({ className: 'voice-island-bars' }).findAllByType('i')
      ).toHaveLength(28);
      expect(renderer.root.findAllByType(VoiceFlow)).toHaveLength(0);
      expect(api.saveVoiceConfig).not.toHaveBeenCalled();
      expect(api.startVoice).not.toHaveBeenCalled();
    }
  );

  it('keeps the moved master, retired island switches and statistics out of the voice page', () => {
    act(() => {
      renderer = create(<VoiceInputMain />);
    });
    expect(
      renderer.root.findAllByProps({ role: 'switch' }).map((node) => node.props['aria-label'])
    ).toEqual([]);
    expect(renderer.root.findAllByType('dl')).toHaveLength(0);
    expect(api.saveVoiceConfig).not.toHaveBeenCalled();
    expect(api.startVoice).not.toHaveBeenCalled();
    expect(api.stopVoice).not.toHaveBeenCalled();
  });

  it('only starts a microphone test, including without a speech source', async () => {
    vi.mocked(api.startVoice).mockResolvedValue(
      voiceSnapshot({ phase: 'testing', origin: 'test', revision: 2 })
    );
    act(() => {
      renderer = create(<VoiceInputMain />);
    });
    expect(button('voice.start')).toBeUndefined();
    expect(button('voice.stop')).toBeUndefined();
    expect(button('btn.cancel')).toBeUndefined();
    await act(async () => button('voice.testMicrophone').props.onClick());
    expect(api.startVoice).toHaveBeenCalledExactlyOnceWith();
    expect(api.stopVoice).not.toHaveBeenCalled();
    expect(button('voice.stopTest')).toBeDefined();
  });

  it('renders incoming levels, stops from the test button and resets to silence', async () => {
    voice.setState({ snapshot: voiceSnapshot({ phase: 'testing', origin: 'test' }) });
    vi.mocked(api.stopVoice).mockResolvedValue(voiceSnapshot({ revision: 3 }));
    act(() => {
      renderer = create(<VoiceInputMain />);
    });
    act(() =>
      voice
        .getState()
        .accept(voiceSnapshot({ phase: 'testing', origin: 'test', level: 0.6, revision: 2 }))
    );
    const meter = () => renderer.root.findByProps({ role: 'meter' });
    expect(meter().props['aria-valuenow']).toBe(60);
    expect(meter().findAllByProps({ className: 'lit' })).toHaveLength(18);
    await act(async () => button('voice.stopTest').props.onClick());
    expect(api.stopVoice).toHaveBeenCalledExactlyOnceWith();
    expect(meter().props['aria-valuenow']).toBe(0);
    expect(meter().findAllByProps({ className: 'lit' })).toHaveLength(0);
    expect(button('voice.testMicrophone')).toBeDefined();
    expect(api.startVoice).not.toHaveBeenCalled();
  });
});

describe('localized shortcut transitions', () => {
  it('does not enable or disable the hotkey when saving fails and leaves mouse input enabled', async () => {
    const config = {
      ...voiceSnapshot().config,
      shortcutEnabled: true,
      channels: [{ kind: 'model' as const, id: 'speech-source' }],
    };
    voice.setState({ snapshot: voiceSnapshot({ config }) });
    act(() => {
      renderer = create(<VoiceInputMain />);
    });
    vi.mocked(api.saveVoiceConfig).mockRejectedValueOnce('voice.error.shortcut');
    act(() => button('voice.enableShortcut').props.onClick());
    expect(api.saveVoiceConfig).not.toHaveBeenCalled();
    await act(async () => button('voice.recordShortcut').props.onKeyDown(keyEvent('F8')));
    expect(button('voice.enableShortcut')).toBeDefined();
    expect(button('voice.disableShortcut')).toBeUndefined();
    expect(voice.getState().snapshot?.config.hotkeyEnabled).toBe(false);
    vi.mocked(api.saveVoiceConfig).mockImplementationOnce(async (saved) =>
      voiceSnapshot({ config: saved, revision: 2 })
    );
    act(() => button('voice.enableShortcut').props.onClick());
    await act(async () => button('voice.recordShortcut').props.onKeyDown(keyEvent('F8')));
    expect(button('F8')).toBeDefined();
    vi.mocked(api.saveVoiceConfig).mockRejectedValueOnce('voice.error.save');
    await act(async () => button('voice.disableShortcut').props.onClick());
    expect(button('F8')).toBeDefined();
    expect(button('voice.disableShortcut')).toBeDefined();
    expect(voice.getState().snapshot?.config.hotkeyEnabled).toBe(true);
    expect(voice.getState().snapshot?.config.shortcutEnabled).toBe(true);
    expect(api.recordVoice).not.toHaveBeenCalled();
  });

  const labels: Record<string, string> = {
    'zh-Hans': '语音输入快捷键',
    'zh-Hant': '語音輸入快捷鍵',
    en: 'Voice input shortcut',
    ja: '音声入力キー',
  };
  const cases = Object.keys(labels).flatMap((locale) =>
    ['Win32', 'MacIntel', 'Linux x86_64'].map((platform) => ({ locale, platform }))
  );
  const surroundingContent = (node: ReactTestInstance | string): unknown => {
    if (typeof node === 'string') return node;
    if (node.props.className === 'voice-shortcut-control') return 'shortcut-control';
    return {
      type: node.type,
      children: node.children.map(surroundingContent),
    };
  };
  const keyEvent = (code: string, key = code) => ({
    code,
    key,
    ctrlKey: false,
    altKey: false,
    shiftKey: false,
    metaKey: false,
    repeat: false,
    preventDefault: vi.fn(),
    stopPropagation: vi.fn(),
  });

  it.each(Object.keys(labels))(
    'changes the shortcut mode independently and preserves it on save failure in %s',
    async (locale) => {
      await loadLocale(locale);
      localization.locale = locale;
      const config = {
        ...voiceSnapshot().config,
        channels: [{ kind: 'model' as const, id: 'speech-model' }],
        shortcutEnabled: true,
      };
      voice.setState({ snapshot: voiceSnapshot({ config }) });
      act(() => {
        renderer = create(<VoiceInputMain />);
      });
      const tabs = () =>
        renderer.root
          .findAllByType(ViewModeTabs)
          .find((node) =>
            node.props.options.some((option: { value: string }) => option.value === 'hold')
          )!;
      expect(tabs().props.value).toBe('hold');
      expect(tabs().props.options.map((option: { label: string }) => option.label)).toEqual([
        translate('voice.shortcutHold', locale),
        translate('voice.shortcutToggle', locale),
      ]);
      expect(api.saveVoiceConfig).not.toHaveBeenCalled();
      vi.mocked(api.saveVoiceConfig).mockImplementationOnce(async (saved) =>
        voiceSnapshot({ config: saved, revision: 2 })
      );
      await act(async () => tabs().props.onChange('toggle'));
      expect(api.saveVoiceConfig).toHaveBeenCalledExactlyOnceWith({
        ...config,
        hotkeyMode: 'toggle',
      });
      expect(tabs().props.value).toBe('toggle');
      expect(voice.getState().snapshot?.config.hotkeyEnabled).toBe(false);
      expect(voice.getState().snapshot?.config.shortcutEnabled).toBe(true);
      vi.mocked(api.saveVoiceConfig).mockRejectedValueOnce('voice.error.save');
      await act(async () => tabs().props.onChange('hold'));
      expect(tabs().props.value).toBe('toggle');
      for (const phase of ['starting', 'recording', 'transcribing', 'inserting'] as const) {
        act(() =>
          voice.setState({
            snapshot: voiceSnapshot({ config: { ...config, hotkeyMode: 'toggle' }, phase }),
          })
        );
        expect(tabs().parent!.props.disabled).toBe(true);
        await act(async () => tabs().props.onChange('hold'));
      }
      expect(api.saveVoiceConfig).toHaveBeenCalledTimes(2);
      expect(api.startVoice).not.toHaveBeenCalled();
      expect(api.recordVoice).not.toHaveBeenCalled();
      expect(api.stopVoice).not.toHaveBeenCalled();
    }
  );

  it.each(cases)(
    'preserves surrounding card content through capture, cancel and save in $locale on $platform',
    async ({ locale, platform }) => {
      await loadLocale(locale);
      expect(translate('voice.shortcut', locale)).toBe(labels[locale]);
      localization.locale = locale;
      vi.stubGlobal('navigator', { platform });
      act(() => {
        renderer = create(<VoiceInputMain />);
      });
      const card = () =>
        renderer.root.findByProps({ className: 'voice-card voice-shortcut-card rounded-card' });
      const control = () =>
        card().findByProps({ className: 'voice-shortcut-control' }).findAllByType('button')[0];
      const originalCard = card();
      const originalLabel = card().findByType('h3');
      const originalContent = surroundingContent(originalCard);
      const stable = () => {
        expect(card()).toBe(originalCard);
        expect(card().findByType('h3')).toBe(originalLabel);
        expect(surroundingContent(card())).toEqual(originalContent);
        expect(card().findAllByType('input')).toHaveLength(0);
      };
      const sides = {
        left: translate('voice.leftSide', locale),
        right: translate('voice.rightSide', locale),
      };
      expect(originalLabel.children).toContain(translate('voice.shortcut', locale));
      expect(
        originalCard.findByProps({ className: 'voice-shortcut-description' }).children
      ).toEqual([translate('voice.shortcutCaution', locale)]);
      expect(control().children).toEqual([translate('voice.enableShortcut', locale)]);
      expect(button(translate('voice.disableShortcut', locale))).toBeUndefined();
      act(() => control().props.onClick());
      expect(control().children).toEqual([translate('voice.recordShortcut', locale)]);
      stable();
      await act(async () => control().props.onKeyDown(keyEvent('Escape')));
      stable();
      expect(api.saveVoiceConfig).not.toHaveBeenCalled();

      let complete!: (snapshot: ReturnType<typeof voiceSnapshot>) => void;
      vi.mocked(api.saveVoiceConfig).mockImplementationOnce(
        () => new Promise((resolve) => (complete = resolve))
      );
      act(() => control().props.onClick());
      act(() =>
        control().props.onKeyDown({ ...keyEvent('KeyK', 'k'), ctrlKey: true, shiftKey: true })
      );
      expect(voice.getState().saving).toBe(true);
      stable();
      const config = vi.mocked(api.saveVoiceConfig).mock.calls[0][0];
      expect(config.shortcut).toEqual({ code: 'KeyK', modifiers: ['Control', 'Shift'] });
      expect(config.hotkeyEnabled).toBe(true);
      await act(async () => complete(voiceSnapshot({ config, revision: 2 })));
      expect(control().children).toEqual([formatBinding(config.shortcut, platform, sides)]);
      stable();
      expect(api.saveVoiceConfig).toHaveBeenCalledTimes(1);

      vi.mocked(api.saveVoiceConfig).mockRejectedValueOnce('voice.error.shortcut');
      act(() => control().props.onClick());
      await act(async () => control().props.onKeyDown(keyEvent('F8')));
      expect(control().children).toEqual([formatBinding(config.shortcut, platform, sides)]);
      stable();
      expect(renderer.root.findByProps({ role: 'alert' }).children).toEqual([
        translate('voice.error.shortcut', locale),
      ]);
      expect(api.saveVoiceConfig).toHaveBeenCalledTimes(2);
      vi.mocked(api.saveVoiceConfig).mockImplementationOnce(async (saved) =>
        voiceSnapshot({ config: saved, revision: 3 })
      );
      await act(async () => button(translate('voice.disableShortcut', locale)).props.onClick());
      expect(api.saveVoiceConfig).toHaveBeenLastCalledWith({ ...config, hotkeyEnabled: false });
      expect(control().children).toEqual([translate('voice.enableShortcut', locale)]);
      expect(button(translate('voice.disableShortcut', locale))).toBeUndefined();
      stable();
      expect(api.startVoice).not.toHaveBeenCalled();
      expect(api.recordVoice).not.toHaveBeenCalled();
    }
  );
});

describe('last result bubble', () => {
  it('keeps empty-source guidance visible in normal text and removes it once a source is available', () => {
    act(() => {
      renderer = create(<VoiceInputMain />);
    });
    expect(
      renderer.root.findByProps({ className: 'voice-result' }).findByType('p').children
    ).toEqual(['voice.error.noChannels']);
    expect(renderer.root.findAllByProps({ role: 'alert' })).toHaveLength(0);
    act(() =>
      voice
        .getState()
        .accept(voiceSnapshot({ phase: 'error', error: 'voice.error.noChannels', revision: 2 }))
    );
    expect(renderer.root.findAllByProps({ role: 'alert' })).toHaveLength(0);
    act(() =>
      voice.getState().accept(
        voiceSnapshot({
          config: {
            ...voiceSnapshot().config,
            channels: [{ kind: 'model', id: 'speech' }],
            shortcutEnabled: true,
          },
          revision: 3,
        })
      )
    );
    expect(renderer.root.findAllByProps({ className: 'voice-result' })).toHaveLength(0);
    expect(api.saveVoiceConfig).not.toHaveBeenCalled();
    expect(api.recordVoice).not.toHaveBeenCalled();
  });
  it('offers only the matching last recording beside copy, including after recognition fails', () => {
    voice
      .getState()
      .accept(
        voiceSnapshot({ phase: 'done', text: 'Recorded text', resultAudioId: 7, revision: 2 })
      );
    act(() => {
      renderer = create(<VoiceInputMain />);
    });
    const actions = renderer.root.findByProps({ className: 'voice-result-actions' });
    expect(actions.findByType(VoiceReplay).props.audioId).toBe(7);
    expect(
      actions
        .findAllByType('button')
        .map((control) => control.children.filter((child) => typeof child === 'string'))
    ).toEqual([['voice.replay'], ['voice.copy']]);
    act(() =>
      voice.getState().accept(
        voiceSnapshot({
          phase: 'error',
          error: 'voice.error.noTranscript',
          resultAudioId: 8,
          revision: 3,
        })
      )
    );
    const replay = renderer.root.findByType(VoiceReplay);
    expect(replay.props.audioId).toBe(8);
    expect(button('voice.copy')).toBeUndefined();
    act(() => replay.props.onError(true));
    expect(renderer.root.findByProps({ role: 'alert' }).children).toEqual(['voice.error.replay']);
    act(() => voice.getState().accept(voiceSnapshot({ phase: 'recording', revision: 4 })));
    expect(renderer.root.findByType(VoiceReplay).props.disabled).toBe(true);
    act(() => useNavigationStore.setState({ activePage: 'apps' }));
    expect(renderer.root.findByType(VoiceReplay).props.active).toBe(false);
    expect(api.getVoiceReplay).not.toHaveBeenCalled();
    expect(copyText).not.toHaveBeenCalled();
  });

  it('shows empty-source guidance and copies the last result even during the next recording', async () => {
    vi.mocked(copyText).mockResolvedValue(true);
    act(() => {
      renderer = create(<VoiceInputMain />);
    });
    expect(
      renderer.root.findByProps({ className: 'voice-result' }).findAllByType('p')[0].children
    ).toEqual(['voice.error.noChannels']);
    expect(renderer.root.findAllByProps({ role: 'alert' })).toHaveLength(0);
    const text = '完整识别并优化后的文字。\n'.repeat(30);
    act(() => voice.getState().accept(voiceSnapshot({ phase: 'done', text, revision: 2 })));
    const bubble = renderer.root.findByProps({ className: 'voice-result' });
    expect(bubble.parent!.props.className).toBe('voice-preview-stack');
    expect(bubble.parent!.findByProps({ className: 'voice-island-preview' })).toBeDefined();
    expect(bubble.findAllByType('p')[0].children).toEqual([text]);
    expect(bubble.findAllByType('p')).toHaveLength(1);
    expect(copyText).not.toHaveBeenCalled();
    await act(async () => button('voice.copy').props.onClick());
    expect(copyText).toHaveBeenLastCalledWith(text);
    act(() => voice.getState().accept(voiceSnapshot({ phase: 'recording', revision: 3 })));
    expect(renderer.root.findByProps({ className: 'voice-result' })).toBe(bubble);
    expect(bubble.findAllByType('p')[0].children).toEqual([text]);
    await act(async () => button('voice.copied').props.onClick());
    expect(copyText).toHaveBeenCalledTimes(2);
    expect(copyText).toHaveBeenLastCalledWith(text);
    act(() =>
      voice.getState().accept(voiceSnapshot({ phase: 'done', text: 'New final text', revision: 4 }))
    );
    expect(bubble.findAllByType('p')[0].children).toEqual(['New final text']);
    expect(button('voice.copy')).toBeDefined();
    act(() =>
      voice
        .getState()
        .accept(voiceSnapshot({ phase: 'error', error: 'voice.error.noTranscript', revision: 5 }))
    );
    expect(
      renderer.root.findByProps({ className: 'voice-result is-error' }).parent!.props.className
    ).toBe('voice-preview-stack');
    expect(renderer.root.findByProps({ role: 'alert' }).children).toEqual([
      'voice.error.noTranscript',
    ]);
    expect(
      renderer.root.findByProps({ className: 'voice-result is-error' }).findAllByType('p')
    ).toHaveLength(1);
    expect(button('voice.copy')).toBeUndefined();
    expect(copyText).toHaveBeenCalledTimes(2);
    expect(api.saveVoiceConfig).not.toHaveBeenCalled();
    expect(api.recordVoice).not.toHaveBeenCalled();
    expect(api.startVoice).not.toHaveBeenCalled();
  });

  it('keeps the visible transcript when clipboard copying fails', async () => {
    vi.mocked(copyText).mockResolvedValue(false);
    voice
      .getState()
      .accept(voiceSnapshot({ phase: 'done', text: 'Keep this result', revision: 2 }));
    act(() => {
      renderer = create(<VoiceInputMain />);
    });
    await act(async () => button('voice.copy').props.onClick());
    const bubble = renderer.root.findByProps({ className: 'voice-result' });
    expect(bubble.findAllByType('p')[0].children).toEqual(['Keep this result']);
    expect(bubble.findByProps({ role: 'alert' }).children).toEqual(['voice.error.copy']);
    expect(copyText).toHaveBeenCalledExactlyOnceWith('Keep this result');
    expect(button('voice.copy')).toBeDefined();
    vi.mocked(copyText).mockResolvedValue(true);
    await act(async () => button('voice.copy').props.onClick());
    expect(bubble.findAllByProps({ role: 'alert' })).toHaveLength(0);
    expect(button('voice.copied')).toBeDefined();
    expect(api.recordVoice).not.toHaveBeenCalled();
  });
});

it('anchors three curved routes to the preview and remeasures or hides them as space changes', () => {
  const config = {
    ...voiceSnapshot().config,
    shortcutEnabled: true,
    channels: [
      { kind: 'model' as const, id: 'primary' },
      { kind: 'model' as const, id: 'backup' },
    ],
  };
  voice.setState({ snapshot: voiceSnapshot({ config }) });
  let width = 800;
  let previewY = 140;
  let frame!: FrameRequestCallback;
  let resize!: () => void;
  const disconnect = vi.fn();
  const observe = vi.fn();
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
    frame = callback;
    return 1;
  });
  const cancelFrame = vi.fn();
  vi.stubGlobal('cancelAnimationFrame', cancelFrame);
  vi.stubGlobal(
    'ResizeObserver',
    class {
      constructor(callback: () => void) {
        resize = callback;
      }
      observe = observe;
      disconnect = disconnect;
    }
  );
  const rect = (left: number, top: number, nodeWidth: number, height: number) => ({
    left,
    right: left + nodeWidth,
    top,
    width: nodeWidth,
    height,
  });
  const previewArea = {};
  const island = { getBoundingClientRect: () => rect(100, 50 + previewY - 22, 176, 44) };
  let cardGeneration = 0;
  const createCards = () => {
    const generation = cardGeneration;
    return Array.from({ length: 3 }, (_, index) => ({
      getBoundingClientRect: () =>
        generation === cardGeneration
          ? rect(40 + width - 280, 58 + index * 116, 254, 80)
          : rect(0, 0, 0, 0),
    }));
  };
  let cards = createCards();
  act(() => {
    renderer = create(<VoiceInputMain />, {
      createNodeMock: (element) =>
        element.props.className === 'voice-routing-stage'
          ? {
              getBoundingClientRect: () => rect(40, 50, width, 336),
              addEventListener: vi.fn(),
              removeEventListener: vi.fn(),
              querySelector: (selector: string) =>
                selector === '.voice-island-preview' ? previewArea : island,
              querySelectorAll: () => cards,
            }
          : null,
    });
  });
  const paths = () =>
    renderer.root
      .findAllByProps({ className: 'free-model-route-path' })
      .map((path) => path.props.d as string);
  act(() => frame(0));
  expect(paths()).toHaveLength(3);
  expect(paths().map((path) => path.split(' C ')[0])).toEqual([
    'M 510 48',
    'M 510 164',
    'M 510 280',
  ]);
  expect(paths().every((path) => path.includes(' C ') && path.endsWith('254 140'))).toBe(true);
  expect(observe).toHaveBeenCalledTimes(5);
  expect(observe).toHaveBeenCalledWith(previewArea);
  const routes = renderer.root.findAllByProps({ className: 'free-model-route-path' });
  expect(routes.map((path) => path.parent!.props.className)).toEqual([
    undefined,
    undefined,
    'is-disabled',
  ]);
  const glow = () => renderer.root.findAllByProps({ className: 'free-model-route-glow' });
  expect(glow()).toHaveLength(0);
  const idlePaths = paths();
  // Entering or leaving a recording recreates the sortable cards to cancel dragging.
  cardGeneration++;
  cards = createCards();
  for (const [revision, phase, activeChannel] of [
    [2, 'recording', 0],
    [3, 'transcribing', 1],
  ] as const) {
    act(() => voice.getState().accept(voiceSnapshot({ config, phase, activeChannel, revision })));
    act(() => {
      frame(0);
      resize();
    });
    expect(paths()).toEqual(idlePaths);
    expect(glow()).toHaveLength(1);
    expect(glow()[0].props.d).toBe(paths()[activeChannel]);
    expect(glow()[0].props.pathLength).toBe('1');
  }
  cardGeneration++;
  cards = createCards();
  act(() => voice.getState().accept(voiceSnapshot({ config, phase: 'done', revision: 4 })));
  act(() => {
    frame(0);
    resize();
  });
  expect(paths()).toEqual(idlePaths);
  expect(glow()).toHaveLength(0);
  act(() =>
    voice.getState().accept(
      voiceSnapshot({
        config: { ...config, shortcutEnabled: false },
        phase: 'transcribing',
        activeChannel: 1,
        revision: 5,
      })
    )
  );
  expect(glow()).toHaveLength(0);
  expect(routes[0].parent!.parent!.props.className).toContain('is-disabled');
  act(() => {
    width = 520;
    resize();
  });
  expect(paths()).toEqual([]);
  expect(renderer.root.findByProps({ className: 'free-model-route-arrow' }).props.d).toBe('');
  act(() => {
    width = 900;
    previewY = 120;
    resize();
  });
  expect(paths()).toHaveLength(3);
  expect(paths().every((path) => path.startsWith('M 610 ') && path.endsWith('254 120'))).toBe(true);
  const route = renderer.root.findByProps({ className: 'free-model-route-arrow' }).parent!;
  expect(route.props.viewBox).toBe('0 0 900 336');
  expect(api.saveVoiceConfig).not.toHaveBeenCalled();
  expect(api.startVoice).not.toHaveBeenCalled();
  expect(api.recordVoice).not.toHaveBeenCalled();
  disconnect.mockClear();
  cancelFrame.mockClear();
  act(() => renderer.unmount());
  expect(disconnect).toHaveBeenCalledOnce();
  expect(cancelFrame).toHaveBeenCalledExactlyOnceWith(1);
  expect(window.removeEventListener).toHaveBeenCalledWith('resize', resize);
});

describe('selection model and island size', () => {
  it('removes text refinement and keeps only the selection model, language and switch', () => {
    act(() => {
      renderer = create(<VoiceInputMain />);
    });
    const json = JSON.stringify(renderer.toJSON());
    for (const removed of [
      'voice.optimization',
      'voice.optimizationMode',
      'voice.autoTranslate',
      'voice-optimization',
    ])
      expect(json).not.toContain(removed);
    const row = renderer.root.findByProps({
      className: 'voice-card voice-selection-settings rounded-card',
    });
    expect(row.findAllByType(RemoteModelSelector)).toHaveLength(1);
    expect(row.findByType(MiniSelect).props.disabled).toBe(true);
    expect(api.saveVoiceConfig).not.toHaveBeenCalled();
  });

  it('uses Mother Agent model icons and router entries and saves only the selection model', async () => {
    mother.models = [
      { internalId: 'text-model', name: 'DeepSeek', modelId: 'deepseek-chat' },
      { internalId: 'smart-router', name: 'Router', modelId: 'auto' },
    ];
    vi.mocked(api.saveVoiceConfig).mockImplementation(async (saved) =>
      voiceSnapshot({ config: saved, revision: 2 })
    );
    act(() => {
      renderer = create(<VoiceInputMain />);
    });
    const selector = renderer.root.findByType(RemoteModelSelector);
    expect(selector.props.models).toEqual([
      expect.objectContaining({
        id: 'text-model',
        name: 'DeepSeek',
        icon: expect.stringContaining('deepseek'),
      }),
      expect.objectContaining({ id: 'smart-router', name: 'Router', iconKind: 'router' }),
    ]);
    act(() => selector.findByType('button').props.onClick());
    expect(JSON.stringify(renderer.toJSON())).not.toContain('Claude Code');
    const option = selector
      .findAllByType('img')
      .find((image) => image.props.src.includes('deepseek'))!;
    await act(async () => option.parent!.props.onClick());
    expect(api.saveVoiceConfig).toHaveBeenCalledExactlyOnceWith({
      ...voiceSnapshot().config,
      selectionModel: 'text-model',
    });
    expect(renderer.root.findByProps({ 'aria-label': 'voice.selection' }).props.disabled).toBe(
      false
    );
    expect(mother.setAgentModel).not.toHaveBeenCalled();
    expect(mother.setParasiteAgent).not.toHaveBeenCalled();
    expect(api.openSelectionTranslation).not.toHaveBeenCalled();
    expect(api.recordVoice).not.toHaveBeenCalled();
    expect(api.startVoice).not.toHaveBeenCalled();
  });

  it('retains the saved model after a failed save without remounting the model menu', async () => {
    let fail!: (reason: Error) => void;
    vi.mocked(api.saveVoiceConfig).mockImplementationOnce(
      () => new Promise((_, reject) => (fail = reject))
    );
    act(() => {
      renderer = create(<VoiceInputMain />);
    });
    const selector = renderer.root.findByType(RemoteModelSelector);
    const config = voice.getState().snapshot!.config;
    act(() => selector.props.onSelect('text-model'));
    expect(voice.getState().saving).toBe(true);
    await act(async () => fail(new Error('save failed')));
    expect(voice.getState().snapshot!.config).toEqual(config);
    expect(renderer.root.findByType(RemoteModelSelector)).toBe(selector);
    expect(renderer.root.findByProps({ role: 'alert' }).children).toContain('Error: save failed');
    expect(api.saveVoiceConfig).toHaveBeenCalledTimes(1);
    expect(api.recordVoice).not.toHaveBeenCalled();
  });

  it('closes the model menu on leaving and blocks changes while hidden or voice is busy', async () => {
    mother.models = [{ internalId: 'text-model', name: 'DeepSeek', modelId: 'deepseek-chat' }];
    useNavigationStore.setState({ activePage: 'voice' });
    act(() => {
      renderer = create(<VoiceInputMain />);
    });
    const selector = () => renderer.root.findByType(RemoteModelSelector);
    act(() => selector().findByType('button').props.onClick());
    expect(selector().findAllByType('img')).toHaveLength(1);
    act(() => useNavigationStore.setState({ activePage: 'models' }));
    expect(selector().findAllByType('img')).toHaveLength(0);
    await act(async () => selector().props.onSelect('text-model'));
    act(() => useNavigationStore.setState({ activePage: 'voice' }));
    act(() => voice.setState({ snapshot: voiceSnapshot({ phase: 'recording' }) }));
    await act(async () => selector().props.onSelect('text-model'));
    expect(api.saveVoiceConfig).not.toHaveBeenCalled();
    expect(api.recordVoice).not.toHaveBeenCalled();
  });

  it('drags the island size locally, saves once on release and locks it while voice is busy', async () => {
    vi.mocked(api.saveVoiceConfig).mockImplementation(async (saved) =>
      voiceSnapshot({ config: saved, revision: 2 })
    );
    act(() => {
      renderer = create(<VoiceInputMain />);
    });
    const slider = () => renderer.root.findByProps({ type: 'range' });
    expect(slider().props).toMatchObject({
      min: 70,
      max: 150,
      step: 10,
      value: 100,
      'aria-label': 'voice.islandScale',
    });
    for (const value of ['110', '120', '130'])
      act(() => slider().props.onChange({ target: { value } }));
    expect(slider().props.value).toBe(130);
    expect(slider().props.disabled).toBe(false);
    expect(api.saveVoiceConfig).not.toHaveBeenCalled();
    act(() => slider().props.onPointerDown());
    const release = vi
      .mocked(window.addEventListener)
      .mock.calls.filter((call) => call[0] === 'pointerup')
      .pop();
    expect(release?.[2]).toEqual({ once: true });
    // Released outside the slider: only the window listener sees it.
    await act(async () => (release![1] as () => void)());
    expect(api.saveVoiceConfig).toHaveBeenCalledExactlyOnceWith({
      ...voiceSnapshot().config,
      islandScale: 130,
    });
    expect(slider().props.value).toBe(130);
    await act(async () => slider().props.onBlur());
    expect(api.saveVoiceConfig).toHaveBeenCalledTimes(1);
    act(() => voice.setState({ snapshot: voiceSnapshot({ phase: 'transcribing' }) }));
    expect(slider().props.disabled).toBe(true);
    expect(api.recordVoice).not.toHaveBeenCalled();
  });
});

function LiveIsland() {
  const snapshot = voice((state) => state.snapshot);
  return snapshot && <IslandContent snapshot={snapshot} />;
}

describe('island speech entry', () => {
  it('allows dragging by default while isolating non-left clicks, preview and the microphone', async () => {
    const config = { ...voiceSnapshot().config, shortcutEnabled: true };
    const mouse = (button = 0, control = false) => ({
      button,
      detail: 1,
      target: { closest: () => (control ? {} : null) },
      preventDefault: vi.fn(),
    });
    const down = (event: ReturnType<typeof mouse>) =>
      renderer.root.findByProps({ 'data-error': false }).props.onMouseDown(event);
    act(() => {
      renderer = create(<IslandContent snapshot={voiceSnapshot({ config })} />);
    });
    await act(async () => {
      down(mouse(2));
      down(mouse(0, true));
    });
    expect(startDragging).not.toHaveBeenCalled();
    const event = mouse();
    await act(async () => down(event));
    expect(event.preventDefault).toHaveBeenCalledOnce();
    expect(startDragging).toHaveBeenCalledOnce();
    startDragging.mockRejectedValueOnce(new Error('native drag unavailable'));
    await act(async () => down(mouse()));
    expect(voice.getState().error).toBeNull();
    act(() => renderer.update(<IslandContent snapshot={voiceSnapshot({ config })} preview />));
    await act(async () => down(mouse()));
    expect(startDragging).toHaveBeenCalledTimes(2);
    expect(api.recordVoice).not.toHaveBeenCalled();
    expect(api.startVoice).not.toHaveBeenCalled();
    expect(api.stopVoice).not.toHaveBeenCalled();
    expect(api.cancelVoice).not.toHaveBeenCalled();
    expect(api.saveVoiceConfig).not.toHaveBeenCalled();
    expect(api.showMainWindow).not.toHaveBeenCalled();
  });

  it.each(['idle', 'recording', 'transcribing', 'inserting'] as const)(
    'restores the existing main window on the second press while %s without changing speech',
    async (phase) => {
      const snapshot = voiceSnapshot({
        phase,
        config: { ...voiceSnapshot().config, shortcutEnabled: true },
      });
      useNavigationStore.setState({ activePage: 'models' });
      voice.setState({ snapshot });
      act(() => {
        renderer = create(<IslandContent snapshot={snapshot} />);
      });
      const down = renderer.root.findByProps({ 'data-error': false }).props.onMouseDown;
      const event = {
        button: 0,
        target: { closest: () => null },
        preventDefault: vi.fn(),
      };
      await act(async () => down({ ...event, detail: 1 }));
      expect(startDragging).toHaveBeenCalledOnce();
      expect(api.showMainWindow).not.toHaveBeenCalled();
      await act(async () => down({ ...event, detail: 2 }));
      expect(startDragging).toHaveBeenCalledOnce();
      expect(api.showMainWindow).toHaveBeenCalledExactlyOnceWith();
      expect(voice.getState().snapshot).toBe(snapshot);
      expect(useNavigationStore.getState().activePage).toBe('models');
      expect(api.recordVoice).not.toHaveBeenCalled();
      expect(api.startVoice).not.toHaveBeenCalled();
      expect(api.stopVoice).not.toHaveBeenCalled();
      expect(api.cancelVoice).not.toHaveBeenCalled();
      expect(api.saveVoiceConfig).not.toHaveBeenCalled();
    }
  );

  it.each([
    [0, true, false],
    [2, false, false],
    [0, false, true],
  ])('isolates double press button=%s control=%s preview=%s', async (button, control, preview) => {
    act(() => {
      renderer = create(<IslandContent snapshot={voiceSnapshot()} preview={Boolean(preview)} />);
    });
    await act(async () =>
      renderer.root.findByProps({ 'data-error': false }).props.onMouseDown({
        button,
        detail: 2,
        target: { closest: () => (control ? {} : null) },
        preventDefault: vi.fn(),
      })
    );
    expect(api.showMainWindow).not.toHaveBeenCalled();
    expect(startDragging).not.toHaveBeenCalled();
    expect(api.recordVoice).not.toHaveBeenCalled();
  });

  it('keeps a failed window restore from changing or interrupting speech', async () => {
    const snapshot = voiceSnapshot({ phase: 'recording' });
    voice.setState({ snapshot });
    vi.mocked(api.showMainWindow).mockRejectedValueOnce(new Error('restore unavailable'));
    act(() => {
      renderer = create(<IslandContent snapshot={snapshot} />);
    });
    await act(async () =>
      renderer.root.findByProps({ 'data-error': false }).props.onMouseDown({
        button: 0,
        detail: 2,
        target: { closest: () => null },
        preventDefault: vi.fn(),
      })
    );
    expect(api.showMainWindow).toHaveBeenCalledOnce();
    expect(startDragging).not.toHaveBeenCalled();
    expect(voice.getState().snapshot).toBe(snapshot);
    expect(voice.getState().error).toBeNull();
    expect(api.stopVoice).not.toHaveBeenCalled();
    expect(api.cancelVoice).not.toHaveBeenCalled();
  });

  it.each(['white', 'aurora', 'ocean', 'sunset', 'iris'] as const)(
    'animates idle %s like the preview without capturing and retains live volume',
    (islandPalette) => {
      const config = { ...voiceSnapshot().config, islandPalette, shortcutEnabled: true };
      const graphics = () =>
        renderer.root.findAllByType(VoiceFlow).length > 0
          ? [renderer.root.findByType(VoiceFlow).props.level]
          : renderer.root.findAllByType('i').map((node) => node.props.style.height);
      act(() => {
        renderer = create(<IslandContent snapshot={voiceSnapshot({ config })} preview />);
      });
      const preview = graphics();
      for (const phase of ['idle', 'done'] as const) {
        act(() => renderer.update(<IslandContent snapshot={voiceSnapshot({ config, phase })} />));
        expect(renderer.root.findByProps({ 'data-error': false }).props['data-active']).toBe(true);
        expect(graphics()).toEqual(preview);
      }
      act(() =>
        renderer.update(
          <IslandContent snapshot={voiceSnapshot({ config, phase: 'recording', level: 0.2 })} />
        )
      );
      expect(graphics()).not.toEqual(preview);
      act(() =>
        renderer.update(
          <IslandContent snapshot={voiceSnapshot({ config, phase: 'transcribing' })} />
        )
      );
      expect(renderer.root.findByProps({ 'data-error': false }).props['data-active']).toBe(false);
      expect(api.recordVoice).not.toHaveBeenCalled();
      expect(api.startVoice).not.toHaveBeenCalled();
      expect(api.saveVoiceConfig).not.toHaveBeenCalled();
    }
  );

  it.each(['white', 'aurora', 'ocean', 'sunset', 'iris'] as const)(
    'uses microphone-test volume in %s without starting speech or saving',
    (islandPalette) => {
      const config = { ...voiceSnapshot().config, islandPalette };
      act(() => {
        renderer = create(<IslandContent snapshot={voiceSnapshot({ config })} preview />);
      });
      expect(renderer.root.findAllByType(VoiceFlow)).toHaveLength(0);
      expect(renderer.root.findAllByType('i')).toHaveLength(28);
      act(() =>
        renderer.update(
          <IslandContent
            snapshot={voiceSnapshot({ config, phase: 'testing', level: 0.7 })}
            preview
          />
        )
      );
      expect(renderer.root.findByType(VoiceFlow).props.level).toBe(0.7);
      expect(renderer.root.findByType(VoiceFlow).props.speaking).toBe(true);
      expect(renderer.root.findByType(VoiceFlow).props.active).toBe(true);
      expect(api.recordVoice).not.toHaveBeenCalled();
      expect(api.startVoice).not.toHaveBeenCalled();
      expect(api.saveVoiceConfig).not.toHaveBeenCalled();
    }
  );

  it.each(['white', 'aurora', 'ocean', 'sunset', 'iris'] as const)(
    'feeds recording volume into scrolling %s including silence',
    (islandPalette) => {
      const config = { ...voiceSnapshot().config, islandPalette };
      act(() => {
        renderer = create(
          <IslandContent snapshot={voiceSnapshot({ config, phase: 'recording', level: 0 })} />
        );
      });
      expect(renderer.root.findByType(VoiceFlow).props).toMatchObject({
        active: true,
        speaking: true,
        level: 0,
        palette: islandPalette,
      });
      act(() =>
        renderer.update(
          <IslandContent snapshot={voiceSnapshot({ config, phase: 'recording', level: 0.6 })} />
        )
      );
      expect(renderer.root.findByType(VoiceFlow).props.level).toBe(0.6);
      act(() => renderer.update(<IslandContent snapshot={voiceSnapshot({ config })} />));
      expect(renderer.root.findAllByType(VoiceFlow)).toHaveLength(0);
      expect(renderer.root.findAllByType('i')).toHaveLength(28);
      expect(api.recordVoice).not.toHaveBeenCalled();
      expect(api.startVoice).not.toHaveBeenCalled();
      expect(api.saveVoiceConfig).not.toHaveBeenCalled();
    }
  );

  it('syncs theme changes in the floating window and cleans up both subscriptions without recording or saving', async () => {
    const handlers = new Map<string, Parameters<typeof listen>[1]>();
    const stops: ReturnType<typeof vi.fn>[] = [];
    vi.mocked(listen).mockImplementation(async (event, callback) => {
      handlers.set(String(event), callback);
      const stop = vi.fn();
      stops.push(stop);
      return stop;
    });
    vi.mocked(api.voiceIslandReady).mockResolvedValue(voiceSnapshot());
    await act(async () => {
      renderer = create(<VoiceIsland />);
    });
    expect(themeSync).not.toHaveBeenCalled();
    const selection = { mode: 'light', colorTheme: 'aurora' };
    act(() =>
      handlers.get('echobird-theme-changed')!({
        event: 'echobird-theme-changed',
        id: 0,
        payload: selection,
      })
    );
    expect(themeSync).toHaveBeenCalledExactlyOnceWith(selection);
    expect(api.saveVoiceConfig).not.toHaveBeenCalled();
    expect(api.recordVoice).not.toHaveBeenCalled();
    expect(api.startVoice).not.toHaveBeenCalled();
    act(() => renderer.unmount());
    expect(stops).toHaveLength(2);
    expect(stops.every((stop) => stop.mock.calls.length === 1)).toBe(true);
  });

  it('starts speech from an explicit click and stops it without using the microphone test command', async () => {
    const config = { ...voiceSnapshot().config, shortcutEnabled: true };
    voice.setState({ snapshot: voiceSnapshot({ config }) });
    vi.mocked(api.recordVoice).mockResolvedValue(
      voiceSnapshot({ config, phase: 'recording', origin: 'island', revision: 2 })
    );
    vi.mocked(api.stopVoice).mockResolvedValue(
      voiceSnapshot({ config, phase: 'done', origin: 'island', revision: 3 })
    );
    act(() => {
      renderer = create(<LiveIsland />);
    });
    expect(api.recordVoice).not.toHaveBeenCalled();
    await act(async () => button('voice.speak').props.onClick());
    expect(api.recordVoice).toHaveBeenCalledExactlyOnceWith();
    expect(api.startVoice).not.toHaveBeenCalled();
    expect(button('voice.speak')).toBeUndefined();
    await act(async () =>
      renderer.root.findByProps({ 'aria-label': 'voice.stop' }).props.onClick()
    );
    expect(api.stopVoice).toHaveBeenCalledExactlyOnceWith();
    expect(button('voice.speak')).toBeDefined();
  });

  it('blocks speech when disabled and keeps the settings preview read-only', async () => {
    act(() => {
      renderer = create(<LiveIsland />);
    });
    expect(button('voice.speak').props.disabled).toBe(true);
    await act(async () => button('voice.speak').props.onClick());
    expect(api.recordVoice).not.toHaveBeenCalled();
    act(() => renderer.update(<IslandContent snapshot={voiceSnapshot()} preview />));
    expect(button('voice.speak')).toBeUndefined();
    expect(renderer.root.findAllByType('button').every((control) => control.props.disabled)).toBe(
      true
    );
    expect(api.startVoice).not.toHaveBeenCalled();
  });

  it('shows the foreground app icon without adding a left-side recording action', () => {
    const config = { ...voiceSnapshot().config, shortcutEnabled: true };
    voice.setState({ snapshot: voiceSnapshot({ config, phase: 'recording', origin: 'island' }) });
    act(() => {
      renderer = create(<LiveIsland />);
    });
    expect(renderer.root.findByType('img').props.src).toBe('/brand/bird.png');
    expect(renderer.root.findAllByType('button')).toHaveLength(1);
    act(() =>
      voice.setState({ snapshot: voiceSnapshot({ config, appIcon: 'data:image/png;base64,icon' }) })
    );
    expect(renderer.root.findByType('img').props.src).toBe('data:image/png;base64,icon');
    expect(api.cancelVoice).not.toHaveBeenCalled();
    expect(api.stopVoice).not.toHaveBeenCalled();
    expect(api.recordVoice).not.toHaveBeenCalled();
    expect(button('voice.speak').props.disabled).toBe(false);
  });

  it('keeps native icon loading out of the settings preview and speech controls', () => {
    const config = { ...voiceSnapshot().config, shortcutEnabled: true };
    const snapshot = voiceSnapshot({ config, appIcon: 'native', appIconLoading: true });
    act(() => {
      renderer = create(<IslandContent snapshot={snapshot} />);
    });
    expect(renderer.root.findByProps({ 'data-spinning': true })).toBeDefined();
    expect(button('voice.speak').props.disabled).toBe(false);
    act(() => renderer.update(<IslandContent snapshot={snapshot} preview />));
    expect(renderer.root.findByType('img').props.src).toBe('/brand/bird.png');
    expect(renderer.root.findAllByProps({ 'data-spinning': true })).toHaveLength(0);
    expect(api.recordVoice).not.toHaveBeenCalled();
    expect(api.stopVoice).not.toHaveBeenCalled();
    expect(api.saveVoiceConfig).not.toHaveBeenCalled();
  });

  it('keeps the speech entry after completion without displaying result text or copy actions', () => {
    const config = { ...voiceSnapshot().config, shortcutEnabled: true };
    voice.setState({
      snapshot: voiceSnapshot({ config, phase: 'done', origin: 'island', text: 'Saved text' }),
    });
    act(() => {
      renderer = create(<LiveIsland />);
    });
    expect(renderer.root.findAllByType('button')).toHaveLength(1);
    expect(JSON.stringify(renderer.toJSON())).not.toContain('Saved text');
    expect(copyText).not.toHaveBeenCalled();
    expect(button('voice.speak')).toBeDefined();
    expect(api.cancelVoice).not.toHaveBeenCalled();
    expect(api.stopVoice).not.toHaveBeenCalled();
  });

  it.each(['white', 'aurora', 'ocean', 'sunset', 'iris'] as const)(
    'previews %s as graphics without time or visible status text',
    (islandPalette) => {
      const config = { ...voiceSnapshot().config, islandPalette };
      act(() => {
        renderer = create(
          <IslandContent
            snapshot={voiceSnapshot({ config, text: 'Transcript', elapsedMs: 6000 })}
            preview
          />
        );
      });
      expect(
        renderer.root
          .findAllByType('span')
          .filter((span) => span.children.some((child) => typeof child === 'string'))
          .every((span) => span.props.className === 'sr-only')
      ).toBe(true);
      expect(JSON.stringify(renderer.toJSON())).not.toContain('0:06');
      expect(JSON.stringify(renderer.toJSON())).not.toContain('Transcript');
      expect(renderer.root.findAllByType('button').every((control) => control.props.disabled)).toBe(
        true
      );
      expect(api.recordVoice).not.toHaveBeenCalled();
      expect(api.startVoice).not.toHaveBeenCalled();
    }
  );

  it('shows an explicit speech-start failure without entering microphone testing', async () => {
    voice.setState({
      snapshot: voiceSnapshot({ config: { ...voiceSnapshot().config, shortcutEnabled: true } }),
    });
    vi.mocked(api.recordVoice).mockRejectedValue('voice.error.noChannels');
    act(() => {
      renderer = create(<LiveIsland />);
    });
    await act(async () => button('voice.speak').props.onClick());
    expect(renderer.root.findByProps({ role: 'status' }).children).toContain(
      'voice.error.noChannels'
    );
    expect(button('voice.speak').props.disabled).toBe(false);
    expect(api.startVoice).not.toHaveBeenCalled();
  });

  it('shows an empty recognition result as an error instead of a completed dictation', () => {
    voice.setState({
      snapshot: voiceSnapshot({
        phase: 'error',
        origin: 'shortcut',
        text: '',
        error: 'voice.error.noTranscript',
      }),
    });
    act(() => {
      renderer = create(<LiveIsland />);
    });
    expect(renderer.root.findByProps({ 'data-error': true }).props['data-active']).toBe(false);
    expect(renderer.root.findByProps({ role: 'status' }).children).toEqual([
      'voice.error.noTranscript',
    ]);
    expect(api.recordVoice).not.toHaveBeenCalled();
    expect(api.stopVoice).not.toHaveBeenCalled();
    expect(copyText).not.toHaveBeenCalled();
  });
});

function systemAppearanceFixture(dark: boolean) {
  const listeners = new Set<() => void>();
  const query = {
    matches: dark,
    addEventListener: vi.fn((_name: string, notify: () => void) => listeners.add(notify)),
    removeEventListener: vi.fn((_name: string, notify: () => void) => listeners.delete(notify)),
  };
  const fallback = window.matchMedia;
  vi.stubGlobal('window', {
    ...window,
    matchMedia: (name: string) =>
      name === '(prefers-color-scheme: dark)' ? query : fallback(name),
  });
  return {
    query,
    listeners,
    change: (value: boolean) =>
      act(() => {
        query.matches = value;
        listeners.forEach((notify) => notify());
      }),
  };
}

describe('independent island appearance', () => {
  it.each(['zh-Hans', 'zh-Hant', 'en', 'ja'])(
    'offers the existing appearance modes and the new iridescent mode in %s',
    (locale) => {
      localization.locale = locale;
      act(() => {
        renderer = create(<VoiceInputMain />);
      });
      expect(
        renderer.root
          .findAllByType(ViewModeTabs)
          .find((tabs) =>
            tabs.props.options.some((option: { value: string }) => option.value === 'auto')
          )!.props.options
      ).toEqual(
        ['auto', 'iridescent', 'dark', 'light', 'system'].map((mode) => ({
          value: mode,
          label: translate(`voice.${mode}` as TKey, locale),
        }))
      );
      expect(api.saveVoiceConfig).not.toHaveBeenCalled();
    }
  );

  it.each(['system', 'iridescent'] as const)(
    'saves the independent %s choice without changing the app theme',
    async (mode) => {
      systemAppearanceFixture(false);
      const config = voiceSnapshot().config;
      vi.mocked(api.saveVoiceConfig).mockImplementation(async (saved) =>
        voiceSnapshot({ config: saved, revision: 2 })
      );
      act(() => {
        renderer = create(<VoiceInputMain />);
      });
      await act(async () => button(`voice.${mode}`).props.onClick());
      expect(api.saveVoiceConfig).toHaveBeenCalledExactlyOnceWith({
        ...config,
        islandTheme: mode,
      });
      expect(renderer.root.findByProps({ 'data-error': false }).props.className).toBe(
        `voice-island voice-theme-${mode === 'system' ? 'light' : 'iridescent'}`
      );
      expect(appAppearance.resolved).toBe('dark');
      expect(themeSync).not.toHaveBeenCalled();
      expect(api.startVoice).not.toHaveBeenCalled();
    }
  );

  it('tracks live OS appearance independently and cleans its media subscription without writes', () => {
    const system = systemAppearanceFixture(false);
    const config = {
      ...voiceSnapshot().config,
      islandTheme: 'system' as const,
      islandPalette: 'aurora' as const,
    };
    act(() => {
      renderer = create(<IslandContent snapshot={voiceSnapshot({ config })} preview />);
    });
    const island = () => renderer.root.findByProps({ 'data-error': false });
    const firstBar = () =>
      renderer.root.findByProps({ className: 'voice-island-bars' }).findAllByType('i')[0];
    expect(island().props.className).toBe('voice-island voice-theme-light');
    expect(firstBar().props.style.backgroundColor).toBe('rgb(11,126,88)');
    system.change(true);
    expect(island().props.className).toBe('voice-island voice-theme-dark');
    expect(firstBar().props.style.backgroundColor).toBe('rgb(16,185,129)');
    system.change(false);
    expect(island().props.className).toBe('voice-island voice-theme-light');
    expect(appAppearance.resolved).toBe('dark');
    expect(themeSync).not.toHaveBeenCalled();
    expect(api.saveVoiceConfig).not.toHaveBeenCalled();
    expect(api.recordVoice).not.toHaveBeenCalled();
    act(() => renderer.unmount());
    expect(system.listeners.size).toBe(0);
    expect(system.query.removeEventListener).toHaveBeenCalled();
  });

  it('keeps the app-following mode tied to EchoBird when app and system disagree', () => {
    const system = systemAppearanceFixture(false);
    const config = { ...voiceSnapshot().config, islandPalette: 'aurora' as const };
    act(() => {
      renderer = create(<IslandContent snapshot={voiceSnapshot({ config })} preview />);
    });
    const firstBar = () =>
      renderer.root.findByProps({ className: 'voice-island-bars' }).findAllByType('i')[0];
    expect(renderer.root.findByProps({ 'data-error': false }).props.className).toBe(
      'voice-island voice-theme-auto'
    );
    expect(firstBar().props.style.backgroundColor).toBe('rgb(16,185,129)');
    appAppearance.resolved = 'light';
    act(() => renderer.update(<IslandContent snapshot={voiceSnapshot({ config })} preview />));
    expect(firstBar().props.style.backgroundColor).toBe('rgb(11,126,88)');
    system.change(true);
    expect(firstBar().props.style.backgroundColor).toBe('rgb(11,126,88)');
    expect(api.saveVoiceConfig).not.toHaveBeenCalled();
  });

  it.each(['dark', 'light', 'iridescent'] as const)(
    'keeps forced %s independent of OS changes',
    (mode) => {
      const system = systemAppearanceFixture(mode !== 'dark');
      appAppearance.resolved = mode === 'dark' ? 'light' : 'dark';
      const config = { ...voiceSnapshot().config, islandTheme: mode };
      act(() => {
        renderer = create(<IslandContent snapshot={voiceSnapshot({ config })} preview />);
      });
      expect(renderer.root.findByProps({ 'data-error': false }).props.className).toBe(
        `voice-island voice-theme-${mode}`
      );
      system.change(mode === 'dark');
      expect(renderer.root.findByProps({ 'data-error': false }).props.className).toBe(
        `voice-island voice-theme-${mode}`
      );
      expect(api.saveVoiceConfig).not.toHaveBeenCalled();
    }
  );

  it.each(['system', 'iridescent'] as const)(
    'keeps the active waveform contrast correct for %s when OS appearance changes',
    (mode) => {
      const system = systemAppearanceFixture(false);
      const config = { ...voiceSnapshot().config, islandTheme: mode };
      act(() => {
        renderer = create(
          <IslandContent snapshot={voiceSnapshot({ config, phase: 'recording', level: 0.5 })} />
        );
      });
      expect(renderer.root.findByType(VoiceFlow).props.theme).toBe(
        mode === 'system' ? 'light' : 'dark'
      );
      system.change(true);
      expect(renderer.root.findByType(VoiceFlow).props.theme).toBe('dark');
      expect(api.recordVoice).not.toHaveBeenCalled();
      expect(api.saveVoiceConfig).not.toHaveBeenCalled();
    }
  );
});

describe('live island transcript', () => {
  it.each(['recording', 'transcribing'] as const)(
    'shows interim text above the capsule while %s',
    (phase) => {
      act(() => {
        renderer = create(
          <IslandTranscript snapshot={voiceSnapshot({ phase, text: '一边说一边出字' })} />
        );
      });
      expect(renderer.root.findByType('p').children).toEqual(['一边说一边出字']);
      expect(api.saveVoiceConfig).not.toHaveBeenCalled();
    }
  );

  it.each([
    ['recording', '  '],
    ['idle', 'old'],
    ['inserting', 'final'],
    ['done', 'final'],
    ['error', 'partial'],
  ] as const)('stays hidden while %s with %j', (phase, text) => {
    act(() => {
      renderer = create(<IslandTranscript snapshot={voiceSnapshot({ phase, text })} />);
    });
    expect(renderer.toJSON()).toBeNull();
  });
});
