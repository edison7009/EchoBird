import { useEffect, useRef } from 'react';

/**
 * Clean grid-line light pulse animation.
 * Glowing dots travel along the existing CSS grid lines (H/V only),
 * never crossing. Creates a calm, structured cyberpunk aesthetic.
 *
 * Health-aware mode:
 *   Pass `channels` prop with error count to drive red pulse spawning.
 *   - 0 errors → ambient green only (3–7 pulses)
 *   - 1–7 errors → matching red pulses added alongside green
 *   - 8+ errors → ALL pulses turn red (full-screen alarm)
 */

const GRID = 40;
const COLOR_OK: [number, number, number] = [0, 255, 157];    // cyber-accent green
const COLOR_ERR: [number, number, number] = [255, 60, 60];   // error red
const AMBIENT_MIN = 3;
const AMBIENT_MAX = 7;
const MAX_ERR_PULSES = 7;
const SPAWN_EVERY = 30;
const SPEED = 2.5;
const TRAIL = 160;
const HEAD_R = 2.5;
const TARGET_FPS = 30;
const FRAME_MS = 1000 / TARGET_FPS;

// ── Public interface ──────────────────────────────────────────
export interface ChannelPulseStatus {
    id: number;
    status: 'ok' | 'error';
}

export interface CircuitFlowProps {
    /** Channel status list — error channels drive red pulse count.
     *  undefined/empty = default ambient mode (random green pulses). */
    channels?: ChannelPulseStatus[];
}

// ── Internal types ────────────────────────────────────────────
interface Pulse {
    x: number;
    y: number;
    dx: number;
    dy: number;
    life: number;
    color: [number, number, number];
}

export function CircuitFlow({ channels }: CircuitFlowProps = {}) {
    const canvasRef = useRef<HTMLCanvasElement>(null);
    const channelsRef = useRef(channels);
    channelsRef.current = channels;

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

        // Random ambient target (3–7), recalculated occasionally
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

        const spawnPulse = (color: [number, number, number]) => {
            const horizontal = Math.random() < 0.5;
            if (horizontal) {
                const rows = Math.floor(h / GRID);
                const row = (Math.floor(Math.random() * rows) + 1) * GRID;
                const goRight = Math.random() < 0.5;
                pulses.push({
                    x: goRight ? -TRAIL : w + TRAIL,
                    y: row, dx: goRight ? 1 : -1, dy: 0,
                    life: w + TRAIL * 2, color,
                });
            } else {
                const cols = Math.floor(w / GRID);
                const col = (Math.floor(Math.random() * cols) + 1) * GRID;
                const goDown = Math.random() < 0.5;
                pulses.push({
                    x: col,
                    y: goDown ? -TRAIL : h + TRAIL,
                    dx: 0, dy: goDown ? 1 : -1,
                    life: h + TRAIL * 2, color,
                });
            }
        };

        /** Spawn logic — ambient green + error red based on channel health */
        const spawn = () => {
            const chs = channelsRef.current;
            const errorCount = chs ? chs.filter(c => c.status === 'error').length : 0;
            const fullAlarm = errorCount >= MAX_ERR_PULSES; // 7+ errors → all 7 red

            if (fullAlarm) {
                // Full alarm: up to 7 red pulses, no green
                if (pulses.length < MAX_ERR_PULSES) {
                    spawnPulse(COLOR_ERR);
                }
            } else {
                // Count current green/red pulses
                const greenCount = pulses.filter(p => p.color === COLOR_OK).length;
                const redCount = pulses.filter(p => p.color === COLOR_ERR).length;

                // Spawn green ambient pulses (3–7)
                if (greenCount < ambientTarget) {
                    spawnPulse(COLOR_OK);
                }
                // Spawn red error pulses (match error count, max 7)
                if (errorCount > 0 && redCount < Math.min(errorCount, MAX_ERR_PULSES)) {
                    spawnPulse(COLOR_ERR);
                }
            }
        };

        const drawPulse = (p: Pulse) => {
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

            // Soft outer glow
            ctx.beginPath();
            ctx.arc(p.x, p.y, 6, 0, Math.PI * 2);
            ctx.fillStyle = `rgba(${r},${g},${b},0.12)`;
            ctx.fill();
        };

        const frame = (ts: number) => {
            rafId = requestAnimationFrame(frame);
            if (ts - last < FRAME_MS) return;
            last = ts;
            tick++;

            ctx.clearRect(0, 0, w, h);

            // Occasionally shift ambient target (organic feel)
            if (tick % 300 === 0) {
                ambientTarget = AMBIENT_MIN + Math.floor(Math.random() * (AMBIENT_MAX - AMBIENT_MIN + 1));
            }

            if (tick % SPAWN_EVERY === 0) spawn();

            for (let i = pulses.length - 1; i >= 0; i--) {
                const p = pulses[i];
                p.x += p.dx * SPEED;
                p.y += p.dy * SPEED;
                p.life -= SPEED;

                if (p.life <= 0) {
                    pulses.splice(i, 1);
                    continue;
                }
                drawPulse(p);
            }
        };

        // Seed initial pulses
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
