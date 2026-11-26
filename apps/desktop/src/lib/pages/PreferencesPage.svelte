<script lang="ts">
  import SettingsPanel from '../components/SettingsPanel.svelte';
  import { route } from '../stores/route.svelte';

  // R2 placeholder: the panel is shown in flow as a page. R5 replaces it
  // with the two-column page layout.
</script>

<SettingsPanel embedded onclose={() => route.back()} />
