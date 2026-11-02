<script lang="ts">
  import type { ActivityItem, HistoryResponse, ZoneState } from '@aircon/api';

  import { api } from '../lib/api';
  import AcCard from '../lib/components/AcCard.svelte';
  import ActivityList from '../lib/components/ActivityList.svelte';
  import OffRoom from '../lib/components/OffRoom.svelte';
  import StatusHero from '../lib/components/StatusHero.svelte';
  import ZoneCard from '../lib/components/ZoneCard.svelte';
  import { live } from '../lib/live.svelte';
  import { need, type Sample, SPARK_HOURS } from '../lib/rooms';

  const s = $derived(live.state);
  const heating = $derived(s?.ac?.direction === 'heat');

  // Active rooms as tiles, the one furthest behind first; off rooms as a list below.
  const gapOf = (z: ZoneState) =>
    z.temperature != null && z.target != null ? z.temperature - z.target : 0;
  const active = $derived(
    (s?.zones.filter((z) => z.on) ?? []).toSorted(
      (a, b) => need(gapOf(b), heating) - need(gapOf(a), heating),
    ),
  );
  const off = $derived(s?.zones.filter((z) => !z.on) ?? []);
  // Driving only says something when that room is really behind.
  const lead = $derived(active.find((z) => z.driving && need(gapOf(z), heating) >= 0.5));

  // The tiles' sparklines: the last 2h in 10-minute samples, refreshed each minute.
  const STEP = 600;
  let spark = $state<HistoryResponse | null>(null);
  const minute = $derived(s ? Math.floor(s.now / 60) : null);
  $effect(() => {
    if (minute == null) return;
    api
      .history(SPARK_HOURS, STEP)
      .then((r) => (spark = r))
      .catch(() => {});
  });
  function samples(z: ZoneState): Sample[] {
    const pts = spark?.zones.find((x) => x.number === z.number)?.points ?? [];
    return [
      ...pts.map((p) => ({ ts: p.ts, v: p.temperature })),
      { ts: s?.now ?? 0, v: z.temperature },
    ];
  }

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
        <span class="text-[13px] text-mu">{active.length} on{lead ? ` · ${lead.name} driving` : ''}</span>
      </div>
      <div class="flex flex-col gap-3.5 px-4 md:px-0">
        {#if active.length}
          <div class="grid grid-cols-[repeat(auto-fit,minmax(min(300px,100%),1fr))] gap-3">
            {#each active as zone (zone.number)}
              <ZoneCard
                {zone}
                limits={s.ac?.limits ?? null}
                {heating}
                driving={zone.number === lead?.number}
                samples={samples(zone)}
                maxGap={STEP * 3}
              />
            {/each}
          </div>
        {:else}
          <p class="m-0 rounded-[18px] bg-s1 p-5 text-[15px] text-mu">All rooms are off — turn one on below to start.</p>
        {/if}
        {#if off.length}
          <div class="flex flex-col gap-2 pt-1.5">
            <h3 class="m-0 px-1 text-[13px] font-semibold tracking-[0.06em] text-mu uppercase">Off</h3>
            <ul class="m-0 list-none rounded-[18px] p-0 shadow-[inset_0_0_0_1px_var(--line)]">
              {#each off as zone (zone.number)}
                <OffRoom {zone} />
              {/each}
            </ul>
          </div>
        {/if}
      </div>
    </section>
  </div>
{/if}
