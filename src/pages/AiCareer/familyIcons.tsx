import { useState } from 'react';
import { CAREER_FAMILIES, type AiCareerFamily } from '../../data/aiCareerFamilies';

export function FamilyIcon({
  family,
  className = 'w-8 h-8',
}: {
  family: AiCareerFamily;
  className?: string;
}) {
  const icon = CAREER_FAMILIES.find((entry) => entry.id === family)!;
  const source = './icons/tools/' + icon.toolId + '.' + icon.format;
  const [fallbackFor, setFallbackFor] = useState<string | null>(null);
  const src = fallbackFor === source ? './icons/tools/' + icon.toolId + '.png' : source;
  return (
    <img
      src={src}
      alt={icon.name}
      className={className + ' flex-shrink-0 object-contain'}
      onError={() => {
        if (src.endsWith('.svg')) setFallbackFor(source);
      }}
    />
  );
}
