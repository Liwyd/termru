/**
 * Brings a section to the top of the element that scrolls it, and scrolls
 * nothing else. `scrollIntoView` would also scroll every `overflow: hidden`
 * ancestor (the page, the shell) to put the section at *its* top, and the
 * header of the app would slide out of view. The jump is instant: a key may
 * have started it, and nothing a key starts animates.
 */

/** How far under the top edge the section lands (a little air over its title). */
export const SECTION_GAP = 8;

export function scrollSectionToTop(scroller: HTMLElement, section: HTMLElement): void {
  const offset = section.getBoundingClientRect().top - scroller.getBoundingClientRect().top;
  scroller.scrollTop += offset - SECTION_GAP;
}
