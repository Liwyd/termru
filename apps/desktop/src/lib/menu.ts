/** One entry of a popup menu (`PopMenu.svelte`). */
export interface MenuEntry {
  id: string;
  label: string;
  /** The shortcut, shown on the right. */
  hint?: string;
  /** A destructive entry. */
  danger?: boolean;
  disabled?: boolean;
  /** Why it is disabled (read out and shown as a tooltip). */
  reason?: string;
  /** A rule above this entry. */
  separatorBefore?: boolean;
}
