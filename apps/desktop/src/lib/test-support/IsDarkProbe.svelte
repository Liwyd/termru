<script lang="ts">
  import { prefs } from '../stores/prefs.svelte';
</script>

<span data-testid="theme">{prefs.isDark ? 'dark' : 'light'}</span>
