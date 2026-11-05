import React from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { SettingsDialog } from './SettingsDialog';
import { useUiScaleStore } from '../stores/uiScaleStore';
import { useEasterEggStore } from '../stores/easterEggStore';
import { useBackgroundStore } from '../stores/backgroundStore';
import { useRewardClaimStore } from '../stores/rewardClaimStore';
import { useProtectionStore } from '../stores/protectionStore';
import { useNavigationStore } from '../stores/navigationStore';

const mocks = vi.hoisted(() => ({ zoom: vi.fn(), save: vi.fn(), get: vi.fn() }));
vi.mock('@tauri-apps/api/app', () => ({ getVersion: async () => '' }));
vi.mock('@tauri-apps/api/webview', () => ({ getCurrentWebview: () => ({ setZoom: mocks.zoom }) }));
vi.mock('@tauri-apps/api/window', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@tauri-apps/api/window')>()),
  currentMonitor: async () => null,
  getCurrentWindow: () => ({}),
}));
vi.mock('@tauri-apps/plugin-autostart', () => ({ isEnabled: async () => false }));
vi.mock('../api/tauri', () => ({ getSettings: mocks.get, saveSettings: mocks.save }));
vi.mock('../hooks/useI18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }));
vi.mock('../stores/themeStore', () => ({
  useThemeStore: (select: (state: unknown) => unknown) =>
    select({ mode: 'dark', colorTheme: 'oatgray' }),
}));

let view: ReactTestRenderer;
const props = { isOpen: true, onClose: vi.fn(), locale: 'en', onLocaleChange: vi.fn() };
beforeEach(() => {
  vi.clearAllMocks();
  useProtectionStore.setState({ enabled: null, revision: 0, pending: false, saveError: false });
  mocks.zoom.mockResolvedValue(undefined);
  mocks.get.mockResolvedValue({});
  mocks.save.mockResolvedValue(undefined);
  vi.stubGlobal('window', { addEventListener: vi.fn(), removeEventListener: vi.fn() });
  vi.stubGlobal('document', {
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    documentElement: { style: { setProperty: vi.fn() } },
  });
  vi.stubGlobal('localStorage', { getItem: vi.fn(), setItem: vi.fn() });
  useEasterEggStore.setState({ enabled: true });
  useRewardClaimStore.setState({ enabled: false });
  useBackgroundStore.setState({ mode: 'characters' });
  useNavigationStore.setState(useNavigationStore.getInitialState());
  useUiScaleStore.setState({
    preference: 'auto',
    requestedPreference: null,
    scale: 1,
    pending: false,
    failed: false,
  });
});
afterEach(async () => {
  await act(async () => view?.unmount());
  vi.unstubAllGlobals();
});

async function tab(label: string) {
  await act(async () => {
    view.root
      .findAllByType('button')
      .find((button) => button.props.children.includes(label))!
      .props.onClick();
  });
}

function developerSwitch() {
  return view.root
    .findAllByProps({ role: 'switch' })
    .find((node) => node.props['aria-label'] === 'settings.developerMode')!;
}

function protectionSwitch() {
  return view.root
    .findAllByProps({ role: 'switch' })
    .find((node) => node.props['aria-label'] === 'settings.sensitiveInfoProtection')!;
}

it('keeps global protection available with developer preview off and merges only its own explicit change', async () => {
  const saved = { developerMode: false, sensitiveInfoProtection: true, locale: 'ja' };
  mocks.get.mockResolvedValue(saved);
  await act(async () => {
    view = create(<SettingsDialog {...props} />);
  });
  expect(protectionSwitch().props['aria-checked']).toBe(true);
  expect(developerSwitch().props['aria-checked']).toBe(false);
  expect(mocks.save).not.toHaveBeenCalled();
  await act(async () => protectionSwitch().props.onClick());
  expect(mocks.save).toHaveBeenCalledExactlyOnceWith({ ...saved, sensitiveInfoProtection: false });
  expect(useNavigationStore.getState().developerMode).toBe(false);
});

it('preserves pending protection across reopening and prevents concurrent master-switch writes', async () => {
  mocks.get.mockResolvedValue({ sensitiveInfoProtection: true });
  await act(async () => {
    view = create(<SettingsDialog {...props} />);
  });
  let finish!: () => void;
  mocks.save.mockImplementationOnce(
    () =>
      new Promise<void>((resolve) => {
        finish = resolve;
      })
  );
  await act(async () => protectionSwitch().props.onClick());
  expect(protectionSwitch().props.disabled).toBe(true);
  expect(developerSwitch().props.disabled).toBe(true);
  await act(async () => view.update(<SettingsDialog {...props} isOpen={false} />));
  await act(async () => view.update(<SettingsDialog {...props} />));
  await act(async () => developerSwitch().props.onClick());
  expect(mocks.save).toHaveBeenCalledOnce();
  await act(async () => finish());
  expect(protectionSwitch().props['aria-checked']).toBe(false);
  expect(protectionSwitch().props.disabled).toBe(false);
});

it.each([false, true])(
  'ignores a stale protection read after a save across reopening (initial=%s)',
  async (initial) => {
    mocks.get.mockResolvedValue({ sensitiveInfoProtection: initial });
    await act(async () => {
      view = create(<SettingsDialog {...props} />);
    });
    let finishSave!: () => void;
    mocks.save.mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          finishSave = resolve;
        })
    );
    await act(async () => protectionSwitch().props.onClick());
    await act(async () => view.update(<SettingsDialog {...props} isOpen={false} />));
    let finishRead!: (value: object) => void;
    mocks.get.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finishRead = resolve;
        })
    );
    await act(async () => view.update(<SettingsDialog {...props} />));
    await act(async () => finishSave());
    await act(async () => finishRead({ sensitiveInfoProtection: initial }));
    expect(protectionSwitch().props['aria-checked']).toBe(!initial);
  }
);

