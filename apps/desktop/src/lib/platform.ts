/** The platforms the window chrome differs on. */
export type UiPlatform = 'macos' | 'windows' | 'linux';

/**
 * The platform the app is running on, from the webview's user agent.
 *
 * The window decorations are applied in Rust per `cfg(target_os)`; this
 * selector only decides which in-app affordances the titlebar shows.
 */
export function uiPlatform(): UiPlatform {
  if (typeof navigator === 'undefined') return 'linux';
  const hint = `${navigator.platform ?? ''} ${navigator.userAgent}`;
  if (/Mac|iPhone|iPad|iPod/.test(hint)) return 'macos';
  if (/Win/.test(hint)) return 'windows';
  return 'linux';
}
