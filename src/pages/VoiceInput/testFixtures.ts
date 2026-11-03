import type { VoiceSnapshot } from '../../api/voiceInput';

export function voiceSnapshot(overrides: Partial<VoiceSnapshot> = {}): VoiceSnapshot {
  return {
    config: {
      channels: [],
      microphone: null,
      shortcutEnabled: false,
      hotkeyEnabled: false,
      hotkeyMode: 'hold',
      shortcut: { code: 'AltRight', modifiers: [] },
      islandTheme: 'auto',
      islandPalette: 'white',
      islandScale: 100,
      selectionModel: null,
    },
    days: {},
    usage: { characters: 0, durationMs: 0 },
    phase: 'idle',
    activeChannel: null,
    level: 0,
    elapsedMs: 0,
    text: '',
    resultAudioId: null,
    error: null,
    revision: 1,
    shortcutActive: false,
    shortcutDescription: null,
    appIcon: null,
    appIconLoading: false,
    origin: 'page',
    ...overrides,
  };
}
