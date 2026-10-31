<script lang="ts">
  import { ChartLine, House, List, Moon, SlidersHorizontal, Sun } from '@lucide/svelte';
  import type { Snippet } from 'svelte';

  import { ago } from '../format';
  import { live } from '../live.svelte';
  import { router, routes } from '../router.svelte';
  import { theme } from '../theme.svelte';

  let { children }: { children: Snippet } = $props();

  const icons = { home: House, history: ChartLine, activity: List, settings: SlidersHorizontal };

  const connection = $derived.by(() => {
    if (live.error && !live.state) return { ok: false, text: "Can't reach Climate" };
    const at = live.state?.airtouch;
    if (!at) return { ok: false, text: 'Connecting…' };
    if (!at.connected) return { ok: false, text: 'AirTouch offline' };
    return { ok: true, text: `AirTouch connected · ${ago(at.last_packet_at, live.now)}` };
  });
</script>

<div class="mx-auto flex min-h-dvh max-w-[1240px] flex-col md:px-6">
  <header class="flex items-center justify-between gap-3 py-4 pr-3 pl-5 md:px-0 md:py-[18px]">
    <div class="flex items-center gap-7">
      <div class="flex flex-col gap-0.5">
        <span class="text-[17px] font-semibold tracking-[-0.01em] md:text-lg">Climate</span>
        <span class="flex items-center gap-1.5 text-xs text-mu md:hidden">
          <span class={['size-1.5 rounded-full', connection.ok ? 'bg-mu' : 'bg-err']}></span>
          {connection.text}
        </span>
      </div>
      <nav aria-label="Sections" class="hidden gap-1 md:flex">
        {#each routes as r (r.id)}
          <a
            href={r.href}
            aria-current={router.route === r.id ? 'page' : undefined}
            class={[
              'rounded-[10px] px-3.5 py-2.5 text-sm',
              router.route === r.id ? 'bg-s1 font-semibold' : 'text-mu',
            ]}>{r.label}</a
          >
        {/each}
      </nav>
    </div>
    <div class="flex items-center gap-3">
      <span class="hidden items-center gap-2 text-[13px] text-mu md:flex">
        <span class={['size-[7px] rounded-full', connection.ok ? 'bg-mu' : 'bg-err']}></span>
        {connection.text}
      </span>
      <button
        type="button"
        aria-label="Switch light or dark mode"
        onclick={() => theme.toggle()}
        class="flex size-11 items-center justify-center rounded-full"
      >
        {#if theme.current === 'dark'}<Sun size={20} />{:else}<Moon size={20} />{/if}
      </button>
    </div>
  </header>

  <main class="flex-1 pb-28 md:pb-10">
    {@render children()}
  </main>

  <nav
    aria-label="Sections"
    class="fixed inset-x-0 bottom-0 grid grid-cols-4 border-t border-line bg-bg px-2 pt-2 pb-[max(24px,env(safe-area-inset-bottom))] md:hidden"
  >
    {#each routes as r (r.id)}
      {@const Icon = icons[r.id]}
      <a
        href={r.href}
        aria-current={router.route === r.id ? 'page' : undefined}
        class={[
          'flex flex-col items-center gap-1 py-2 text-[11px]',
          router.route === r.id ? 'font-semibold text-tx' : 'text-mu',
        ]}
      >
        <Icon size={22} strokeWidth={1.8} aria-hidden="true" />{r.label}
      </a>
    {/each}
  </nav>

  {#if live.notice}
    <div
      role="status"
      class="fixed inset-x-4 bottom-28 mx-auto max-w-md rounded-2xl bg-tx px-4 py-3 text-sm text-bg shadow-lg md:bottom-8"
    >
      {live.notice}
    </div>
  {/if}
</div>
