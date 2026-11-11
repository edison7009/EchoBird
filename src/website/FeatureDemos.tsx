import { useEffect, useRef, useState } from 'react';
import { Check, Loader2, Sparkles, Network } from 'lucide-react';
import { ModelIdCombobox } from '../components/ModelIdCombobox';
import { ChatBubble, ToolCallCard } from '../components/chat';
import { PET_KINDS, petModel } from '../pages/Pets/petConstants';
import { featureDemoAt } from './featureTimeline';
import { useI18n } from '../hooks/useI18n';
import { featureCopy } from './demoLocale';
function useFeatureCopy() {
  const { locale } = useI18n();
  return featureCopy[locale === 'en' ? 'en' : 'zh-Hans'];
}

// Only the parent website controls playback. These demos never load accounts or install tools.
function useFeatureClock(ticking = true) {
  const [elapsed, setElapsed] = useState(0);
  const [active, setActive] = useState(window.parent === window);
  const [visible, setVisible] = useState(!document.hidden);
  const [reduced, setReduced] = useState(
    () => matchMedia('(prefers-reduced-motion: reduce)').matches
  );
  const clock = useRef(0);
  useEffect(() => {
    const onMessage = (event: MessageEvent) => {
      if (
        event.source === window.parent &&
        event.origin === location.origin &&
        event.data?.type === 'echobird-feature-preview'
      )
        setActive(event.data.visible === true);
    };
    const onVisible = () => setVisible(!document.hidden);
    const preference = matchMedia('(prefers-reduced-motion: reduce)');
    const onPreference = () => setReduced(preference.matches);
    window.addEventListener('message', onMessage);
    document.addEventListener('visibilitychange', onVisible);
    preference.addEventListener('change', onPreference);
    window.parent.postMessage({ type: 'echobird-feature-ready' }, location.origin);
    return () => {
      window.removeEventListener('message', onMessage);
      document.removeEventListener('visibilitychange', onVisible);
      preference.removeEventListener('change', onPreference);
    };
  }, []);
  useEffect(() => {
    if (!ticking || !active || !visible || reduced) return;
    let last = performance.now();
    const timer = window.setInterval(() => {
      const now = performance.now();
      clock.current += now - last;
      last = now;
      setElapsed(clock.current);
    }, 100);
    return () => clearInterval(timer);
  }, [active, visible, reduced, ticking]);
  return { ...featureDemoAt(elapsed), reduced, playing: active && visible && !reduced };
}

const routes = [
  {
    name: 'DeepSeek',
    icon: '/icons/models/deepseek.svg',
    ids: ['deepseek-flash', 'deepseek-chat', 'deepseek-reasoner'],
    free: [],
    keyless: false,
  },
  {
    name: '自定义中转站',
    icon: null,
    ids: ['claude-sonnet', 'gpt-codex', 'gemini-flash'],
    free: [],
    keyless: false,
  },
  {
    name: 'Kilo AI',
    icon: '/icons/tools/kilo.png',
    ids: ['dots-studio/dots-3', 'kimi-k2:free', 'glm:free'],
    free: ['dots-studio/dots-3', 'kimi-k2:free', 'glm:free'],
    keyless: true,
  },
  {
    name: 'OpenCode Zen Free',
    icon: '/icons/models/opencode.svg',
    ids: ['deepseek-v4.1-flash-free', 'big-pickle', 'minimax-free'],
    free: ['deepseek-v4.1-flash-free', 'big-pickle', 'minimax-free'],
    keyless: true,
  },
];
const flowPaths = [
  'M150 355 V212 Q150 200 162 200 H300 V154',
  'M450 355 V212 Q450 200 438 200 H300 V154',
];

