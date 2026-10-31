<script lang="ts">
  import type { ActivityItem } from '@aircon/api';

  import { api } from '../lib/api';
  import AcCard from '../lib/components/AcCard.svelte';
  import ActivityList from '../lib/components/ActivityList.svelte';
  import StatusHero from '../lib/components/StatusHero.svelte';
  import ZoneCard from '../lib/components/ZoneCard.svelte';
  import { live } from '../lib/live.svelte';

  const s = $derived(live.state);
  const on = $derived(s?.zones.filter((z) => z.on) ?? []);
  const lead = $derived(s?.zones.find((z) => z.driving));
  const heating = $derived(s?.ac?.direction === 'heat');

  // Recent activity for the desktop column; refreshed with each state poll.
  let recent = $state<ActivityItem[]>([]);
  $effect(() => {
    void s?.now;
    api
      .activity({ limit: 4 })
      .then((r) => (recent = r.items))
      .catch(() => {});
  });
</script>

{#if !s}
  <section aria-busy="true" aria-label="Loading" class="mx-4 flex flex-col gap-2.5 rounded-[20px] bg-s1 p-[18px]">
    <span class="h-6 w-[70px] rounded-xl bg-s2"></span>
    <span class="h-[26px] w-[260px] max-w-full rounded-lg bg-s2"></span>
    <span class="h-3.5 w-[180px] rounded-md bg-s2"></span>
    {#if live.error}<p class="m-0 pt-2 text-sm text-err">{live.error}</p>{/if}
  </section>
{:else}
  <div class="flex flex-col gap-6 md:flex-row md:flex-wrap md:items-start">
    <div class="flex min-w-0 flex-col gap-4 md:max-w-[420px] md:flex-[1_1_340px]">
      <div class="px-5 pt-5 pb-1 md:px-1 md:pt-6">
        <StatusHero large={false} />
      </div>
      {#if s.ac}
        <div class="px-4 md:px-0"><AcCard /></div>
      {/if}
      {#if recent.length}
        <section aria-label="Recent activity" class="hidden rounded-[20px] bg-s1 py-1.5 md:block">
          <div class="flex items-baseline justify-between px-[18px] pt-3 pb-1.5">
            <h2 class="m-0 text-[15px] font-semibold">Recent</h2>
            <a href="#/activity" class="py-1.5 text-[13px] font-semibold text-cool">All activity</a>
          </div>
          <div class="border-t border-line"><ActivityList items={recent} compact /></div>
        </section>
      {/if}
    </div>

    <section aria-label="Rooms" class="flex min-w-0 flex-col gap-2.5 md:flex-[999_1_560px] md:gap-3 md:pt-6">
      <div class="flex items-baseline justify-between px-5 pt-2 md:px-0 md:pt-0">
        <h2 class="m-0 text-xl font-semibold tracking-[-0.01em]">Rooms</h2>
        <span class="text-[13px] text-mu">{on.length} on{lead ? ` · ${lead.name} leads` : ''}</span>
      </div>
      <div class="grid gap-2.5 px-4 md:grid-cols-[repeat(auto-fill,minmax(260px,1fr))] md:gap-3 md:px-0">
        {#each s.zones as zone (zone.number)}
          <ZoneCard {zone} limits={s.ac?.limits ?? null} {heating} />
        {/each}
      </div>
    </section>
  </div>
{/if}
