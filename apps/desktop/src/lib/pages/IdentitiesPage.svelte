<script lang="ts">
  import IdentityEditor from '../components/IdentityEditor.svelte';
  import { route } from '../stores/route.svelte';

  // R2 placeholder: the editor is shown in flow as a page. R5 replaces it
  // with the list + rail page layout.
</script>

<IdentityEditor embedded onclose={() => route.back()} />
