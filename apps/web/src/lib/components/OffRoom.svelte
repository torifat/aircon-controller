<script lang="ts">
  import type { ZoneState } from '@aircon/api';
  import { BatteryLow } from '@lucide/svelte';

  import { api } from '../api';
  import { deg } from '../format';
  import { live } from '../live.svelte';
  import { delta } from '../rooms';
  import Switch from './Switch.svelte';

  let { zone }: { zone: ZoneState } = $props();

  const target = $derived(zone.target ?? zone.setpoint);
  const d = $derived(
    zone.temperature != null && target != null ? delta(zone.temperature - target) : null,
  );
</script>

<li class="flex items-center gap-3 py-1 pr-2 pl-4 not-first:border-t not-first:border-line">
  <!-- Tags wrap below the name rather than squeezing it on a phone. -->
  <span class="flex min-w-0 flex-1 flex-wrap items-center gap-x-2 gap-y-1 py-1.5 text-[11px]">
    <span class="max-w-full truncate text-[15px] font-semibold">{zone.name}</span>
    {#if zone.spill}
      <span class="flex-none rounded-md border border-line px-1.5 py-px text-mu">Spill</span>
    {/if}
    {#if zone.control === 'temperature'}
      <span class="flex-none rounded-md border border-line px-1.5 py-px whitespace-nowrap text-mu">Temperature control</span>
    {/if}
    {#if zone.low_battery}
      <span class="inline-flex flex-none items-center gap-1 rounded-md border border-line px-1.5 py-px text-warm">
        <BatteryLow size={12} aria-hidden="true" /><span class="sr-only">Low battery</span>
      </span>
    {/if}
  </span>
  {#if d}
    <span class="text-[13px] font-semibold whitespace-nowrap" style:color={`var(--${d.tone})`}
      >{d.text}{#if d.tone !== 'mu'}<span class="sr-only"> {d.words}</span>{/if}</span
    >
  {/if}
  <span class="w-16 text-right text-[22px] font-semibold text-mu">{deg(zone.temperature)}</span>
  <Switch
    checked={false}
    outlined
    label={`${zone.name} power`}
    onchange={(on) => live.act(() => api.patchZone(zone.number, { on }))}
  />
</li>
