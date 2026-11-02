<script lang="ts">
  import type { Limits, ZoneState } from '@aircon/api';
  import { BatteryLow } from '@lucide/svelte';

  import { api } from '../api';
  import { deg } from '../format';
  import { live } from '../live.svelte';
  import { barPos, delta, RANGE, spark, SPARK_H, SPARK_W, type Sample, trend } from '../rooms';
  import Switch from './Switch.svelte';

  let {
    zone,
    limits,
    heating,
    driving,
    samples,
    maxGap,
  }: {
    zone: ZoneState;
    limits: Limits | null;
    heating: boolean;
    /** Shown as Driving: the server's driving room, when it's really off target. */
    driving: boolean;
    /** The last 2h of readings, ending with the current one. */
    samples: Sample[];
    maxGap: number;
  } = $props();

  // Target taps collect locally and go out once the user stops tapping. The new value
  // stays on screen until the AirTouch reports it (or gives up after a while), so a
  // state refresh that lands before the console catches up doesn't flick it back.
  const CONFIRM_MS = 10_000;
  let pending = $state<number | null>(null);
  let sendTimer: ReturnType<typeof setTimeout> | undefined;
  let confirmTimer: ReturnType<typeof setTimeout> | undefined;
  const target = $derived(pending ?? zone.target ?? zone.setpoint ?? 24);
  const [min, max] = $derived(limits ? (heating ? limits.heat : limits.cool) : [16, 30]);

  function setTarget(v: number) {
    pending = Math.round(Math.min(max, Math.max(min, v)) * 100) / 100;
    const sending = pending;
    clearTimeout(sendTimer);
    clearTimeout(confirmTimer);
    sendTimer = setTimeout(async () => {
      const sent = await live.act(() => api.patchZone(zone.number, { target: sending }));
      if (!sent) {
        if (pending === sending) pending = null;
        return;
      }
      confirmTimer = setTimeout(() => {
        if (pending === sending) pending = null;
      }, CONFIRM_MS);
    }, 700);
  }

  $effect(() => {
    if (pending != null && zone.target != null && Math.abs(zone.target - pending) < 0.05) {
      pending = null;
      clearTimeout(confirmTimer);
    }
  });

  const gap = $derived(zone.temperature != null ? zone.temperature - target : null);
  const d = $derived(gap != null ? delta(gap) : null);
  const pos = $derived(gap != null ? barPos(gap) : 50);
  const line = $derived(spark(samples, target, live.state?.now ?? Date.now() / 1000, maxGap));
  const tr = $derived(gap != null ? trend(gap, line.rate) : null);
  const pct = $derived(zone.control === 'percentage');
  const damper = $derived(Math.round(zone.damper * 100));
  const tone = (t: string) => `var(--${t})`;
</script>

