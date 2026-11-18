import { lazy, Suspense, useEffect, useState } from 'react';
import { Clock3, Gift } from 'lucide-react';
import { useI18n } from '../../hooks/useI18n';
import { usePetsStore } from '../../stores/petsStore';
import { rewardCountdown } from './petRewardRules';
import './PetRewards.css';

const RewardDialog = lazy(() => import('./PetRewardDialog'));

export function PetRewards() {
  const { t } = useI18n();
  const rewards = usePetsStore((state) => state.rewards);
  const busy = usePetsStore((state) => state.busy);
  const [now, setNow] = useState(Date.now);
  const [open, setOpen] = useState(false);
  const ready = rewards.nextClaimAt <= now;
  const canClaim = ready || Boolean(rewards.pending);
  const ticking = (!rewards.pending && !ready) || rewards.doubleUntil > now;
  useEffect(() => {
    const refresh = () => {
      usePetsStore.getState().refreshRewards();
      setNow(Date.now());
    };
    refresh();
    const midnight = new Date();
    midnight.setHours(24, 0, 0, 0);
    const timer = setTimeout(refresh, midnight.getTime() - Date.now() + 10);
    return () => clearTimeout(timer);
  }, [rewards.day]);
  useEffect(() => {
    if (!ticking) return;
    const timer = setInterval(() => {
      usePetsStore.getState().refreshRewards();
      setNow(Date.now());
    }, 1000);
    return () => clearInterval(timer);
  }, [ticking]);
  return (
    <div className="pet-rewards">
      <button
        type="button"
        className="pet-reward-claim"
        disabled={busy || !canClaim}
        onClick={() => {
          setNow(Date.now());
          if (usePetsStore.getState().claimReward()) setOpen(true);
        }}
      >
        {canClaim ? (
          <Gift
            size={16}
            className={ready && !rewards.pending ? 'pet-reward-gift-ready' : undefined}
          />
        ) : (
          <Clock3 size={16} />
        )}
        <span>{t(canClaim ? 'pets.reward.claim' : 'pets.reward.next')}</span>
        {!canClaim && <span>{rewardCountdown(rewards.nextClaimAt, now)}</span>}
      </button>
      {rewards.doubleUntil > now && (
        <small className="pet-reward-boost">
          {t('pets.reward.boost')} · {rewardCountdown(rewards.doubleUntil, now)}
        </small>
      )}
      {open && rewards.pending && (
        <Suspense fallback={null}>
          <RewardDialog
            reward={rewards.pending}
            onClose={() => {
              setNow(Date.now());
              setOpen(false);
            }}
          />
        </Suspense>
      )}
    </div>
  );
}
