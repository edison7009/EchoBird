import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { useVoiceInputStore as voice } from '../stores/voiceInputStore';
import { voiceSnapshot } from '../pages/VoiceInput/testFixtures';
import * as voiceApi from '../api/voiceInput';
import { useSidebarStore } from '../stores/sidebarStore';
import { useNavigationStore } from '../stores/navigationStore';
import { Mic, RefreshCw, MessagesSquare, Waypoints } from 'lucide-react';

vi.mock('./BirdEasterEgg', () => ({ BirdEasterEgg: () => null }));
const showToast = vi.hoisted(() => vi.fn());
vi.mock('./Toast', () => ({ useToast: () => ({ showToast }) }));
vi.mock('../api/voiceInput', () => ({
  saveVoiceConfig: vi.fn(),
  recordVoice: vi.fn(),
  startVoice: vi.fn(),
  stopVoice: vi.fn(),
  getVoiceState: vi.fn(),
}));

let Sidebar: typeof import('./Sidebar').Sidebar;
let renderer: ReactTestRenderer;

beforeAll(async () => {
  vi.stubGlobal('__APP_EDITION__', 'full');
  ({ Sidebar } = await import('./Sidebar'));
});

beforeEach(() => {
  vi.clearAllMocks();
  voice.setState(voice.getInitialState());
  useSidebarStore.setState({ collapsed: false });
  useNavigationStore.setState(useNavigationStore.getInitialState());
  useNavigationStore.setState({ optionalPagesReady: true });
});

afterEach(() => act(() => renderer?.unmount()));

const baseProps = {
  activePage: 'freeModels' as const,
  onPageChange: vi.fn(),
  smartRouterEnabled: false,
  smartRouterTogglePending: false,
  onSmartRouterChange: vi.fn(),
};
const sourceConfig = {
  ...voiceSnapshot().config,
  channels: [{ kind: 'model' as const, id: 'speech-model' }],
};
const routerGroups = [{ candidateIds: ['one'], groupEnabled: true }];

