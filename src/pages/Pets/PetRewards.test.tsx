import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { PetRewards } from './PetRewards';
import { rewardCountdown, rewardDay, REWARD_INTERVAL } from './petRewardRules';
import { usePetsStore as pets } from '../../stores/petsStore';
import { useNavigationStore as nav } from '../../stores/navigationStore';

vi.mock('../../hooks/useI18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }));
vi.mock('./PetRewardDialog', () => ({
  default: ({ onClose }: { onClose: () => void }) => (
    <button className="test-close" onClick={onClose} />
  ),
}));
let view: ReactTestRenderer;
beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2026-10-29T04:00:00Z'));
  vi.stubGlobal('window', { setInterval, clearInterval });
  vi.stubGlobal('localStorage', { getItem: () => null, setItem: vi.fn() });
  pets.setState(pets.getInitialState(), true);
  pets.setState({
    loaded: true,
    rewards: {
      ...pets.getState().rewards,
      day: rewardDay(Date.now()),
      usedToday: 0,
      nextClaimAt: Date.now() + REWARD_INTERVAL,
    },
  });
  nav.setState({ developerMode: true, activePage: 'apps', appsTab: 'pets', developerTab: 'pets' });
});
afterEach(() => {
  act(() => view?.unmount());
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  vi.useRealTimers();
});
const nextAt = (until: number) =>
  pets.setState({ rewards: { ...pets.getState().rewards, nextClaimAt: until } });
const button = () => view.root.findByProps({ className: 'pet-reward-claim' });

it('counts down a visible deadline, disposes timers on exit and resumes the same deadline', () => {
  act(() => {
    view = create(<PetRewards />);
  });
  expect(JSON.stringify(view.toJSON())).toContain('05:00');
  act(() => {
    vi.advanceTimersByTime(1000);
  });
  expect(JSON.stringify(view.toJSON())).toContain('04:59');
  act(() => view.unmount());
  expect(vi.getTimerCount()).toBe(0);
  vi.advanceTimersByTime(REWARD_INTERVAL);
  act(() => {
    view = create(<PetRewards />);
  });
  expect(button().props.disabled).toBe(false);
  // Only the next local midnight is scheduled while ready.
  expect(vi.getTimerCount()).toBe(1);
});

it('does not automatically open saved cards and preserves one on closing', async () => {
  vi.advanceTimersByTime(REWARD_INTERVAL);
  pets.getState().claimReward();
  const pending = pets.getState().rewards.pending;
  act(() => {
    view = create(<PetRewards />);
  });
  expect(view.root.findAllByProps({ className: 'test-close' })).toHaveLength(0);
  await act(async () => button().props.onClick());
  act(() => view.root.findByProps({ className: 'test-close' }).props.onClick());
  expect(pets.getState().rewards.pending).toEqual(pending);
  expect(pets.getState().rewards.nextClaimAt).toBe(0);
});

it('holds a ready swaying gift without a countdown or automatically granting anything', () => {
  nextAt(Date.now() + 1000);
  act(() => {
    view = create(<PetRewards />);
  });
  expect(button().props.disabled).toBe(true);
  act(() => {
    vi.advanceTimersByTime(1000);
  });
  expect(button().props.disabled).toBe(false);
  expect(
    view.root
      .findAllByType('svg')
      .some((icon) => icon.props.className?.includes('pet-reward-gift-ready'))
  ).toBe(true);
  expect(button().findAllByType('span')).toHaveLength(1);
  expect(vi.getTimerCount()).toBe(1);
  act(() => {
    vi.advanceTimersByTime(2 * 60 * 60_000);
  });
  expect(button().props.disabled).toBe(false);
  expect(pets.getState().rewards.pending).toBeNull();
  expect(localStorage.setItem).not.toHaveBeenCalled();
  expect(rewardCountdown(1001, 1)).toBe('00:01');
  expect(rewardCountdown(1001, 1002)).toBe('00:00');
});

it('pauses the next opportunity while holding a card and starts ten minutes only after use', async () => {
  vi.spyOn(Math, 'random').mockReturnValue(0);
  nextAt(Date.now());
  act(() => {
    view = create(<PetRewards />);
  });
  await act(async () => button().props.onClick());
  const close = view.root.findByProps({ className: 'test-close' }).props.onClick;
  expect(pets.getState().rewards.nextClaimAt).toBe(0);
  act(() => {
    vi.advanceTimersByTime(60 * 60_000);
  });
  expect(pets.getState().rewards.nextClaimAt).toBe(0);
  expect(JSON.stringify(view.toJSON())).not.toContain('pets.reward.next');
  expect(vi.getTimerCount()).toBe(1);
  act(() => {
    pets.getState().revealReward();
    pets.getState().useReward();
    close();
  });
  expect(pets.getState().rewards.nextClaimAt).toBe(Date.now() + 2 * REWARD_INTERVAL);
  expect(JSON.stringify(view.toJSON())).toContain('10:00');
  expect(button().props.disabled).toBe(true);
});

it('resets an open page at local midnight even when the ready countdown had stopped', () => {
  const late = new Date();
  late.setHours(23, 59, 59, 0);
  vi.setSystemTime(late);
  pets.setState({
    rewards: {
      ...pets.getState().rewards,
      day: rewardDay(Date.now()),
      usedToday: 5,
      nextClaimAt: Date.now() - 1000,
    },
  });
  act(() => {
    view = create(<PetRewards />);
  });
  expect(button().props.disabled).toBe(false);
  act(() => {
    vi.advanceTimersByTime(1010);
  });
  expect(pets.getState().rewards.day).toBe(rewardDay(Date.now()));
  expect(pets.getState().rewards.usedToday).toBe(0);
  expect(button().props.disabled).toBe(true);
  expect(JSON.stringify(view.toJSON())).toContain('05:00');
});