it('preserves protection and shows its existing error when saving fails', async () => {
  mocks.get.mockResolvedValue({ sensitiveInfoProtection: true });
  await act(async () => {
    view = create(<SettingsDialog {...props} />);
  });
  mocks.save.mockRejectedValueOnce(new Error('disk full'));
  await act(async () => protectionSwitch().props.onClick());
  expect(protectionSwitch().props['aria-checked']).toBe(true);
  expect(JSON.stringify(view.toJSON())).toContain('settings.sensitiveInfoProtectionError');
});

it('defaults developer preview on without writing settings', async () => {
  await act(async () => {
    view = create(<SettingsDialog {...props} />);
  });
  expect(developerSwitch().props['aria-checked']).toBe(true);
  expect(useNavigationStore.getState().developerMode).toBe(true);
  expect(mocks.save).not.toHaveBeenCalled();
});

it('persists developer mode only after an explicit change and keeps the control pending until saved', async () => {
  mocks.get.mockResolvedValue({
    developerMode: false,
    locale: 'ja',
    sensitiveInfoProtection: true,
  });
  await act(async () => {
    view = create(<SettingsDialog {...props} />);
  });
  expect(developerSwitch().props['aria-checked']).toBe(false);
  expect(mocks.save).not.toHaveBeenCalled();
  let finish!: () => void;
  mocks.save.mockImplementationOnce(
    () =>
      new Promise<void>((resolve) => {
        finish = resolve;
      })
  );
  await act(async () => {
    developerSwitch().props.onClick();
  });
  expect(developerSwitch().props.disabled).toBe(true);
  expect(useNavigationStore.getState().developerMode).toBe(false);
  expect(mocks.save).toHaveBeenCalledExactlyOnceWith({
    locale: 'ja',
    sensitiveInfoProtection: true,
    developerMode: true,
  });
  await act(async () => {
    finish();
  });
  expect(developerSwitch().props['aria-checked']).toBe(true);
  expect(useNavigationStore.getState().developerMode).toBe(true);
  mocks.get.mockResolvedValue({ developerMode: true });
  await act(async () => {
    view.update(<SettingsDialog {...props} isOpen={false} />);
  });
  await act(async () => {
    view.update(<SettingsDialog {...props} />);
  });
  expect(developerSwitch().props['aria-checked']).toBe(true);
  expect(mocks.save).toHaveBeenCalledOnce();
});

it('preserves the developer page and saved selection when disabling fails', async () => {
  mocks.get.mockResolvedValue({ developerMode: true });
  await act(async () => {
    view = create(<SettingsDialog {...props} />);
  });
  useNavigationStore.getState().setActivePage('developer');
  useNavigationStore.getState().setDeveloperTab('plugins');
  mocks.save.mockRejectedValueOnce(new Error('disk full'));
  await act(async () => {
    developerSwitch().props.onClick();
  });
  expect(developerSwitch().props['aria-checked']).toBe(true);
  expect(useNavigationStore.getState().activePage).toBe('developer');
  expect(useNavigationStore.getState().developerTab).toBe('plugins');
  expect(JSON.stringify(view.toJSON())).toContain('settings.developerModeError');
});

