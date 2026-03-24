// OverscrollWrap.tsx — iOS-like rubber-band bounce for Android WebView
// Uses transform: translateY() on inner wrapper — compositing-only, zero reflow.
// Native touch listeners for 60fps performance.

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

        let startY = 0;
        let isPulling = false;

        const onStart = (e: TouchEvent) => {
            startY = e.touches[0].clientY;
            isPulling = false;
            // No transition during drag — instant response
            inner.style.transition = 'none';
        };

        const onMove = (e: TouchEvent) => {
            const delta = e.touches[0].clientY - startY;
            const atTop = scroll.scrollTop <= 0;
            const atBot = scroll.scrollHeight - scroll.scrollTop - scroll.clientHeight < 2;

            if (atTop && delta > 6) {
                // Pull down at top — translateY positive
                const d = Math.min(delta * DAMPING, MAX_PX);
                inner.style.transform = `translateY(${d}px)`;
                isPulling = true;
                e.preventDefault();
            } else if (atBot && delta < -6) {
                // Pull up at bottom — translateY negative
                const d = Math.max(delta * DAMPING, -MAX_PX);
                inner.style.transform = `translateY(${d}px)`;
                isPulling = true;
                e.preventDefault();
            } else if (isPulling) {
                inner.style.transform = '';
                isPulling = false;
            }
        };

        const onEnd = () => {
            if (!isPulling) return;
            inner.style.transition = `transform ${SPRING_MS}ms cubic-bezier(0.25,0.46,0.45,0.94)`;
            inner.style.transform = 'translateY(0)';
            isPulling = false;
        };

        scroll.addEventListener('touchstart', onStart, { passive: true });
        scroll.addEventListener('touchmove', onMove, { passive: false });
        scroll.addEventListener('touchend', onEnd, { passive: true });

        return () => {
            scroll.removeEventListener('touchstart', onStart);
            scroll.removeEventListener('touchmove', onMove);
            scroll.removeEventListener('touchend', onEnd);
        };
    }, []);

    return (
        <div ref={scrollRef} className={className} style={style}>
            <div ref={innerRef} style={{ willChange: 'transform', display: 'flex', flexDirection: 'column', gap: 'inherit', minHeight: '100%' }}>
                {children}
            </div>
        </div>
    );
}
