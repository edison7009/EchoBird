import type { CSSProperties, ReactNode } from 'react';
import { Box, ExternalLink, LoaderCircle, X } from 'lucide-react';
import { useBrandColour } from '../utils/brandColour';
import './brandCards.css';

/**
 * A directory entry to add from: a deck card in the provider's colour (place
 * it inside a BrandDeck). The card adds; the link icon opens the website.
 * While a login waits, the status and its close button sit on the card.
 */
export function DirectoryRow({
  name,
  url,
  iconSrc,
  secondary,
  onOpen,
  openLabel,
  add,
}: {
  name: string;
  url?: string;
  iconSrc?: string | null;
  secondary?: ReactNode;
  onOpen: () => void;
  openLabel: string;
  add?: {
    onClick: () => void;
    label: string;
    disabled?: boolean;
    busy?: boolean;
    onCancel?: () => void;
    cancelLabel?: string;
  };
}) {
  const colour = useBrandColour(iconSrc);
  const busy = !!add?.busy;
  const canCancel = busy && !!add?.onCancel;
  const style = { '--deck-from': colour.from, '--deck-to': colour.to } as CSSProperties;
  return (
    <div className={`brand-deck-card${busy ? ' is-busy' : ''}`} style={style}>
      {add ? (
        <button
          type="button"
          className="brand-deck-hit"
          onClick={add.onClick}
          aria-label={add.label}
          aria-busy={busy || undefined}
          disabled={add.disabled || busy}
        />
      ) : (
        <button
          type="button"
          className="brand-deck-hit"
          onClick={onOpen}
          aria-label={openLabel}
          disabled={!url}
        />
      )}
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
        {secondary && !canCancel && <em> · {secondary}</em>}
      </span>
      {add && (
        // Last on the card, before it in the markup: the card's own button comes first.
        <button
          type="button"
          className="brand-deck-open brand-deck-control order-last"
          aria-label={openLabel}
          disabled={!url}
          onClick={() => onOpen()}
        >
          <ExternalLink size={16} strokeWidth={2.25} aria-hidden="true" />
        </button>
      )}
      {canCancel && (
        <div className="brand-deck-extra">
          <LoaderCircle size={13} className="animate-spin" aria-hidden="true" />
          <span className="max-w-[9rem] truncate">{secondary}</span>
          <button
            type="button"
            className="brand-deck-cancel brand-deck-control"
            aria-label={add?.cancelLabel}
            onClick={(event) => {
              event.stopPropagation();
              add?.onCancel?.();
            }}
          >
            <X size={11} aria-hidden="true" />
          </button>
        </div>
      )}
    </div>
  );
}
