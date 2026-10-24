<script lang="ts">
  import SettingsPanel from '../components/SettingsPanel.svelte';
  import { route } from '../stores/route.svelte';

  // R2 placeholder: the existing dialog is shown open on its own route.
  // R5 replaces it with the two-column page layout.
  let visible = $state(true);

  $effect(() => {
    if (!visible) route.go('hosts');
  });
</script>

<SettingsPanel bind:open={visible} />