it.each([false, true])(
  'does not let a stale read undo a saved developer mode after reopening (initial=%s)',
  async (initial) => {
    const target = !initial;
    mocks.get.mockResolvedValue({ developerMode: initial });
    await act(async () => {
      view = create(<SettingsDialog {...props} />);
    });
    let finishSave!: () => void;
    mocks.save.mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          finishSave = resolve;
        })
    );
    await act(async () => {
      developerSwitch().props.onClick();
    });
    await act(async () => {
      view.update(<SettingsDialog {...props} isOpen={false} />);
    });
    let finishRead!: (settings: { developerMode: boolean }) => void;
    mocks.get.mockImplementationOnce(
      () =>
        new Promise<{ developerMode: boolean }>((resolve) => {
          finishRead = resolve;
        })
    );
    await act(async () => {
      view.update(<SettingsDialog {...props} />);
    });
    await act(async () => {
      finishSave();
    });
    expect(useNavigationStore.getState().developerMode).toBe(target);
    if (target) useNavigationStore.getState().setActivePage('developer');
    await act(async () => {
      finishRead({ developerMode: initial });
    });
    expect(useNavigationStore.getState().developerMode).toBe(target);
    expect(useNavigationStore.getState().activePage).toBe(target ? 'developer' : 'apps');
    expect(developerSwitch().props['aria-checked']).toBe(target);
  }
);

it('places scale in General and offers languages directly on their own page', async () => {
  await act(async () => {
    view = create(<SettingsDialog {...props} />);
  });
  expect(view.root.findByProps({ id: 'ui-scale' })).toBeDefined();
  await tab('settings.appearance');
  expect(view.root.findAllByProps({ id: 'ui-scale' })).toHaveLength(0);
  await tab('settings.language');
  const languages = view.root.findAllByType('button').filter((button) => button.props.lang);
  expect(languages.map((button) => button.props.lang)).toEqual(['en', 'zh-Hans', 'zh-Hant', 'ja']);
  expect(
    languages.filter((button) => button.props['aria-pressed']).map((button) => button.props.lang)
  ).toEqual(['en']);
  expect(props.onLocaleChange).not.toHaveBeenCalled();
  await act(async () => {
    languages[3].props.onClick();
  });
  expect(props.onLocaleChange).toHaveBeenCalledExactlyOnceWith('ja');
  await act(async () => {
    view.update(<SettingsDialog {...props} locale="ja" />);
  });
  expect(view.root.findByProps({ lang: 'ja' }).props['aria-pressed']).toBe(true);
  expect(mocks.zoom).not.toHaveBeenCalled();
  expect(mocks.save).not.toHaveBeenCalled();
});

it('keeps the background and Easter Egg controls independent', async () => {
  await act(async () => {
    view = create(<SettingsDialog {...props} />);
  });
  await act(async () => {
    easterEggSwitch().props.onClick();
  });
  expect(useEasterEggStore.getState().enabled).toBe(false);
  expect(useBackgroundStore.getState().mode).toBe('characters');
  await tab('settings.appearance');
  const none = view.root
    .findAllByType('button')
    .find((button) => button.props.children === 'settings.backgroundNone')!;
  await act(async () => {
    none.props.onClick();
  });
  expect(useBackgroundStore.getState().mode).toBe('none');
  expect(localStorage.setItem).toHaveBeenLastCalledWith('echobird_background', 'none');
  await tab('settings.general');
  expect(easterEggSwitch().props['aria-checked']).toBe(false);
  await act(async () => {
    easterEggSwitch().props.onClick();
  });
  expect(useBackgroundStore.getState().mode).toBe('none');
  expect(mocks.save).not.toHaveBeenCalled();
});

