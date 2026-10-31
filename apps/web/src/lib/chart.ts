// Geometry for the History chart: pure functions over the API's history points.

export type Span = { from: number; to: number };
export type Run<T> = Span & { value: T };

/**
 * Collapse consecutive points into runs of the same value. A run ends at the next point,
 * or after `maxGap` seconds of missing data.
 */
export function runs<P extends { ts: number }, T>(
  points: P[],
  value: (p: P) => T | null,
  maxGap: number,
): Run<T>[] {
  const out: Run<T>[] = [];
  let cur: Run<T> | null = null;
  for (let i = 0; i < points.length; i++) {
    const p = points[i];
    const next = points[i + 1];
    const end = next && next.ts - p.ts <= maxGap ? next.ts : p.ts + Math.min(maxGap, 60);
    const v = value(p);
    if (cur && v !== null && v === cur.value && p.ts - cur.to <= maxGap) {
      cur.to = end;
    } else {
      if (cur) out.push(cur);
      cur = v === null ? null : { from: p.ts, to: end, value: v };
    }
  }
  if (cur) out.push(cur);
  return out;
}

/** SVG path through the points, breaking the line across gaps in the data. */
export function linePath(
  points: { ts: number; v: number | null }[],
  x: (ts: number) => number,
  y: (v: number) => number,
  maxGap: number,
): string {
  let d = '';
  let prev: number | null = null;
  for (const p of points) {
    if (p.v == null) {
      prev = null;
      continue;
    }
    const cmd = prev !== null && p.ts - prev <= maxGap ? 'L' : 'M';
    d += `${cmd}${x(p.ts).toFixed(1)} ${y(p.v).toFixed(1)} `;
    prev = p.ts;
  }
  return d.trim();
}

/** Round axis bounds out to whole degrees with a little headroom. */
export function yRange(values: number[], pad = 0.5): [number, number] {
  if (!values.length) return [20, 28];
  const lo = Math.floor(Math.min(...values) - pad);
  const hi = Math.ceil(Math.max(...values) + pad);
  return hi - lo < 3 ? [lo - 1, hi + 1] : [lo, hi];
}

export function nearest<P extends { ts: number }>(points: P[], ts: number): P | null {
  let best: P | null = null;
  for (const p of points) if (!best || Math.abs(p.ts - ts) < Math.abs(best.ts - ts)) best = p;
  return best;
}
