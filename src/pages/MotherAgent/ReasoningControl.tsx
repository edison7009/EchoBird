import { useEffect, useRef, useState, type CSSProperties } from 'react';
import type { ReasoningEffort } from '../../api/types';
import { useI18n } from '../../hooks/useI18n';
import { ReasoningEnergy } from './ReasoningEnergy';

interface Props {
  levels: ReasoningEffort[];
  value?: ReasoningEffort;
  disabled: boolean;
  onChange: (effort: ReasoningEffort | undefined) => void;
}

export function ReasoningControl({ levels, value, disabled, onChange }: Props) {
  const { t } = useI18n();
  const [isOpen, setIsOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const choices = [undefined, ...levels];
  const index = Math.max(0, choices.indexOf(value));
  const label = t(value ? `mother.reasoning.${value}` : 'mother.reasoning.default');
  const unavailable = disabled || !levels.length;

  useEffect(() => {
    if (!isOpen) return;
    const outside = (event: MouseEvent) => {
      if (!containerRef.current?.contains(event.target as Node)) setIsOpen(false);
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        setIsOpen(false);
        triggerRef.current?.focus();
      }
    };
    document.addEventListener('mousedown', outside);
    document.addEventListener('keydown', escape);
    return () => {
      document.removeEventListener('mousedown', outside);
      document.removeEventListener('keydown', escape);
    };
  }, [isOpen]);

  return (
    <div
      ref={containerRef}
      className="mother-reasoning-control relative shrink-0"
      data-max={value === 'max' || undefined}
    >
      <button
        ref={triggerRef}
        type="button"
        disabled={unavailable}
        aria-label={`${t('mother.reasoning.title')}: ${label}`}
        aria-expanded={isOpen && !unavailable}
        aria-haspopup="dialog"
        data-open={(isOpen && !unavailable) || undefined}
        onClick={() => !unavailable && setIsOpen((open) => !open)}
        className="composer-tool-trigger mother-reasoning-trigger inline-grid h-8 items-center whitespace-nowrap px-2 py-1 text-xs font-sans rounded-button transition-all disabled:opacity-50"
      >
        {choices.map((effort) => (
          <span
            key={effort || 'default'}
            aria-hidden="true"
            className="invisible col-start-1 row-start-1"
          >
            {t(effort ? `mother.reasoning.${effort}` : 'mother.reasoning.default')}
          </span>
        ))}
        <span className="col-start-1 row-start-1">{label}</span>
      </button>
      {isOpen && !unavailable && (
        <div
          role="dialog"
          aria-label={t('mother.reasoning.title')}
          className="app-tooltip selection-popover mother-reasoning-popup absolute bottom-full mb-2 right-0 z-50 w-[220px] p-3"
        >
          <div className="mb-1 flex items-center justify-between gap-3 text-xs">
            <span className="text-cyber-text-secondary">{t('mother.reasoning.title')}</span>
            <span className="overflow-hidden">
              <span
                key={value || 'default'}
                className="mother-reasoning-label block text-cyber-text"
              >
                {label}
              </span>
            </span>
          </div>
          <div
            className="relative h-8"
            style={
              {
                '--reasoning-progress': `calc(9px + (100% - 18px) * ${index / levels.length})`,
              } as CSSProperties
            }
          >
            <div aria-hidden="true" className="mother-reasoning-track">
              <div className="mother-reasoning-fill" />
              <ReasoningEnergy active={value === 'max'} />
              {value !== 'max' &&
                choices.map((effort, position) => (
                  <span
                    key={effort || 'default'}
                    className="mother-reasoning-stop"
                    style={{
                      left: `calc(9px + (100% - 18px) * ${position / levels.length})`,
                    }}
                  />
                ))}
            </div>
            <input
              type="range"
              min={0}
              max={levels.length}
              step={1}
              value={index}
              autoFocus
              aria-label={t('mother.reasoning.title')}
              aria-valuetext={label}
              onChange={(event) => {
                if (!unavailable) onChange(choices[Number(event.currentTarget.value)]);
              }}
              className="mother-reasoning-slider relative block m-0 w-full"
            />
          </div>
        </div>
      )}
    </div>
  );
}