it('preserves the background through passive navigation and reopening without writes', async () => {
  useBackgroundStore.setState({ mode: 'monogram' });
  await act(async () => {
    view = create(<SettingsDialog {...props} />);
  });
  for (const label of ['settings.appearance', 'settings.language', 'settings.general'])
    await tab(label);
  await act(async () => {
    view.update(<SettingsDialog {...props} isOpen={false} />);
  });
  await act(async () => {
    view.update(<SettingsDialog {...props} />);
  });
  expect(useBackgroundStore.getState().mode).toBe('monogram');
  expect(props.onLocaleChange).not.toHaveBeenCalled();
  expect(localStorage.setItem).not.toHaveBeenCalled();
  expect(mocks.save).not.toHaveBeenCalled();
  expect(mocks.zoom).not.toHaveBeenCalled();
});

function easterEggSwitch() {
  return view.root
    .findByProps({ children: 'settings.easterEgg' })
    .parent!.parent!.findByProps({ role: 'switch' });
}

it('groups general preferences before optional features and preserves rewards through reopening', async () => {
  await act(async () => {
    view = create(<SettingsDialog {...props} />);
  });
  const control = () =>
    view.root
      .findByProps({ children: 'settings.autoClaimRewards' })
      .parent!.parent!.findByProps({ role: 'switch' });
  expect(control().props['aria-checked']).toBe(false);
  const toggles = control().parent!.parent!.parent!;
  expect(
    toggles
      .findAllByType('span')
      .map((span) => span.props.children)
      .filter((value) => typeof value === 'string' && value !== '?')
  ).toEqual([
    'settings.launchAtStartup',
    'settings.autoClaimRewards',
    'settings.sensitiveInfoProtection',
    'nav.aiNews',
    'nav.aiCareer',
    'settings.developerMode',
    'nav.localServer',
    'settings.easterEgg',
  ]);
  expect(localStorage.setItem).not.toHaveBeenCalled();
  await act(async () => control().props.onClick());
  expect(useRewardClaimStore.getState().enabled).toBe(true);
  expect(localStorage.setItem).toHaveBeenLastCalledWith('echobird_auto_claim_rewards', 'true');
  await act(async () => view.update(<SettingsDialog {...props} isOpen={false} />));
  await act(async () => view.update(<SettingsDialog {...props} />));
  expect(control().props['aria-checked']).toBe(true);
  await act(async () => control().props.onClick());
  expect(useRewardClaimStore.getState().enabled).toBe(false);
  expect(mocks.save).not.toHaveBeenCalled();
});

function EasterEggObserver() {
  const enabled = useEasterEggStore((state) => state.enabled);
  return <span data-easter-egg-state>{String(enabled)}</span>;
}

it('keeps global protection independent in the three-column General preferences', async () => {
  await act(async () => {
    view = create(<SettingsDialog {...props} />);
  });
  expect(view.root.findAllByProps({ role: 'switch' })).toHaveLength(8);
  expect(
    view.root.findAllByProps({ 'aria-label': 'settings.sensitiveInfoProtection' })
  ).toHaveLength(1);
  expect(
    view.root.findAllByProps({
      className: 'grid grid-cols-1 min-[700px]:grid-cols-2 min-[880px]:grid-cols-3 gap-x-4 gap-y-6',
    })
  ).toHaveLength(1);
  expect(mocks.save).not.toHaveBeenCalled();
});

it('updates the shared Easter Egg setting and other consumers immediately from the existing switch', async () => {
  await act(async () => {
    view = create(
      <>
        <SettingsDialog {...props} />
        <EasterEggObserver />
      </>
    );
  });
  expect(easterEggSwitch().props['aria-checked']).toBe(true);
  await act(async () => {
    easterEggSwitch().props.onClick();
  });
  expect(easterEggSwitch().props['aria-checked']).toBe(false);
  expect(view.root.findByProps({ 'data-easter-egg-state': true }).props.children).toBe('false');
  expect(localStorage.setItem).toHaveBeenCalledExactlyOnceWith('echobird_easter_egg', 'false');
  expect(mocks.save).not.toHaveBeenCalled();
});

it('reflects a shared Easter Egg change while settings is already open', async () => {
  await act(async () => {
    view = create(<SettingsDialog {...props} />);
  });
  await act(async () => {
    useEasterEggStore.getState().setEnabled(false);
  });
  expect(easterEggSwitch().props['aria-checked']).toBe(false);
});

