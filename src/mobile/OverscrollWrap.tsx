// OverscrollWrap.tsx — iOS-like rubber-band bounce for Android WebView
// Both top and bottom overscroll: transform inner div only (background stays fixed)

import { useRef, useEffect, CSSProperties } from 'react';

const DAMPING = 0.35;
const MAX_PX = 120;
const SPRING_MS = 280;

interface Props {
    className?: string;
    style?: CSSProperties;
    children: React.ReactNode;
}

export default function OverscrollWrap({ className, style, children }: Props) {
    const scrollRef = useRef<HTMLDivElement>(null);
    const innerRef = useRef<HTMLDivElement>(null);

    useEffect(() => {
        const scroll = scrollRef.current;
        const inner = innerRef.current;
        if (!scroll || !inner) return;

        let touching = false;
        let lastY = 0;
        let anchorY = 0;
        let overscrollDir: 'none' | 'top' | 'bottom' = 'none';

        const isAtTop = () => scroll.scrollTop <= 0;
        const isAtBot = () => scroll.scrollHeight - scroll.scrollTop - scroll.clientHeight < 2;

        const onTouchStart = (e: TouchEvent) => {
            touching = true;
            lastY = e.touches[0].clientY;
            anchorY = lastY;
            overscrollDir = 'none';
            inner.style.transition = 'none';
        };

        const onTouchMove = (e: TouchEvent) => {
            if (!touching) return;
            const curY = e.touches[0].clientY;
            const moveDelta = curY - lastY;
            lastY = curY;

            if (overscrollDir === 'top') {
                const totalDelta = curY - anchorY;
                if (totalDelta <= 0) {
                    inner.style.transform = '';
                    overscrollDir = 'none';
                    return;
                }
                const d = Math.min(totalDelta * DAMPING, MAX_PX);
                inner.style.transform = `translateY(${d}px)`;
                e.preventDefault();
                return;
            }

            if (overscrollDir === 'bottom') {
                const totalDelta = anchorY - curY;
                if (totalDelta <= 0) {
                    inner.style.transform = '';
                    overscrollDir = 'none';
                    return;
                }
                const d = Math.min(totalDelta * DAMPING, MAX_PX);
                // Transform inner div UP — gap appears at bottom within the scroll viewport
                inner.style.transform = `translateY(-${d}px)`;
                e.preventDefault();
                return;
            }

            // Not in overscroll yet — check boundaries
            if (isAtTop() && moveDelta > 0) {
                overscrollDir = 'top';
                anchorY = curY;
            } else if (isAtBot() && moveDelta < 0) {
                overscrollDir = 'bottom';
                anchorY = curY;
            }
        };

        const onTouchEnd = () => {
            touching = false;
            if (overscrollDir !== 'none') {
                inner.style.transition = `transform ${SPRING_MS}ms cubic-bezier(0.25,0.46,0.45,0.94)`;
                inner.style.transform = 'translateY(0)';
                overscrollDir = 'none';
            }
        };

        scroll.addEventListener('touchstart', onStart, { passive: true });
        scroll.addEventListener('touchmove', onTouchMove, { passive: false });
        scroll.addEventListener('touchend', onTouchEnd, { passive: true });
        scroll.addEventListener('touchcancel', onTouchEnd, { passive: true });

        return () => {
            scroll.removeEventListener('touchstart', onStart);
            scroll.removeEventListener('touchmove', onTouchMove);
            scroll.removeEventListener('touchend', onTouchEnd);
            scroll.removeEventListener('touchcancel', onTouchEnd);
        };

        function onStart(e: TouchEvent) { onTouchStart(e); }
    }, []);

    return (
        <div ref={scrollRef} className={className} style={{ ...style, overflowY: 'auto' }}>
            <div ref={innerRef} style={{ willChange: 'transform', display: 'flex', flexDirection: 'column', gap: 'inherit', minHeight: '100%' }}>
                {children}
            </div>
        </div>
    );
}

