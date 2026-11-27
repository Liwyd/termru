<script lang="ts">
  /**
   * An on/off preference: a switch that says its state in words (On, Off), so
   * the state is never carried by colour alone. It is a button with
   * `role="switch"`, named by the label of its row (`labelledby`) and
   * described by the hint under it (`describedby`).
   */
  let {
    checked,
    onchange,
    labelledby,
    describedby,
    id,
  }: {
    checked: boolean;
    onchange: (next: boolean) => void;
    labelledby: string;
    describedby?: string;
    id?: string;
  } = $props();
</script>

<button
  {id}
  type="button"
  role="switch"
  class="toggle"
  class:on={checked}
  aria-checked={checked}
  aria-labelledby={labelledby}
  aria-describedby={describedby}
  onclick={() => onchange(!checked)}
>
  {checked ? 'On' : 'Off'}
</button>

<style>
  .toggle {
    min-width: 56px;
    min-height: 36px;
    padding: 0 var(--termru-space-3);
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-secondary-size);
    font-weight: 500;
    color: var(--termru-muted);
    /* A switch is flipped with Space as often as with a click: its state
       changes at once. Only the press gives. */
    transition: transform var(--motion-fast) var(--ease-out);
  }

  .toggle.on {
    border-color: color-mix(in srgb, var(--termru-accent) 55%, var(--termru-border));
    background: var(--termru-accent-soft);
    color: var(--termru-accent);
  }
</style>