it.each([true, false])(
  'preserves the shared %s Easter Egg choice through tabs and reopening without storage writes',
  async (enabled) => {
    useEasterEggStore.setState({ enabled });
    await act(async () => {
      view = create(<SettingsDialog {...props} />);
    });
    expect(easterEggSwitch().props['aria-checked']).toBe(enabled);
    await tab('settings.appearance');
    await tab('settings.general');
    await act(async () => {
      view.update(<SettingsDialog {...props} isOpen={false} />);
    });
    await act(async () => {
      view.update(<SettingsDialog {...props} />);
    });
    expect(easterEggSwitch().props['aria-checked']).toBe(enabled);
    expect(localStorage.getItem).not.toHaveBeenCalled();
    expect(localStorage.setItem).not.toHaveBeenCalled();
    expect(mocks.save).not.toHaveBeenCalled();
  }
);

it('keeps Easter Egg disabled in memory and on reopening when storage is unavailable', async () => {
  vi.mocked(localStorage.getItem).mockImplementation(() => {
    throw new Error('storage unavailable');
  });
  vi.mocked(localStorage.setItem).mockImplementation(() => {
    throw new Error('storage unavailable');
  });
  await act(async () => {
    view = create(<SettingsDialog {...props} />);
  });
  await act(async () => {
    easterEggSwitch().props.onClick();
  });
  expect(useEasterEggStore.getState().enabled).toBe(false);
  expect(easterEggSwitch().props['aria-checked']).toBe(false);
  await act(async () => {
    view.update(<SettingsDialog {...props} isOpen={false} />);
  });
  await act(async () => {
    view.update(<SettingsDialog {...props} />);
  });
  expect(easterEggSwitch().props['aria-checked']).toBe(false);
  expect(localStorage.getItem).not.toHaveBeenCalled();
  expect(localStorage.setItem).toHaveBeenCalledExactlyOnceWith('echobird_easter_egg', 'false');
  expect(mocks.save).not.toHaveBeenCalled();
});

it('only changes scale on explicit selection, not settings entry, tab changes or reopening', async () => {
  await act(async () => {
    view = create(<SettingsDialog {...props} />);
  });
  await tab('settings.appearance');
  await tab('settings.language');
  await tab('settings.general');
  await act(async () => {
    view.update(<SettingsDialog {...props} isOpen={false} />);
  });
  await act(async () => {
    view.update(<SettingsDialog {...props} />);
  });
  expect(mocks.zoom).not.toHaveBeenCalled();
  expect(mocks.save).not.toHaveBeenCalled();
  expect(localStorage.setItem).not.toHaveBeenCalled();

  const slider = view.root.findByProps({ id: 'ui-scale' });
  expect(slider.props).toMatchObject({ type: 'range', min: 70, max: 150, step: 'any', value: 100 });
  await act(async () => {
    slider.props.onChange({ target: { value: '110' } });
  });
  expect(mocks.zoom).toHaveBeenCalledExactlyOnceWith(1.1);
  expect(localStorage.setItem).toHaveBeenCalledExactlyOnceWith('echobird-ui-scale', '110');
});

it('keeps the slider responsive while applying and shows failure without a blocking dialog', async () => {
  await act(async () => {
    view = create(<SettingsDialog {...props} />);
  });
  await tab('settings.general');
  await act(async () => {
    useUiScaleStore.setState({ pending: true, requestedPreference: 70 });
  });
  expect(view.root.findByProps({ id: 'ui-scale' }).props.disabled).toBeUndefined();
  expect(view.root.findByProps({ id: 'ui-scale' }).props.value).toBe(70);
  await act(async () => {
    useUiScaleStore.setState({ pending: false, failed: true, requestedPreference: null });
  });
  expect(view.root.findByProps({ role: 'alert' }).props.children).toBe('settings.uiScaleError');
  expect(view.root.findByProps({ id: 'ui-scale' }).props.disabled).toBeUndefined();
});

