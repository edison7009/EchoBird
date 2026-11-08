import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import PetRewardDialog from './PetRewardDialog';
import { usePetsStore as pets } from '../../stores/petsStore';
import { useNavigationStore as nav } from '../../stores/navigationStore';

const mock = vi.hoisted(() => ({
  attach: vi.fn((_element: HTMLElement, _options: unknown) => ({ destroy: mock.destroy })),
  destroy: vi.fn(),
  showModal: vi.fn(),
  close: vi.fn(),
  reduced: false,
  style: { setProperty: vi.fn() },
}));
vi.mock('@kongyo2/cards-css', () => ({ attachHoloCard: mock.attach }));
vi.mock('../../hooks/useI18n', () => ({
  useI18n: () => ({ locale: 'zh-Hans', t: (key: string) => key }),
}));
let view: ReactTestRenderer;
beforeEach(() => {
  vi.useFakeTimers();
  vi.clearAllMocks();
  mock.reduced = false;
  vi.stubGlobal('window', {
    matchMedia: () => ({ matches: mock.reduced }),
    getComputedStyle: () => ({ transform: 'matrix3d(fixture-angle)' }),
  });
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
const reward = { kind: 'growth', amount: 50_000_000, revealed: false } as const;
const render = (onClose = vi.fn()) => <PetRewardDialog reward={reward} onClose={onClose} />;

it('shows only the animated card and one use button, without applying the reward on display', () => {
  pets.setState({ rewards: { ...pets.getState().rewards, pending: reward } });
  const balance = pets.getState().balance;
  act(() => {
    view = create(render(), { createNodeMock: () => mock });
  });
  expect(mock.showModal).toHaveBeenCalledOnce();
  expect(mock.attach).toHaveBeenCalledOnce();
  expect(mock.attach.mock.calls[0][1]).toMatchObject({
    gyroscope: false,
    showcase: false,
    interactive: true,
  });
  expect(view.root.findAllByType('button').filter((button) => !button.props.disabled)).toHaveLength(
    1
  );
  expect(view.root.findByProps({ className: 'pets-upgrade pet-reward-use' }).props.children).toBe(
    'pets.reward.use'
  );
  expect(view.root.findAllByType('h3')).toHaveLength(0);
  expect(pets.getState().balance).toBe(balance);
  act(() => view.unmount());
  expect(mock.destroy).toHaveBeenCalledOnce();
  expect(mock.close).toHaveBeenCalledOnce();
});

it('uses the reward exactly once, then closes the card', () => {
  pets.setState({ rewards: { ...pets.getState().rewards, pending: reward } });
  const balance = pets.getState().balance;
  const close = vi.fn();
  act(() => {
    view = create(render(close), { createNodeMock: () => mock });
  });
  const use = view.root.findByProps({ className: 'pets-upgrade pet-reward-use' }).props.onClick;
  act(() => {
    use();
    use();
  });
  expect(pets.getState().balance).toBe(balance + reward.amount);
  expect(pets.getState().rewards.pending).toBeNull();
  expect(close).toHaveBeenCalledOnce();
});

it('Escape and background clicks close without using or discarding the card', () => {
  pets.setState({ rewards: { ...pets.getState().rewards, pending: reward } });
  const balance = pets.getState().balance;
  const close = vi.fn();
  act(() => {
    view = create(render(close), { createNodeMock: () => mock });
  });
  const dialog = view.root.findByType('dialog');
  const element = {};
  act(() => dialog.props.onClick({ target: {}, currentTarget: element }));
  expect(close).not.toHaveBeenCalled();
  act(() => dialog.props.onClick({ target: element, currentTarget: element }));
  expect(close).toHaveBeenCalledOnce();
  const preventDefault = vi.fn();
  act(() => dialog.props.onCancel({ preventDefault }));
  expect(preventDefault).toHaveBeenCalledOnce();
  expect(close).toHaveBeenCalledTimes(2);
  expect(pets.getState().rewards.pending).toEqual(reward);
  expect(pets.getState().balance).toBe(balance);
});

it('respects reduced motion for the automatic rotation and pointer tilt', () => {
  mock.reduced = true;
  act(() => {
    view = create(render(), { createNodeMock: () => mock });
  });
  expect(mock.attach.mock.calls[0][1]).toMatchObject({ interactive: false });
  expect(pets.getState().rewards.pending).toBeNull();
});

it('shows one silver sealed card, stops its loop on click and reveals the saved reward before enabling use', () => {
  const sealed = { ...reward, opened: false };
  pets.setState({ rewards: { ...pets.getState().rewards, pending: sealed } });
  const balance = pets.getState().balance;
  act(() => {
    view = create(<PetRewardDialog reward={sealed} onClose={vi.fn()} />, {
      createNodeMock: () => mock,
    });
  });
  expect(mock.attach.mock.calls[0][1]).toMatchObject({
    effect: 'metal',
    palette: { preset: 'mono' },
    showcase: false,
  });
  expect(JSON.stringify(view.toJSON())).not.toContain('pets.reward.growth');
  expect(view.root.findAllByType('small')).toHaveLength(0);
  expect(view.root.findAllByType('strong')).toHaveLength(0);
  expect(view.root.findAllByProps({ className: 'pets-upgrade pet-reward-use' })).toHaveLength(0);
  const reveal = view.root.findByProps({ className: 'pet-reward-reveal' });
  act(() => reveal.props.onClick());
  expect(mock.destroy).toHaveBeenCalledTimes(1);
  expect(mock.attach.mock.lastCall![1]).toMatchObject({ showcase: false });
  expect(mock.style.setProperty).toHaveBeenCalledWith('--pet-card-stop', 'matrix3d(fixture-angle)');
  expect(view.root.findByProps({ className: 'pet-reward-perspective' }).props['data-phase']).toBe(
    'settling'
  );
  expect(view.root.findByProps({ className: 'pet-reward-reveal' }).props.role).toBeUndefined();
  act(() => {
    vi.advanceTimersByTime(20);
  });
  act(() => {
    vi.advanceTimersByTime(45);
  });
  expect(JSON.stringify(view.toJSON())).not.toContain('pets.reward.growth');
  act(() => {
    vi.advanceTimersByTime(1);
  });
  expect(JSON.stringify(view.toJSON())).toContain('pets.reward.growth');
  expect(pets.getState().rewards.pending?.opened).toBe(true);
  expect(view.root.findAllByProps({ className: 'pets-upgrade pet-reward-use' })).toHaveLength(0);
  act(() => {
    vi.advanceTimersByTime(54);
  });
  expect(view.root.findAllByProps({ className: 'pets-upgrade pet-reward-use' })).toHaveLength(1);
  expect(mock.attach.mock.lastCall![1]).toMatchObject({ showcase: false, interactive: true });
  expect(pets.getState().balance).toBe(balance);
  act(() => view.root.findByProps({ className: 'pets-upgrade pet-reward-use' }).props.onClick());
  expect(pets.getState().balance).toBe(balance + reward.amount);
});

it('cleans up a cancelled reveal and preserves its unopened result without awarding anything', () => {
  const sealed = { ...reward, opened: false };
  pets.setState({ rewards: { ...pets.getState().rewards, pending: sealed } });
  const balance = pets.getState().balance;
  act(() => {
    view = create(<PetRewardDialog reward={sealed} onClose={vi.fn()} />, {
      createNodeMock: () => mock,
    });
  });
  act(() => view.root.findByProps({ className: 'pet-reward-reveal' }).props.onClick());
  act(() => view.unmount());
  act(() => {
    vi.advanceTimersByTime(1000);
  });
  expect(vi.getTimerCount()).toBe(0);
  expect(pets.getState().rewards.pending).toEqual(sealed);
  expect(pets.getState().balance).toBe(balance);
});

it('reveals immediately without spinning or delayed use when reduced motion is enabled', () => {
  mock.reduced = true;
  const sealed = { ...reward, opened: false };
  pets.setState({ rewards: { ...pets.getState().rewards, pending: sealed } });
  act(() => {
    view = create(<PetRewardDialog reward={sealed} onClose={vi.fn()} />, {
      createNodeMock: () => mock,
    });
  });
  expect(mock.attach.mock.lastCall![1]).toMatchObject({ showcase: false });
  act(() => view.root.findByProps({ className: 'pet-reward-reveal' }).props.onClick());
  expect(view.root.findAllByProps({ className: 'pets-upgrade pet-reward-use' })).toHaveLength(1);
  expect(vi.getTimerCount()).toBe(0);
  expect(pets.getState().rewards.pending?.opened).toBe(true);
});
