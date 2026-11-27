<script lang="ts">
  import { prefs } from '../stores/prefs.svelte';
  import { terminalTheme } from '../terminal/theme';

  /**
   * A static specimen of the terminal with the preferences applied: the font,
   * its size, the line height, the cursor and the colours of the current
   * theme. It is not a terminal and moves nothing: the cursor does not blink
   * here even when the terminal's does (nothing in chrome animates the caret).
   */

  /** xterm's cell height is about 1.3 times the font size; the line height multiplies it. */
  const GLYPH_ROW = 1.3;

  const terminal = $derived(prefs.prefs.terminal);
  const colors = $derived(terminalTheme(prefs.isDark));
  const row = $derived(Math.round(terminal.font_size * GLYPH_ROW * terminal.line_height * 10) / 10);
</script>

<figure class="preview">
  <figcaption>LIVE APPEARANCE PREVIEW · NO CONNECTION</figcaption>
  <div
    class="screen"
    aria-hidden="true"
    data-testid="preview-screen"
    style:font-family={terminal.font_family}
    style:font-size="{terminal.font_size}px"
    style:line-height="{row}px"
    style:--row="{row}px"
    style:background={colors.background}
    style:color={colors.foreground}
  >
    <div class="line">
      <span style:color={colors.cyan}>ops@atlas</span>
      <span style:color={colors.white}>~</span> $ ls /srv/atlas
    </div>
    <div class="line files">
      <span style:color={colors.blue}>current/</span>
      <span style:color={colors.blue}>releases/</span>
      <span style:color={colors.blue}>compose.yml</span>
    </div>
    <div class="line">
      <span style:color={colors.green}>√</span> Service ready · All checks passed
    </div>
    <div class="line">
      <span style:color={colors.cyan}>ops@atlas</span>
      <span style:color={colors.white}>~</span> $
      <span class="cursor {terminal.cursor_style}" style:background={colors.cursor}></span>
    </div>
  </div>
  <p class="sr-only">
    A sample of terminal text in {terminal.font_family} at {terminal.font_size} pixels, line height {terminal.line_height},
    with a {terminal.cursor_style} cursor.
  </p>
</figure>

<style>
  .preview {
    margin: 0;
    border: 1px solid var(--termru-border);
    border-radius: var(--termru-radius-input);
    background: var(--termru-terminal);
    overflow: hidden;
  }

  figcaption {
    padding: var(--termru-space-3) var(--termru-space-4) 0;
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-meta-size);
    line-height: var(--termru-text-meta-lh);
    letter-spacing: 0.04em;
    color: var(--termru-muted);
    background: var(--termru-terminal);
  }

  .screen {
    padding: var(--termru-space-2) var(--termru-space-4) var(--termru-space-4);
    overflow: hidden;
    white-space: nowrap;
  }

  .files {
    display: flex;
    gap: 2ch;
  }

  .cursor {
    display: inline-block;
    vertical-align: bottom;
  }

  .cursor.block {
    width: 1ch;
    height: calc(var(--row) * 0.8);
  }

  .cursor.bar {
    width: 2px;
    height: calc(var(--row) * 0.8);
  }

  .cursor.underline {
    width: 1ch;
    height: 2px;
    margin-bottom: calc(var(--row) * 0.12);
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
  }
</style>
