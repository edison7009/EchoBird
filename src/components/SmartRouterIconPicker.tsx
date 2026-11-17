import { useEffect, useRef, useState } from 'react';
import { useI18n } from '../hooks/useI18n';
import { SmartRouterIcon, smartRouterIcons } from './SmartRouterIcon';

export function SmartRouterIconPicker({
  value,
  onChange,
}: {
  value?: string;
  onChange: (icon: string) => void;
}) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const selected =
    value && Object.prototype.hasOwnProperty.call(smartRouterIcons, value) ? value : 'waypoints';
  useEffect(() => {
    if (!open) return;
    panel.current?.querySelector<HTMLButtonElement>('[aria-pressed="true"]')?.focus();
    const dismiss = (event: PointerEvent) => {
      if (event.target instanceof Node && !root.current?.contains(event.target)) setOpen(false);
    };
    document.addEventListener('pointerdown', dismiss);
    return () => document.removeEventListener('pointerdown', dismiss);
  }, [open]);
  return (
    <div
      ref={root}
      className="relative"
      onKeyDown={(event) => {
        if (event.key === 'Escape' && open) {
          event.stopPropagation();
          setOpen(false);
          trigger.current?.focus();
        }
      }}
    >
      <button
        ref={trigger}
        type="button"
        aria-label={t('freeModels.router.chooseIcon')}
        aria-expanded={open}
        aria-haspopup="dialog"
        onClick={() => setOpen((current) => !current)}
        className="flex h-7 w-7 items-center justify-center rounded-button text-cyber-accent hover:bg-cyber-accent/15 focus-visible:outline focus-visible:outline-2 focus-visible:outline-cyber-accent"
      >
        <SmartRouterIcon name={value} size={20} />
      </button>
      {open && (
        <div
          ref={panel}
          role="dialog"
          aria-label={t('freeModels.router.chooseIcon')}
          className="app-tooltip absolute left-0 top-full z-20 mt-1 grid w-72 grid-cols-7 gap-1 rounded-md border p-2 shadow-lg"
        >
          {Object.keys(smartRouterIcons).map((name) => (
            <button
              key={name}
              type="button"
              aria-label={name}
              aria-pressed={selected === name}
              onClick={() => {
                onChange(name);
                setOpen(false);
                trigger.current?.focus();
              }}
              className={`flex h-9 w-9 items-center justify-center rounded-button focus-visible:outline focus-visible:outline-2 focus-visible:outline-cyber-accent ${selected === name ? 'bg-cyber-accent/15 text-cyber-accent' : 'text-cyber-text-secondary hover:bg-cyber-elevated hover:text-cyber-text'}`}
            >
              <SmartRouterIcon name={name} size={20} />
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
