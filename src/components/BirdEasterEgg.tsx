import { useEffect, useRef, useState, type RefObject } from 'react';
import { createPortal } from 'react-dom';
import { useDocumentVisible } from '../hooks/useDocumentVisible';
import { useI18n } from '../hooks/useI18n';
import { useEasterEggStore } from '../stores/easterEggStore';
import './BirdEasterEgg.css';

type Phase =
  | 'idle'
  | 'logo-out'
  | 'flight-in'
  | 'home-hover'
  | 'outbound'
  | 'landing'
  | 'standing'
  | 'takeoff'
  | 'inbound'
  | 'return-hover'
  | 'flight-out'
  | 'logo-in';

const steps: Record<Exclude<Phase, 'idle'>, [number, Phase]> = {
  'logo-out': [220, 'flight-in'],
  'flight-in': [280, 'home-hover'],
  'home-hover': [900, 'outbound'],
  outbound: [2400, 'landing'],
  landing: [220, 'standing'],
  standing: [30000, 'takeoff'],
  takeoff: [220, 'inbound'],
  inbound: [2400, 'return-hover'],
  'return-hover': [900, 'flight-out'],
  'flight-out': [280, 'logo-in'],
  'logo-in': [220, 'idle'],
};
const standingClips = [
  ['look', 2000],
  ['preen', 3000],
  ['ruffle', 2000],
] as const;
const assetRoot = '/brand/animations/echobird-';

interface Anchors {
  container: HTMLElement;
  home: { x: number; y: number };
  perch: { x: number; y: number };
}

interface BirdEasterEggProps {
  sidebarRef: RefObject<HTMLElement>;
  perchRef: RefObject<HTMLDivElement>;
}

