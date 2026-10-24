<script lang="ts">
  import SyncPanel from '../components/SyncPanel.svelte';
  import { route } from '../stores/route.svelte';
  import { sync } from '../stores/sync.svelte';

  // R2 placeholder: the existing dialog is shown open on its own route.
  // R5 replaces it with the page layout.
  let visible = $state(true);

  $effect(() => {
    if (!visible) route.go('hosts');
  });

  $effect(() => {
    if (visible) void sync.load();
  });
</script>

<SyncPanel bind:open={visible} />
