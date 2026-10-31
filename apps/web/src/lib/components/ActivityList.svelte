<script lang="ts">
  import type { ActivityItem } from '@aircon/api';

  import { clock } from '../format';

  let { items, compact = false }: { items: ActivityItem[]; compact?: boolean } = $props();

  function tag(e: ActivityItem): string | null {
    if (e.kind === 'error') return 'Error';
    if (e.dry_run) return 'Dry run';
    if (e.kind === 'external' && e.reason.includes('AirTouch app')) return 'AirTouch app';
    return null;
  }
</script>

<ol class="m-0 list-none p-0">
  {#each items as e, i (e.id)}
    {@const t = tag(e)}
    <li class={['flex gap-2.5', compact ? 'px-[18px] py-2.5' : 'px-4 py-3.5', i > 0 && 'border-t border-line']}>
      <span class="w-[42px] flex-none text-[13px] text-mu">{clock(e.ts)}</span>
      {#if !compact}
        <span
          aria-hidden="true"
          class={[
            'mt-[5px] size-2.5 flex-none rounded-full',
            e.kind === 'decision' && 'bg-cool',
            e.kind === 'error' && 'bg-err',
            e.kind === 'external' && 'shadow-[inset_0_0_0_2px_var(--tx)]',
            e.kind === 'info' && 'bg-mu',
          ]}
        ></span>
      {/if}
      <div class="flex min-w-0 flex-col gap-0.5">
        <span class={['font-semibold', compact ? 'text-sm' : 'text-[15px]']}>{e.title}</span>
        {#if e.reason}<span class="text-[13px] leading-snug text-mu">{e.reason}</span>{/if}
        {#if t && !compact}
          <span
            class={[
              'mt-[3px] self-start rounded-md px-[7px] py-0.5 text-[11px] font-semibold',
              t === 'Error' ? 'bg-err-bg text-err' : 'border border-line text-mu',
            ]}>{t}</span
          >
        {/if}
      </div>
    </li>
  {/each}
</ol>
