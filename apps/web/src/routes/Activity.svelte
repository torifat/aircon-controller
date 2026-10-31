<script lang="ts">
  import type { ActivityItem, ActivityKind } from '@aircon/api';
  import { untrack } from 'svelte';

  import { api } from '../lib/api';
  import ActivityList from '../lib/components/ActivityList.svelte';
  import { dayLabel } from '../lib/format';
  import { live } from '../lib/live.svelte';

  const filters: { id: ActivityKind | 'all'; label: string }[] = [
    { id: 'all', label: 'All' },
    { id: 'decision', label: 'Decisions' },
    { id: 'external', label: 'External' },
    { id: 'error', label: 'Errors' },
  ];
  const PAGE = 100;

  let filter = $state<ActivityKind | 'all'>('all');
  let items = $state<ActivityItem[]>([]);
  let counts = $state<Record<string, number>>({});
  let more = $state(false);
  let loading = $state(true);

  async function load(reset: boolean) {
    loading = true;
    try {
      const kind = filter === 'all' ? undefined : filter;
      const before = reset ? undefined : items.at(-1)?.id;
      const r = await api.activity({ kind, before, limit: PAGE });
      items = reset ? r.items : [...items, ...r.items];
      counts = r.counts;
      more = r.items.length === PAGE;
    } catch (e) {
      live.flash(e instanceof Error ? e.message : String(e));
    } finally {
      loading = false;
    }
  }

  // A new filter starts over.
  $effect(() => {
    void filter;
    untrack(() => void load(true));
  });

  // Pick up new entries as the controller checks in, unless older pages are loaded.
  $effect(() => {
    void live.state?.now;
    untrack(() => {
      if (!loading && items.length <= PAGE) void load(true);
    });
  });

  const total = $derived(Object.values(counts).reduce((a, b) => a + b, 0));
  const groups = $derived.by(() => {
    const out: { day: string; items: ActivityItem[] }[] = [];
    for (const e of items) {
      const day = dayLabel(e.ts, live.now);
      if (out.at(-1)?.day !== day) out.push({ day, items: [] });
      out.at(-1)!.items.push(e);
    }
    return out;
  });
</script>

<div class="flex flex-col gap-1 px-5 pt-5 pb-2 md:px-0">
  <h1 class="m-0 text-[28px] font-semibold tracking-[-0.02em]">Activity</h1>
  <span class="text-sm text-mu">Every decision and every change seen in AirTouch</span>
</div>

<div role="group" aria-label="Filter" class="flex flex-wrap gap-2 px-4 pt-2 pb-4 md:px-0">
  {#each filters as f (f.id)}
    {@const on = filter === f.id}
    <button
      type="button"
      aria-pressed={on}
      onclick={() => (filter = f.id)}
      class={[
        'min-h-9 rounded-full border px-3.5 text-[13px] font-semibold',
        on ? 'border-tx bg-tx text-bg' : 'border-line text-mu',
      ]}
    >
      {f.label}
      <span class="ml-1 opacity-70">{f.id === 'all' ? total : (counts[f.id] ?? 0)}</span>
    </button>
  {/each}
</div>

<div class="flex flex-col gap-4 px-4 md:max-w-3xl md:px-0">
  {#each groups as g (g.day)}
    <section class="flex flex-col gap-2">
      <h2 class="m-0 px-1 text-[13px] font-semibold text-mu">{g.day}</h2>
      <div class="rounded-[20px] bg-s1"><ActivityList items={g.items} /></div>
    </section>
  {:else}
    {#if !loading}
      <p class="m-0 rounded-[20px] bg-s1 p-6 text-sm text-mu">Nothing logged yet.</p>
    {/if}
  {/each}
  {#if more}
    <button
      type="button"
      disabled={loading}
      onclick={() => load(false)}
      class="min-h-11 self-center rounded-xl border border-line px-4 text-sm font-semibold"
      >Show older</button
    >
  {/if}
</div>
