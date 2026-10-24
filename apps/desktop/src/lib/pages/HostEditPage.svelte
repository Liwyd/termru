<script lang="ts">
  import HostEditor from '../components/HostEditor.svelte';
  import { route } from '../stores/route.svelte';
</script>

<HostEditor onmanageidentities={() => route.go('identities')} />
