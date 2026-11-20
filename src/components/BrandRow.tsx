import type { CSSProperties, ReactNode } from 'react';
import { useBrandColour } from '../utils/brandColour';
import './brandCards.css';

/**
 * A right-hand list row in its own colour, read from its logo (rows without a
 * logo take the accent): the tint runs in from the left, a chosen row
 * (`is-selected` in className) lights evenly. Black-and-white logos get a
 * quieter slate and are drawn in the text colour where they are a glyph.
 */
export function BrandRow({
  iconSrc,
  disabled = false,
  className,
  style,
  children,
  onClick,
}: {
  iconSrc: string | null;
  disabled?: boolean;
  className: string;
  style?: CSSProperties;
  children: ReactNode;
  onClick?: () => void;
}) {
  const colour = useBrandColour(iconSrc);
  const neutral = !!iconSrc && colour.neutral;
  return (
    <div
      aria-disabled={disabled || undefined}
      onClick={onClick}
      style={
        { '--brand': iconSrc ? colour.from : 'rgb(var(--accent-rgb))', ...style } as CSSProperties
      }
      className={`brand-row${neutral ? ' is-neutral' : ''}${neutral && colour.glyph ? ' is-glyph' : ''} relative overflow-hidden ${className}`}
    >
      {children}
    </div>
  );
}
