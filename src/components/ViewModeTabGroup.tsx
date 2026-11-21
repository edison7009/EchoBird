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

// The canvas paints the selected tab consistently across desktop webviews.
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
  }, [positionRef]);

  return (
    <div {...props} ref={groupRef}>
      {children}
    </div>
  );
}
