<script lang="ts">
  import FolderEditor from '../components/FolderEditor.svelte';
  import { route } from '../stores/route.svelte';

  // R2 placeholder: the existing dialog is shown open on its own route.
  // R5 replaces it with the three-column page layout.
  let visible = $state(true);

  $effect(() => {
    if (!visible) route.go('hosts');
  });
</script>

<FolderEditor bind:open={visible} />
