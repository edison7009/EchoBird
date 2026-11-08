import { useEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from 'react';
import { Egg, Plus } from 'lucide-react';
import { useI18n } from '../../hooks/useI18n';
import { useDocumentVisible } from '../../hooks/useDocumentVisible';
import { useNavigationStore } from '../../stores/navigationStore';
import { PageActionButton } from '../../components/PageActionButton';
import { getSmartRouterTokenCredits } from '../../api/routerStatistics';
import { petThumbnailKey, usePetsStore } from '../../stores/petsStore';
import { PET_STAGES, PET_TINTS, petModel, petStage, type PreviewPet } from './petConstants';
import { EGG_COST, MAX_PET_LEVEL, isPetMilestone, petUpgradeCost } from './petProgression';
import { createPetAudio } from './petAudio';
import type { PetScene } from './petScene';
import { PetRewards } from './PetRewards';
import './Pets.css';

function usePetsActive() {
  return useNavigationStore(
    (state) => state.developerMode && state.activePage === 'apps' && state.appsTab === 'pets'
  );
}

function TokenPetBalance({
  balance,
  spending,
  locale,
  active,
}: {
  balance: number;
  spending: number;
  locale: string;
  active: boolean;
}) {
  const [value, setValue] = useState(balance);
  const displayed = useRef(balance),
    target = useRef(balance);
  useEffect(() => {
    const previous = target.current;
    target.current = balance;
    if (
      !active ||
      balance <= previous ||
      window.matchMedia('(prefers-reduced-motion: reduce)').matches
    ) {
      displayed.current = balance;
      setValue(balance);
      return;
    }
    const from = displayed.current,
      start = performance.now();
    let frame: number;
    const tick = (now: number) => {
      const progress = Math.min(1, Math.max(0, (now - start) / 350));
      displayed.current = Math.round(from + (balance - from) * progress);
      setValue(displayed.current);
      if (progress < 1) frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [balance, active]);
  return (
    <strong>
      {new Intl.NumberFormat(locale, { notation: 'compact', maximumFractionDigits: 2 }).format(
        Math.max(0, value - spending)
      )}
    </strong>
  );
}

export function PetsMain() {
  const { t, locale } = useI18n();
  const active = usePetsActive();
  const generation = useNavigationStore((state) => state.developerGeneration);
  const visible = useDocumentVisible();
  const items = usePetsStore((state) => state.items);
  const selected = usePetsStore((state) => state.selected);
  const busy = usePetsStore((state) => state.busy);
  const balance = usePetsStore((state) => state.balance);
  const pet = items.find((item) => item.id === selected)!;
  const name = petStage(pet.level) < 2 ? t('pets.eggName') : t(`pets.kind.${pet.kind}`);
  const stage = t(`pets.stage.${PET_STAGES[petStage(pet.level)]}`);
  const maxed = pet.level >= MAX_PET_LEVEL;
  const cost = petUpgradeCost(pet.level);
  const [spending, setSpending] = useState(0);
  const [completing, setCompleting] = useState(false);
  const [celebrating, setCelebrating] = useState(false);
  const completionTimer = useRef<ReturnType<typeof setTimeout>>();
  const pendingSpending = busy ? spending : 0;
  const progress = maxed
    ? 100
    : Math.min(100, (((pet.experience ?? 0) + pendingSpending) / cost) * 100);
  const canvas = useRef<HTMLCanvasElement>(null);
  const scene = useRef<PetScene | null>(null);
  const audio = useRef<ReturnType<typeof createPetAudio> | null>(null);
  const lifecycle = useRef({ revision: 0 });
  const drag = useRef<{ x: number; y: number; moved: boolean } | null>(null);
  const [renderState, setRenderState] = useState<'loading' | 'ready' | 'error'>('loading');

  useEffect(() => {
    if (!active || !visible) return;
    const currentLifecycle = lifecycle.current;
    const sound = createPetAudio();
    audio.current = sound;
    let cancelled = false;
    let unsubscribe: (() => void) | undefined;
    let idleTimer: ReturnType<typeof setInterval> | undefined;
    usePetsStore.getState().load();
    void import('./petScene.js')
      .then(({ createPetScene }) => {
        if (cancelled || !canvas.current) return;
        const renderer = createPetScene(canvas.current);
        scene.current = renderer;
        const sync = () => {
          currentLifecycle.revision++;
          clearTimeout(completionTimer.current);
          completionTimer.current = undefined;
          setCompleting(false);
          setCelebrating(false);
          setSpending(0);
          sound.stop();
          const state = usePetsStore.getState();
          const thumbnails = { ...state.thumbnails };
          for (const pet of state.items) {
            const key = petThumbnailKey(pet);
            if (!thumbnails[key]) thumbnails[key] = renderer.thumbnail(petModel(pet));
          }
          renderer.show(petModel(state.items.find((pet) => pet.id === state.selected)!));
          usePetsStore.setState({ thumbnails, busy: false });
        };
        sync();
        unsubscribe = usePetsStore.subscribe((state, previous) => {
          if (state.selected !== previous.selected || state.items !== previous.items) sync();
        });
        setRenderState('ready');
        if (!window.matchMedia('(prefers-reduced-motion: reduce)').matches)
          idleTimer = setInterval(() => {
            if (
              !usePetsStore.getState().busy &&
              !drag.current &&
              !canvas.current?.ownerDocument?.querySelector('dialog[open]')
            )
              renderer.interact();
          }, 6000);
      })
      .catch(() => {
        if (!cancelled) setRenderState('error');
      });
    return () => {
      cancelled = true;
      currentLifecycle.revision++;
      clearInterval(idleTimer);
      clearTimeout(completionTimer.current);
      completionTimer.current = undefined;
      setCompleting(false);
      setCelebrating(false);
      unsubscribe?.();
      sound.dispose();
      audio.current = null;
      scene.current?.dispose();
      scene.current = null;
      drag.current = null;
      setSpending(0);
      usePetsStore.setState({ busy: false });
    };
  }, [active, visible]);

  useEffect(() => {
    if (!active || !visible) return;
    let current = true,
      pending = false;
    const syncCredits = async () => {
      if (!current || pending) return;
      pending = true;
      try {
        const credits = await getSmartRouterTokenCredits();
        if (current) usePetsStore.getState().creditRouter(credits);
      } catch {
        // Keep the last saved wallet and retry on the next visible refresh.
      } finally {
        pending = false;
      }
    };
    void syncCredits();
    const timer = setInterval(() => void syncCredits(), 5000);
    return () => {
      current = false;
      clearInterval(timer);
    };
  }, [active, visible, generation]);

  const upgrade = () => {
    if (!scene.current || !active || !visible) return;
    const from = usePetsStore.getState().startUpgrade();
    if (!from) return;
    setCelebrating(false);
    audio.current?.prepare();
    const amount = Math.min(
      usePetsStore.getState().balance,
      petUpgradeCost(from.level) - (from.experience ?? 0)
    );
    const leveledUp = (from.experience ?? 0) + amount >= petUpgradeCost(from.level);
    const currentRevision = lifecycle.current.revision;
    const finish = () => {
      if (
        lifecycle.current.revision === currentRevision &&
        usePetsStore.getState().completeUpgrade(from.id, from.level) &&
        leveledUp
      ) {
        audio.current?.play(isPetMilestone(from.level + 1));
        if (!window.matchMedia('(prefers-reduced-motion: reduce)').matches) setCelebrating(true);
      }
    };
    scene.current.upgrade(
      petModel({ ...from, level: from.level + (leveledUp ? 1 : 0) }),
      () => {
        if (lifecycle.current.revision !== currentRevision) return;
        if (leveledUp && !window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
          setSpending(amount);
          setCompleting(true);
          completionTimer.current = setTimeout(finish, 800);
        } else finish();
      },
      (progress) => {
        if (lifecycle.current.revision === currentRevision)
          setSpending(Math.floor(amount * progress));
      }
    );
  };
  const pointerDown = (event: PointerEvent<HTMLButtonElement>) => {
    if (event.button !== 0) return;
    drag.current = { x: event.clientX, y: event.clientY, moved: false };
    event.currentTarget.setPointerCapture(event.pointerId);
  };
  const pointerMove = (event: PointerEvent<HTMLButtonElement>) => {
    const previous = drag.current;
    if (!previous || !event.buttons) return;
    const dx = event.clientX - previous.x;
    const dy = event.clientY - previous.y;
    if (!previous.moved && Math.hypot(dx, dy) < 4) return;
    scene.current?.rotate(dx, dy);
    drag.current = { x: event.clientX, y: event.clientY, moved: true };
  };
  const rotateWithKeys = (event: KeyboardEvent<HTMLButtonElement>) => {
    const moves: Record<string, [number, number]> = {
      ArrowLeft: [-15, 0],
      ArrowRight: [15, 0],
      ArrowUp: [0, -10],
      ArrowDown: [0, 10],
    };
    if (moves[event.key]) {
      event.preventDefault();
      scene.current?.rotate(...moves[event.key]);
    }
  };

  return (
    <section className="pets-main" aria-label={t('developer.pets')}>
      <div className="pets-title">
        <span className="pets-level">Lv.{pet.level}</span>
        <h3>{name}</h3>
        <span>{maxed ? t('pets.maxLevel') : stage}</span>
      </div>
      {active && visible && <PetRewards />}
      <button
        className="pets-stage"
        type="button"
        data-celebrating={celebrating || undefined}
        aria-label={`${name} · Lv.${pet.level} · ${stage} · ${t('pets.interact')}`}
        onPointerDown={pointerDown}
        onPointerMove={pointerMove}
        onPointerCancel={() => {
          drag.current = null;
        }}
        onKeyDown={rotateWithKeys}
        onClick={() => {
          drag.current = null;
        }}
        onAnimationEnd={(event) => {
          if (event.animationName === 'pets-level-pixels') setCelebrating(false);
        }}
      >
        <canvas ref={canvas} role="img" aria-label={`${name} · ${stage}`} />
        {renderState !== 'ready' && (
          <span role={renderState === 'error' ? 'alert' : 'status'}>
            {t(renderState === 'error' ? 'pets.renderError' : 'pets.loading')}
          </span>
        )}
      </button>
      <div className="pets-token-panel">
        <div className="pets-upgrade-row">
          <div
            className="pets-experience"
            data-completing={completing || undefined}
            data-final-level={pet.level === MAX_PET_LEVEL - 1 || undefined}
            role="progressbar"
            aria-label={`${name} · Lv.${pet.level}`}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={Math.floor(progress)}
          >
            <span
              className="quota-progress-fill pets-experience-fill"
              style={{ width: `${progress}%` }}
            />
            <span className="pets-experience-value">{Math.floor(progress)}%</span>
          </div>
          <PageActionButton
            className="pets-upgrade"
            aria-busy={busy}
            disabled={busy || maxed || balance <= 0 || renderState !== 'ready'}
            onClick={upgrade}
          >
            {t(maxed ? 'pets.maxLevel' : 'pets.upgrade')}
          </PageActionButton>
        </div>
        <div className="pets-balance">
          <span>TokenPet</span>
          <TokenPetBalance
            balance={balance}
            spending={spending}
            locale={locale}
            active={active && visible}
          />
        </div>
        <p className="pets-help">{t('pets.balanceHelp')}</p>
      </div>
    </section>
  );
}

function PetCard({ pet }: { pet: PreviewPet }) {
  const { t } = useI18n();
  const selected = usePetsStore((state) => state.selected === pet.id);
  const thumbnail = usePetsStore((state) => state.thumbnails[petThumbnailKey(pet)]);
  const name = petStage(pet.level) < 2 ? t('pets.eggName') : t(`pets.kind.${pet.kind}`);
  const stage = t(`pets.stage.${PET_STAGES[petStage(pet.level)]}`);
  return (
    <button
      type="button"
      className="pets-card"
      aria-pressed={selected}
      onClick={() => usePetsStore.getState().select(pet.id)}
      aria-label={`${name} #${pet.id} · Lv.${pet.level} · ${stage}`}
    >
      <span className="pets-thumbnail">
        {thumbnail ? <img src={thumbnail} alt="" /> : <Egg size={24} />}
      </span>
      <span className="pets-identity">
        <strong>
          {name} #{pet.id}
        </strong>
        <small>{t(`pets.color.${PET_TINTS[pet.color].id}`)}</small>
      </span>
      <span className="pets-status">
        <span>{pet.level >= MAX_PET_LEVEL ? t('pets.maxLevel') : `Lv.${pet.level}`}</span>
      </span>
    </button>
  );
}

export function PetsPanel() {
  const { t } = useI18n();
  const items = usePetsStore((state) => state.items);
  const balance = usePetsStore((state) => state.balance);
  return (
    <section className="pets-list" aria-label={t('developer.pets')}>
      {items.map((pet) => (
        <PetCard key={pet.id} pet={pet} />
      ))}
      <button
        className="pets-card pets-acquire"
        type="button"
        disabled={balance < EGG_COST}
        onClick={() => usePetsStore.getState().acquire()}
      >
        <Plus size={18} />
        <span className="pets-thumbnail">
          <Egg size={24} />
        </span>
        <span className="pets-identity">
          <strong>{t('pets.acquire')}</strong>
          <small>{t('pets.acquireCost')}</small>
        </span>
      </button>
    </section>
  );
}
