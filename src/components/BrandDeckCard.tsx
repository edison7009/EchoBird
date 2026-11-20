import type { CSSProperties, ReactNode } from 'react';
import { Box, Check, ExternalLink, LoaderCircle, Plus, X } from 'lucide-react';
import { useBrandColour } from '../utils/brandColour';
import './brandCards.css';

/** A stack of deck cards; each tucks its foot under the next. */
export function BrandDeck({ children }: { children: ReactNode }) {
  return <div className="brand-deck">{children}</div>;
}

/**
 * One card of a right-hand directory: the provider's colour, its logo and
 * name. The whole card is the main action (add); the link icon opens the
 * provider's site. `extra` sits between the name and the link (a region
 * choice, "not installed", a countdown).
 */
export function BrandDeckCard({
  region,
  name,
  detail,
  iconSrc,
  label,
  onClick,
  disabled,
  busy,
  off,
  picked,
  pressed,
  addMark,
  extra,
  open,
  cancel,
}: {
  /** Makes the card a labelled region (section) for assistive tech. */
  region?: string;
  name: string;
  detail?: string;
  iconSrc?: string | null;
  /** Accessible name of the main action. */
  label: string;
  onClick: () => void;
  disabled?: boolean;
  /** Waiting on the browser: a light sweeps across the card. */
  busy?: boolean;
  /** Not installed: the card loses its colour. */
  off?: boolean;
  /** Already added: a white check on the card. */
  picked?: boolean;
  /** The main action is a toggle; its state for assistive tech. */
  pressed?: boolean;
  /** Each click adds one more: a quiet plus at the card's end. */
  addMark?: boolean;
  extra?: ReactNode;
  open?: { onClick: () => void; label: string; disabled?: boolean };
  cancel?: { onClick: () => void; label: string };
}) {
  const colour = useBrandColour(iconSrc);
  const style = { '--deck-from': colour.from, '--deck-to': colour.to } as CSSProperties;
  const Root = region ? 'section' : 'div';
  return (
    <Root
      aria-label={region}
      className={`brand-deck-card${busy ? ' is-busy' : ''}${off ? ' is-off' : ''}${picked ? ' is-picked' : ''}`}
      style={style}
    >
      <button
        type="button"
        className="brand-deck-hit"
        aria-label={label}
        aria-busy={busy || undefined}
        aria-pressed={pressed}
        disabled={disabled || busy}
        onClick={onClick}
      />
      {iconSrc ? (
        <img
          src={iconSrc}
          alt=""
          draggable={false}
          className={`brand-deck-icon${colour.glyph ? ' is-glyph' : ''}`}
          onError={(event) => {
            event.currentTarget.style.visibility = 'hidden';
          }}
        />
      ) : (
        <Box size={22} className="brand-deck-icon" aria-hidden="true" />
      )}
      <span className="brand-deck-name">
        {name}
        {detail && <em> · {detail}</em>}
      </span>
      {(extra || busy) && (
        <span className="brand-deck-extra">
          {busy && <LoaderCircle size={13} className="animate-spin" aria-hidden="true" />}
          {extra}
        </span>
      )}
      {addMark && !busy && (
        <span className="brand-deck-addmark" aria-hidden="true">
          <Plus size={16} strokeWidth={2.5} />
        </span>
      )}
      {picked && !busy && (
        <span className="brand-deck-check" aria-hidden="true">
          <Check size={12} strokeWidth={3} />
        </span>
      )}
      {cancel && busy && (
        <button
          type="button"
          className="brand-deck-cancel brand-deck-control"
          aria-label={cancel.label}
          onClick={(event) => {
            event.stopPropagation();
            cancel.onClick();
          }}
        >
          <X size={11} aria-hidden="true" />
        </button>
      )}
      {open && !busy && (
        <button
          type="button"
          className="brand-deck-open brand-deck-control"
          aria-label={open.label}
          disabled={open.disabled ?? false}
          // A sibling of the card's own button, so the click never reaches it.
          onClick={() => open.onClick()}
        >
          <ExternalLink size={16} strokeWidth={2.25} aria-hidden="true" />
        </button>
      )}
    </Root>
  );
}

/** The light "add" card at the top of a deck. */
export function BrandDeckAddCard({ name, onClick }: { name: string; onClick: () => void }) {
  return (
    <div className="brand-deck-card is-add">
      <button type="button" className="brand-deck-hit" aria-label={name} onClick={onClick} />
      <span className="brand-deck-plus" aria-hidden="true">
        <Plus size={14} strokeWidth={3} />
      </span>
      <span className="brand-deck-name">{name}</span>
    </div>
  );
}
