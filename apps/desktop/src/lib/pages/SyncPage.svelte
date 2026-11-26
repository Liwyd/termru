<script lang="ts">
  import SyncPanel from '../components/SyncPanel.svelte';
  import { route } from '../stores/route.svelte';
  import { sync } from '../stores/sync.svelte';

  // R2 placeholder: the panel is shown in flow as a page. R5 replaces it
  // with the page layout.
  $effect(() => {
    void sync.load();
  });
</script>

<SyncPanel embedded onclose={() => route.back()} />
