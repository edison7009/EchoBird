import type { MutableRefObject } from 'react';
import type { TerminalSegment } from '../../api/routerTerminal';
import { terminalDemoRow } from './terminalDemo';

export interface TerminalPlayback {
  group: string;
  generation: number;
  realtime: boolean;
  protection: boolean;
}

const FONT_SIZE = 18;
const LINE_HEIGHT = 30;
const INTERVAL = 560;
const MAX_QUEUED_ROWS = 4;
const MAX_QUEUE_AGE = 2000;
// Matches source/protection categories used by RouterStatistics.
const COLORS = ['#8bb9dc', '#8dbda7', '#b9a7e5', '#d0b27f', '#aca8a0'];

function displayToken(token: string) {
  return token.replace(
    /^(\{\{SECRET_)([a-f0-9]{32})(\}\})$/i,
    (_, prefix, hash, suffix) => `${prefix}${hash.slice(0, 4)}…${suffix}`
  );
}

interface Chunk extends TerminalSegment {
  width: number;
}
interface Row {
  chunks: Chunk[];
  seed: number;
  receivedAt: number;
  flipAt?: number;
}

export function animateTerminal(
  canvas: HTMLCanvasElement,
  pending: MutableRefObject<TerminalSegment[][]>,
  protectedLabel: string,
  waiting: string,
  playback: MutableRefObject<TerminalPlayback>
) {
  const ctx = canvas.getContext('2d');
  if (!ctx) return;
  let width = 0,
    height = 0,
    frame = 0,
    elapsed = 0,
    previous = 0,
    next = 0;
  let generation = playback.current.generation;
  const rows: (Row | null)[] = [];
  const queued: Row[] = [];
  const resize = () => {
    const bounds = canvas.getBoundingClientRect();
    width = bounds.width;
    height = bounds.height;
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.font = `${FONT_SIZE}px Consolas, "Microsoft YaHei", monospace`;
    ctx.textBaseline = 'middle';
  };
  const observer = new ResizeObserver(resize);
  observer.observe(canvas);
  resize();
  const wrap = (segments: TerminalSegment[], receivedAt: number) => {
    let chunks: Chunk[] = [],
      used = 0;
    const flush = () => {
      if (chunks.length) queued.push({ chunks, seed: Math.random() * 1000, receivedAt });
      chunks = [];
      used = 0;
    };
    for (const segment of segments) {
      if (segment.protectedText) {
        const size = Math.min(
          width,
          Math.max(
            ctx.measureText(segment.text).width,
            ctx.measureText(displayToken(segment.protectedText)).width
          ) +
            ctx.measureText(protectedLabel).width +
            14
        );
        if (used + size > width) flush();
        chunks.push({ ...segment, width: size });
        used += size;
      } else {
        for (const ch of segment.text) {
          const size = ctx.measureText(ch).width;
          if (used + size > width) flush();
          const last = chunks[chunks.length - 1];
          if (last && !last.protectedText) {
            last.text += ch;
            last.width += size;
          } else chunks.push({ text: ch, protectedText: null, kind: null, width: size });
          used += size;
        }
      }
    }
    flush();
  };
  const draw = (now: number) => {
    frame = requestAnimationFrame(draw);
    elapsed += Math.min(50, now - (previous || now));
    previous = now;
    if (generation !== playback.current.generation) {
      queued.length = 0;
      generation = playback.current.generation;
    }
    const incoming = pending.current.splice(0);
    if (incoming.length) {
      // This is a live sample, so each poll supersedes undisplayed older input.
      queued.length = 0;
      const overloaded = incoming.length > MAX_QUEUED_ROWS;
      const highlights = overloaded
        ? incoming.filter((segments) => segments.some((s) => s.protectedText))
        : [];
      const selected = (highlights.length ? highlights : incoming).slice(-MAX_QUEUED_ROWS);
      for (const segments of overloaded ? selected.reverse() : selected) wrap(segments, now);
      if (overloaded || queued.length > MAX_QUEUED_ROWS) {
        const important = queued.filter((row) => row.chunks.some((chunk) => chunk.protectedText));
        const sample = important.length ? important : queued;
        queued.splice(0, queued.length, ...sample.slice(0, MAX_QUEUED_ROWS));
      }
    }
    while (queued.length && now - queued[0].receivedAt >= MAX_QUEUE_AGE) queued.shift();
    if (elapsed >= next) {
      next = elapsed + INTERVAL;
      if (!playback.current.realtime && !queued.length) wrap(terminalDemoRow(), now);
      rows.push(queued.shift() ?? null);
      if (rows.length > Math.ceil(height / LINE_HEIGHT) + 4) rows.shift();
    }
    // Repaint the frame as transparent: the screen behind the canvas carries
    // the CRT glass (glow, vignette, scanlines) in CSS.
    ctx.save();
    ctx.globalCompositeOperation = 'copy';
    ctx.fillStyle = 'rgba(0, 0, 0, 0)';
    ctx.fillRect(0, 0, width, height);
    ctx.restore();
    const scroll = Math.min(1, (elapsed - (next - INTERVAL)) / INTERVAL) * LINE_HEIGHT;
    rows.forEach((row, index) => {
      if (!row) return;
      const y = height - 9 - (rows.length - 1 - index) * LINE_HEIGHT - scroll;
      if (y < -LINE_HEIGHT) return;
      const settle = Math.max(0, Math.min(1, (height - y) / (height * 0.3)));
      if (!playback.current.protection) row.flipAt = undefined;
      else if (y <= height * 0.5 && row.flipAt === undefined) row.flipAt = elapsed;
      const flip = row.flipAt === undefined ? 0 : Math.min(1, (elapsed - row.flipAt) / 850);
      let x = 0;
      for (const chunk of row.chunks) {
        const tagged = chunk.protectedText !== null;
        const color = tagged ? COLORS[chunk.kind ?? 4] : '#397461';
        let text = tagged && flip >= 0.5 ? displayToken(chunk.protectedText!) : chunk.text;
        if (tagged && flip > 0 && flip < 1) {
          const target = displayToken(chunk.protectedText!);
          const letters = Array.from(flip < 0.4 ? chunk.text : target);
          const chars = '01ABCDEF▒░';
          const settled = Math.max(0, (flip - 0.4) / 0.6);
          text = letters
            .map((ch, i) =>
              i < letters.length * settled
                ? ch
                : chars[Math.floor(elapsed / 48 + i * 7 + row.seed) % chars.length]
            )
            .join('');
        }
        ctx.save();
        if (tagged && settle > 0.75) {
          ctx.globalAlpha = 0.85 * (0.18 + 0.2 * Math.sin(flip * Math.PI));
          ctx.fillStyle = color;
          ctx.fillRect(x, y - 11, chunk.width, 22);
          ctx.globalAlpha = 0.5;
          ctx.strokeStyle = color;
          ctx.lineWidth = 0.5;
          ctx.strokeRect(x, y - 11, chunk.width, 22);
        }
        ctx.beginPath();
        ctx.rect(x, y - 14, chunk.width, 28);
        ctx.clip();
        ctx.fillStyle = color;
        ctx.globalAlpha = 0.85;
        if (settle < 1) {
          const edge = settle * (width + 120) - 60 + (row.seed % 5) * 24;
          let cx = x;
          for (const ch of text) {
            const cw = ctx.measureText(ch).width,
              distance = edge - cx;
            if (distance > 0) {
              ctx.globalAlpha = 0.85 * Math.min(1, distance / 35);
              ctx.fillStyle = color;
              ctx.fillText(ch, cx, y);
            }
            if (distance >= -cw && distance < cw && ch !== ' ') {
              ctx.globalAlpha = 0.76;
              ctx.fillStyle = '#81d5a6';
              ctx.fillRect(cx, y - 8, Math.max(3, cw - 2), 16);
            }
            cx += cw;
          }
        } else {
          const markerWidth = tagged && flip === 1 ? ctx.measureText(protectedLabel).width + 4 : 0;
          const available = chunk.width - markerWidth;
          let shown = text;
          while (shown.length > 1 && ctx.measureText(shown).width > available)
            shown = Array.from(shown).slice(0, -1).join('');
          if (shown !== text) shown = Array.from(shown).slice(0, -1).join('') + '…';
          ctx.fillText(shown, x, y);
          if (markerWidth) {
            ctx.fillStyle = '#82d9a0';
            ctx.fillText(protectedLabel, x + ctx.measureText(shown).width + 2, y);
          }
        }
        ctx.restore();
        x += chunk.width;
      }
    });
    if (!rows.some(Boolean) && !queued.length) {
      ctx.fillStyle = '#577765';
      ctx.font = '12px Consolas, "Microsoft YaHei", monospace';
      ctx.fillText(waiting, 0, 26);
      ctx.font = `${FONT_SIZE}px Consolas, "Microsoft YaHei", monospace`;
    }
  };
  frame = requestAnimationFrame(draw);
  return () => {
    cancelAnimationFrame(frame);
    observer.disconnect();
    pending.current = [];
    rows.length = 0;
    queued.length = 0;
    ctx.clearRect(0, 0, width, height);
  };
}
