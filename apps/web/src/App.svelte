<script lang="ts">
  import Shell from './lib/components/Shell.svelte';
  import { live } from './lib/live.svelte';
  import { router } from './lib/router.svelte';
  import Activity from './routes/Activity.svelte';
  import History from './routes/History.svelte';
  import Home from './routes/Home.svelte';
  import Settings from './routes/Settings.svelte';

  live.start();
</script>

<Shell>
  {#if router.route === 'history'}
    <History />
  {:else if router.route === 'activity'}
    <Activity />
  {:else if router.route === 'settings'}
    <Settings />
  {:else}
    <Home />
  {/if}
</Shell>
