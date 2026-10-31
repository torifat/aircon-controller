<script lang="ts">
  import { api } from '../api';
  import { capitalize, deg, duration, modeName } from '../format';
  import { live } from '../live.svelte';
  import Switch from './Switch.svelte';

  const s = $derived(live.state!);
  const ac = $derived(s.ac!);
  const cmd = $derived(s.controller.command);
  const limits = $derived(ac.direction === 'heat' ? ac.limits.heat : ac.limits.cool);
  const since = $derived(ac.power_since != null ? duration(live.now - ac.power_since) : null);
  const minOn = $derived((live.settings?.min_on_minutes ?? 5) * 60);
  const minMet = $derived(ac.power_since == null || live.now - ac.power_since >= minOn);

  function confirmPower(on: boolean) {
    // Turning it off here hands control back: Climate won't turn it on again by itself.
    void live.act(() => api.setAc(on));
  }
</script>

{#snippet stat(label: string, value: string, note: string, noteClass = 'text-mu')}
  <div class="flex flex-col gap-[3px]">
    <span class="text-xs text-mu">{label}</span>
    <span class="text-lg font-semibold">{value}</span>
    <span class={['text-xs', noteClass]}>{note}</span>
  </div>
{/snippet}

<section aria-label="Air conditioner" class="flex flex-col gap-3.5 rounded-[20px] bg-s1 p-4">
  <div class="flex items-center justify-between">
    <div class="flex flex-col gap-0.5">
      <span class="text-[15px] font-semibold">Air conditioner</span>
      <span class="text-xs text-mu">
        {s.controller.enabled ? 'Fan and setpoint managed by Climate' : 'Managed by AirTouch'}
      </span>
    </div>
    <Switch checked={ac.on} label="AC power" onchange={confirmPower} />
  </div>
  <div class="grid grid-cols-3 gap-x-3 gap-y-3.5">
    {@render stat(
      'Mode',
      modeName(ac.mode),
      ac.direction === 'none' ? 'not controlled' : ac.direction === 'heat' ? 'heating' : 'cooling',
      ac.direction === 'heat' ? 'text-warm' : ac.direction === 'cool' ? 'text-cool' : 'text-mu',
    )}
    {@render stat(
      'Fan',
      capitalize(ac.fan),
      cmd?.fan && cmd.fan !== ac.fan
        ? `${s.controller.dry_run ? 'would be' : '→'} ${capitalize(cmd.fan)}`
        : 'holding',
    )}
    {@render stat('AC setpoint', deg(ac.setpoint), `limit ${limits[0]}–${limits[1]}°`)}
    {@render stat('Return air', deg(ac.temperature), 'AC sensor')}
    <div class="col-span-2 flex flex-col gap-[3px]">
      <span class="text-xs text-mu">Compressor</span>
      <span class="text-lg font-semibold">
        {ac.on ? 'On' : 'Off'}{since ? ` ${since}` : ''}
      </span>
      <span class="flex items-center gap-2 text-xs text-mu">
        {#if ac.power_since != null}
          <span class="h-1 flex-1 overflow-hidden rounded-sm bg-s2">
            <span
              class="block h-full bg-mu"
              style:width={`${Math.min(100, ((live.now - ac.power_since) / minOn) * 100)}%`}
            ></span>
          </span>
        {/if}
        {minMet ? `min ${minOn / 60} min met` : `min ${minOn / 60} min`}
      </span>
    </div>
  </div>
</section>