it('keeps the original drag coordinates as the window and UI resize and can return to automatic', async () => {
  await act(async () => {
    view = create(<SettingsDialog {...props} />);
  });
  await tab('settings.general');
  const target = {
    focus: vi.fn(),
    setPointerCapture: vi.fn(),
    getBoundingClientRect: () => ({ left: 100, width: 414 }),
  };
  await act(async () => {
    view.root.findByProps({ id: 'ui-scale' }).props.onPointerDown({
      button: 0,
      preventDefault: vi.fn(),
      currentTarget: target,
      pointerId: 1,
      clientX: 257,
      screenX: 1000,
    });
    // The new client coordinate deliberately disagrees after zoom/recentering.
    view.root.findByProps({ id: 'ui-scale' }).props.onPointerMove({ screenX: 1250, clientX: 10 });
    view.root.findByProps({ id: 'ui-scale' }).props.onPointerUp();
  });
  expect(target.setPointerCapture).toHaveBeenCalledWith(1);
  expect(useUiScaleStore.getState()).toMatchObject({ preference: 150, scale: 1.5 });
  expect(localStorage.setItem).toHaveBeenLastCalledWith('echobird-ui-scale', '150');
  await act(async () => {
    view.root.findByProps({ id: 'ui-scale' }).props.onPointerMove({ screenX: 0 });
  });
  expect(useUiScaleStore.getState().preference).toBe(150);
  await act(async () => {
    view.root
      .findAllByType('button')
      .find((button) => button.props.children === 'settings.uiScaleAuto')!
      .props.onClick();
  });
  expect(useUiScaleStore.getState().preference).toBe('auto');
  expect(localStorage.setItem).toHaveBeenLastCalledWith('echobird-ui-scale', 'auto');
});

async function startScaleDrag(percent = 100) {
  await act(async () => {
    view = create(<SettingsDialog {...props} />);
  });
  await tab('settings.general');
  await act(async () => {
    view.root.findByProps({ id: 'ui-scale' }).props.onPointerDown({
      button: 0,
      preventDefault: vi.fn(),
      pointerId: 1,
      clientX: 107 + (percent - 70) * 5,
      screenX: 1000,
      currentTarget: {
        focus: vi.fn(),
        setPointerCapture: vi.fn(),
        getBoundingClientRect: () => ({ left: 100, width: 414 }),
      },
    });
  });
}

it.each(['close', 'tab'])(
  'clears an interrupted scale drag on %s before keyboard input',
  async (exit) => {
    await startScaleDrag();
    await act(async () => {
      view.root.findByProps({ id: 'ui-scale' }).props.onPointerMove({ screenX: 1035 });
    });
    vi.mocked(localStorage.setItem).mockClear();
    if (exit === 'close') {
      await act(async () => {
        view.update(<SettingsDialog {...props} isOpen={false} />);
      });
      await act(async () => {
        view.update(<SettingsDialog {...props} />);
      });
    } else {
      await tab('settings.appearance');
      await tab('settings.general');
    }
    expect(view.root.findByProps({ id: 'ui-scale' }).props.value).toBe(100);
    expect(localStorage.setItem).not.toHaveBeenCalled();
    await act(async () => {
      view.root.findByProps({ id: 'ui-scale' }).props.onChange({ target: { value: '110' } });
    });
    expect(useUiScaleStore.getState().preference).toBe(110);
    expect(localStorage.setItem).toHaveBeenLastCalledWith('echobird-ui-scale', '110');
  }
);

it('shows nine non-interactive ticks at every ten percent', async () => {
  await startScaleDrag();
  const ticks = view.root.findByProps({ 'data-scale-ticks': true });
  expect(ticks.props['aria-hidden']).toBe(true);
  expect(ticks.findAllByType('span').map((tick) => tick.props['data-scale-tick'])).toEqual([
    70, 80, 90, 100, 110, 120, 130, 140, 150,
  ]);
});

it.each([
  [72, 70],
  [78, 80],
  [98, 100],
  [102, 100],
  [108, 110],
  [148, 150],
  [74, 70],
  [75, 80],
  [77, 80],
  [125, 130],
])(
  'attracts a pointer press at %s percent to the nearby %s percent tick',
  async (pointer, expected) => {
    await startScaleDrag(pointer);
    expect(useUiScaleStore.getState().preference).toBe(expected);
  }
);

it.each([
  [97, 100],
  [103, 100],
  [106, 100],
  [114, 110],
  [86, 90],
  [160, 150],
  [60, 70],
])(
  'moves the thumb to %s but applies only the last crossed %s percent tick',
  async (pointer, expected) => {
    await startScaleDrag();
    await act(async () => {
      view.root
        .findByProps({ id: 'ui-scale' })
        .props.onPointerMove({ screenX: 1000 + (pointer - 100) * 5 });
    });
    expect(useUiScaleStore.getState().preference).toBe(expected);
    expect(view.root.findByProps({ id: 'ui-scale' }).props.value).toBe(
      Math.max(70, Math.min(150, pointer))
    );
    expect(view.root.findByProps({ id: 'ui-scale' }).props['aria-valuetext']).toBe(`${expected}%`);
    await act(async () => {
      view.root.findByProps({ id: 'ui-scale' }).props.onPointerUp();
    });
    expect(useUiScaleStore.getState().preference).toBe(expected);
    expect(view.root.findByProps({ id: 'ui-scale' }).props.value).toBe(expected);
    expect(localStorage.setItem).toHaveBeenLastCalledWith('echobird-ui-scale', String(expected));
  }
);

