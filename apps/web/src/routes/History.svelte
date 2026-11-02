<script lang="ts">
  import type { ActivityItem, HistoryResponse } from '@aircon/api';

  import { api } from '../lib/api';
  import { linePath, nearest, runs, yRange } from '../lib/chart';
  import { capitalize, clock, dayLabel, deg } from '../lib/format';
  import { live } from '../lib/live.svelte';

  const W = 1000;
  const H = 300;

  let hours = $state<24 | 168>(24);
  // Room tiles on Home link here with `?zone=N`.
  const linked = new URLSearchParams(location.hash.split('?')[1] ?? '').get('zone');
  let zoneNo = $state<number | null>(linked != null ? Number(linked) : null);
  let data = $state<HistoryResponse | null>(null);
  let decisions = $state<ActivityItem[]>([]);
  let selected = $state<number | null>(null);
  let error = $state<string | null>(null);

  async function load(h: number) {
    try {
      const [hist, act] = await Promise.all([
        api.history(h),
        api.activity({ kind: 'decision', limit: 500 }),
      ]);
      data = hist;
      decisions = act.items;
      error = null;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  $effect(() => {
    void load(hours);
  });

  // Default to the room that's driving decisions, else the first with data.
  const zones = $derived(data?.zones ?? []);
  const zone = $derived(
    zones.find((z) => z.number === zoneNo) ??
      zones.find((z) => z.number === live.state?.zones.find((s) => s.driving)?.number) ??
      zones[0],
  );
  const maxGap = $derived((data?.step ?? 60) * 3);

  const view = $derived.by(() => {
    if (!data || !zone) return null;
    const { from, to } = data;
    const x = (ts: number) => ((ts - from) / (to - from)) * W;
    const values = zone.points.flatMap((p) => [p.temperature, p.target]).filter((v) => v != null);
    const [lo, hi] = yRange(values as number[]);
    const y = (v: number) => H - ((v - lo) / (hi - lo)) * H;
    const temps = zone.points.map((p) => ({ ts: p.ts, v: p.temperature }));
    const onTemps = zone.points.map((p) => ({ ts: p.ts, v: p.on ? p.temperature : null }));
    const targets = zone.points.map((p) => ({ ts: p.ts, v: p.target }));
    const pct = (s: { from: number; to: number }) => ({
      left: `${((s.from - from) / (to - from)) * 100}%`,
      width: `${((s.to - s.from) / (to - from)) * 100}%`,
    });
    const acOn = runs(data.ac, (p) => (p.on ? 'on' : null), maxGap).map(pct);
    const fan = runs(data.ac, (p) => (p.on ? p.fan : null), maxGap).map((r) => ({ ...pct(r), fan: r.value }));
    const roomOn = runs(zone.points, (p) => (p.on ? 'on' : null), maxGap).map(pct);
    const grid = [];
    for (let v = Math.ceil(lo); v <= hi; v += hi - lo > 6 ? 2 : 1) grid.push({ v, top: `${(y(v) / H) * 100}%` });
    const ticks =
      hours === 24
        ? [0, 6, 12, 18, 24].map((h) => ({ ts: from + h * 3600, text: h === 24 ? 'Now' : clock(from + h * 3600) }))
        : Array.from({ length: 7 }, (_, i) => {
            const ts = from + (i + 0.5) * 86400;
            return { ts, text: new Date(ts * 1000).toLocaleDateString(undefined, { weekday: 'short' }) };
          });
    return {
      x,
      y,
      temp: linePath(temps, x, y, maxGap),
      on: linePath(onTemps, x, y, maxGap),
      target: linePath(targets, x, y, maxGap),
      acOn,
      fan,
      roomOn,
      grid,
      ticks: ticks.map((t) => ({ ...t, left: `${(x(t.ts) / W) * 100}%` })),
    };
  });

  const point = $derived.by(() => {
    if (!zone) return null;
    const pts = zone.points;
    return selected == null ? (pts.at(-1) ?? null) : nearest(pts, selected);
  });
  const acAt = $derived(point && data ? nearest(data.ac, point.ts) : null);
  const decision = $derived(
    point ? decisions.find((d) => d.ts <= point.ts + 30 && d.ts > point.ts - 6 * 3600) : undefined,
  );

  function pick(e: PointerEvent) {
    if (!data) return;
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const f = Math.min(1, Math.max(0, (e.clientX - r.left) / r.width));
    selected = data.from + f * (data.to - data.from);
  }

  function step(dir: -1 | 1) {
    if (!zone || !point) return;
    const i = zone.points.indexOf(point) + dir;
    if (zone.points[i]) selected = zone.points[i].ts;
  }

  const fanOpacity: Record<string, number> = { high: 1, medium: 0.6, low: 0.3, auto: 0.45 };
</script>

<header class="flex items-center justify-between px-5 pt-5 pb-2 md:px-0">
  <h1 class="m-0 text-[28px] font-semibold tracking-[-0.02em]">History</h1>
  <div role="group" aria-label="Range" class="flex rounded-xl bg-s2 p-[3px]">
    {#each [[24, '24h'], [168, '7d']] as const as [h, label] (h)}
      <button
        type="button"
        aria-pressed={hours === h}
        onclick={() => ((hours = h), (selected = null))}
        class={[
          'min-h-9 min-w-[52px] rounded-[9px] text-[13px] font-semibold',
          hours === h ? 'bg-s1 text-tx shadow-[0_1px_2px_rgb(0_0_0/0.25)]' : 'text-mu',
        ]}>{label}</button
      >
    {/each}
  </div>
</header>

<div role="group" aria-label="Room" class="flex flex-wrap gap-2 px-4 pt-2 pb-4 md:px-0">
  {#each live.state?.zones ?? [] as z (z.number)}
    {@const on = zone?.number === z.number}
    <button
      type="button"
      aria-pressed={on}
      onclick={() => ((zoneNo = z.number), (selected = null))}
      class={[
        'min-h-9 rounded-full border px-3.5 text-[13px] font-semibold',
        on ? 'border-tx bg-tx text-bg' : 'border-line text-mu',
      ]}>{z.name}</button
    >
  {/each}
</div>

<div class="flex flex-col gap-3 px-4 md:grid md:grid-cols-[minmax(0,2fr)_minmax(0,1fr)] md:items-start md:px-0">
  <section aria-label="Temperature chart" class="flex flex-col gap-3.5 rounded-[20px] bg-s1 p-4">
    {#if error}
      <p class="m-0 text-sm text-err">{error}</p>
    {:else if !view || !zone}
      <p class="m-0 py-16 text-center text-sm text-mu">{data ? 'No readings yet.' : 'Loading…'}</p>
    {:else}
      <div class="flex items-baseline justify-between">
        <div class="flex flex-col gap-0.5">
          <span class="text-base font-semibold">{zone.name}</span>
          <span class="text-xs text-mu">
            Target {deg(zone.points.at(-1)?.target)} · last {hours === 24 ? '24 hours' : '7 days'}
          </span>
        </div>
        <div class="flex flex-col items-end gap-0.5">
          <span class="text-[28px] leading-none font-semibold tracking-[-0.02em]">
            {deg(zone.points.at(-1)?.temperature)}
          </span>
          <span class="text-xs text-mu">now</span>
        </div>
      </div>

      <!-- Pointer picks a moment; arrow keys step through readings. -->
      <div
        role="slider"
        tabindex="0"
        aria-label="Reading at"
        aria-valuemin={data!.from}
        aria-valuemax={data!.to}
        aria-valuenow={point?.ts ?? data!.to}
        aria-valuetext={point ? `${clock(point.ts)}, ${deg(point.temperature)}` : ''}
        onpointerdown={pick}
        onpointermove={(e) => e.buttons && pick(e)}
        onkeydown={(e) => {
          if (e.key === 'ArrowLeft') step(-1);
          if (e.key === 'ArrowRight') step(1);
        }}
        class="relative h-[190px] cursor-crosshair touch-none select-none md:h-[260px]"
      >
        {#each view.acOn as b, i (i)}
          <span class="absolute inset-y-0 bg-cool opacity-[0.07]" style:left={b.left} style:width={b.width}></span>
        {/each}
        {#each view.grid as g (g.v)}
          <span class="absolute inset-x-0 border-t border-line" style:top={g.top}></span>
          <span class="absolute left-0 -translate-y-full text-[10px] text-mu" style:top={g.top}>{g.v}°</span>
        {/each}
        <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" class="absolute inset-0 size-full overflow-visible" aria-hidden="true">
          <path d={view.target} fill="none" stroke="var(--tx)" stroke-opacity="0.5" stroke-width="1.25" stroke-dasharray="4 4" vector-effect="non-scaling-stroke" />
          <path d={view.temp} fill="none" stroke="var(--tx)" stroke-opacity="0.35" stroke-width="1.5" stroke-linejoin="round" vector-effect="non-scaling-stroke" />
          <path d={view.on} fill="none" stroke="var(--cool)" stroke-width="2.5" stroke-linejoin="round" stroke-linecap="round" vector-effect="non-scaling-stroke" />
        </svg>
        {#if point && point.temperature != null}
          {@const left = `${(view.x(point.ts) / W) * 100}%`}
          <span class="pointer-events-none absolute inset-y-0 border-l border-tx opacity-50" style:left></span>
          <span
            class="pointer-events-none absolute size-3 -translate-x-1/2 -translate-y-1/2 rounded-full bg-bg shadow-[inset_0_0_0_3px_var(--cool)]"
            style:left
            style:top={`${(view.y(point.temperature) / H) * 100}%`}
          ></span>
        {/if}
      </div>

      <div class="relative h-3.5 text-[11px] text-mu">
        {#each view.ticks as t, i (i)}
          <span
            class={['absolute top-0', i === 0 ? '' : i === view.ticks.length - 1 && hours === 24 ? '-translate-x-full' : '-translate-x-1/2']}
            style:left={t.left}>{t.text}</span
          >
        {/each}
      </div>

      <div class="grid grid-cols-[40px_minmax(0,1fr)] items-center gap-x-2.5 gap-y-2 text-[11px] text-mu">
        <span>AC</span>
        <div class="relative h-2.5 overflow-hidden rounded-[3px] bg-s2">
          {#each view.acOn as b, i (i)}<span class="absolute inset-y-0 bg-cool opacity-85" style:left={b.left} style:width={b.width}></span>{/each}
        </div>
        <span>Fan</span>
        <div class="relative h-2.5 overflow-hidden rounded-[3px] bg-s2">
          {#each view.fan as b, i (i)}<span class="absolute inset-y-0 bg-cool" style:left={b.left} style:width={b.width} style:opacity={fanOpacity[b.fan ?? 'low'] ?? 0.3}></span>{/each}
        </div>
        <span>Room</span>
        <div class="relative h-2.5 overflow-hidden rounded-[3px] bg-s2">
          {#each view.roomOn as b, i (i)}<span class="absolute inset-y-0 bg-tx opacity-70" style:left={b.left} style:width={b.width}></span>{/each}
        </div>
      </div>

      <div class="flex flex-wrap gap-x-3.5 gap-y-1.5 text-[11px] text-mu">
        <span class="flex items-center gap-1.5"><span class="w-3.5 border-t-[1.5px] border-dashed border-mu"></span>Target</span>
        <span class="flex items-center gap-1.5"><span class="h-[3px] w-3.5 rounded-sm bg-cool"></span>Room on</span>
        <span class="flex items-center gap-1.5"><span class="size-2.5 rounded-sm bg-cool opacity-30"></span>Low</span>
        <span class="flex items-center gap-1.5"><span class="size-2.5 rounded-sm bg-cool opacity-60"></span>Medium</span>
        <span class="flex items-center gap-1.5"><span class="size-2.5 rounded-sm bg-cool"></span>High</span>
      </div>
    {/if}
  </section>

  {#if point && zone}
    <section aria-live="polite" aria-label="Decision at selected time" class="flex flex-col gap-2.5 rounded-[20px] bg-s1 p-4">
      <div class="flex flex-wrap items-baseline justify-between gap-x-3 text-[13px] text-mu">
        <span>{dayLabel(point.ts, live.now)} {clock(point.ts)}</span>
        <span>{zone.name} {deg(point.temperature)} · room {point.on ? 'on' : 'off'}</span>
      </div>
      <p class="m-0 text-[17px] leading-snug font-semibold">
        {decision ? decision.title : acAt?.on ? 'No change needed' : 'AC off'}
      </p>
      {#if decision?.reason}<p class="m-0 text-[13px] text-mu">{decision.reason} · {clock(decision.ts)}</p>{/if}
      <div class="flex gap-2 text-xs">
        <span class="rounded-lg bg-s2 px-2 py-[3px] text-mu">AC {acAt?.on ? 'on' : 'off'}</span>
        <span class="rounded-lg bg-s2 px-2 py-[3px] text-mu">Fan {acAt?.on && acAt.fan ? capitalize(acAt.fan) : '—'}</span>
        {#if acAt?.setpoint != null}
          <span class="rounded-lg bg-s2 px-2 py-[3px] text-mu">Setpoint {deg(acAt.setpoint)}</span>
        {/if}
      </div>
      <a href="#/activity" class="py-1.5 text-[13px] font-semibold text-cool">Open activity log →</a>
    </section>
  {/if}
</div>
