import type { ActiveNotice } from '../notify.svelte';

/** What a reader sees of a notice: the title, then the sentence under it. */
export function noticeText(notice: ActiveNotice | undefined): string {
  if (!notice) return '';
  return notice.description ? `${notice.title}. ${notice.description}` : notice.title;
}
