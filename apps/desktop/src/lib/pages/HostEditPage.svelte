<script lang="ts">
  import HostEditor from '../components/HostEditor.svelte';
  import { route } from '../stores/route.svelte';
</script>

<!-- One editor per `route.edit()`: opening it on another host, or on a new
     one, starts a fresh form. -->
{#key route.editToken}
  <HostEditor />
{/key}
