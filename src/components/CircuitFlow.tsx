import { useEffect, useRef } from 'react';

/**
 * Clean grid-line light pulse animation.
 * Glowing dots travel along CSS grid lines (H/V only).
 *
 * Flash-aware mode:
 *   Pass `flashCount` to trigger one-shot red pulses — each error in the
 *   chat spawns one red pulse that travels once then disappears.
 *   Background stays ambient green otherwise.
 */

const GRID = 40;
const COLOR_OK: [number, number, number] = [0, 255, 157];    // cyber-accent green
const COLOR_ERR: [number, number, number] = [255, 60, 60];   // error red
const AMBIENT_MIN = 3;
const AMBIENT_MAX = 7;
const SPAWN_EVERY = 30;
const SPEED = 2.5;
const TRAIL = 160;
const HEAD_R = 2.5;
const TARGET_FPS = 30;
const FRAME_MS = 1000 / TARGET_FPS;

export interface CircuitFlowProps {
    /** Accumulated error flash count — each increment triggers one red pulse */
    flashCount?: number;
}

interface Pulse {
    x: number;
    y: number;
    dx: number;
    dy: number;
    life: number;
    color: [number, number, number];
    oneShot?: boolean; // red flash pulses — not replenished
}

export function CircuitFlow({ flashCount = 0 }: CircuitFlowProps = {}) {
    const canvasRef = useRef<HTMLCanvasElement>(null);
    // Queue of pending one-shot red pulses to spawn at next frame
    const pendingRedRef = useRef(0);
    const prevFlashRef = useRef(0);

    // Detect new flashes
    useEffect(() => {
        const diff = flashCount - prevFlashRef.current;
        if (diff > 0) {
            pendingRedRef.current += diff;
        }
        prevFlashRef.current = flashCount;
    }, [flashCount]);

    useEffect(() => {
        const canvas = canvasRef.current;
        if (!canvas) return;
        const ctx = canvas.getContext('2d');
        if (!ctx) return;

        let w = 0, h = 0;
        let pulses: Pulse[] = [];
        let rafId = 0;
        let last = 0;
        let tick = 0;

        let ambientTarget = AMBIENT_MIN + Math.floor(Math.random() * (AMBIENT_MAX - AMBIENT_MIN + 1));

        const resize = () => {
            const p = canvas.parentElement;
            if (!p) return;
            const dpr = window.devicePixelRatio || 1;
            w = p.clientWidth;
            h = p.clientHeight;
            canvas.width = w * dpr;
            canvas.height = h * dpr;
            canvas.style.width = `${w}px`;
            canvas.style.height = `${h}px`;
            ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
        };

        const ro = new ResizeObserver(resize);
        ro.observe(canvas.parentElement!);
        resize();

        const spawnPulse = (color: [number, number, number], oneShot = false) => {
            const horizontal = Math.random() < 0.5;
            if (horizontal) {
                const rows = Math.floor(h / GRID);
                const row = (Math.floor(Math.random() * rows) + 1) * GRID;
                const goRight = Math.random() < 0.5;
                pulses.push({
                    x: goRight ? -TRAIL : w + TRAIL,
                    y: row, dx: goRight ? 1 : -1, dy: 0,
                    life: w + TRAIL * 2, color, oneShot,
                });
            } else {
                const cols = Math.floor(w / GRID);
                const col = (Math.floor(Math.random() * cols) + 1) * GRID;
                const goDown = Math.random() < 0.5;
                pulses.push({
                    x: col,
                    y: goDown ? -TRAIL : h + TRAIL,
                    dx: 0, dy: goDown ? 1 : -1,
                    life: h + TRAIL * 2, color, oneShot,
                });
            }
        };

        const spawn = () => {
            // One-shot red flashes — highest priority, spawn immediately
            if (pendingRedRef.current > 0) {
                spawnPulse(COLOR_ERR, true);
                pendingRedRef.current--;
                return; // one per frame tick to avoid burst
            }

            // Ambient green pulses
            const greenCount = pulses.filter(p => !p.oneShot).length;
            if (greenCount < ambientTarget) {
                spawnPulse(COLOR_OK);
            }
        };

        const drawPulse = (p: Pulse, tick: number) => {
            const [r, g, b] = p.color;
            const tailX = p.x - p.dx * TRAIL;
            const tailY = p.y - p.dy * TRAIL;

            const grad = ctx.createLinearGradient(tailX, tailY, p.x, p.y);
            grad.addColorStop(0, `rgba(${r},${g},${b},0)`);
            grad.addColorStop(0.6, `rgba(${r},${g},${b},0.12)`);
            grad.addColorStop(1, `rgba(${r},${g},${b},0.4)`);

            ctx.beginPath();
            ctx.moveTo(tailX, tailY);
            ctx.lineTo(p.x, p.y);
            ctx.strokeStyle = grad;
            ctx.lineWidth = 1;
            ctx.stroke();

            // Head glow
            ctx.beginPath();
            ctx.arc(p.x, p.y, HEAD_R, 0, Math.PI * 2);
            ctx.fillStyle = `rgba(${r},${g},${b},0.8)`;
            ctx.fill();

            if (p.color === COLOR_ERR) {
                // Red pulse: ripple rings expanding outward
                const phase = (tick % 90) / 90;
                for (let ring = 0; ring < 3; ring++) {
                    const ringPhase = (phase + ring * 0.33) % 1;
                    const radius = 4 + ringPhase * 36;
                    const alpha = 0.45 * (1 - ringPhase);
                    ctx.beginPath();
                    ctx.arc(p.x, p.y, radius, 0, Math.PI * 2);
                    ctx.strokeStyle = `rgba(${r},${g},${b},${alpha})`;
                    ctx.lineWidth = 1.5;
                    ctx.stroke();
                }
                ctx.beginPath();
                ctx.arc(p.x, p.y, 16, 0, Math.PI * 2);
                ctx.fillStyle = `rgba(${r},${g},${b},0.18)`;
                ctx.fill();
            } else {
                ctx.beginPath();
                ctx.arc(p.x, p.y, 6, 0, Math.PI * 2);
                ctx.fillStyle = `rgba(${r},${g},${b},0.12)`;
                ctx.fill();
            }
        };

        const frame = (ts: number) => {
            rafId = requestAnimationFrame(frame);
            if (ts - last < FRAME_MS) return;
            last = ts;
            tick++;

            ctx.clearRect(0, 0, w, h);

            if (tick % 300 === 0) {
                ambientTarget = AMBIENT_MIN + Math.floor(Math.random() * (AMBIENT_MAX - AMBIENT_MIN + 1));
            }

            // Spawn every SPAWN_EVERY ticks, OR immediately if pending red pulses
            if (tick % SPAWN_EVERY === 0 || pendingRedRef.current > 0) spawn();

            for (let i = pulses.length - 1; i >= 0; i--) {
                const p = pulses[i];
                p.x += p.dx * SPEED;
                p.y += p.dy * SPEED;
                p.life -= SPEED;

                if (p.life <= 0) {
                    pulses.splice(i, 1);
                    continue;
                }
                drawPulse(p, tick);
            }
        };

        for (let i = 0; i < 4; i++) spawn();
        rafId = requestAnimationFrame(frame);

        return () => {
            cancelAnimationFrame(rafId);
            ro.disconnect();
        };
    }, []);

    return (
        <canvas
            ref={canvasRef}
            className="absolute inset-0 pointer-events-none"
            style={{ zIndex: -1 }}
        />
    );
}