it('waits for the next tick in either direction while the thumb moves freely', async () => {
  await startScaleDrag();
  for (const [pointer, expected] of [
    [101, 100],
    [109.9, 100],
    [110, 110],
    [107, 110],
    [100.1, 110],
    [100, 100],
    [99, 100],
    [90, 90],
  ]) {
    mocks.zoom.mockClear();
    vi.mocked(localStorage.setItem).mockClear();
    const previous = useUiScaleStore.getState().preference;
    await act(async () => {
      view.root
        .findByProps({ id: 'ui-scale' })
        .props.onPointerMove({ screenX: 1000 + (pointer - 100) * 5 });
    });
    expect(useUiScaleStore.getState().preference).toBe(expected);
    expect(view.root.findByProps({ id: 'ui-scale' }).props.value).toBeCloseTo(pointer);
    if (expected === previous) {
      expect(mocks.zoom).not.toHaveBeenCalled();
      expect(localStorage.setItem).not.toHaveBeenCalled();
    } else {
      expect(mocks.zoom).toHaveBeenCalledExactlyOnceWith(expected / 100);
      expect(localStorage.setItem).toHaveBeenCalledExactlyOnceWith(
        'echobird-ui-scale',
        String(expected)
      );
    }
  }
});

it.each(['onPointerUp', 'onPointerCancel', 'onLostPointerCapture'])(
  'returns the thumb to the reached tick on %s without another scale change',
  async (finish) => {
    await startScaleDrag();
    await act(async () => {
      view.root.findByProps({ id: 'ui-scale' }).props.onPointerMove({ screenX: 1085 });
    });
    expect(view.root.findByProps({ id: 'ui-scale' }).props.value).toBe(117);
    expect(useUiScaleStore.getState().preference).toBe(110);
    mocks.zoom.mockClear();
    vi.mocked(localStorage.setItem).mockClear();
    await act(async () => {
      view.root.findByProps({ id: 'ui-scale' }).props[finish]();
      view.root.findByProps({ id: 'ui-scale' }).props.onPointerMove({ screenX: 1250 });
    });
    expect(view.root.findByProps({ id: 'ui-scale' }).props.value).toBe(110);
    expect(mocks.zoom).not.toHaveBeenCalled();
    expect(localStorage.setItem).not.toHaveBeenCalled();
  }
);

it('keeps keyboard changes at ten percent with a continuous pointer range', async () => {
  await startScaleDrag();
  await act(async () => {
    view.root.findByProps({ id: 'ui-scale' }).props.onPointerUp();
  });
  for (const [key, expected] of [
    ['ArrowRight', 110],
    ['ArrowUp', 120],
    ['ArrowLeft', 110],
    ['ArrowDown', 100],
    ['End', 150],
    ['ArrowRight', 150],
    ['Home', 70],
    ['ArrowLeft', 70],
  ] as const) {
    const preventDefault = vi.fn();
    await act(async () => {
      view.root.findByProps({ id: 'ui-scale' }).props.onKeyDown({ key, preventDefault });
    });
    expect(preventDefault).toHaveBeenCalledOnce();
    expect(useUiScaleStore.getState().preference).toBe(expected);
    expect(view.root.findByProps({ id: 'ui-scale' }).props.value).toBe(expected);
  }
});

it('reflects the terminal protection control and locks settings controls while its save is pending', async () => {
  await act(async () => {
    view = create(<SettingsDialog {...props} />);
  });
  await act(async () => useProtectionStore.getState().publish(true));
  expect(protectionSwitch().props['aria-checked']).toBe(true);
  await act(async () => useProtectionStore.setState({ pending: true }));
  expect(protectionSwitch().props.disabled).toBe(true);
  expect(developerSwitch().props.disabled).toBe(true);
  expect(mocks.save).not.toHaveBeenCalled();
  await act(async () => useProtectionStore.setState({ pending: false }));
});

