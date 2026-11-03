import type { CSSProperties } from 'react';
import './IslandScaleSlider.css';

/** Floating-island size slider with a labelled tick for every step. */
export function IslandScaleSlider({
  scales,
  value,
  label,
  disabled = false,
  onChange,
  onCommit,
}: {
  scales: readonly number[];
  value: number;
  label: string;
  disabled?: boolean;
  onChange: (value: number) => void;
  /** Called once a drag or key press ends, for settings that save asynchronously. */
  onCommit?: () => void;
}) {
  const min = scales[0];
  const max = scales[scales.length - 1];
  return (
    <div className="island-scale">
      <input
        type="range"
        min={min}
        max={max}
        step={scales[1] - min}
        value={value}
        disabled={disabled}
        aria-label={label}
        aria-valuetext={`${value}%`}
        className="ui-scale-slider relative block w-full"
        style={
          { '--ui-scale-progress': `${((value - min) / (max - min)) * 100}%` } as CSSProperties
        }
        onChange={(event) => onChange(Number(event.target.value))}
        onPointerUp={onCommit}
        onKeyUp={onCommit}
        onBlur={onCommit}
      />
      <div className="island-scale-ticks" aria-hidden="true">
        {scales.map((scale) => (
          <span
            key={scale}
            data-active={scale === value}
            style={{ '--island-scale-tick': (scale - min) / (max - min) } as CSSProperties}
          >
            {scale}%
          </span>
        ))}
      </div>
    </div>
  );
}
