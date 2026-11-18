import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import PetRewardDialog from './PetRewardDialog';
import { usePetsStore as pets } from '../../stores/petsStore';
import { useNavigationStore as nav } from '../../stores/navigationStore';
import { RustSupplyCanvas } from './RustSupplyCanvas';

const mock = vi.hoisted(() => ({
  showModal: vi.fn(),
  close: vi.fn(),
  sound: { prepare: vi.fn(), prime: vi.fn(), play: vi.fn(), dispose: vi.fn() },
}));
vi.mock('./RustSupplyCanvas', () => ({ RustSupplyCanvas: () => null }));
vi.mock('./petAudio', () => ({ createRewardAudio: () => mock.sound }));
vi.mock('../../hooks/useI18n', () => ({
  useI18n: () => ({ locale: 'zh-Hans', t: (key: string) => key }),
}));
let view: ReactTestRenderer;
const reward = { kind: 'growth', amount: 50_000_000, revealed: false } as const;
const sealed = { ...reward, opened: false };
function render(pending: typeof reward & { opened?: boolean } = reward, close = vi.fn()) {
  pets.setState({ rewards: { ...pets.getState().rewards, pending } });
  act(() => {
    view = create(<PetRewardDialog reward={pending} onClose={close} />, {
      createNodeMock: () => mock,
    });
  });
}
const scene = () => view.root.findByType(RustSupplyCanvas).props;
const emit = (name: string, ...args: unknown[]) => act(() => scene()[name](...args));
const use = () => view.root.findAllByProps({ className: 'pets-upgrade pet-reward-use' });
const ready = () => {
  emit('onOpen', 99999, 99999);
  emit('onReady');
};
beforeEach(() => {
  vi.useFakeTimers();
  vi.clearAllMocks();
  vi.stubGlobal('localStorage', { setItem: vi.fn() });
  pets.setState(pets.getInitialState(), true);
  pets.setState({ loaded: true });
  nav.setState({ developerMode: true, activePage: 'apps', appsTab: 'pets', developerTab: 'pets' });
});
afterEach(() => {
  act(() => view?.unmount());
  vi.unstubAllGlobals();
  vi.useRealTimers();
});
it('restores legacy opened cards without rerolling, sound or an automatic award', () => {
  const balance = pets.getState().balance;
  render();
  expect(scene().phase).toBe('revealed');
  expect(use()).toHaveLength(1);
  expect(pets.getState().balance).toBe(balance);
  expect(mock.sound.prepare).not.toHaveBeenCalled();
});
it('waits for real tear and readiness events rather than wall-clock timers', () => {
  render(sealed);
  emit('onOpen', 1, 1);
  emit('onOpen', 1, 1);
  act(() => {
    vi.advanceTimersByTime(100000);
  });
  expect(scene().phase).toBe('opening');
  expect(mock.sound.prime).toHaveBeenCalledOnce();
  expect(mock.sound.play).not.toHaveBeenCalled();
  emit('onTearCue');
  emit('onTearCue');
  expect(mock.sound.play).toHaveBeenCalledOnce();
  emit('onReady');
  expect(scene().phase).toBe('choosing');
  expect(pets.getState().rewards.pending).toEqual(sealed);
  expect(vi.getTimerCount()).toBe(0);
});
it('retains a drag cue until the committed gesture can unlock audio', () => {
  render(sealed);
  emit('onTearCue');
  expect(mock.sound.play).not.toHaveBeenCalled();
  emit('onOpen', 1000, 0);
  expect(mock.sound.prepare).toHaveBeenCalledOnce();
  expect(mock.sound.play).toHaveBeenCalledOnce();
});
it('plays the tear cue during a held drag after pointerdown unlocks audio', () => {
  render(sealed);
  emit('onGesture');
  emit('onTearCue');
  expect(scene().phase).toBe('sealed');
  expect(mock.sound.play).toHaveBeenCalledOnce();
  emit('onOpen', 1000, 0);
  expect(mock.sound.prime).toHaveBeenCalledOnce();
  expect(mock.sound.play).toHaveBeenCalledOnce();
});
it.each([0, 1, 2, 3, 4])(
  'choice %i reveals the original reward and explicit use grants it once',
  (index) => {
    const close = vi.fn();
    render(sealed, close);
    const balance = pets.getState().balance;
    ready();
    emit('onPick', index);
    emit('onPick', (index + 1) % 5);
    expect(scene().selected).toBe(index);
    expect(pets.getState().rewards.pending).toEqual(sealed);
    expect(use()).toHaveLength(0);
    emit('onRevealMidpoint');
    emit('onRevealMidpoint');
    expect(pets.getState().rewards.pending).toEqual({ ...sealed, opened: true });
    expect(use()).toHaveLength(0);
    emit('onRevealFinished');
    expect(pets.getState().balance).toBe(balance);
    const apply = use()[0].props.onClick;
    act(() => {
      apply();
      apply();
    });
    expect(pets.getState().balance).toBe(balance + reward.amount);
    expect(pets.getState().rewards.pending).toBeNull();
    expect(close).toHaveBeenCalledOnce();
  }
);
it('ignores out-of-order events and never exposes use before the reveal midpoint', () => {
  render(sealed);
  emit('onReady');
  emit('onPick', 0);
  emit('onRevealMidpoint');
  emit('onRevealFinished');
  expect(scene().phase).toBe('sealed');
  ready();
  emit('onPick', 1);
  emit('onRevealFinished');
  expect(use()).toHaveLength(0);
  emit('onRevealMidpoint');
  emit('onRevealFinished');
  expect(use()).toHaveLength(1);
});
it.each(['opening', 'choosing', 'revealing'])(
  'closing during %s preserves pending and rejects late callbacks',
  (phase) => {
    const close = vi.fn();
    render(sealed, close);
    emit('onOpen');
    if (phase !== 'opening') emit('onReady');
    if (phase === 'revealing') emit('onPick', 3);
    const callbacks = scene();
    act(() =>
      view.root.findByProps({ className: 'pet-reward-close close-button' }).props.onClick()
    );
    act(() => {
      callbacks.onReady();
      callbacks.onTearCue();
      callbacks.onRevealMidpoint();
      callbacks.onRevealFinished();
    });
    expect(close).toHaveBeenCalledOnce();
    expect(pets.getState().rewards.pending).toEqual(sealed);
    expect(mock.sound.play).not.toHaveBeenCalled();
    expect(mock.sound.dispose).toHaveBeenCalledOnce();
  }
);
it('retains a mid-flip revealed card when unmounted and reopens straight to it', () => {
  render(sealed);
  ready();
  emit('onPick', 3);
  emit('onRevealMidpoint');
  const callbacks = scene();
  act(() => view.unmount());
  act(() => {
    callbacks.onRevealFinished();
    callbacks.onTearCue();
  });
  render(pets.getState().rewards.pending! as typeof reward & { opened?: boolean });
  expect(scene().phase).toBe('revealed');
  expect(use()).toHaveLength(1);
});
it('accepts compressed event timing for reduced motion with the same five-way choice and award rules', () => {
  render(sealed);
  act(() => {
    const c = scene();
    c.onOpen(0, 0);
    c.onTearCue();
    c.onReady();
    c.onPick(4);
    c.onRevealMidpoint();
    c.onRevealFinished();
  });
  expect(scene().selected).toBe(4);
  expect(use()).toHaveLength(1);
  expect(pets.getState().rewards.pending?.opened).toBe(true);
  expect(pets.getState().rewards.pending?.revealed).toBe(false);
});
it.each(['escape', 'background'])('%s closes without consuming the reward', (method) => {
  const close = vi.fn();
  render(sealed, close);
  const d = view.root.findByType('dialog');
  const preventDefault = vi.fn();
  const element = {};
  act(() =>
    method === 'escape'
      ? d.props.onCancel({ preventDefault })
      : d.props.onClick({ target: element, currentTarget: element })
  );
  expect(close).toHaveBeenCalledOnce();
  expect(pets.getState().rewards.pending).toEqual(sealed);
});

it.each([
  ['combo', 25_000_000, '−01:15'],
  ['super', 100_000_000, '+1亿'],
] as const)('presents the saved %s card and consumes its reward once', (kind, amount, text) => {
  const pending = { kind, amount, opened: true, revealed: false };
  pets.setState({ rewards: { ...pets.getState().rewards, pending } });
  const close = vi.fn();
  act(() => {
    view = create(<PetRewardDialog reward={pending} onClose={close} />, {
      createNodeMock: () => mock,
    });
  });
  expect(scene().kind).toBe(kind);
  expect(scene().value).toContain(text);
  const balance = pets.getState().balance;
  const apply = use()[0].props.onClick;
  act(() => {
    apply();
    apply();
  });
  expect(pets.getState().balance).toBe(balance + amount);
  expect(close).toHaveBeenCalledOnce();
});
