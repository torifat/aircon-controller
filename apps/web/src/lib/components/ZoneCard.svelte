<script lang="ts">
  import type { Limits, ZoneState } from '@aircon/api';
  import { BatteryLow } from '@lucide/svelte';

  import { api } from '../api';
  import { deg, gapChip } from '../format';
  import { live } from '../live.svelte';
  import Stepper from './Stepper.svelte';
  import Switch from './Switch.svelte';

  let { zone, limits, heating }: { zone: ZoneState; limits: Limits | null; heating: boolean } =
    $props();

  // Target clicks collect locally and go out once the user stops tapping.
  let pending = $state<number | null>(null);
  let timer: ReturnType<typeof setTimeout> | undefined;
  const target = $derived(pending ?? zone.target ?? zone.setpoint ?? 24);
  const [min, max] = $derived(limits ? (heating ? limits.heat : limits.cool) : [16, 30]);

  function setTarget(v: number) {
    pending = v;
    clearTimeout(timer);
    timer = setTimeout(async () => {
      await live.act(() => api.patchZone(zone.number, { target: v }));
      pending = null;
    }, 700);
  }

  const gap = $derived(zone.temperature != null ? zone.temperature - target : null);
  const chip = $derived(gap != null ? gapChip(gap) : null);
  const dim = $derived(!zone.on);
</script>

<article
  aria-label={zone.name}
  class={[
    'flex flex-col gap-3.5 rounded-[20px] p-4',
    zone.on ? 'bg-s1' : 'bg-transparent',
    zone.driving
      ? 'shadow-[inset_0_0_0_1.5px_var(--cool)]'
      : zone.on
        ? ''
        : 'shadow-[inset_0_0_0_1px_var(--line)]',
  ]}
>
  <div class="flex items-start justify-between gap-2">
    <div class="flex min-w-0 flex-col gap-1.5">
      <span class="text-base font-semibold">{zone.name}</span>
      <div class="flex min-h-5 flex-wrap gap-1.5 text-[11px]">
        {#if zone.driving}
          <span class="rounded-md bg-cool-bg px-[7px] py-[3px] font-semibold text-cool">Driving decisions</span>
        {/if}
        {#if zone.spill}
          <span class="rounded-md border border-line px-1.5 py-0.5 font-medium text-mu">Spill</span>
        {/if}
        {#if zone.control === 'percentage'}
          <span class="rounded-md border border-line px-1.5 py-0.5 font-medium text-mu">% control</span>
        {/if}
        {#if zone.low_battery}
          <span class="inline-flex items-center gap-1 rounded-md border border-line px-1.5 py-0.5 font-medium text-warm">
            <BatteryLow size={12} aria-hidden="true" />Low battery
          </span>
        {/if}
      </div>
    </div>
    <div class="-mt-2 -mr-1.5">
      <Switch
        checked={zone.on}
        label={`${zone.name} power`}
        onchange={(on) => live.act(() => api.patchZone(zone.number, { on }))}
      />
    </div>
  </div>

  <div class="flex items-end justify-between gap-2">
    <div class="flex flex-col gap-1.5">
      <span
        class={[
          'text-[44px] leading-none font-semibold tracking-[-0.03em] md:text-[40px]',
          zone.on ? 'text-tx' : 'text-mu',
        ]}>{deg(zone.temperature)}</span
      >
      {#if chip}
        <span
          class={[
            'self-start rounded-lg px-2 py-[3px] text-xs font-semibold',
            dim && 'opacity-55',
            chip.tone === 'none' && 'bg-s2 text-mu',
            chip.tone === 'warm' && (chip.far ? 'bg-warm text-on' : 'bg-warm-bg text-warm'),
            chip.tone === 'cool' && (chip.far ? 'bg-cool text-on' : 'bg-cool-bg text-cool'),
          ]}>{chip.text}</span
        >
      {/if}
    </div>
    <div class={['flex flex-col gap-1', dim && 'opacity-55']}>
      <span class="text-center text-[11px] text-mu">Target</span>
      <Stepper
        value={target}
        step={0.5}
        {min}
        {max}
        label={`${zone.name} target`}
        format={(v) => deg(v)}
        compact
        onchange={setTarget}
      />
    </div>
  </div>

  <div class={['flex items-center gap-2.5 text-xs text-mu', dim && 'opacity-55']}>
    <span class="w-[82px]">Damper {Math.round(zone.damper * 100)}%</span>
    <span class="h-1.5 flex-1 overflow-hidden rounded-[3px] bg-s2">
      <span
        class={['block h-full rounded-[3px]', zone.on ? 'bg-cool' : 'bg-mu']}
        style:width={`${Math.round(zone.damper * 100)}%`}
      ></span>
    </span>
  </div>
</article>