export function BirdEasterEgg({ sidebarRef, perchRef }: BirdEasterEggProps) {
  const { t } = useI18n();
  const enabled = useEasterEggStore((state) => state.enabled);
  const visible = useDocumentVisible();
  const logoRef = useRef<HTMLButtonElement>(null);
  const [anchors, setAnchors] = useState<Anchors | null>(null);
  const [phase, setPhase] = useState<Phase>('idle');
  const [action, setAction] = useState<{ index: number; ready: boolean } | null>(null);
  const [assetsAvailable, setAssetsAvailable] = useState(true);
  const allowed = enabled && visible && assetsAvailable && anchors !== null;
  const [previousAllowed, setPreviousAllowed] = useState(allowed);
  if (allowed !== previousAllowed) {
    setPreviousAllowed(allowed);
    if (!allowed) {
      setPhase('idle');
      setAction(null);
    }
  }
  const active = allowed && phase !== 'idle';

  useEffect(() => {
    const sidebar = sidebarRef.current;
    const perch = perchRef.current;
    const logo = logoRef.current;
    if (!sidebar || !perch || !logo) return;
    const measure = () => {
      const bounds = sidebar.getBoundingClientRect();
      const home = logo.getBoundingClientRect();
      const line = perch.getBoundingClientRect();
      setAnchors({
        container: sidebar,
        home: {
          x: home.left + home.width / 2 - bounds.left,
          y: home.top + home.height / 2 - bounds.top,
        },
        // The approved standing canvas places the feet at (374, 533) / 640.
        perch: {
          x: line.right - bounds.left - 16 - (54 / 640) * 64,
          y: line.top - bounds.top - (213 / 640) * 64,
        },
      });
    };
    measure();
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(measure);
    observer?.observe(sidebar);
    observer?.observe(perch);
    observer?.observe(logo);
    window.addEventListener('resize', measure);
    return () => {
      observer?.disconnect();
      window.removeEventListener('resize', measure);
    };
  }, [sidebarRef, perchRef]);

  useEffect(() => {
    if (!allowed) return;
    if (phase === 'idle') {
      const timer = setTimeout(() => setPhase('logo-out'), (30 + Math.random() * 30) * 60000);
      return () => clearTimeout(timer);
    }
    const [duration, next] = steps[phase];
    const timer = setTimeout(() => {
      if (next === 'landing' || next === 'inbound') setAction(null);
      if (next === 'takeoff') setAction((current) => (current?.ready ? current : null));
      setPhase(next);
    }, duration);
    return () => clearTimeout(timer);
  }, [allowed, phase]);

  useEffect(() => {
    if (!allowed || phase !== 'standing') return;
    let timer: ReturnType<typeof setTimeout>;
    let remaining: number[] = [];
    const play = () => {
      if (!remaining.length) remaining = [0, 1, 2];
      const next = remaining.splice(Math.floor(Math.random() * remaining.length), 1)[0];
      setAction({ index: next, ready: false });
      timer = setTimeout(rest, standingClips[next][1]);
    };
    const rest = () => {
      setAction(null);
      timer = setTimeout(play, 600 + Math.random() * 900);
    };
    rest();
    return () => clearTimeout(timer);
  }, [allowed, phase]);

  const atPerch = ['outbound', 'landing', 'standing', 'takeoff'].includes(phase);
  const returning = ['takeoff', 'inbound', 'return-hover', 'flight-out'].includes(phase);
  const flying = [
    'flight-in',
    'home-hover',
    'outbound',
    'takeoff',
    'inbound',
    'return-hover',
  ].includes(phase);
  const standing = phase === 'landing' || phase === 'standing';
  const moving = phase === 'outbound' || phase === 'inbound';
  const position = anchors && (atPerch ? anchors.perch : anchors.home);
  const assetError = () => setAssetsAvailable(false);
  const actionLoaded = (image: HTMLImageElement) => {
    const requested = action;
    if (!requested || requested.ready) return;
    void image
      .decode()
      .then(() => {
        if (image.isConnected) {
          setAction((current) => (current === requested ? { ...current, ready: true } : current));
        }
      })
      .catch(() => {
        if (image.isConnected) assetError();
      });
  };

  return (
    <>
      <button
        ref={logoRef}
        type="button"
        className="bird-easter-egg-logo flex-shrink-0 h-7 w-7 select-none"
        aria-label={t('app.name')}
        disabled={!allowed || active}
        onClick={() => anchors && setPhase('logo-out')}
      >
        <img
          src="/brand/bird.png"
          alt=""
          aria-hidden="true"
          draggable={false}
          className="h-7 w-7"
          style={{ opacity: !active || phase === 'logo-in' ? 1 : 0 }}
        />
      </button>
      {active &&
        position &&
        anchors &&
        createPortal(
          <div
            className="bird-easter-egg-overlay"
            data-phase={phase}
            aria-hidden={phase !== 'standing'}
          >
            <div
              className="bird-easter-egg-x"
              style={{
                transform: `translateX(${position.x}px)`,
                transitionDuration: moving ? '2400ms' : '0ms',
              }}
            >
              <div
                className="bird-easter-egg-y"
                style={{
                  transform: `translateY(${position.y}px)`,
                  transitionDuration: moving ? '2400ms' : '0ms',
                  transitionTimingFunction: returning
                    ? 'cubic-bezier(.1,.45,.4,1)'
                    : 'cubic-bezier(.45,0,.9,.55)',
                }}
              >
                <button
                  type="button"
                  className="bird-easter-egg-bird"
                  aria-label={t('app.name')}
                  disabled={phase !== 'standing'}
                  tabIndex={phase === 'standing' ? 0 : -1}
                  onClick={() => {
                    setAction((current) => (current?.ready ? current : null));
                    setPhase('takeoff');
                  }}
                  style={{
                    width: atPerch ? 64 : 46,
                    height: atPerch ? 64 : 46,
                    pointerEvents: phase === 'standing' ? 'auto' : 'none',
                    // Exclude transparent canvas margins from the perched hit area.
                    clipPath: phase === 'standing' ? 'inset(30% 16% 12% 20%)' : undefined,
                  }}
                >
                  <img
                    src={`${assetRoot}flight-loop.webp`}
                    alt=""
                    draggable={false}
                    onError={assetError}
                    style={{
                      opacity: flying ? 1 : 0,
                      transform: returning ? 'scaleX(-1)' : undefined,
                    }}
                  />
                  <div className="bird-easter-egg-standing" style={{ opacity: standing ? 1 : 0 }}>
                    <img
                      src={`${assetRoot}standing-idle.png`}
                      alt=""
                      draggable={false}
                      onError={assetError}
                      style={{ opacity: action?.ready ? 0 : 1 }}
                    />
                    {action && (
                      <img
                        key={action.index}
                        src={`${assetRoot}standing-${standingClips[action.index][0]}.webp`}
                        alt=""
                        draggable={false}
                        onError={assetError}
                        onLoad={(event) => actionLoaded(event.currentTarget)}
                        style={{ opacity: action.ready ? 1 : 0 }}
                      />
                    )}
                  </div>
                </button>
              </div>
            </div>
          </div>,
          anchors.container
        )}
    </>
  );
}