it.each([false, true])(
  'keeps the shared settings lock after closing during a developer save and releases it on failure=%s',
  async (fails) => {
    let finish!: () => void;
    mocks.save.mockImplementationOnce(
      () =>
        new Promise<void>((resolve, reject) => {
          finish = () => (fails ? reject(new Error('fixture')) : resolve());
        })
    );
    await act(async () => {
      view = create(<SettingsDialog {...props} />);
    });
    expect(useNavigationStore.getState().developerMode).toBe(true);
    await act(async () => developerSwitch().props.onClick());
    expect(useProtectionStore.getState().pending).toBe(true);
    await act(async () => view.update(<SettingsDialog {...props} isOpen={false} />));
    expect(useProtectionStore.getState().pending).toBe(true);
    await act(async () => finish());
    expect(useProtectionStore.getState().pending).toBe(false);
    expect(useNavigationStore.getState().developerMode).toBe(fails);
    expect(mocks.save).toHaveBeenCalledExactlyOnceWith({ developerMode: false });
  }
);

const optionalSwitch = (page: 'aiNews' | 'aiCareer' | 'localServer') =>
  view.root
    .findAllByProps({ role: 'switch' })
    .find((node) => node.props['aria-label'] === `nav.${page}`)!;

it.each([
  ['aiNews', 'aiNews', 'aiNewsEnabled'],
  ['aiCareer', 'aiCareer', 'aiCareerEnabled'],
  ['localLlm', 'localServer', 'localLlmEnabled'],
] as const)(
  'saves only an explicit %s change, retaining other switches and settings',
  async (page, label, key) => {
    const saved = {
      locale: 'ja',
      aiNewsEnabled: true,
      aiCareerEnabled: true,
      localLlmEnabled: true,
    };
    mocks.get.mockResolvedValue(saved);
    vi.stubGlobal('window', {
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent: vi.fn(),
    });
    await act(async () => {
      view = create(<SettingsDialog {...props} />);
    });
    expect(mocks.save).not.toHaveBeenCalled();
    expect(optionalSwitch(label).props['aria-checked']).toBe(true);
    useNavigationStore.getState().setActivePage(page);
    await act(async () => optionalSwitch(label).props.onClick());
    expect(mocks.save).toHaveBeenCalledExactlyOnceWith({ ...saved, [key]: false });
    expect(useNavigationStore.getState().optionalPages[page]).toBe(false);
    expect(useNavigationStore.getState().activePage).toBe('apps');
  }
);

it('keeps optional pages enabled on a failed save and reports the explicit failure', async () => {
  await act(async () => {
    view = create(<SettingsDialog {...props} />);
  });
  mocks.save.mockRejectedValueOnce(new Error('disk full'));
  useNavigationStore.getState().setActivePage('aiNews');
  await act(async () => optionalSwitch('aiNews').props.onClick());
  expect(useNavigationStore.getState().optionalPages.aiNews).toBe(true);
  expect(useNavigationStore.getState().activePage).toBe('aiNews');
  expect(JSON.stringify(view.toJSON())).toContain('settings.optionalFeaturesError');
});

it('serializes optional and master-switch saves and ignores stale reads after reopening', async () => {
  await act(async () => {
    view = create(<SettingsDialog {...props} />);
  });
  let finishSave!: () => void;
  mocks.save.mockReturnValueOnce(
    new Promise<void>((resolve) => {
      finishSave = resolve;
    })
  );
  await act(async () => optionalSwitch('aiCareer').props.onClick());
  expect(optionalSwitch('aiNews').props.disabled).toBe(true);
  expect(developerSwitch().props.disabled).toBe(true);
  // Saving locks the controls without fading them out and back in.
  for (const control of view.root.findAllByProps({ role: 'switch' })) {
    expect(control.props.className).not.toContain('disabled:opacity-50');
  }
  await act(async () => optionalSwitch('aiNews').props.onClick());
  expect(mocks.save).toHaveBeenCalledOnce();
  await act(async () => view.update(<SettingsDialog {...props} isOpen={false} />));
  let finishRead!: (value: object) => void;
  mocks.get.mockReturnValueOnce(
    new Promise((resolve) => {
      finishRead = resolve;
    })
  );
  await act(async () => view.update(<SettingsDialog {...props} />));
  await act(async () => finishSave());
  await act(async () => finishRead({ aiCareerEnabled: true }));
  expect(optionalSwitch('aiCareer').props['aria-checked']).toBe(false);
});