<article aria-label={zone.name} class="flex flex-col overflow-hidden rounded-[18px] bg-s1">
  <div class="flex items-center justify-between gap-2 pt-3 pr-2 pl-4">
    <div class="flex min-w-0 items-center gap-2 text-[11px]">
      <span class="min-w-0 truncate text-[15px] font-semibold">{zone.name}</span>
      {#if driving}
        <!-- In the AC's colour: blue while it cools, amber while it heats. -->
        <span
          class={[
            'flex-none rounded-md px-[7px] py-0.5 font-semibold whitespace-nowrap',
            heating ? 'bg-warm-bg text-warm' : 'bg-cool-bg text-cool',
          ]}>Driving</span
        >
      {/if}
      {#if zone.spill}
        <span class="flex-none rounded-md border border-line px-1.5 py-px text-mu">Spill</span>
      {/if}
      {#if !pct}
        <!-- Rooms normally run in percentage control with Climate setting the damper. -->
        <span class="flex-none rounded-md border border-line px-1.5 py-px whitespace-nowrap text-mu">Temperature control</span>
      {/if}
      {#if zone.low_battery}
        <span class="inline-flex flex-none items-center gap-1 rounded-md border border-line px-1.5 py-px whitespace-nowrap text-warm">
          <BatteryLow size={12} aria-hidden="true" />Low battery
        </span>
      {/if}
    </div>
    <Switch
      checked
      label={`${zone.name} power`}
      onchange={(on) => live.act(() => api.patchZone(zone.number, { on }))}
    />
  </div>

  <div class="flex items-center justify-between gap-3 px-4 pt-0.5">
    <span class="text-[46px] leading-none font-semibold tracking-[-0.03em]">{deg(zone.temperature)}</span>
    {#if gap != null && d}
      <div role="img" aria-label={d.words} class="flex w-[150px] flex-none flex-col gap-1">
        <div class="relative h-4">
          <span class="absolute inset-x-0 top-[7px] h-0.5 rounded-[1px] bg-mu opacity-35"></span>
          <span class="absolute top-px left-1/2 -ml-px h-3.5 w-0.5 rounded-[1px] bg-mu"></span>
          <span
            class="absolute top-1.5 h-1 rounded-sm"
            style:background={tone(d.tone)}
            style:left={`${Math.min(50, pos)}%`}
            style:width={`${Math.abs(pos - 50)}%`}
          ></span>
          <span
            class="absolute top-px -ml-[7px] size-3.5 rounded-full bg-s1"
            style:left={`${pos}%`}
            style:box-shadow={`inset 0 0 0 3px ${tone(d.tone)}`}
          ></span>
          {#if Math.abs(gap) > RANGE}
            <!-- Off the scale: an arrow at the bar's end. -->
            <span
              class={[
                'absolute top-0.5 size-0 border-y-[6px] border-y-transparent',
                gap > 0 ? '-right-[9px] border-l-[7px]' : '-left-[9px] border-r-[7px]',
              ]}
              style:border-left-color={gap > 0 ? tone(d.tone) : undefined}
              style:border-right-color={gap < 0 ? tone(d.tone) : undefined}
            ></span>
          {/if}
        </div>
        <span class="self-center text-[13px] font-semibold whitespace-nowrap" style:color={tone(d.tone)}>{d.text}</span>
      </div>
    {/if}
  </div>

  <a
    href={`#/history?zone=${zone.number}`}
    aria-label={`${zone.name}, last 2 hours: ${tr?.words ?? 'no reading'}. Open history`}
    class="mx-4 my-3 flex flex-col gap-1 text-mu no-underline"
  >
    <span class="relative block h-10">
      <svg
        width="100%"
        height="40"
        viewBox={`0 0 ${SPARK_W} ${SPARK_H}`}
        preserveAspectRatio="none"
        aria-hidden="true"
        class="block overflow-visible"
      >
        <line x1="0" x2={SPARK_W} y1={SPARK_H / 2} y2={SPARK_H / 2} stroke="currentColor" stroke-opacity="0.7" stroke-width="1" stroke-dasharray="3 3" vector-effect="non-scaling-stroke" />
        {#if line.d}
          <path d={line.d} fill="none" stroke="currentColor" stroke-width="1.75" stroke-linejoin="round" stroke-linecap="round" vector-effect="non-scaling-stroke" />
        {/if}
      </svg>
      {#if line.endY != null}
        <span
          class="absolute -right-[3px] size-1.5 rounded-full bg-current"
          style:top={`${((line.endY / SPARK_H) * 40 - 3).toFixed(1)}px`}
        ></span>
      {/if}
    </span>
    <span class="flex items-baseline justify-between gap-2 text-[11px]">
      <span>Last 2h</span>
      {#if tr}
        <span class={tr.strong ? 'font-semibold' : 'font-medium'} style:color={tone(tr.tone)}>{tr.text}</span>
      {/if}
    </span>
  </a>

  <div class="flex items-center justify-between gap-3 border-t border-line pt-2 pr-4 pb-2.5 pl-2.5">
    <div role="group" aria-label={`${zone.name} target`} class="flex items-center">
      <button
        type="button"
        aria-label={`Lower ${zone.name} target`}
        disabled={target <= min}
        onclick={() => setTarget(target - 0.5)}
        class="flex size-11 items-center justify-center disabled:opacity-40"
      >
        <span aria-hidden="true" class="flex size-8 items-center justify-center rounded-full bg-s2 text-lg leading-none">−</span>
      </button>
      <span aria-live="polite" class="flex min-w-12 flex-col items-center">
        <span class="text-[15px] font-semibold">{deg(target)}</span>
        <span class="text-[10px] text-mu">target</span>
      </span>
      <button
        type="button"
        aria-label={`Raise ${zone.name} target`}
        disabled={target >= max}
        onclick={() => setTarget(target + 0.5)}
        class="flex size-11 items-center justify-center disabled:opacity-40"
      >
        <span aria-hidden="true" class="flex size-8 items-center justify-center rounded-full bg-s2 text-lg leading-none">+</span>
      </button>
    </div>
    <span class="flex flex-col items-end gap-[5px] text-xs text-mu">
      <span>Damper {damper}%{pct ? '' : ' · auto'}</span>
      <span class="h-1 w-[72px] overflow-hidden rounded-sm bg-s2">
        <span class={['block h-full', pct ? 'bg-tx' : 'bg-mu']} style:width={`${damper}%`}></span>
      </span>
    </span>
  </div>
</article>
