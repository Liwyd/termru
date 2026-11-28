<script lang="ts">
  import { onMount, tick } from 'svelte';
  import Icon from '../components/Icon.svelte';
  import PageHeader from '../components/PageHeader.svelte';
  import SectionIndex, { type SectionItem } from '../components/SectionIndex.svelte';
  import SessionDialog from '../components/SessionDialog.svelte';
  import TerminalPreview from '../components/TerminalPreview.svelte';
  import Toggle from '../components/Toggle.svelte';
  import { api } from '../ipc';
  import {
    COMMANDS,
    acceleratorFor,
    acceleratorParts,
    effectiveBindings,
    isMac,
    type Command,
    type CommandId,
  } from '../keybindings';
  import { notify } from '../notify.svelte';
  import { scrollSectionToTop } from '../scroll-section';
  import { errorMessage } from '../stores/hosts.svelte';
  import { intents } from '../stores/intents.svelte';
  import { defaultPrefs, prefs } from '../stores/prefs.svelte';
  import { shortcuts } from '../stores/shortcuts.svelte';
  import type {
    AppInfo,
    CursorStyle,
    MotionPreference,
    Preferences,
    ThemePreference,
  } from '../types';

  /**
   * Preferences (frame 8). Every change applies at once and is saved at once:
   * there is no Save button, the live preview and the rest of the app follow
   * the value. A field with a typed value (size, line height, scrollback, a
   * custom font) is checked when it is left; an invalid value shows its
   * reason under the field and is not saved.
   *
   * The preferences are the synced `preferences` record: with sync on they
   * follow the account. Secrets are never part of it.
   */

  const mac = isMac();

  /**
   * The sections, in order, by the id of their heading: the section index lists
   * them, and the palette's "Preferences: …" entries go to the same ids.
   */
  const SECTIONS: SectionItem[] = [
    { id: 'appearance-label', label: 'Appearance' },
    { id: 'terminal-label', label: 'Terminal' },
    { id: 'shortcuts-label', label: 'Keyboard shortcuts' },
    { id: 'motion-label', label: 'Motion & accessibility' },
    { id: 'notices-label', label: 'Notifications' },
    { id: 'about-label', label: 'About' },
  ];

  /** The element that scrolls the page: the section index follows it. */
  let scroller = $state<HTMLElement | null>(null);

  // ---- saving --------------------------------------------------------------

  /** Applies a change to a copy of the preferences and saves it. */
  async function patch(change: (draft: Preferences) => void): Promise<void> {
    const next = structuredClone($state.snapshot(prefs.prefs)) as Preferences;
    change(next);
    try {
      await prefs.update(next);
    } catch (err) {
      // The store refused it: show what is really saved, and say why.
      await prefs.load();
      notify({ key: 'prefs.save-failed', values: { detail: errorMessage(err) } });
    }
  }

  // ---- font ----------------------------------------------------------------

  const CUSTOM = '__custom__';

  /** Fonts a terminal can use. Geist Mono (the default) and IBM Plex Mono are bundled. */
  const FONTS: { value: string; label: string }[] = [
    { value: 'Geist Mono, monospace', label: 'Geist Mono' },
    { value: 'IBM Plex Mono, monospace', label: 'IBM Plex Mono' },
    {
      value: 'ui-monospace, SFMono-Regular, Menlo, Consolas, monospace',
      label: 'System monospace',
    },
    { value: 'JetBrains Mono, monospace', label: 'JetBrains Mono' },
    { value: 'Fira Code, monospace', label: 'Fira Code' },
    { value: 'Cascadia Code, monospace', label: 'Cascadia Code' },
    { value: 'DejaVu Sans Mono, monospace', label: 'DejaVu Sans Mono' },
    { value: 'Menlo, monospace', label: 'Menlo' },
    { value: 'Consolas, monospace', label: 'Consolas' },
  ];

  const family = $derived(prefs.prefs.terminal.font_family);
  const isPreset = $derived(FONTS.some((font) => font.value === family));
  /** The user chose "Other": the text field is shown even while it is empty. */
  let customOpen = $state(false);
  const fontChoice = $derived(isPreset && !customOpen ? family : CUSTOM);
  let customText = $derived(isPreset ? '' : family);
  let fontError = $state<string | null>(null);

  function chooseFont(event: Event): void {
    const value = (event.currentTarget as HTMLSelectElement).value;
    if (value === CUSTOM) {
      customOpen = true;
      return;
    }
    customOpen = false;
    fontError = null;
    void patch((draft) => (draft.terminal.font_family = value));
  }

  function commitCustomFont(): void {
    const text = customText.trim();
    if ([...text].length > 128) {
      fontError = 'Font family must be at most 128 characters.';
      return;
    }
    // eslint-disable-next-line no-control-regex
    if (/[\u0000-\u001f\u007f-\u009f]/.test(text)) {
      fontError = 'Font family must not contain control characters.';
      return;
    }
    fontError = null;
    // Empty falls back to the default, as it always did.
    const value = text.length > 0 ? text : FONTS[0].value;
    customOpen = text.length === 0 ? false : customOpen;
    void patch((draft) => (draft.terminal.font_family = value));
  }

  // ---- size, line height, scrollback --------------------------------------

  let sizeText = $derived(String(prefs.prefs.terminal.font_size));
  let lineText = $derived(String(prefs.prefs.terminal.line_height));
  /** 10000 shows as 10,000 (a number input cannot group digits). */
  const grouped = (value: number): string => value.toLocaleString('en-US');
  let scrollText = $derived(grouped(prefs.prefs.terminal.scrollback));
  let sizeError = $state<string | null>(null);
  let lineError = $state<string | null>(null);
  let scrollError = $state<string | null>(null);

  function commitSize(): void {
    const value = Number(sizeText);
    if (sizeText.trim() === '' || !Number.isInteger(value) || value < 6 || value > 72) {
      sizeError = 'Size is a whole number from 6 to 72.';
      return;
    }
    sizeError = null;
    if (value !== prefs.prefs.terminal.font_size) {
      void patch((draft) => (draft.terminal.font_size = value));
    }
  }

  function commitLine(): void {
    const value = Number(lineText);
    if (lineText.trim() === '' || !Number.isFinite(value) || value < 1 || value > 3) {
      lineError = 'Line height is a number from 1.0 to 3.0.';
      return;
    }
    lineError = null;
    // Two decimals are what a row height can show.
    const rounded = Math.round(value * 100) / 100;
    if (rounded !== prefs.prefs.terminal.line_height) {
      void patch((draft) => (draft.terminal.line_height = rounded));
    }
  }

  function commitScrollback(): void {
    const plain = scrollText.replace(/[,\s]/g, '');
    const value = Number(plain);
    if (plain === '' || !/^\d+$/.test(plain) || value < 1000 || value > 1_000_000) {
      scrollError = 'Scrollback is a whole number from 1,000 to 1,000,000 lines.';
      return;
    }
    scrollError = null;
    scrollText = grouped(value);
    if (value !== prefs.prefs.terminal.scrollback) {
      void patch((draft) => (draft.terminal.scrollback = value));
    }
  }

  // ---- simple choices ------------------------------------------------------

  const THEMES: { value: ThemePreference; label: string }[] = [
    { value: 'system', label: 'System' },
    { value: 'light', label: 'Light' },
    { value: 'dark', label: 'Dark' },
  ];

  const CURSORS: { value: CursorStyle; label: string }[] = [
    { value: 'block', label: 'Block' },
    { value: 'bar', label: 'Bar' },
    { value: 'underline', label: 'Underline' },
  ];

  const MOTION: { value: MotionPreference; label: string }[] = [
    { value: 'system', label: 'Follow the system' },
    { value: 'on', label: 'Reduce' },
    { value: 'off', label: 'Keep full motion' },
  ];

  // ---- notices -------------------------------------------------------------

  function previewNotice(): void {
    notify({ key: 'prefs.preview' });
  }

  // ---- shortcuts -----------------------------------------------------------

  /** Commands grouped as the palette groups them, in table order. */
  const groups = $derived.by(() => {
    const out: { name: string; commands: Command[] }[] = [];
    for (const command of COMMANDS) {
      const group = out.find((entry) => entry.name === command.group);
      if (group) group.commands.push(command);
      else out.push({ name: command.group, commands: [command] });
    }
    return out;
  });

  /** Keys the terminal itself handles; they cannot be rebound. */
  const BUILT_IN: { title: string; mac: string; pc: string }[] = [
    { title: 'Copy the terminal selection', mac: 'Cmd+C', pc: 'Ctrl+Shift+C' },
    { title: 'Paste into the terminal', mac: 'Cmd+V', pc: 'Ctrl+Shift+V' },
    { title: 'Find in the terminal', mac: 'Cmd+F', pc: 'Ctrl+Shift+F' },
  ];

  const overrides = $derived(prefs.prefs.keybindings);

  function keysFor(id: CommandId, forMac: boolean): string[] {
    return acceleratorParts(acceleratorFor(id, overrides, forMac), forMac);
  }

  /** The command being recorded takes every key until Escape or a combination. */
  $effect(() => {
    if (shortcuts.recording === null) return;
    const onKey = (event: KeyboardEvent): void => {
      const target = shortcuts.recording;
      if (target === null) return;
      event.preventDefault();
      event.stopPropagation();
      const draft: Record<string, string> = { ...prefs.prefs.keybindings };
      shortcuts.record(event, draft);
      // A combination was taken (the recorder is done and the draft differs).
      if (shortcuts.recording === null && draft[target] !== prefs.prefs.keybindings[target]) {
        void patch((next) => (next.keybindings[target] = draft[target]));
      }
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  });

  // Leaving the page ends a recording that is still running: a recorder left
  // on would swallow every shortcut of the app.
  $effect(() => () => shortcuts.cancel());

  function unbind(id: CommandId): void {
    shortcuts.cancel();
    void patch((draft) => (draft.keybindings[id] = ''));
  }

  function resetBinding(id: CommandId): void {
    shortcuts.cancel();
    void patch((draft) => delete draft.keybindings[id]);
  }

  /** Whether a command has an override (a changed or an emptied binding). */
  function overridden(id: CommandId): boolean {
    return id in overrides;
  }

  const bound = $derived(new Set(effectiveBindings(overrides).map((entry) => entry.commandId)));

  // ---- restore defaults ----------------------------------------------------

  let restoreOpen = $state(false);

  async function restore(): Promise<void> {
    restoreOpen = false;
    shortcuts.cancel();
    sizeError = lineError = scrollError = fontError = null;
    customOpen = false;
    try {
      await prefs.update(defaultPrefs());
      notify({ key: 'prefs.restored' });
    } catch (err) {
      await prefs.load();
      notify({ key: 'prefs.save-failed', values: { detail: errorMessage(err) } });
    }
  }

  // ---- about ---------------------------------------------------------------

  let info = $state<AppInfo | null>(null);
  let infoFailed = $state(false);

  onMount(() => {
    void api
      .appInfo()
      .then((result) => (info = result))
      .catch(() => (infoFailed = true));
  });

  // "Preferences: Keyboard shortcuts" from the palette: the section comes into
  // view (the request may arrive before this page exists).
  $effect(() => {
    const intent = intents.take('preferences-section');
    if (!intent) return;
    void tick().then(() => {
      const heading = document.getElementById(intent.id);
      if (scroller && heading) scrollSectionToTop(scroller, heading.closest('section') ?? heading);
      heading?.focus({ preventScroll: true });
    });
  });
</script>

<div class="workpage prefs">
  <PageHeader title="Preferences">
    {#snippet actions()}
      <button type="button" class="quiet restore" onclick={() => (restoreOpen = true)}>
        Restore defaults
      </button>
    {/snippet}
  </PageHeader>

  <div class="stage-wrap">
    <div class="stage">
      <SectionIndex items={SECTIONS} label="Sections of Preferences" {scroller} />

      <div class="scroll" bind:this={scroller}>
        <div class="column">
          <section aria-labelledby="appearance-label">
            <h2 class="sec-title" id="appearance-label" tabindex="-1">Appearance</h2>

            <TerminalPreview />

            <div class="rows">
              <div class="row">
                <div class="text">
                  <label class="name" for="pref-theme">Theme</label>
                  <p class="desc" id="pref-theme-desc">System follows the operating system.</p>
                </div>
                <select
                  id="pref-theme"
                  class="compact"
                  aria-describedby="pref-theme-desc"
                  value={prefs.prefs.theme}
                  onchange={(event) => {
                    const value = event.currentTarget.value as ThemePreference;
                    void patch((draft) => (draft.theme = value));
                  }}
                >
                  {#each THEMES as theme (theme.value)}
                    <option value={theme.value}>{theme.label}</option>
                  {/each}
                </select>
              </div>

              <div class="row">
                <div class="text">
                  <label class="name" for="pref-font">Font family</label>
                  <p class="desc" id="pref-font-desc">
                    Geist Mono and IBM Plex Mono ship with Termru. Others need to be installed.
                  </p>
                </div>
                <select
                  id="pref-font"
                  class="compact wide"
                  aria-describedby="pref-font-desc"
                  value={fontChoice}
                  onchange={chooseFont}
                >
                  {#each FONTS as font (font.value)}
                    <option value={font.value}>{font.label}</option>
                  {/each}
                  <option value={CUSTOM}>Other…</option>
                </select>
              </div>

              {#if fontChoice === CUSTOM}
                <div class="row">
                  <div class="text">
                    <label class="name" for="pref-font-custom">Custom font family</label>
                    <p class="desc" id="pref-font-custom-desc">
                      A CSS font list, for example Iosevka, monospace.
                    </p>
                    {#if fontError}<p class="error" id="pref-font-custom-message" role="alert">
                        {fontError}
                      </p>{/if}
                  </div>
                  <input
                    id="pref-font-custom"
                    class="wide"
                    type="text"
                    placeholder="Iosevka, monospace"
                    spellcheck="false"
                    autocomplete="off"
                    aria-invalid={fontError ? 'true' : undefined}
                    aria-describedby={fontError
                      ? 'pref-font-custom-message'
                      : 'pref-font-custom-desc'}
                    bind:value={customText}
                    onchange={commitCustomFont}
                    onblur={commitCustomFont}
                  />
                </div>
              {/if}

              <div class="row">
                <div class="text">
                  <label class="name" for="pref-size">Size</label>
                  <p class="desc" id="pref-size-desc">Terminal text, from 6 to 72 pixels.</p>
                  {#if sizeError}<p class="error" id="pref-size-message" role="alert">
                      {sizeError}
                    </p>{/if}
                </div>
                <div class="unit">
                  <input
                    id="pref-size"
                    type="number"
                    min="6"
                    max="72"
                    step="1"
                    inputmode="numeric"
                    aria-invalid={sizeError ? 'true' : undefined}
                    aria-describedby={sizeError ? 'pref-size-message' : 'pref-size-desc'}
                    value={sizeText}
                    oninput={(event) => (sizeText = event.currentTarget.value)}
                    onchange={commitSize}
                    onblur={commitSize}
                  />
                  <span class="suffix" aria-hidden="true">px</span>
                </div>
              </div>

              <div class="row">
                <div class="text">
                  <label class="name" for="pref-line">Line height</label>
                  <p class="desc" id="pref-line-desc">
                    Space between lines, as a multiple of the text, from 1.0 to 3.0.
                  </p>
                  {#if lineError}<p class="error" id="pref-line-message" role="alert">
                      {lineError}
                    </p>{/if}
                </div>
                <div class="unit">
                  <input
                    id="pref-line"
                    type="number"
                    min="1"
                    max="3"
                    step="0.05"
                    inputmode="decimal"
                    aria-invalid={lineError ? 'true' : undefined}
                    aria-describedby={lineError ? 'pref-line-message' : 'pref-line-desc'}
                    value={lineText}
                    oninput={(event) => (lineText = event.currentTarget.value)}
                    onchange={commitLine}
                    onblur={commitLine}
                  />
                </div>
              </div>
            </div>
          </section>

          <section aria-labelledby="terminal-label">
            <h2 class="sec-title" id="terminal-label" tabindex="-1">Terminal</h2>
            <div class="rows">
              <div class="row">
                <div class="text">
                  <p class="name" id="pref-cursor-name">Cursor</p>
                  <p class="desc" id="pref-cursor-desc">Shape of the cursor in the terminal</p>
                </div>
                <select
                  class="compact"
                  aria-labelledby="pref-cursor-name"
                  aria-describedby="pref-cursor-desc"
                  value={prefs.prefs.terminal.cursor_style}
                  onchange={(event) => {
                    const value = event.currentTarget.value as CursorStyle;
                    void patch((draft) => (draft.terminal.cursor_style = value));
                  }}
                >
                  {#each CURSORS as cursor (cursor.value)}
                    <option value={cursor.value}>{cursor.label}</option>
                  {/each}
                </select>
              </div>

              <div class="row">
                <div class="text">
                  <p class="name" id="pref-blink-name">Cursor blink</p>
                  <p class="desc" id="pref-blink-desc">
                    The cursor blinks while the terminal has focus. The preview stays still.
                  </p>
                </div>
                <Toggle
                  labelledby="pref-blink-name"
                  describedby="pref-blink-desc"
                  checked={prefs.prefs.terminal.cursor_blink}
                  onchange={(value) => void patch((draft) => (draft.terminal.cursor_blink = value))}
                />
              </div>

              <div class="row">
                <div class="text">
                  <p class="name" id="pref-scroll-name">Scrollback</p>
                  <p class="desc" id="pref-scroll-desc">Keep recent terminal output in memory</p>
                  {#if scrollError}<p class="error" id="pref-scroll-error" role="alert">
                      {scrollError}
                    </p>{/if}
                </div>
                <div class="unit wide">
                  <input
                    id="pref-scrollback"
                    type="text"
                    inputmode="numeric"
                    aria-labelledby="pref-scroll-name"
                    aria-describedby={scrollError
                      ? 'pref-scroll-desc pref-scroll-error'
                      : 'pref-scroll-desc'}
                    aria-invalid={scrollError ? 'true' : undefined}
                    value={scrollText}
                    oninput={(event) => (scrollText = event.currentTarget.value)}
                    onchange={commitScrollback}
                    onblur={commitScrollback}
                  />
                  <span class="suffix" aria-hidden="true">lines</span>
                </div>
              </div>

              <div class="row">
                <div class="text">
                  <p class="name" id="pref-copy-name">Copy on select</p>
                  <p class="desc" id="pref-copy-desc">
                    Selecting text in the terminal copies it to the clipboard
                  </p>
                </div>
                <Toggle
                  labelledby="pref-copy-name"
                  describedby="pref-copy-desc"
                  checked={prefs.prefs.terminal.copy_on_select}
                  onchange={(value) =>
                    void patch((draft) => (draft.terminal.copy_on_select = value))}
                />
              </div>

              <div class="row">
                <div class="text">
                  <p class="name" id="pref-paste-name">Confirm multi-line paste</p>
                  <p class="desc" id="pref-paste-desc">
                    Ask before a paste of several lines reaches the shell. The shell runs each line
                    as soon as it reads it.
                  </p>
                </div>
                <Toggle
                  labelledby="pref-paste-name"
                  describedby="pref-paste-desc"
                  checked={prefs.prefs.terminal.confirm_multiline_paste}
                  onchange={(value) =>
                    void patch((draft) => (draft.terminal.confirm_multiline_paste = value))}
                />
              </div>
            </div>
          </section>

          <section class="shortcuts" aria-labelledby="shortcuts-label">
            <h2 class="sec-title" id="shortcuts-label" tabindex="-1">Keyboard shortcuts</h2>

            <table>
              <thead>
                <tr>
                  <th scope="col">Command</th>
                  <th scope="col" class:other={!mac}>macOS</th>
                  <th scope="col" class:other={mac}>Windows / Linux</th>
                  <th scope="col"><span class="sr-only">Change</span></th>
                </tr>
              </thead>
              {#each groups as group (group.name)}
                <tbody>
                  <tr class="group">
                    <th scope="rowgroup" colspan="4">{group.name}</th>
                  </tr>
                  {#each group.commands as command (command.id)}
                    {@const recording = shortcuts.recording === command.id}
                    <tr>
                      <th scope="row">{command.title}</th>
                      {#each [true, false] as forMac (forMac)}
                        <td class:other={forMac !== mac}>
                          {#if recording && forMac === mac}
                            <span class="recording" role="status">Press the new keys</span>
                          {:else if keysFor(command.id, forMac).length > 0}
                            <span class="keys">
                              {#each keysFor(command.id, forMac) as key, index (index)}
                                <kbd>{key}</kbd>
                              {/each}
                            </span>
                          {:else}
                            <span class="unbound">{overridden(command.id) ? 'Unbound' : '—'}</span>
                          {/if}
                        </td>
                      {/each}
                      <td class="actions">
                        {#if recording}
                          <button
                            type="button"
                            class="quiet small"
                            onclick={() => shortcuts.cancel()}
                          >
                            Cancel
                          </button>
                        {:else}
                          <button
                            type="button"
                            class="quiet small"
                            aria-label="Record a shortcut for {command.title}"
                            onclick={() => shortcuts.start(command.id)}
                          >
                            Record
                          </button>
                          {#if bound.has(command.id)}
                            <button
                              type="button"
                              class="quiet small"
                              aria-label="Unbind {command.title}"
                              onclick={() => unbind(command.id)}
                            >
                              Unbind
                            </button>
                          {/if}
                          {#if overridden(command.id)}
                            <button
                              type="button"
                              class="quiet small"
                              aria-label="Reset {command.title} to its default"
                              onclick={() => resetBinding(command.id)}
                            >
                              Reset
                            </button>
                          {/if}
                        {/if}
                      </td>
                    </tr>
                  {/each}
                </tbody>
              {/each}
              <tbody>
                <tr class="group">
                  <th scope="rowgroup" colspan="4">Built in</th>
                </tr>
                {#each BUILT_IN as entry (entry.title)}
                  <tr>
                    <th scope="row">{entry.title}</th>
                    <td class:other={!mac}>
                      <span class="keys">
                        {#each acceleratorParts(entry.mac, true) as key, index (index)}<kbd
                            >{key}</kbd
                          >{/each}
                      </span>
                    </td>
                    <td class:other={mac}>
                      <span class="keys">
                        {#each acceleratorParts(entry.pc, false) as key, index (index)}<kbd
                            >{key}</kbd
                          >{/each}
                      </span>
                    </td>
                    <td class="actions"><span class="fixed">Fixed</span></td>
                  </tr>
                {/each}
              </tbody>
            </table>

            {#if shortcuts.conflict}
              <p class="error conflict" role="alert">{shortcuts.conflict}</p>
            {/if}
            <p class="note">
              Press Escape to stop recording. Shortcuts follow the active OS. Ctrl C remains the
              terminal interrupt on Windows and Linux.
            </p>
          </section>

          <section aria-labelledby="motion-label">
            <h2 class="sec-title" id="motion-label" tabindex="-1">Motion &amp; accessibility</h2>
            <div class="rows">
              <div class="row">
                <div class="text">
                  <p class="name" id="pref-motion-name">Reduce motion</p>
                  <p class="desc" id="pref-motion-desc">Follow the operating system preference</p>
                </div>
                <select
                  class="compact wide"
                  aria-labelledby="pref-motion-name"
                  aria-describedby="pref-motion-desc"
                  value={prefs.prefs.reduced_motion}
                  onchange={(event) => {
                    const value = event.currentTarget.value as MotionPreference;
                    void patch((draft) => (draft.reduced_motion = value));
                  }}
                >
                  {#each MOTION as option (option.value)}
                    <option value={option.value}>{option.label}</option>
                  {/each}
                </select>
              </div>
              <div class="row">
                <div class="text">
                  <p class="name" id="pref-focus-name">High-visibility focus</p>
                  <p class="desc" id="pref-focus-desc">
                    A thicker focus ring on every control. Focus never relies on color alone.
                  </p>
                </div>
                <Toggle
                  labelledby="pref-focus-name"
                  describedby="pref-focus-desc"
                  checked={prefs.prefs.focus_visibility}
                  onchange={(value) => void patch((draft) => (draft.focus_visibility = value))}
                />
              </div>
            </div>
          </section>

          <section aria-labelledby="notices-label">
            <h2 class="sec-title" id="notices-label" tabindex="-1">Notifications</h2>
            <div class="rows">
              <div class="row">
                <div class="text">
                  <p class="name" id="pref-notices-name">Routine notices</p>
                  <p class="desc" id="pref-notices-desc">
                    No sound. They never take terminal focus. Failures always show.
                  </p>
                </div>
                <Toggle
                  labelledby="pref-notices-name"
                  describedby="pref-notices-desc"
                  checked={prefs.prefs.routine_notices}
                  onchange={(value) => void patch((draft) => (draft.routine_notices = value))}
                />
              </div>
            </div>

            <div class="specimen" aria-hidden="true">
              <span class="specimen-glyph"><Icon name="check-circle" size={20} /></span>
              <span class="specimen-text">
                <span class="specimen-title">Host saved</span>
                <span class="specimen-desc">web-1 · Saved on this device</span>
              </span>
            </div>
            <div class="preview-row">
              <button type="button" onclick={previewNotice}>Preview notice</button>
              <span class="note">Shows once at the top right</span>
            </div>
            <p class="note">
              Routine notices never take focus. Important connection errors stay next to the session
              so you can act when ready.
            </p>
          </section>

          <section aria-labelledby="about-label">
            <h2 class="sec-title" id="about-label" tabindex="-1">About</h2>
            {#if info}
              <p class="about-name">Termru</p>
              <p class="about-meta">
                Version {info.version} · <span class="mono">{info.name}</span>
              </p>
            {:else if infoFailed}
              <p class="note">The version could not be read.</p>
            {:else}
              <p class="note">Reading the version…</p>
            {/if}
            <p class="note">
              Preferences are saved as you change them. With sync on, they follow your account;
              secrets never do.
            </p>
          </section>
        </div>
      </div>
    </div>
  </div>

  <SessionDialog
    open={restoreOpen}
    title="Restore default preferences?"
    description="Font, theme, cursor, scrollback, notices, accessibility and every shortcut go back to their defaults. Hosts, identities and credentials are not touched."
    onclose={() => (restoreOpen = false)}
  >
    {#snippet actions()}
      <button type="button" class="quiet" data-autofocus onclick={() => (restoreOpen = false)}>
        Cancel
      </button>
      <button type="button" class="danger-fill" onclick={() => void restore()}>
        Restore defaults
      </button>
    {/snippet}
  </SessionDialog>
</div>

<style>
  .restore {
    font-weight: 550;
  }

  /* The index on the left, the sections to its right. The room is the width of
     this box (the sidebar takes its share first). */
  .stage-wrap {
    display: flex;
    flex: 1;
    min-height: 0;
    min-width: 0;
    container: stage / inline-size;
  }

  .stage {
    display: grid;
    grid-template-columns: 172px minmax(0, 1fr);
    grid-template-rows: minmax(0, 1fr);
    flex: 1;
    min-width: 0;
    min-height: 0;
  }

  .scroll {
    min-width: 0;
    min-height: 0;
    overflow: auto;
    padding: var(--termru-page-inset-top) var(--termru-page-inset-x) var(--termru-space-48)
      var(--termru-space-12);
  }

  .column {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-40);
    max-width: 760px;
    min-width: 0;
  }

  .column > section {
    display: flex;
    flex-direction: column;
    gap: var(--termru-space-12);
    min-width: 0;
    scroll-margin-top: var(--termru-space-8);
  }

  /* A group's title: sentence case, 15 px, the same line as a page's title. */
  .sec-title {
    margin: 0;
    font-size: var(--termru-text-title-size);
    line-height: var(--termru-text-title-lh);
    font-weight: var(--termru-text-title-weight);
    letter-spacing: -0.01em;
    color: var(--termru-text);
  }

  .sec-title:focus {
    outline: none;
  }

  .sec-title:focus-visible {
    outline: var(--termru-focus-w) solid var(--termru-accent-text);
    outline-offset: var(--termru-focus-offset);
    border-radius: 4px;
  }

  /* ---- setting rows: the name and what it does, the control at the right ---- */

  .rows {
    display: flex;
    flex-direction: column;
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--termru-space-24);
    padding: var(--termru-space-10) 0;
  }

  .text {
    min-width: 0;
  }

  .name {
    display: block;
    margin: 0;
    font-size: var(--termru-text-body-size);
    line-height: var(--termru-text-body-lh);
    font-weight: 500;
    color: var(--termru-text);
  }

  .desc {
    margin: 2px 0 0;
    max-width: 52ch;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-text-3);
  }

  .error {
    margin: var(--termru-space-4) 0 0;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-err-text);
  }

  .error.conflict {
    margin-top: var(--termru-space-12);
  }

  .note {
    margin: 0;
    max-width: 62ch;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-text-3);
  }

  /* The controls of a row keep a width of their own. */
  .row select,
  .row input {
    flex: none;
    width: 168px;
  }

  .row .wide {
    width: 220px;
  }

  .unit {
    position: relative;
    flex: none;
    width: 112px;
  }

  .unit.wide {
    width: 168px;
  }

  .unit input {
    width: 100%;
    padding-right: 40px;
  }

  .unit.wide input {
    padding-right: 56px;
  }

  .unit .suffix {
    position: absolute;
    top: 50%;
    right: var(--termru-space-12);
    transform: translateY(-50%);
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-secondary-size);
    color: var(--termru-text-3);
    pointer-events: none;
  }

  /* A compact select, as the frame's small mono "Block ▾". */
  .compact {
    min-width: 132px;
    padding-left: var(--termru-space-12);
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-secondary-size);
  }

  /* ---- shortcuts ---- */

  table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
  }

  thead th {
    padding: 0 var(--termru-space-12) var(--termru-space-8) var(--termru-space-12);
    text-align: left;
    font-size: var(--termru-text-meta-size);
    font-weight: var(--termru-text-meta-weight);
    color: var(--termru-text-3);
  }

  tbody th[scope='row'] {
    text-align: left;
    font-weight: 400;
    font-size: var(--termru-text-body-size);
    color: var(--termru-text);
  }

  td,
  tbody th[scope='row'] {
    height: 40px;
    padding: 0 var(--termru-space-12);
    vertical-align: middle;
  }

  @media (hover: hover) and (pointer: fine) {
    tbody tr:not(.group):hover :is(td, th) {
      background: color-mix(in srgb, var(--termru-raised) 60%, transparent);
    }
  }

  tbody tr:not(.group) > :first-child {
    border-radius: var(--termru-radius-sm) 0 0 var(--termru-radius-sm);
  }

  tbody tr:not(.group) > :last-child {
    border-radius: 0 var(--termru-radius-sm) var(--termru-radius-sm) 0;
  }

  tr.group th {
    padding: var(--termru-space-20) var(--termru-space-12) var(--termru-space-4);
    text-align: left;
    font-family: var(--termru-font-ui);
    font-size: var(--termru-text-meta-size);
    font-weight: var(--termru-text-meta-weight);
    color: var(--termru-text-3);
  }

  tbody:first-of-type tr.group th {
    padding-top: var(--termru-space-4);
  }

  .keys {
    display: inline-flex;
    flex-wrap: wrap;
    gap: 6px;
    font-family: var(--termru-font-mono);
    font-size: var(--termru-text-secondary-size);
  }

  kbd {
    font: inherit;
  }

  .recording {
    font-family: var(--termru-font-mono);
    color: var(--termru-accent-text);
  }

  .unbound,
  .fixed {
    color: var(--termru-text-3);
    font-family: var(--termru-font-mono);
  }

  td.actions {
    white-space: nowrap;
    text-align: right;
  }

  button.small {
    min-height: 28px;
    padding: 0 var(--termru-space-8);
    font-size: var(--termru-text-secondary-size);
  }

  /* ---- notifications and about ---- */

  .specimen {
    display: flex;
    align-items: center;
    gap: var(--termru-space-12);
    max-width: 330px;
    margin-top: var(--termru-space-4);
    padding: var(--termru-space-12) var(--termru-space-16);
    border-radius: var(--termru-radius-pane);
    background: var(--termru-side);
  }

  .specimen-glyph {
    display: inline-flex;
    color: var(--termru-accent-text);
  }

  .specimen-text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .specimen-title {
    font-size: var(--termru-text-body-size);
    line-height: var(--termru-text-body-lh);
    font-weight: 550;
  }

  .specimen-desc {
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-text-2);
  }

  .preview-row {
    display: flex;
    align-items: center;
    gap: var(--termru-space-16);
  }

  .about-name {
    margin: 0;
    font-size: var(--termru-text-body-size);
    font-weight: 550;
  }

  .about-meta {
    margin: 0;
    font-size: var(--termru-text-secondary-size);
    line-height: var(--termru-text-secondary-lh);
    color: var(--termru-text-2);
  }

  .mono {
    font-family: var(--termru-font-mono);
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
  }

  @container stage (max-width: 760px) {
    .stage {
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: auto minmax(0, 1fr);
    }

    .scroll {
      padding-left: var(--termru-page-inset-x);
    }
  }

  /* Only the column of this machine's OS: the other one is a reference. */
  @container stage (max-width: 900px) {
    .other {
      display: none;
    }
  }
</style>
