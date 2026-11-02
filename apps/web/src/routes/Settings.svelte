<script lang="ts">
  import type { Settings, SettingsPatch } from '@aircon/api';

  import { api } from '../lib/api';
  import Stepper from '../lib/components/Stepper.svelte';
  import Switch from '../lib/components/Switch.svelte';
  import { ago, deg } from '../lib/format';
  import { live } from '../lib/live.svelte';

  const s = $derived(live.settings);
  const current = $derived(live.state);
  const zones = $derived(current?.zones ?? []);
  const ac = $derived(current?.ac);

  const save = (patch: SettingsPatch) => live.patchSettings(patch);
  const mins = (v: number) => `${v} min`;

  function toggleZone(current: Settings, n: number, on: boolean) {
    const set = new Set(current.zones ?? []);
    if (on) set.add(n);
    else set.delete(n);
    void save({ zones: [...set].sort((a, b) => a - b) });
  }

  let testing = $state(false);
  async function test() {
    testing = true;
    try {
      live.state = await api.tick();
      live.flash(live.state.airtouch.connected ? 'AirTouch answered' : "AirTouch didn't answer");
    } catch (e) {
      live.flash(e instanceof Error ? e.message : String(e));
    } finally {
      testing = false;
    }
  }
</script>

{#snippet row(label: string, sub: string, first = false)}
  <div class="flex min-w-0 flex-1 flex-col gap-0.5">
    <span class="text-[15px] font-semibold">{label}</span>
    <span class="text-[13px] leading-snug text-mu">{sub}</span>
  </div>
{/snippet}

<div class="px-5 pt-5 pb-2 md:px-0">
  <h1 class="m-0 text-[28px] font-semibold tracking-[-0.02em]">Settings</h1>
</div>

{#if !s}
  <p class="mx-4 rounded-[20px] bg-s1 p-6 text-sm text-mu md:mx-0">Loading settings…</p>
{:else}
  {@const card = 'overflow-hidden rounded-[20px] bg-s1'}
  {@const line = 'flex items-center gap-3 py-3 pr-3 pl-4'}
  <div class="flex flex-col gap-2 px-4 md:max-w-2xl md:px-0">
    <h2 class="m-0 px-1 pt-4 text-[13px] font-semibold text-mu">Controller</h2>
    <div class={card}>
      <div class={line}>
        {@render row(
          'Controller enabled',
          s.enabled ? 'Climate is adjusting fan, AC setpoint and dampers' : 'Off — rooms keep their last opening',
        )}
        <Switch checked={s.enabled} label="Controller enabled" onchange={(v) => save({ enabled: v })} />
      </div>
      <div class={[line, 'border-t border-line']}>
        {@render row('Dry run', 'Log decisions without sending them to the AC')}
        <Switch checked={s.dry_run} label="Dry run" onchange={(v) => save({ dry_run: v })} />
      </div>
    </div>

    <h2 class="m-0 px-1 pt-4 text-[13px] font-semibold text-mu">Decisions</h2>
    <div class={card}>
      <div class={line}>
        {@render row('Deadband', 'AC back on when a room drifts this far past target')}
        <Stepper value={s.deadband} step={0.5} min={0.5} max={4} label="deadband" format={(v) => deg(v)}
          onchange={(v) => save({ deadband: v })} />
      </div>
      <div class={[line, 'border-t border-line']}>
        {@render row('Fan High at', 'Worst room this far from target')}
        <Stepper value={s.fan_high_at} step={0.5} min={s.fan_medium_at + 0.5} max={5} label="fan High threshold"
          format={(v) => deg(v)} onchange={(v) => save({ fan_high_at: v })} />
      </div>
      <div class={[line, 'border-t border-line']}>
        {@render row('Fan Medium at', 'Below this the fan runs Low')}
        <Stepper value={s.fan_medium_at} step={0.5} min={0.5} max={s.fan_high_at - 0.5}
          label="fan Medium threshold" format={(v) => deg(v)} onchange={(v) => save({ fan_medium_at: v })} />
      </div>
      <div class={[line, 'border-t border-line']}>
        {@render row('Ease off within', 'Closer than this, the compressor idles so the air comes out milder')}
        <Stepper value={s.ease_within} step={0.5} min={0} max={3} label="ease off within"
          format={(v) => (v ? deg(v) : 'Off')} onchange={(v) => save({ ease_within: v })} />
      </div>
      <div class="border-t border-line px-4 pt-3 pb-4">
        <div aria-hidden="true" class="flex h-2 overflow-hidden rounded-full">
          <span class="bg-cool opacity-30" style:flex={s.fan_medium_at}></span>
          <span class="bg-cool opacity-60" style:flex={s.fan_high_at - s.fan_medium_at}></span>
          <span class="bg-cool" style:flex={Math.max(0.2, 3 - s.fan_high_at)}></span>
        </div>
        <div class="flex justify-between pt-1.5 text-[11px] text-mu">
          <span>0° Low</span><span>{deg(s.fan_medium_at)} Medium</span><span>{deg(s.fan_high_at)} High</span><span>3°+</span>
        </div>
      </div>
    </div>

    <h2 class="m-0 px-1 pt-4 text-[13px] font-semibold text-mu">Dampers</h2>
    <div class={card}>
      <div class={line}>
        {@render row('Fully open from', 'A room this far from target gets all the air it can')}
        <Stepper value={s.damper_full_at} step={0.5} min={0.5} max={3} label="fully open from"
          format={(v) => deg(v)} onchange={(v) => save({ damper_full_at: v })} />
      </div>
      <div class={[line, 'border-t border-line']}>
        {@render row('At target', 'Opening for a room that has reached target')}
        <Stepper value={s.damper_min} step={5} min={0} max={50} label="opening at target"
          format={(v) => `${v}%`} onchange={(v) => save({ damper_min: v })} />
      </div>
      <p class="m-0 border-t border-line px-4 py-3 text-[13px] leading-snug text-mu">
        The room furthest from target is always fully open, so nothing spills.
      </p>
    </div>

    <h2 class="m-0 px-1 pt-4 text-[13px] font-semibold text-mu">Timing</h2>
    <div class={card}>
      <div class={line}>
        {@render row('Min on/off time', 'Protects the compressor from short cycling')}
        <Stepper value={s.min_on_minutes} step={1} min={3} max={15} label="minimum on/off time" format={mins}
          onchange={(v) => save({ min_on_minutes: v, min_off_minutes: v })} />
      </div>
      <div class={[line, 'border-t border-line']}>
        {@render row('Pause after app change', 'When someone uses the AirTouch app')}
        <Stepper value={s.manual_pause_minutes} step={5} min={5} max={120} label="pause after app change"
          format={mins} onchange={(v) => save({ manual_pause_minutes: v })} />
      </div>
      <div class={[line, 'border-t border-line justify-between py-4']}>
        <span class="text-[15px] font-semibold">Check every</span>
        <span class="text-[15px] text-mu">{s.tick_seconds} s</span>
      </div>
    </div>

    <h2 class="m-0 px-1 pt-4 text-[13px] font-semibold text-mu">Rooms that count</h2>
    <fieldset class={[card, 'm-0 border-0 p-0']}>
      <legend class="sr-only">Rooms that count</legend>
      <label class={[line, 'cursor-pointer']}>
        <input type="radio" name="scope" class="size-5 accent-[var(--cool)]" checked={s.zones == null}
          onchange={() => save({ zones: null })} />
        {@render row('Whichever rooms are on', 'Default — follows AirTouch')}
      </label>
      <label class={[line, 'cursor-pointer border-t border-line']}>
        <input type="radio" name="scope" class="size-5 accent-[var(--cool)]" checked={s.zones != null}
          onchange={() => save({ zones: zones.filter((z) => z.on).map((z) => z.number) })} />
        {@render row('Only these rooms', 'Others are ignored even when on')}
      </label>
      {#if s.zones != null}
        <div class="grid grid-cols-2 gap-1 border-t border-line px-4 py-3">
          {#each zones as z (z.number)}
            <label class="flex min-h-11 cursor-pointer items-center gap-2.5 text-[15px]">
              <input type="checkbox" class="size-5 accent-[var(--cool)]" checked={s.zones.includes(z.number)}
                onchange={(e) => toggleZone(s, z.number, e.currentTarget.checked)} />
              {z.name}
            </label>
          {/each}
        </div>
      {/if}
    </fieldset>

    <h2 class="m-0 px-1 pt-4 text-[13px] font-semibold text-mu">AirTouch</h2>
    <div class={[card, 'flex flex-col gap-3 p-4']}>
      <div class="flex items-center justify-between">
        <span class="flex items-center gap-2 text-[15px] font-semibold">
          <span class={['size-2 rounded-full', current?.airtouch.connected ? 'bg-cool' : 'bg-err']}></span>
          {current?.airtouch.connected ? 'Connected' : 'Not connected'}
        </span>
        <span class="text-[13px] text-mu">last reply {ago(current?.airtouch.last_packet_at, live.now)}</span>
      </div>
      <dl class="m-0 grid grid-cols-[auto_1fr] gap-x-6 gap-y-2 text-sm">
        <dt class="text-mu">Address</dt><dd class="m-0">{current?.airtouch.host ?? '—'}:9005</dd>
        <dt class="text-mu">Console</dt><dd class="m-0">{current?.airtouch.console_version ?? '—'}</dd>
        <dt class="text-mu">Rooms</dt><dd class="m-0">{zones.length}</dd>
        {#if ac}
          <dt class="text-mu">Cool limits</dt><dd class="m-0">{ac.limits.cool[0]}–{ac.limits.cool[1]}°</dd>
          <dt class="text-mu">Heat limits</dt><dd class="m-0">{ac.limits.heat[0]}–{ac.limits.heat[1]}°</dd>
        {/if}
        {#if current?.airtouch.last_error && !current.airtouch.connected}
          <dt class="text-mu">Error</dt><dd class="m-0 text-err">{current.airtouch.last_error}</dd>
        {/if}
      </dl>
      <button type="button" disabled={testing} onclick={test}
        class="min-h-11 self-start rounded-xl border border-line px-4 text-sm font-semibold disabled:opacity-50"
        >{testing ? 'Checking…' : 'Test connection'}</button
      >
    </div>

    {#if current}
      <h2 class="m-0 px-1 pt-4 text-[13px] font-semibold text-mu">About</h2>
      <div class={[card, 'p-4']}>
        <dl class="m-0 grid grid-cols-[auto_1fr] gap-x-6 gap-y-2 text-sm">
          <dt class="text-mu">Version</dt><dd class="m-0">{current.build.version}</dd>
          {#if current.build.commit}
            <dt class="text-mu">Commit</dt><dd class="m-0 font-mono">{current.build.commit}</dd>
          {/if}
        </dl>
      </div>
    {/if}
  </div>
{/if}
