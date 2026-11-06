import { forwardRef } from 'react';

interface CareerTooltipProps {
  text: string;
  left: number;
  top: number;
  fixed?: boolean;
}

export const CareerTooltip = forwardRef<HTMLDivElement, CareerTooltipProps>(function CareerTooltip(
  { text, left, top, fixed = false },
  ref
) {
  return (
    <div
      ref={ref}
      role="tooltip"
      className={`app-tooltip pointer-events-none ${fixed ? 'fixed' : 'absolute'} z-20 -translate-x-1/2 -translate-y-full rounded-md border px-2 py-1 text-xs whitespace-nowrap shadow-lg`}
      style={{ left, top }}
    >
      {text}
    </div>
  );
});