describe('Sidebar', () => {
  it.each([false, true])(
    'opens settings from the final footer button (collapsed=%s) without changing quick switches',
    (collapsed) => {
      useSidebarStore.setState({ collapsed });
      voice.setState({ snapshot: voiceSnapshot({ config: sourceConfig }) });
      const onSettingsClick = vi.fn();
      act(() => {
        renderer = create(
          <Sidebar
            {...baseProps}
            smartRouterGroups={routerGroups}
            onSettingsClick={onSettingsClick}
          />
        );
      });
      const settings = renderer.root.findByProps({ 'aria-label': 'settings.title' });
      const buttons = renderer.root.findAllByType('button');
      expect(buttons[buttons.length - 1]).toBe(settings);
      const switches = renderer.root.findAllByProps({ role: 'switch' });
      expect(switches).toHaveLength(2);
      act(() => settings.props.onClick());
      expect(onSettingsClick).toHaveBeenCalledOnce();
      expect(baseProps.onPageChange).not.toHaveBeenCalled();
      expect(baseProps.onSmartRouterChange).not.toHaveBeenCalled();
      expect(voiceApi.saveVoiceConfig).not.toHaveBeenCalled();
      expect(voiceApi.getVoiceState).not.toHaveBeenCalled();
      expect(renderer.root.findAllByProps({ role: 'switch' })).toEqual(switches);
    }
  );
  it('keeps the collapsed assistant spinning while working without marking another page active', () => {
    useSidebarStore.setState({ collapsed: true });
    act(() => {
      renderer = create(<Sidebar {...baseProps} smartRouterGroups={[]} agentRunning />);
    });
    const assistant = () => renderer.root.findByProps({ 'aria-label': 'nav.motherAgent' });
    expect(assistant().props['aria-current']).toBeUndefined();
    expect(assistant().findByType(RefreshCw).props).toMatchObject({
      size: 20,
      className: 'animate-spin',
    });
    expect(
      assistant()
        .findAllByType('span')
        .some((node) => node.props.className?.includes('bg-cyber-accent/15'))
    ).toBe(true);
    expect(baseProps.onPageChange).not.toHaveBeenCalled();
    act(() => useSidebarStore.getState().setCollapsed(false));
    expect(assistant().findByType(RefreshCw).props).toMatchObject({ size: 14 });
    expect(assistant().findByType(MessagesSquare)).toBeDefined();
    act(() => useSidebarStore.getState().setCollapsed(true));
    expect(assistant().findByType(RefreshCw).props).toMatchObject({
      size: 20,
      className: 'animate-spin',
    });
    act(() =>
      renderer.update(<Sidebar {...baseProps} smartRouterGroups={[]} agentRunning={false} />)
    );
    expect(assistant().findAllByType(RefreshCw)).toHaveLength(0);
    expect(assistant().findByType(MessagesSquare)).toBeDefined();
  });
  it('shows the developer entry only when enabled and removes the separate projects entry', () => {
    act(() => {
      renderer = create(<Sidebar {...baseProps} smartRouterGroups={[]} />);
    });
    const labels = () =>
      renderer.root.findAllByType('button').map((node) => node.props['aria-label']);
    expect(labels()).not.toContain('nav.developer');
    act(() => useNavigationStore.getState().setDeveloperMode(true));
    expect(JSON.stringify(renderer.toJSON())).toContain('nav.developer');
    act(() => useNavigationStore.getState().setDeveloperMode(false));
    expect(JSON.stringify(renderer.toJSON())).not.toContain('nav.developer');
  });
  it('collapses to accessible icons without changing the page, sources or global switches', () => {
    voice.setState({ snapshot: voiceSnapshot({ config: sourceConfig }) });
    act(() => {
      renderer = create(<Sidebar {...baseProps} smartRouterGroups={routerGroups} />);
    });
    act(() => useSidebarStore.getState().setCollapsed(true));
    const rail = renderer.root.findByType('nav');
    expect(rail.props['data-collapsed']).toBe(true);
    expect(rail.props.className).toContain('w-16');
    const items = renderer.root
      .findAllByType('button')
      .filter((button) => button.props['aria-label'] && button.props.role !== 'switch');
    expect(items).toHaveLength(10);
    expect(items.every((item) => item.findByType('span').props.className === 'sr-only')).toBe(true);
    expect(renderer.root.findAllByProps({ 'aria-current': 'page' })).toHaveLength(1);
    expect(renderer.root.findByProps({ 'aria-current': 'page' }).props['aria-label']).toBe(
      'nav.freeModels'
    );
    expect(
      renderer.root
        .findAllByType('span')
        .filter((span) => span.props.className?.includes('brand-mark'))
    ).toHaveLength(0);
    expect(baseProps.onPageChange).not.toHaveBeenCalled();
    expect(baseProps.onSmartRouterChange).not.toHaveBeenCalled();
    expect(voiceApi.saveVoiceConfig).not.toHaveBeenCalled();
    expect(voiceApi.getVoiceState).not.toHaveBeenCalled();
    expect(voiceApi.recordVoice).not.toHaveBeenCalled();
    act(() => items.find((item) => item.props['aria-label'] === 'voice.input')!.props.onClick());
    expect(baseProps.onPageChange).toHaveBeenCalledExactlyOnceWith('voice');
    act(() => useSidebarStore.getState().setCollapsed(false));
    expect(rail.props.className).toContain('w-48');
    const expandedItems = renderer.root
      .findAllByType('button')
      .filter((button) => button.props['aria-label'] && button.props.role !== 'switch');
    expect(
      expandedItems.every((item) => item.findByType('span').props.className !== 'sr-only')
    ).toBe(true);
    expect(voiceApi.saveVoiceConfig).not.toHaveBeenCalled();
  });

  it('uses the same microphone and routing icons for collapsed switches and keeps their actions separate from navigation', async () => {
    useSidebarStore.setState({ collapsed: true });
    voice.setState({ snapshot: voiceSnapshot({ config: sourceConfig }) });
    vi.mocked(voiceApi.saveVoiceConfig).mockResolvedValue(
      voiceSnapshot({ config: { ...sourceConfig, shortcutEnabled: true }, revision: 2 })
    );
    act(() => {
      renderer = create(<Sidebar {...baseProps} smartRouterGroups={routerGroups} />);
    });
    const microphone = () =>
      renderer.root.findByProps({ role: 'switch', 'aria-label': 'voice.enableInput' });
    const router = () =>
      renderer.root.findByProps({ role: 'switch', 'aria-label': 'nav.smartRouter' });
    expect(microphone().findByType(Mic)).toBeDefined();
    expect(router().findByType(Waypoints)).toBeDefined();
    expect(microphone().props.className).not.toContain('bg-cyber-accent/15');
    await act(async () => microphone().props.onClick());
    expect(voiceApi.saveVoiceConfig).toHaveBeenCalledExactlyOnceWith({
      ...sourceConfig,
      shortcutEnabled: true,
    });
    expect(microphone().props['aria-checked']).toBe(true);
    expect(microphone().props.className).toContain('bg-cyber-accent/15');
    expect(baseProps.onSmartRouterChange).not.toHaveBeenCalled();
    act(() => router().props.onClick());
    expect(baseProps.onSmartRouterChange).toHaveBeenCalledExactlyOnceWith(true);
    expect(baseProps.onPageChange).not.toHaveBeenCalled();
    expect(voiceApi.recordVoice).not.toHaveBeenCalled();
    expect(voiceApi.startVoice).not.toHaveBeenCalled();
  });

  it.each(['saving', 'recording'] as const)(
    'keeps collapsed controls stable while %s and after expanding',
    async (mode) => {
      useSidebarStore.setState({ collapsed: true });
      voice.setState({
        saving: mode === 'saving',
        snapshot: voiceSnapshot({
          config: { ...sourceConfig, shortcutEnabled: true },
          phase: mode === 'recording' ? 'recording' : 'idle',
        }),
      });
      act(() => {
        renderer = create(
          <Sidebar
            {...baseProps}
            smartRouterEnabled
            smartRouterTogglePending
            smartRouterGroups={routerGroups}
          />
        );
      });
      const switches = () => renderer.root.findAllByProps({ role: 'switch' });
      expect(switches()).toHaveLength(2);
      for (const control of switches()) {
        expect(control.props.disabled).toBe(true);
        expect(control.props['aria-checked']).toBe(true);
        expect(control.props.className.includes('disabled:opacity-40')).toBe(
          control.props['aria-label'] === 'voice.enableInput' && mode === 'recording'
        );
        await act(async () => control.props.onClick());
      }
      expect(baseProps.onSmartRouterChange).not.toHaveBeenCalled();
      expect(voiceApi.saveVoiceConfig).not.toHaveBeenCalled();
      act(() => useSidebarStore.getState().setCollapsed(false));
      for (const control of switches()) {
        expect(control.props.disabled).toBe(true);
        expect(control.props['aria-checked']).toBe(true);
      }
    }
  );

  it('keeps source-less global controls hidden in the collapsed rail', () => {
    useSidebarStore.setState({ collapsed: true });
    voice.setState({ snapshot: voiceSnapshot() });
    act(() => {
      renderer = create(<Sidebar {...baseProps} smartRouterGroups={[]} />);
    });
    expect(renderer.root.findAllByProps({ role: 'switch' })).toHaveLength(0);
    expect(
      renderer.root
        .findAllByType('button')
        .filter((button) => button.props['aria-label'] && button.props.role !== 'switch')
    ).toHaveLength(10);
    expect(baseProps.onPageChange).not.toHaveBeenCalled();
  });

  it('places AI news immediately above My AI Career', () => {
    act(() => {
      renderer = create(<Sidebar {...baseProps} smartRouterGroups={[]} />);
    });
    const markup = JSON.stringify(renderer.toJSON());
    expect(markup.indexOf('nav.aiNews')).toBeGreaterThan(-1);
    expect(markup.indexOf('nav.aiNews')).toBeLessThan(markup.indexOf('nav.aiCareer'));
  });

  it('hides the smart router switch when no model is connected', () => {
    const markup = renderToStaticMarkup(<Sidebar {...baseProps} smartRouterGroups={[]} />);

    expect(markup).not.toContain('aria-label="nav.smartRouter"');
    expect(markup).not.toContain('aria-label="voice.enableInput"');
  });

  it('shows the smart router switch when a model is connected', () => {
    voice.setState({ snapshot: voiceSnapshot({ config: sourceConfig }) });
    const markup = renderToStaticMarkup(
      <Sidebar {...baseProps} smartRouterGroups={routerGroups} />
    );

    expect(markup).toContain('aria-label="nav.smartRouter"');
    expect(markup.indexOf('aria-label="voice.enableInput"')).toBeLessThan(
      markup.indexOf('aria-label="nav.smartRouter"')
    );
  });

  it.each([true, false])(
    'keeps the master reachable for seven closed groups while preserving master=%s',
    async (masterEnabled) => {
      const allOff = Array.from({ length: 7 }, (_, index) => ({
        candidateIds: [`model-${index}`],
        groupEnabled: false,
      }));
      const renderSidebar = (groups: typeof allOff) => (
        <Sidebar {...baseProps} smartRouterEnabled={masterEnabled} smartRouterGroups={groups} />
      );
      const toggles = () =>
        renderer.root.findAllByProps({ role: 'switch', 'aria-label': 'nav.smartRouter' });
      act(() => {
        renderer = create(renderSidebar(allOff));
      });
      expect(toggles()).toHaveLength(1);
      expect(toggles()[0].props['aria-checked']).toBe(masterEnabled);
      for (let index = 0; index < 7; index++) {
        const oneOpen = allOff.map((group, position) => ({
          ...group,
          groupEnabled: position === index,
        }));
        act(() => renderer.update(renderSidebar(oneOpen)));
        expect(toggles()).toHaveLength(1);
        expect(toggles()[0].props['aria-checked']).toBe(masterEnabled);
        act(() => renderer.update(renderSidebar(allOff)));
        expect(toggles()).toHaveLength(1);
        expect(toggles()[0].props['aria-checked']).toBe(masterEnabled);
      }
      for (const activePage of ['freeModels', 'voice', 'models'] as const) {
        act(() =>
          renderer.update(
            <Sidebar
              {...baseProps}
              activePage={activePage}
              smartRouterEnabled={masterEnabled}
              smartRouterGroups={allOff}
            />
          )
        );
        expect(toggles()[0].props['aria-checked']).toBe(masterEnabled);
      }
      expect(baseProps.onSmartRouterChange).not.toHaveBeenCalled();
      expect(voiceApi.saveVoiceConfig).not.toHaveBeenCalled();
      expect(voiceApi.getVoiceState).not.toHaveBeenCalled();
      expect(voiceApi.startVoice).not.toHaveBeenCalled();
      expect(voiceApi.recordVoice).not.toHaveBeenCalled();
      await act(async () => toggles()[0].props.onClick());
      expect(baseProps.onSmartRouterChange).toHaveBeenCalledExactlyOnceWith(!masterEnabled);
    }
  );

  it('keeps the master hidden without sources even when a group is enabled', () => {
    const markup = renderToStaticMarkup(
      <Sidebar
        {...baseProps}
        smartRouterEnabled
        smartRouterGroups={[{ candidateIds: [], groupEnabled: true }]}
      />
    );
    expect(markup).not.toContain('aria-label="nav.smartRouter"');
    expect(baseProps.onSmartRouterChange).not.toHaveBeenCalled();
  });

  it('waits for saved state, saves explicit toggles globally and blocks repeated clicks while pending', async () => {
    act(() => {
      renderer = create(<Sidebar {...baseProps} smartRouterGroups={[]} />);
    });
    const toggle = () =>
      renderer.root.findByProps({ role: 'switch', 'aria-label': 'voice.enableInput' });
    expect(renderer.root.findAllByProps({ 'aria-label': 'voice.enableInput' })).toHaveLength(0);
    expect(voiceApi.saveVoiceConfig).not.toHaveBeenCalled();
    act(() => voice.setState({ snapshot: voiceSnapshot({ config: sourceConfig }) }));
    expect(toggle().props.disabled).toBe(false);
    let finish!: (snapshot: ReturnType<typeof voiceSnapshot>) => void;
    vi.mocked(voiceApi.saveVoiceConfig).mockReturnValueOnce(
      new Promise((resolve) => {
        finish = resolve;
      })
    );
    act(() => {
      void toggle().props.onClick();
    });
    const enabled = { ...sourceConfig, shortcutEnabled: true };
    expect(voiceApi.saveVoiceConfig).toHaveBeenCalledExactlyOnceWith(enabled);
    expect(toggle().props['aria-busy']).toBe(true);
    expect(toggle().props.disabled).toBe(true);
    act(() => {
      void toggle().props.onClick();
    });
    expect(voiceApi.saveVoiceConfig).toHaveBeenCalledTimes(1);
    await act(async () => finish(voiceSnapshot({ config: enabled, revision: 2 })));
    expect(toggle().props['aria-checked']).toBe(true);
    vi.mocked(voiceApi.saveVoiceConfig).mockResolvedValueOnce(
      voiceSnapshot({ config: sourceConfig, revision: 3 })
    );
    await act(async () => toggle().props.onClick());
    expect(voiceApi.saveVoiceConfig).toHaveBeenLastCalledWith(sourceConfig);
    expect(toggle().props['aria-checked']).toBe(false);
    expect(voiceApi.recordVoice).not.toHaveBeenCalled();
    expect(voiceApi.startVoice).not.toHaveBeenCalled();
    expect(voiceApi.stopVoice).not.toHaveBeenCalled();
    expect(showToast).not.toHaveBeenCalled();
  });

  it('preserves the checked state when saving fails and stays available across pages', async () => {
    const config = { ...sourceConfig, shortcutEnabled: true };
    voice.setState({ snapshot: voiceSnapshot({ config }) });
    vi.mocked(voiceApi.saveVoiceConfig).mockRejectedValueOnce('voice.error.shortcut');
    act(() => {
      renderer = create(<Sidebar {...baseProps} smartRouterGroups={routerGroups} />);
    });
    const toggle = () =>
      renderer.root.findByProps({ role: 'switch', 'aria-label': 'voice.enableInput' });
    await act(async () => toggle().props.onClick());
    expect(toggle().props['aria-checked']).toBe(true);
    expect(toggle().props.disabled).toBe(false);
    expect(voice.getState().error).toBe('voice.error.shortcut');
    expect(showToast).toHaveBeenCalledExactlyOnceWith('error', 'voice.error.shortcut');
    for (const activePage of ['voice', 'models', 'apps'] as const) {
      act(() =>
        renderer.update(
          <Sidebar {...baseProps} activePage={activePage} smartRouterGroups={routerGroups} />
        )
      );
      expect(toggle().props['aria-checked']).toBe(true);
    }
    expect(voiceApi.saveVoiceConfig).toHaveBeenCalledTimes(1);
    expect(voiceApi.getVoiceState).not.toHaveBeenCalled();
    expect(voiceApi.recordVoice).not.toHaveBeenCalled();
    expect(voiceApi.startVoice).not.toHaveBeenCalled();
    expect(baseProps.onSmartRouterChange).not.toHaveBeenCalled();
  });

  it.each(['starting', 'testing', 'recording', 'transcribing', 'inserting'] as const)(
    'blocks the global toggle while %s without writing or interrupting audio',
    async (phase) => {
      voice.setState({ snapshot: voiceSnapshot({ phase, config: sourceConfig }) });
      act(() => {
        renderer = create(<Sidebar {...baseProps} smartRouterGroups={routerGroups} />);
      });
      const toggle = renderer.root.findByProps({
        role: 'switch',
        'aria-label': 'voice.enableInput',
      });
      expect(toggle.props.disabled).toBe(true);
      await act(async () => toggle.props.onClick());
      expect(voiceApi.saveVoiceConfig).not.toHaveBeenCalled();
      expect(voiceApi.recordVoice).not.toHaveBeenCalled();
      expect(voiceApi.startVoice).not.toHaveBeenCalled();
      expect(voiceApi.stopVoice).not.toHaveBeenCalled();
      expect(showToast).not.toHaveBeenCalled();
    }
  );

  it('hides source-less voice input and restores its switch when a source is added without enabling it', () => {
    voice.setState({ snapshot: voiceSnapshot() });
    act(() => {
      renderer = create(<Sidebar {...baseProps} smartRouterGroups={routerGroups} />);
    });
    expect(renderer.root.findAllByProps({ 'aria-label': 'voice.enableInput' })).toHaveLength(0);
    expect(renderer.root.findByProps({ 'aria-label': 'nav.smartRouter' })).toBeDefined();
    act(() => voice.getState().accept(voiceSnapshot({ config: sourceConfig, revision: 2 })));
    expect(
      renderer.root.findByProps({ role: 'switch', 'aria-label': 'voice.enableInput' }).props[
        'aria-checked'
      ]
    ).toBe(false);
    act(() => voice.getState().accept(voiceSnapshot({ revision: 3 })));
    expect(renderer.root.findAllByProps({ 'aria-label': 'voice.enableInput' })).toHaveLength(0);
    expect(voiceApi.saveVoiceConfig).not.toHaveBeenCalled();
    expect(voiceApi.getVoiceState).not.toHaveBeenCalled();
    expect(voiceApi.recordVoice).not.toHaveBeenCalled();
    expect(voiceApi.startVoice).not.toHaveBeenCalled();
    expect(showToast).not.toHaveBeenCalled();
  });
});

it('hides optional pages before settings load and independently removes only disabled entries', () => {
  useNavigationStore.setState({ optionalPagesReady: false });
  act(() => {
    renderer = create(<Sidebar {...baseProps} smartRouterGroups={[]} />);
  });
  const render = () => JSON.stringify(renderer.toJSON());
  for (const label of ['nav.aiNews', 'nav.aiCareer', 'nav.localServer'])
    expect(render()).not.toContain(label);
  act(() =>
    useNavigationStore.getState().setOptionalPages({ aiNews: true, aiCareer: true, localLlm: true })
  );
  for (const label of ['nav.aiNews', 'nav.aiCareer', 'nav.localServer'])
    expect(render()).toContain(label);
  act(() => useNavigationStore.getState().setOptionalPages({ aiNews: false, localLlm: false }));
  expect(render()).not.toContain('nav.aiNews');
  expect(render()).not.toContain('nav.localServer');
  expect(render()).toContain('nav.aiCareer');
  expect(render()).toContain('nav.modelNexus');
  expect(baseProps.onPageChange).not.toHaveBeenCalled();
});
