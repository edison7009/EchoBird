import { useEffect, useRef } from 'react';

/**
 * Clean grid-line light pulse animation.
 * Pulses travel along CSS grid lines (H/V only), appearing organically.
 *
 * Design principles:
 *   - Only 1 pulse spawned at a time, at randomised intervals (organic, not wave-like)
 *   - Max 3 ambient green pulses alive simultaneously
 *   - Red one-shot pulses spawn immediately on chat errors, travel once and disappear
 */

const GRID = 40;
const COLOR_OK: [number, number, number] = [0, 255, 157];
const COLOR_ERR: [number, number, number] = [255, 60, 60];
const AMBIENT_MAX = 3;
const SPAWN_INTERVAL_MIN = 18;  // ticks (min ~0.6s at 30fps)
const SPAWN_INTERVAL_MAX = 45;  // ticks (max ~1.5s)
const SPEED = 2.5;
const TRAIL = 160;
const HEAD_R = 2.5;
const TARGET_FPS = 30;
const FRAME_MS = 1000 / TARGET_FPS;

export interface CircuitFlowProps {
    /** Accumulated error flash count — each increment triggers one one-shot red pulse */
    flashCount?: number;
}

interface Pulse {
    x: number; y: number;
    dx: number; dy: number;
    life: number;
    color: [number, number, number];
    oneShot?: boolean;
}

export function CircuitFlow({ flashCount = 0 }: CircuitFlowProps = {}) {
    const canvasRef = useRef<HTMLCanvasElement>(null);
    const pendingRedRef = useRef(0);
    const prevFlashRef = useRef(0);

    useEffect(() => {
        const diff = flashCount - prevFlashRef.current;
        if (diff > 0) pendingRedRef.current += diff;
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
        // Each pulse waits its own random countdown before spawning next
        let nextSpawnTick = SPAWN_INTERVAL_MIN + Math.floor(Math.random() * (SPAWN_INTERVAL_MAX - SPAWN_INTERVAL_MIN));

        const resize = () => {
            const p = canvas.parentElement;
            if (!p) return;
            const dpr = window.devicePixelRatio || 1;
            w = p.clientWidth; h = p.clientHeight;
            canvas.width = w * dpr; canvas.height = h * dpr;
            canvas.style.width = `${w}px`; canvas.style.height = `${h}px`;
            ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
        };

        const ro = new ResizeObserver(resize);
        ro.observe(canvas.parentElement!);
        resize();

        const spawnPulse = (color: [number, number, number], oneShot = false) => {
            if (w === 0 || h === 0) return;
            const horizontal = Math.random() < 0.5;
            if (horizontal) {
                const rows = Math.floor(h / GRID);
                const row = (Math.floor(Math.random() * rows) + 1) * GRID;
                const goRight = Math.random() < 0.5;
                pulses.push({ x: goRight ? -TRAIL : w + TRAIL, y: row, dx: goRight ? 1 : -1, dy: 0, life: w + TRAIL * 2, color, oneShot });
            } else {
                const cols = Math.floor(w / GRID);
                const col = (Math.floor(Math.random() * cols) + 1) * GRID;
                const goDown = Math.random() < 0.5;
                pulses.push({ x: col, y: goDown ? -TRAIL : h + TRAIL, dx: 0, dy: goDown ? 1 : -1, life: h + TRAIL * 2, color, oneShot });
            }
        };

        const drawPulse = (p: Pulse, t: number) => {
            const [r, g, b] = p.color;
            const tailX = p.x - p.dx * TRAIL, tailY = p.y - p.dy * TRAIL;
            const grad = ctx.createLinearGradient(tailX, tailY, p.x, p.y);
            grad.addColorStop(0, `rgba(${r},${g},${b},0)`);
            grad.addColorStop(0.6, `rgba(${r},${g},${b},0.12)`);
            grad.addColorStop(1, `rgba(${r},${g},${b},0.4)`);
            ctx.beginPath(); ctx.moveTo(tailX, tailY); ctx.lineTo(p.x, p.y);
            ctx.strokeStyle = grad; ctx.lineWidth = 1; ctx.stroke();

            ctx.beginPath(); ctx.arc(p.x, p.y, HEAD_R, 0, Math.PI * 2);
            ctx.fillStyle = `rgba(${r},${g},${b},0.8)`; ctx.fill();

            if (p.color === COLOR_ERR) {
                const phase = (t % 90) / 90;
                for (let ring = 0; ring < 3; ring++) {
                    const rp = (phase + ring * 0.33) % 1;
                    ctx.beginPath(); ctx.arc(p.x, p.y, 4 + rp * 36, 0, Math.PI * 2);
                    ctx.strokeStyle = `rgba(${r},${g},${b},${0.45 * (1 - rp)})`; ctx.lineWidth = 1.5; ctx.stroke();
                }
                ctx.beginPath(); ctx.arc(p.x, p.y, 16, 0, Math.PI * 2);
                ctx.fillStyle = `rgba(${r},${g},${b},0.18)`; ctx.fill();
            } else {
                ctx.beginPath(); ctx.arc(p.x, p.y, 6, 0, Math.PI * 2);
                ctx.fillStyle = `rgba(${r},${g},${b},0.12)`; ctx.fill();
            }
        };

        const frame = (ts: number) => {
            rafId = requestAnimationFrame(frame);
            if (ts - last < FRAME_MS) return;
            last = ts; tick++;
            ctx.clearRect(0, 0, w, h);

            // One-shot red flash pulses — spawn as fast as one per frame
            if (pendingRedRef.current > 0) {
                spawnPulse(COLOR_ERR, true);
                pendingRedRef.current--;
            }

            // Ambient green — one pulse per randomised interval, max 3 alive
            if (tick >= nextSpawnTick) {
                const ambientAlive = pulses.filter(p => !p.oneShot).length;
                if (ambientAlive < AMBIENT_MAX) spawnPulse(COLOR_OK);
                // Always reset timer so next check has fresh interval
                nextSpawnTick = tick + SPAWN_INTERVAL_MIN + Math.floor(Math.random() * (SPAWN_INTERVAL_MAX - SPAWN_INTERVAL_MIN));
            }

            for (let i = pulses.length - 1; i >= 0; i--) {
                const p = pulses[i];
                p.x += p.dx * SPEED; p.y += p.dy * SPEED; p.life -= SPEED;
                if (p.life <= 0) { pulses.splice(i, 1); continue; }
                drawPulse(p, tick);
            }
        };

        // Seed: 2 staggered pulses, not all at once
        spawnPulse(COLOR_OK);
        nextSpawnTick = tick + 12; // second one spawns after ~0.4s

        rafId = requestAnimationFrame(frame);
        return () => { cancelAnimationFrame(rafId); ro.disconnect(); };
    }, []);

    return <canvas ref={canvasRef} className="absolute inset-0 pointer-events-none" style={{ zIndex: -1 }} />;
}
