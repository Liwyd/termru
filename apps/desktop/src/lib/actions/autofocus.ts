/** Focuses (and selects the text of) an input as soon as it is in the page. */
export function autofocus(node: HTMLInputElement): void {
  node.focus();
  node.select();
}
