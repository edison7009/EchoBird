import { useEffect, useRef, useState } from 'react';
import { createRewardAudio } from './petAudio';
import { useI18n } from '../../hooks/useI18n';
import { usePetsStore } from '../../stores/petsStore';
import {
  COMBO_HASTE_DURATION,
  HASTE_DURATION,
  rewardCountdown,
  type PetReward,
} from './petRewardRules';
import { RustSupplyCanvas } from './RustSupplyCanvas';

export default function PetRewardDialog({
  reward,
  onClose,
}: {
  reward: PetReward;
  onClose: () => void;
}) {
  const { t, locale } = useI18n();
  const dialog = useRef<HTMLDialogElement>(null);
  type Phase = 'sealed' | 'opening' | 'choosing' | 'revealing' | 'revealed';
  const initialPhase = reward.opened === false ? 'sealed' : 'revealed';
  const current = useRef<Phase>(initialPhase);
  const closed = useRef(false);
  const cueReached = useRef(false);
  const cuePlayed = useRef(false);
  const markedOpened = useRef(reward.opened !== false);
  const sound = useRef<ReturnType<typeof createRewardAudio> | null>(null);
  const [phase, setPhase] = useState<Phase>(initialPhase);
  const [selected, setSelected] = useState(2);
  const transition = (next: Phase) => {
    current.current = next;
    setPhase(next);
  };
  const close = () => {
    if (closed.current) return;
    closed.current = true;
    sound.current?.dispose();
    sound.current = null;
    onClose();
  };
  useEffect(() => {
    const element = dialog.current!;
    closed.current = false;
    element.showModal();
    return () => {
      closed.current = true;
      element.close();
      sound.current?.dispose();
      sound.current = null;
    };
  }, []);
  const playCue = () => {
    if (closed.current) return;
    cueReached.current = true;
    if (!sound.current || cuePlayed.current) return;
    cuePlayed.current = true;
    sound.current.play();
  };
  const prepareSound = () => {
    if (closed.current || current.current !== 'sealed' || sound.current) return;
    sound.current = createRewardAudio();
    sound.current.prepare();
    sound.current.prime();
  };
  const reveal = () => {
    if (closed.current || current.current !== 'sealed') return;
    prepareSound();
    transition('opening');
    // A drag may cross the sound cue before pointerup commits the tear.
    if (cueReached.current) playCue();
  };
  const pick = (index: number) => {
    if (closed.current || current.current !== 'choosing' || index < 0 || index > 4) return;
    setSelected(index);
    transition('revealing');
  };
  return (
    <dialog
      ref={dialog}
      className="pet-reward-dialog pet-reward-rust-dialog"
      data-count="5"
      aria-label={t('pets.reward.title')}
      onCancel={(event) => {
        event.preventDefault();
        close();
      }}
      onClick={(event) => {
        if (event.target === event.currentTarget) close();
      }}
    >
      <RustSupplyCanvas
        phase={phase}
        selected={selected}
        kind={reward.kind}
        value={
          reward.kind === 'double'
            ? '×2'
            : reward.kind === 'haste'
              ? `−${rewardCountdown(HASTE_DURATION, 0)}`
              : `+${new Intl.NumberFormat(locale, { notation: 'compact', maximumFractionDigits: 2 }).format(reward.amount)}${reward.kind === 'combo' ? ` · −${rewardCountdown(COMBO_HASTE_DURATION, 0)}` : ''}`
        }
        onGesture={prepareSound}
        onOpen={reveal}
        onPick={pick}
        onTearCue={playCue}
        onReady={() => {
          if (!closed.current && current.current === 'opening') transition('choosing');
        }}
        onRevealMidpoint={() => {
          if (closed.current || current.current !== 'revealing' || markedOpened.current) return;
          markedOpened.current = true;
          usePetsStore.getState().revealReward();
        }}
        onRevealFinished={() => {
          if (!closed.current && current.current === 'revealing' && markedOpened.current)
            transition('revealed');
        }}
      />
      <div className="pet-reward-actions">
        {phase === 'revealed' && (
          <button
            type="button"
            className="pets-upgrade pet-reward-use"
            autoFocus
            onClick={() => {
              if (
                !closed.current &&
                current.current === 'revealed' &&
                usePetsStore.getState().useReward()
              )
                close();
            }}
          >
            {t('pets.reward.use')}
          </button>
        )}
      </div>
    </dialog>
  );
}
