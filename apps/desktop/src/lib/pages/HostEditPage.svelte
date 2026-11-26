<script lang="ts">
  import HostEditor from '../components/HostEditor.svelte';
  import { route } from '../stores/route.svelte';
</script>

<!-- The page shows the editor in flow; saving or cancelling returns to the
     page the user came from. "Manage identities…" parks the form and comes
     back to it. -->
<HostEditor
  embedded
  onclose={() => route.back()}
  onmanageidentities={(draft) => route.leaveEditor(draft, 'identities')}
/>
