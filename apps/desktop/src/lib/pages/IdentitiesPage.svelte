<script lang="ts">
  import IdentityEditor from '../components/IdentityEditor.svelte';
  import { route } from '../stores/route.svelte';

  // R2 placeholder: the existing dialog is shown open on its own route.
  // R5 replaces it with the list + rail page layout.
  let visible = $state(true);

  $effect(() => {
    if (!visible) route.go('hosts');
  });
</script>

<IdentityEditor bind:open={visible} />
