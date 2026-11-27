/**
 * `false` while a page is making its first paint, `true` from the next frame
 * on. A block that mounts later arrived while the user was looking
 * (`use:appear={{ enabled: page.value }}`); a block that is part of the page
 * as it opens does not animate.
 *
 * Call it while a component initialises.
 */
export function afterFirstPaint(): { readonly value: boolean } {
  let value = $state(false);
  $effect(() => {
    const frame = requestAnimationFrame(() => {
      value = true;
    });
    return () => cancelAnimationFrame(frame);
  });
  return {
    get value() {
      return value;
    },
  };
}
