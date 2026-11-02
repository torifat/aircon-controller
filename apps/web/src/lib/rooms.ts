// The Rooms section's numbers, from the design handoff: how far each room is from target,
// where that sits on the target bar and sparkline, and which way it's heading.

import { linePath } from './chart';

/** The target bar and the sparkline both span target ±3°. */
export const RANGE = 3;
/** The sparkline's window. */
export const SPARK_HOURS = 2;
/** Sparkline viewBox; the dashed target line runs across the middle. */
export const SPARK_W = 120;
export const SPARK_H = 36;

export type Tone = 'mu' | 'warm' | 'cool';

const f1 = (n: number) => n.toFixed(1);
const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v));

/**
 * How far a room is from target in the direction the AC is working: positive when it
 * still needs cooling (or heating). `gap` is temperature minus target.
 */
export function need(gap: number, heating: boolean): number {
  return heating ? -gap : gap;
}

/** "+0.9°" / "−1.2°" / "on target" within 0.5°, its colour, and the same in words. */
export function delta(gap: number): { text: string; words: string; tone: Tone } {
  const a = Math.abs(gap);
  if (a < 0.5) return { text: 'on target', words: 'on target', tone: 'mu' };
  return {
    text: `${gap > 0 ? '+' : '−'}${f1(a)}°`,
    words: `${f1(a)}° ${gap > 0 ? 'above' : 'below'} target`,
    tone: gap > 0 ? 'warm' : 'cool',
  };
}

/** Where the current temperature sits on the target bar, in percent (target = 50). */
export function barPos(gap: number): number {
  return 50 + clamp(gap / RANGE, -1, 1) * 48;
}

/** Sparkline y for a temperature, in viewBox units: target in the middle, ±3° at the edges. */
export function sparkY(v: number, target: number): number {
  return SPARK_H / 2 - clamp((v - target) / RANGE, -1, 1) * (SPARK_H / 2 - 2);
}

export type Sample = { ts: number; v: number | null };

/**
 * The sparkline path over [now − 2h, now], plus the end dot's y and the rate of change in
 * °/h (null with under ~20 minutes of readings). `samples` end with the current reading.
 */
export function spark(
  samples: Sample[],
  target: number,
  now: number,
  maxGap: number,
): { d: string; endY: number | null; rate: number | null } {
  const from = now - SPARK_HOURS * 3600;
  const pts = samples.filter((p) => p.ts >= from && p.ts <= now);
  const x = (ts: number) => ((ts - from) / (now - from)) * SPARK_W;
  const y = (v: number) => sparkY(v, target);
  const known = pts.filter((p): p is { ts: number; v: number } => p.v != null);
  const first = known[0];
  const last = known.at(-1);
  const span = first && last ? last.ts - first.ts : 0;
  return {
    d: linePath(pts, x, y, maxGap),
    endY: last ? y(last.v) : null,
    rate: first && last && span >= 20 * 60 ? ((last.v - first.v) / span) * 3600 : null,
  };
}

/** Minutes to go, to the nearest 10: "40m", "1h 20m". */
export function eta(hours: number): string {
  const m = Math.max(10, Math.round(hours * 6) * 10);
  const h = Math.floor(m / 60);
  const mm = m % 60;
  return h ? `${h}h${mm ? ` ${mm}m` : ''}` : `${mm}m`;
}

/**
 * The sparkline's caption. `rate` is °/h; heading toward target counts as closing whichever
 * side of it the room is on. Drifting away is the one that wants attention.
 */
export function trend(
  gap: number,
  rate: number | null,
): { text: string; words: string; tone: Tone; strong: boolean } {
  const tone = delta(gap).tone;
  if (rate == null)
    return { text: 'Collecting…', words: 'not enough history yet', tone: 'mu', strong: false };
  const a = Math.abs(gap);
  if (a < 0.5)
    return Math.abs(rate) < 0.1
      ? { text: 'Holding at target', words: 'holding at target', tone: 'mu', strong: false }
      : { text: 'Near target', words: 'near target', tone: 'mu', strong: false };
  const toward = -rate * Math.sign(gap);
  if (toward >= 0.05) {
    const left = eta(a / toward);
    return {
      text: `Closing ${f1(toward)}°/h · ~${left}`,
      words: `closing on target at ${f1(toward)} degrees per hour, about ${left} to go`,
      tone: 'mu',
      strong: false,
    };
  }
  if (toward <= -0.05)
    return {
      text: `Drifting away ${f1(-toward)}°/h`,
      words: `drifting away from target at ${f1(-toward)} degrees per hour`,
      tone,
      strong: true,
    };
  return { text: 'Not moving', words: 'not moving toward target', tone, strong: true };
}
