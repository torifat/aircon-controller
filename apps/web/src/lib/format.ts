// Display helpers. Times from the API are Unix seconds.

export function deg(v: number | null | undefined, digits = 1): string {
  return v == null ? '—' : `${v.toFixed(digits)}°`;
}

/** "1h 37m", "12m", "45s". */
export function duration(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  return m % 60 ? `${h}h ${m % 60}m` : `${h}h`;
}

/** "0:38", "4:05", "1:02:09". */
export function countdown(seconds: number): string {
  const s = Math.max(0, Math.ceil(seconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const ss = String(s % 60).padStart(2, '0');
  return h ? `${h}:${String(m).padStart(2, '0')}:${ss}` : `${m}:${ss}`;
}

/** "4 s ago", "3 min ago", "2 h ago". */
export function ago(ts: number | null | undefined, now: number): string {
  if (ts == null) return 'never';
  const s = Math.max(0, Math.round(now - ts));
  if (s < 60) return `${s} s ago`;
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  return `${Math.floor(s / 3600)} h ago`;
}

/** Local "14:02". */
export function clock(ts: number): string {
  const d = new Date(ts * 1000);
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
}

/** "Today", "Yesterday", or "Mon 27 Oct". */
export function dayLabel(ts: number, now: number): string {
  const day = (t: number) => {
    const d = new Date(t * 1000);
    return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  };
  const diff = Math.round((day(now) - day(ts)) / 86_400_000);
  if (diff === 0) return 'Today';
  if (diff === 1) return 'Yesterday';
  return new Date(ts * 1000).toLocaleDateString(undefined, {
    weekday: 'short',
    day: 'numeric',
    month: 'short',
  });
}

export function capitalize(s: string): string {
  return s ? s[0].toUpperCase() + s.slice(1) : s;
}

/** AirTouch mode names ("auto_cool") as shown: "Auto". */
export function modeName(mode: string): string {
  return capitalize(mode.split('_')[0]);
}

/**
 * How far a room is from target, as the design's chip says it: "On target" within 0.5°,
 * otherwise "1.4° too warm" / "0.8° too cool". `gap` is temperature minus target.
 */
export function gapChip(gap: number): {
  text: string;
  tone: 'none' | 'warm' | 'cool';
  far: boolean;
} {
  const a = Math.abs(gap);
  if (a < 0.5) return { text: 'On target', tone: 'none', far: false };
  return {
    text: `${a.toFixed(1)}° ${gap > 0 ? 'too warm' : 'too cool'}`,
    tone: gap > 0 ? 'warm' : 'cool',
    far: a >= 1.5,
  };
}
