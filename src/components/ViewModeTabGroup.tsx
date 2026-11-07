import {
  useLayoutEffect,
  useRef,
  type ComponentPropsWithoutRef,
  type MutableRefObject,
} from 'react';
import { mountTabCanvas } from './useTabCanvas';

export interface ViewModeTabPosition {
  x: number;
  y: number;
  width: number;
  height: number;
}

// Every desktop platform paints the selected tab through the same canvas
// owner. The former CSS pseudo-element background flowed too faintly in
// WebView2 to match the macOS result.
export function ViewModeTabGroup({
  children,
  positionRef,
  ...props
}: ComponentPropsWithoutRef<'div'> & {
  positionRef?: MutableRefObject<ViewModeTabPosition | null>;
}) {
  const groupRef = useRef<HTMLDivElement>(null);
  const localPosition = useRef<ViewModeTabPosition | null>(null);

  useLayoutEffect(() => {
    const group = groupRef.current;
    if (!group) return;
    return mountTabCanvas(group, positionRef ?? localPosition);
  }, [children, positionRef]);

  return (
    <div {...props} ref={groupRef}>
      {children}
    </div>
  );
}