export function ModelsDemo() {
  const copy = useFeatureCopy();
  const { playing, route, modelOpen, modelRound, modelSelected, reduced } = useFeatureClock();
  return (
    <div className="feature-models" data-demo-kind="models">
      <div className="route-network" data-playing={playing}>
        <svg
          className="route-network-lines"
          viewBox="0 0 600 470"
          preserveAspectRatio="none"
          fill="none"
          aria-hidden="true"
        >
          <defs>
            <filter id="route-glow">
              <feGaussianBlur stdDeviation="2" />
            </filter>
          </defs>
          {flowPaths.map((path, index) => (
            <g key={path}>
              <path d={path} className="route-wire" />
              <path
                d={path}
                pathLength="100"
                className="route-energy route-energy-glow"
                style={{ animationDelay: `${index * -1.4}s` }}
              />
              <path
                d={path}
                pathLength="100"
                className="route-energy"
                style={{ animationDelay: `${index * -1.4}s` }}
              />
            </g>
          ))}
        </svg>
        <div className="route-demo-hub">
          <div className="route-hub-title">
            <Sparkles size={18} />
            <strong>{copy.smart}</strong>
          </div>
          <div className="route-hub-endpoint">
            <span>OpenAI</span>
            <code>127.0.0.1:53686/v1</code>
          </div>
          <div className="route-hub-endpoint">
            <span>Anthropic</span>
            <code>127.0.0.1:53686</code>
          </div>
        </div>
        <div className="route-demo-grid">
          {routes.map((item, index) => (
            <div
              className="route-demo-card"
              key={item.name}
              data-active={!reduced && route === index}
            >
              <span className="route-demo-priority" aria-hidden="true">
                {index + 1}
              </span>
              {item.icon ? (
                <img src={item.icon} width="34" height="34" alt="" />
              ) : (
                <span className="route-relay-icon">
                  <Network size={29} />
                </span>
              )}
              <div className="route-demo-content">
                <div className="route-demo-name">
                  {item.name === '自定义中转站' ? copy.relay : item.name}
                </div>
                <ModelIdCombobox
                  compact
                  selectOnly
                  showSelectedFreeLabel
                  value={
                    item.ids[
                      (modelRound + (index < route || (index === route && modelSelected) ? 1 : 0)) %
                        item.ids.length
                    ]
                  }
                  options={item.ids}
                  freeModelIds={item.free}
                  freeLabel={copy.free}
                  ariaLabel={`${item.name === '自定义中转站' ? copy.relay : item.name} model ID`}
                  open={!reduced && route === index && modelOpen}
                  onOpenChange={() => {}}
                  onChange={() => {}}
                />
              </div>
              {item.keyless && <span className="route-keyless">{copy.keyless}</span>}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

export function AssistantDemo() {
  const copy = useFeatureCopy();
  const { installStep, reduced } = useFeatureClock();
  const step = reduced ? 6 : installStep;
  const progress = [0, 0, 18, 38, 64, 86, 100][step];
  const status = copy.statuses[step];
  return (
    <div className="assistant-demo" data-demo-kind="assistant" data-install-step={step}>
      <div className="demo-heading">
        <img src="/brand/bird.png" width="23" height="23" alt="" />
        <span>{copy.assistant}</span>
        <span className="demo-meta">EchoBird</span>
      </div>
      <ChatBubble role="user" content={copy.prompt} />
      <ChatBubble role="assistant" content={copy.reply} />
      <div className="install-demo-card">
        <div className="install-demo-tool">
          <img src="/icons/tools/claudecode.svg" width="36" height="36" alt="" />
          <div>
            <strong>Claude Code</strong>
            <span>{copy.tool}</span>
          </div>
          <button
            type="button"
            tabIndex={-1}
            aria-disabled="true"
            className="demo-install-button"
            data-tapping={step === 1}
          >
            {step >= 6 ? (
              <Check size={15} />
            ) : step >= 2 ? (
              <Loader2 size={15} className="animate-spin" />
            ) : null}
            {step >= 6 ? copy.installed : step >= 2 ? copy.installing : copy.install}
          </button>
        </div>
        {step >= 2 && (
          <>
            <div className="install-demo-progress">
              <span style={{ width: `${progress}%` }} />
            </div>
            <div className="install-demo-status">{status}</div>
          </>
        )}
      </div>
      {step >= 2 && (
        <ToolCallCard
          name="shell_exec"
          args={JSON.stringify({ command: status })}
          status={step >= 6 ? 'done' : 'running'}
          output={step >= 6 ? copy.output : undefined}
        />
      )}
      {step >= 6 && (
        <div className="install-demo-complete">
          <Check size={16} />
          <span>{copy.complete}</span>
        </div>
      )}
    </div>
  );
}

export function PetsDemo() {
  const copy = useFeatureCopy();
  const canvas = useRef<HTMLCanvasElement>(null);
  const [images, setImages] = useState<string[]>([]);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let cancelled = false;
    void import('../pages/Pets/petScene.js')
      .then(({ createPetScene }) => {
        if (cancelled || !canvas.current) return;
        const scene = createPetScene(canvas.current);
        try {
          const thumbnails = PET_KINDS.map((kind, index) =>
            scene.thumbnail(petModel({ id: index + 1, kind, level: 60, color: index }))
          );
          if (!cancelled) setImages(thumbnails);
        } finally {
          scene.dispose();
        }
      })
      .catch(() => {
        if (!cancelled) setFailed(true);
      });
    return () => {
      cancelled = true;
    };
  }, []);
  return (
    <div className="pet-demo-line" data-demo-kind="pets">
      <canvas ref={canvas} className="pet-thumbnail-source" aria-hidden="true" />
      {images.map((src, index) => (
        <img src={src} alt={copy.pets[index]} key={PET_KINDS[index]} width="96" height="96" />
      ))}
      {failed && <p className="pet-render-error">{copy.webgl}</p>}
    </div>
  );
}
