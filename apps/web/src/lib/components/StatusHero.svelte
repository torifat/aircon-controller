<script lang="ts">
  import { Flame, Pause, Snowflake, WifiOff } from '@lucide/svelte';

  import { api } from '../api';
  import { countdown } from '../format';
  import { hero } from '../hero';
  import { live } from '../live.svelte';

  let { large = false }: { large?: boolean } = $props();

  const s = $derived(live.state!);
  const h = $derived(hero(s, live.settings));
  const next = $derived(
    s.controller.next_tick_at != null ? countdown(s.controller.next_tick_at - live.now) : null,
  );
  // hold_seconds is as of the last check; count it down between checks.
  const holdLeft = $derived(h.hold > 0 ? Math.max(0, h.hold - (live.now - s.now)) : 0);
  const holdTotal = $derived(
    (h.holdLabel === 'turns off in'
      ? (live.settings?.min_on_minutes ?? 5)
      : (live.settings?.min_off_minutes ?? 5)) * 60,
  );
</script>

{#if h.tone === 'error'}
  <section role="alert" class="flex items-start gap-3 rounded-[20px] bg-err-bg px-[18px] py-4">
    <WifiOff size={20} class="mt-0.5 flex-none text-err" aria-hidden="true" />
    <div class="flex flex-col gap-1">
      <span class="text-base font-semibold text-err">{h.headline}</span>
      <span class="text-[13px] leading-snug">
        {s.controller.reason}{h.sub ? ` · ${h.sub}` : ''}
      </span>
    </div>
  </section>
{:else}
  <section aria-live="polite" class="flex flex-col gap-2.5">
    <div class="flex flex-wrap gap-1.5">
      <span
        class={[
          'inline-flex items-center gap-1.5 rounded-full px-2.5 py-[5px] text-[13px] font-semibold',
          h.tone === 'cool' && 'bg-cool-bg text-cool',
          h.tone === 'warm' && 'bg-warm-bg text-warm',
          h.tone === 'neutral' && 'bg-s2 text-mu',
        ]}
      >
        {#if h.pill === 'Cooling'}<Snowflake size={14} aria-hidden="true" />{/if}
        {#if h.pill === 'Heating'}<Flame size={14} aria-hidden="true" />{/if}
        {#if h.pill === 'Paused'}<Pause size={12} aria-hidden="true" />{/if}
        {h.pill}
      </span>
      {#if s.controller.dry_run && s.controller.enabled}
        <span class="rounded-full border border-dashed border-mu px-2.5 py-1 text-[13px] font-semibold text-mu"
          >Dry run</span
        >
      {/if}
    </div>
    <h1
      class={[
        'm-0 leading-[1.12] font-semibold tracking-[-0.02em]',
        large ? 'text-[38px]' : 'text-[30px]',
      ]}
    >
      {h.headline}
    </h1>
    <p class="m-0 text-[15px] text-mu">
      {[h.sub, next && `next check in ${next}`].filter(Boolean).join(' · ')}
    </p>
    {#if holdLeft > 0 && h.holdLabel}
      <div class="flex items-center gap-2.5 pt-1 text-xs text-mu">
        <span class="h-1 flex-1 overflow-hidden rounded-sm bg-s2">
          <span
            class="block h-full bg-mu"
            style:width={`${Math.max(0, 100 - (holdLeft / holdTotal) * 100)}%`}
          ></span>
        </span>
        <span class="whitespace-nowrap">{h.holdLabel} {countdown(holdLeft)}</span>
      </div>
    {/if}
    {#if h.actions.length}
      <div class="flex flex-wrap gap-2 pt-1">
        {#if h.actions.includes('resume')}
          <button
            type="button"
            class="min-h-11 rounded-xl bg-tx px-4 text-sm font-semibold text-bg"
            onclick={() => live.act(api.resume)}>Resume now</button
          >
        {/if}
        {#if h.actions.includes('extend')}
          <button
            type="button"
            class="min-h-11 rounded-xl border border-line px-4 text-sm font-semibold"
            onclick={() => live.act(() => api.pause(30))}>+30 min</button
          >
        {/if}
        {#if h.actions.includes('enable')}
          <button
            type="button"
            class="min-h-11 rounded-xl border border-line px-4 text-sm font-semibold"
            onclick={() => live.patchSettings({ enabled: true })}>Turn on</button
          >
        {/if}
      </div>
    {/if}
  </section>
{/if}
