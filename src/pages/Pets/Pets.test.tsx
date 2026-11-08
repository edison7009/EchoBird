import { Profiler } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { PetsMain, PetsPanel } from './Pets';
import { usePetsStore as pets } from '../../stores/petsStore';
import { useNavigationStore as nav } from '../../stores/navigationStore';

const mock = vi.hoisted(() => ({
  visible: true,
  modalOpen: false,
  credits: vi.fn(async () => ({ ledgerId: 'fixture-ledger', total: 0 })),
  done: undefined as undefined | (() => void),
  progress: undefined as undefined | ((progress: number) => void),
  show: vi.fn(),
  dispose: vi.fn(),
  interact: vi.fn(),
  rotate: vi.fn(),
  upgrade: vi.fn((_next: unknown, done: () => void, onProgress?: (progress: number) => void) => {
    mock.done = done;
    mock.progress = onProgress;
  }),
  thumbnail: vi.fn(() => 'data:image/png;base64,fixture'),
  sound: { prepare: vi.fn(), play: vi.fn(), stop: vi.fn(), dispose: vi.fn() },
}));
vi.mock('./petScene.js', () => ({ createPetScene: () => mock }));
vi.mock('../../api/routerStatistics', () => ({ getSmartRouterTokenCredits: mock.credits }));
vi.mock('./petAudio', () => ({ createPetAudio: () => mock.sound }));
vi.mock('../../hooks/useDocumentVisible', () => ({ useDocumentVisible: () => mock.visible }));
vi.mock('../../hooks/useI18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }));
let view: ReactTestRenderer;
const setItem = vi.fn();
beforeEach(() => {
  vi.useFakeTimers();
  vi.clearAllMocks();
  mock.done = undefined;
  mock.progress = undefined;
  mock.visible = true;
  mock.modalOpen = false;
  mock.credits.mockResolvedValue({ ledgerId: 'fixture-ledger', total: 0 });
  vi.stubGlobal(
    'requestAnimationFrame',
    vi.fn(() => 1)
  );
  vi.stubGlobal('cancelAnimationFrame', vi.fn());
  vi.stubGlobal('window', { matchMedia: vi.fn(() => ({ matches: false })) });
  vi.stubGlobal('localStorage', { getItem: vi.fn(() => null), setItem });
  pets.setState(pets.getInitialState(), true);
  pets.setState({
    loaded: true,
    balance: 100_000_000_000,
    items: [
      { id: 1, kind: 'mecha', level: 0, color: 0 },
      { id: 2, kind: 'unicorn', level: 0, color: 1 },
      { id: 3, kind: 'kaiju', level: 10, color: 2 },
    ],
  });
  nav.setState(
    {
      ...nav.getInitialState(),
      developerMode: true,
      activePage: 'apps',
      appsTab: 'pets',
      developerTab: 'pets',
    },
    true
  );
});
afterEach(() => {
  act(() => view?.unmount());
  vi.unstubAllGlobals();
  vi.useRealTimers();
});
async function mount(onRender?: () => void) {
  await act(async () => {
    const content = (
      <>
        <PetsMain />
        <PetsPanel />
      </>
    );
    view = create(
      onRender ? (
        <Profiler id="pets" onRender={onRender}>
          {content}
        </Profiler>
      ) : (
        content
      ),
      {
        createNodeMock: (element) =>
          element.type === 'canvas'
            ? { ownerDocument: { querySelector: () => (mock.modalOpen ? {} : null) } }
            : {},
      }
    );
  });
}
function upgrade() {
  act(() => view.root.findByProps({ className: 'pets-upgrade' }).props.onClick());
}
function finishUpgrade() {
  act(() => {
    mock.done!();
    vi.advanceTimersByTime(800);
  });
}

it('only initializes the daily local clock and gives the selected card one border-based selection', async () => {
  await mount();
  expect(mock.show).toHaveBeenCalled();
  expect(setItem).toHaveBeenCalledOnce();
  expect(view.root.findAllByProps({ 'aria-pressed': true })).toHaveLength(1);
  const buttons = view.root.findAllByType('button');
  expect(buttons[buttons.length - 1].props.className).toContain('pets-acquire');
  expect(JSON.stringify(view.toJSON())).not.toContain('radio');
  expect(JSON.stringify(view.toJSON())).not.toContain('pets.requirement');
  expect(view.root.findByProps({ role: 'progressbar' }).props['aria-valuenow']).toBe(0);
  expect(
    view.root.findByProps({ className: 'pets-title' }).findAllByType('span')[0].props.className
  ).toBe('pets-level');
});

it('advances experience and spends a small balance without a level-up or level-up sound', async () => {
  pets.setState({ balance: 40_000_000 });
  await mount();
  expect(view.root.findByProps({ className: 'pets-upgrade' }).props.disabled).toBe(false);
  upgrade();
  expect(mock.upgrade.mock.calls[0][0]).toMatchObject({ level: 0, stage: 0 });
  act(() => mock.progress!(0.5));
  expect(view.root.findByProps({ role: 'progressbar' }).props['aria-valuenow']).toBe(20);
  expect(pets.getState().balance).toBe(40_000_000);
  finishUpgrade();
  expect(view.root.findByProps({ role: 'progressbar' }).props['aria-valuenow']).toBe(40);
  expect(pets.getState().balance).toBe(0);
  expect(pets.getState().items[0].level).toBe(0);
  expect(mock.sound.play).not.toHaveBeenCalled();
  expect(
    view.root.findByProps({ className: 'pets-stage' }).props['data-celebrating']
  ).toBeUndefined();
  expect(view.root.findByProps({ className: 'pets-upgrade' }).props.children).toBe('pets.upgrade');
  expect(view.root.findByProps({ className: 'pets-upgrade' }).props.disabled).toBe(true);
});

it.each([
  [0, 40_000_000],
  [90_000_000, 5_000_000],
])('never overshoots partial experience %i + %i during completion', async (experience, balance) => {
  pets.setState({ items: [{ id: 1, kind: 'mecha', level: 0, color: 0, experience }], balance });
  let recording = false;
  const frames: number[] = [];
  await mount(() => {
    if (recording)
      frames.push(view.root.findByProps({ role: 'progressbar' }).props['aria-valuenow']);
  });
  recording = true;
  upgrade();
  act(() => mock.progress!(0.5));
  act(() => mock.progress!(1));
  // Observe the store notification before the local animation state is cleared.
  mock.done!();
  const target = ((experience + balance) / 100_000_000) * 100;
  expect(frames[frames.length - 1]).toBe(target);
  expect(frames.every((progress) => progress <= target)).toBe(true);
  expect(frames.every((progress, index) => !index || progress >= frames[index - 1])).toBe(true);
  expect(pets.getState().items[0]).toMatchObject({ level: 0, experience: experience + balance });
  expect(pets.getState().balance).toBe(0);
  expect(mock.sound.play).not.toHaveBeenCalled();
  expect(view.root.findByProps({ role: 'progressbar' }).props['data-completing']).toBeUndefined();
});

it('finishes only the current level and resets the bar while keeping the remaining balance', async () => {
  pets.setState({
    items: [{ id: 1, kind: 'mecha', level: 9, color: 0, experience: 40_000_000 }],
    balance: 1_000_000_000,
  });
  await mount();
  expect(view.root.findByProps({ role: 'progressbar' }).props['aria-valuenow']).toBe(40);
  upgrade();
  act(() => mock.progress!(0.5));
  expect(view.root.findByProps({ role: 'progressbar' }).props['aria-valuenow']).toBe(70);
  finishUpgrade();
  expect(view.root.findByProps({ role: 'progressbar' }).props['aria-valuenow']).toBe(0);
  expect(pets.getState().items[0].level).toBe(10);
  expect(pets.getState().balance).toBe(940_000_000);
  expect(mock.sound.play).toHaveBeenCalledExactlyOnceWith(true);
});

it('finishes the model and bar before starting confetti and sound together, without advancing twice', async () => {
  await mount();
  const balance = pets.getState().balance;
  upgrade();
  expect(
    view.root.findByProps({ className: 'pets-stage' }).props['data-celebrating']
  ).toBeUndefined();
  expect(mock.sound.play).not.toHaveBeenCalled();
  act(() => mock.done!());
  expect(view.root.findByProps({ role: 'progressbar' }).props['aria-valuenow']).toBe(100);
  expect(view.root.findByProps({ role: 'progressbar' }).props['data-completing']).toBe(true);
  expect(view.root.findByProps({ className: 'pets-upgrade' }).props.disabled).toBe(true);
  expect(pets.getState().busy).toBe(true);
  upgrade();
  expect(mock.upgrade).toHaveBeenCalledTimes(1);
  act(() => {
    vi.advanceTimersByTime(799);
  });
  expect(pets.getState().balance).toBe(balance);
  expect(
    view.root.findByProps({ className: 'pets-stage' }).props['data-celebrating']
  ).toBeUndefined();
  expect(mock.sound.play).not.toHaveBeenCalled();
  act(() => {
    vi.advanceTimersByTime(1);
  });
  expect(pets.getState().items[0].level).toBe(1);
  expect(view.root.findByProps({ role: 'progressbar' }).props['aria-valuenow']).toBe(0);
  expect(pets.getState().busy).toBe(false);
  expect(view.root.findByProps({ className: 'pets-stage' }).props['data-celebrating']).toBe(true);
  expect(mock.sound.play).toHaveBeenCalledExactlyOnceWith(false);
  act(() =>
    view.root.findByProps({ className: 'pets-stage' }).props.onAnimationEnd({
      animationName: 'pets-level-pixels',
    })
  );
  expect(
    view.root.findByProps({ className: 'pets-stage' }).props['data-celebrating']
  ).toBeUndefined();
  act(() => {
    vi.advanceTimersByTime(1000);
  });
  expect(pets.getState().balance).toBe(balance - 100_000_000);
});

it('cancels an unfinished glow when switching pets without spending or carrying the glow to another pet', async () => {
  await mount();
  const balance = pets.getState().balance;
  upgrade();
  act(() => mock.done!());
  act(() => pets.getState().select(2));
  act(() => {
    vi.advanceTimersByTime(800);
  });
  expect(pets.getState().balance).toBe(balance);
  expect(pets.getState().items[0].level).toBe(0);
  expect(view.root.findByProps({ role: 'progressbar' }).props['data-completing']).toBeUndefined();
  expect(
    view.root.findByProps({ className: 'pets-stage' }).props['data-celebrating']
  ).toBeUndefined();
  expect(mock.sound.play).not.toHaveBeenCalled();
});

it('plays periodic idle actions without click restarts and pauses during upgrades and hidden visits', async () => {
  await mount();
  const stage = view.root.findByProps({ className: 'pets-stage' });
  act(() => {
    stage.props.onClick();
    stage.props.onClick();
  });
  expect(mock.interact).not.toHaveBeenCalled();
  act(() => {
    vi.advanceTimersByTime(6000);
  });
  expect(mock.interact).toHaveBeenCalledTimes(1);
  upgrade();
  act(() => {
    vi.advanceTimersByTime(6000);
  });
  expect(mock.interact).toHaveBeenCalledTimes(1);
  mock.visible = false;
  act(() =>
    view.update(
      <>
        <PetsMain />
        <PetsPanel />
      </>
    )
  );
  act(() => {
    vi.advanceTimersByTime(12000);
  });
  expect(mock.interact).toHaveBeenCalledTimes(1);
  expect(vi.getTimerCount()).toBe(0);
});

it('skips automatic motion and the glow delay when reduced motion is requested', async () => {
  vi.mocked(window.matchMedia).mockReturnValue({ matches: true } as MediaQueryList);
  await mount();
  act(() => {
    vi.advanceTimersByTime(6000);
  });
  expect(mock.interact).not.toHaveBeenCalled();
  upgrade();
  act(() => mock.done!());
  expect(pets.getState().items[0].level).toBe(1);
  expect(pets.getState().busy).toBe(false);
  expect(
    view.root.findByProps({ className: 'pets-stage' }).props['data-celebrating']
  ).toBeUndefined();
});

it('pauses automatic pet gestures while a modal card is open and resumes after closing it', async () => {
  await mount();
  mock.modalOpen = true;
  act(() => {
    vi.advanceTimersByTime(12000);
  });
  expect(mock.interact).not.toHaveBeenCalled();
  mock.modalOpen = false;
  act(() => {
    vi.advanceTimersByTime(6000);
  });
  expect(mock.interact).toHaveBeenCalledTimes(1);
});

it('rolls a newly credited balance upwards with a finite animation', async () => {
  pets.setState({ balance: 100 });
  vi.spyOn(performance, 'now').mockReturnValue(0);
  await mount();
  mock.credits.mockResolvedValue({ ledgerId: 'income', total: 100 });
  await act(async () => {
    vi.advanceTimersByTime(5000);
  });
  expect(pets.getState().balance).toBe(200);
  const amount = () =>
    Number(
      view.root.findByProps({ className: 'pets-balance' }).findByType('strong').children.join('')
    );
  expect(amount()).toBe(100);
  act(() => vi.mocked(requestAnimationFrame).mock.lastCall![0](175));
  expect(amount()).toBeGreaterThan(100);
  expect(amount()).toBeLessThan(200);
  act(() => vi.mocked(requestAnimationFrame).mock.lastCall![0](350));
  expect(amount()).toBe(200);
  expect(requestAnimationFrame).toHaveBeenCalledTimes(2);
  vi.mocked(performance.now).mockRestore();
});

it('ignores a late usage response after hiding the page and stops its refresh timer', async () => {
  let resolve!: (value: { ledgerId: string; total: number }) => void;
  mock.credits.mockReturnValueOnce(
    new Promise((done) => {
      resolve = done;
    })
  );
  await mount();
  const balance = pets.getState().balance;
  mock.visible = false;
  act(() =>
    view.update(
      <>
        <PetsMain />
        <PetsPanel />
      </>
    )
  );
  await act(async () => resolve({ ledgerId: 'late', total: 1000 }));
  expect(pets.getState().balance).toBe(balance);
  expect(vi.getTimerCount()).toBe(0);
});

it('commits only completed upgrades and ignores old callbacks after switching away and back', async () => {
  await mount();
  upgrade();
  expect(pets.getState().items[0].level).toBe(0);
  expect(pets.getState().busy).toBe(true);
  const upgradingButton = view.root.findByProps({ className: 'pets-upgrade' });
  expect(upgradingButton.props.children).toBe('pets.upgrade');
  expect(upgradingButton.props['aria-busy']).toBe(true);
  expect(upgradingButton.props.disabled).toBe(true);
  const old = mock.done!;
  act(() => {
    pets.getState().select(2);
    pets.getState().select(1);
    old();
  });
  expect(pets.getState().items[0].level).toBe(0);
  upgrade();
  finishUpgrade();
  expect(pets.getState().items[0].level).toBe(1);
  expect(mock.sound.play).toHaveBeenCalledExactlyOnceWith(false);
});

it('disposes rendering when hidden and cannot complete the cancelled growth on resume', async () => {
  await mount();
  upgrade();
  const old = mock.done!;
  mock.visible = false;
  act(() =>
    view.update(
      <>
        <PetsMain />
        <PetsPanel />
      </>
    )
  );
  expect(mock.dispose).toHaveBeenCalledTimes(1);
  expect(pets.getState().busy).toBe(false);
  mock.visible = true;
  await act(async () =>
    view.update(
      <>
        <PetsMain />
        <PetsPanel />
      </>
    )
  );
  act(() => old());
  expect(pets.getState().items[0].level).toBe(0);
  expect(mock.sound.play).not.toHaveBeenCalled();
});

it('cancels and disposes on tab changes and on the developer master switch', async () => {
  await mount();
  upgrade();
  const old = mock.done!;
  act(() => nav.getState().setDeveloperTab('plugins'));
  expect(mock.dispose).toHaveBeenCalledTimes(1);
  act(() => old());
  expect(pets.getState().items[0].level).toBe(0);
  await act(async () => nav.getState().setDeveloperTab('pets'));
  act(() => nav.getState().setDeveloperMode(false));
  expect(mock.dispose).toHaveBeenCalledTimes(2);
});

it.each([9, 19, 29, 39, 49, 59])(
  'plays only the special cue after successful level %i completion',
  async (from) => {
    pets.setState({ items: [{ id: 1, kind: 'mecha', level: from, color: 0 }] });
    await mount();
    upgrade();
    expect(mock.sound.play).not.toHaveBeenCalled();
    finishUpgrade();
    expect(pets.getState().items[0].level).toBe(from + 1);
    expect(mock.sound.play).toHaveBeenCalledExactlyOnceWith(true);
  }
);
