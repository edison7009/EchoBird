import type { ReactNode } from 'react';
import { ArrowRight } from 'lucide-react';
import './brandCards.css';

/**
 * An empty list: a small fanned stack of cards (the shape the list will take)
 * with the page's icon on the front card, a title, and a hint pointing to
 * the directory on the right where entries are added.
 */
export function BrandEmpty({
  icon,
  title,
  hint,
  className = '',
}: {
  icon: ReactNode;
  title: string;
  hint?: string;
  className?: string;
}) {
  return (
    <div className={`brand-empty ${className}`}>
      <div className="brand-empty-stack" aria-hidden="true">
        <span />
        <span />
        <span className="brand-empty-front">{icon}</span>
      </div>
      <div className="brand-empty-title">{title}</div>
      {hint && (
        <div className="brand-empty-hint">
          {hint}
          <ArrowRight size={14} strokeWidth={2.5} aria-hidden="true" />
        </div>
      )}
    </div>
  );
}
