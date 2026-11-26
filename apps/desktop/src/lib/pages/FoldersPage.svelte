<script lang="ts">
  import FolderEditor from '../components/FolderEditor.svelte';
  import { route } from '../stores/route.svelte';

  // R2 placeholder: the editor is shown in flow as a page. R5 replaces it
  // with the three-column page layout.
</script>

<FolderEditor embedded onclose={() => route.back()} />
