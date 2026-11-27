# Accessibility

What is checked, how to check it again, and how to reach and operate every
control of every page without a mouse. Shortcuts are the Linux and Windows
defaults (`Ctrl+Shift+…`); on macOS they are `Cmd+…` (see Preferences, Keyboard
shortcuts, which shows both columns and lets you rebind any of them).

## What is tested

`apps/desktop/src/lib/a11y.test.ts` mounts the whole app (shell and page, as a
screen reader meets it) in jsdom and runs axe-core 4.14 over it with the tags
`wcag2a`, `wcag2aa`, `wcag21a`, `wcag21aa`, `wcag22aa` and `best-practice`.
It covers:

- every route: Hosts (loaded, with sessions running, empty, failed to load),
  Sessions (a live tab, nothing open), the host editor (new, editing, with
  errors showing), Folders (loaded, empty), Identities (loaded, empty), Sync
  (signed out, with form errors, signed in with a conflict), Preferences;
- every dialog and layer: the delete confirmation, the row menu, the connect
  dialog, the host key question (unknown and changed), the sign-in question
  (keyboard-interactive and passphrase), the close-session question, the
  multi-line paste question, the command palette, the restore-defaults
  question, notices;
- form errors: in every form (host editor, Sync sign-in, Preferences, Folders,
  Identities, the passphrase retry) each control that reports an error has
  `aria-invalid="true"` and an `aria-describedby` that points at the text that
  explains it, and every field message is pointed at by its control.

**axe cannot judge everything in jsdom.** There is no layout engine and no
canvas there, so these rules are switched off in `src/lib/test-support/axe.ts`
(not skipped silently):

| Rule | Why it is off | What covers it instead |
|---|---|---|
| `color-contrast`, `color-contrast-enhanced` | needs rendered colours | `design-tokens.test.ts` computes the WCAG ratio of every text and background pair of both palettes |
| `target-size` | needs element rectangles | by hand: row actions are 32 px high, the More button 28 px wide (WCAG 2.2 asks for 24); buttons are 40 px |
| `scrollable-region-focusable` | needs scroll sizes | by hand: the scrolling regions (the hosts table, the folder tree) hold focusable rows and buttons |
| `link-in-text-block` | needs computed colours | there are no inline links in running text; links are buttons |

Run it with `npx vitest run src/lib/a11y.test.ts --maxWorkers=1` in
`apps/desktop`.

## Conventions

- **Landmarks.** The title bar is the banner; `nav` "Primary" holds the six
  pages; the page is `main`; the status bar is a `status` region; notices are
  the region "Notices". Every page has one `h1` (the page title).
- **Names.** Every control has a visible label or an `aria-label`; an icon
  button says what it does and, for rows, to which host ("More actions for
  atlas-prod"). A toggle is a `switch` with the name of its setting.
- **Errors.** The message sits under the field, in a polite live region, and
  is linked with `aria-describedby`; the field has `aria-invalid="true"`. The
  check is on blur, not while typing. A save with errors focuses the first bad
  field and says how many there are. A message that belongs to no single field
  (a refused save, a failed sync) is `role="alert"`.
- **Disabled.** A button is not silently disabled: it carries a reason as a
  `title` (and `aria-disabled` where it must stay reachable).
- **Focus.** A 2 px accent ring with a 2 px offset on every control (3 px with
  the High-visibility focus preference); never removed. A dialog traps Tab,
  focuses its first control (the safe answer where there is one: Cancel, or the
  host key question's Cancel) and gives the focus back to what opened it.
- **Status is never colour alone.** A connected session is a square glyph plus
  the word "Connected"; the active page, tab and filter also change weight.
- **Motion.** Nothing a key starts is animated (palette, tab and route
  switches, pane focus, menus opened with the keyboard). Reduced motion (the
  operating system setting or Preferences, Reduce motion) removes every move
  and keeps an 80 ms fade.
- **Language.** `lang="en"`; the interface is English only.

## Keyboard checklist

Every page is reached from the nav (Tab to its item; Enter or Space opens it;
each page is one Tab stop) or with the command palette (`Ctrl+Shift+P`, type
"Go to Folders"). From anywhere:

| Key | Does |
|---|---|
| `Ctrl+Shift+P` | Command palette: type, Up and Down choose, Enter runs, Escape closes |
| `Ctrl+Shift+N` | Add a host |
| `Ctrl+Shift+F` | Search hosts (Hosts page; focuses and selects the field) |
| `Ctrl+Shift+S` / `Ctrl+,` | Sync / Preferences |
| `Ctrl+Shift+L` | Toggle the theme |
| `Ctrl+Plus` / `Ctrl+Minus` | Larger or smaller terminal font |
| `Ctrl+Shift+T`, `Ctrl+Shift+W`, `Ctrl+Shift+D` | New, close, duplicate tab |
| `Ctrl+Tab`, `Ctrl+Shift+Tab` | Next, previous tab |

Shortcuts do not fire while a text field has the focus (except the palette
chord) or while a dialog is open.

The tab order of every page starts in the shell: the window buttons (Windows
only: Linux keeps the native title bar, macOS the traffic lights), the six nav items, then the page, then the notices (a notice's
buttons come last, in the order the notices appeared). The status bar has no
focusable controls.

### Hosts

Order: New host, Search hosts, Folder, the filter group (All hosts, Favorites,
Recent, Unfiled), the table, the running-sessions banner.

| Control | How | Operate |
|---|---|---|
| Search | `Ctrl+Shift+F` or Tab | Type; the table filters as you type. The clear button of the search field empties it |
| Folder | Tab | Arrow keys change the folder (a native select) |
| Filter group | Tab through four toggle buttons | Enter or Space; the pressed one says `aria-pressed` and is weightier |
| Table | Tab once: the table has one stop, the current row | Up, Down, Home, End, PageUp, PageDown move between rows (a row scrolls into view); Enter connects (or opens the running session); `E` or `F2` edits; the Menu key or `Shift+F10` opens the row menu |
| Buttons of a row | Tab from the row: Star, Connect or Open, Edit, More (only the current row's buttons are tab stops) | Enter or Space. Star toggles the favorite (`aria-pressed`). More opens a menu |
| Row menu | Opens on the first entry | Up, Down, Home, End move; Enter chooses; Escape closes and returns to the button; Tab leaves |
| Delete | row menu, then a confirmation | The confirmation focuses Cancel; Tab to Delete |
| Banner | Tab after the table | "Return to …" opens the first running session |
| Empty, filtered-empty, error | the state's button | New host, Clear filters, Retry |

### Sessions

Order: the open sessions (each: its name, then its close button), New session,
the toolbar (Split H, Split V, Duplicate, Detach, More), the panes, the session
bar (Reconnect, Close session).

| Control | How | Operate |
|---|---|---|
| Session list | Tab through name and close buttons | Enter or Space on the name switches to it (`aria-current` marks the open one); `F2` renames (Enter keeps, Escape cancels); the close button asks first when the session is live |
| Toolbar | Tab | Enter or Space. A disabled button says why in its title |
| Pane | `Ctrl+Alt+Right`, `Ctrl+Alt+Left` or click | The terminal has the focus: typing goes to the shell. `Ctrl+Shift+E` and `Ctrl+Shift+O` split, `Ctrl+Shift+Q` closes the pane |
| Pane header | Tab | Expand or Restore the pane, Close the pane |
| Divider | Tab (it is a focusable separator with a value) | Arrow keys resize by 5 %; double-click evens the panes |
| Terminal menu | the Menu key or `Shift+F10` in the terminal | Copy, Paste, Select all, Clear scrollback, Find, Split. Same keys as every menu |
| Find in terminal | `Ctrl+Shift+F` in the terminal | Enter next, `Shift+Enter` previous, Escape closes and returns to the terminal |
| Paste | `Ctrl+Shift+V` | A paste of several lines asks first |
| Empty pane | Tab | Search hosts for this pane, then a host's button |

### Host editor

Order: the "Hosts" crumb, then the four sections top to bottom, the rail, the
footer.

| Section | Controls |
|---|---|
| 01 Connection | Name, Username, Address, Port |
| 02 Authentication | Method (three radio buttons: Left and Right move, Space selects), then the password field and "Save in this device's credential store", or the Identity select and "Manage identities…", or nothing for the agent |
| 03 Organization | Folder, Favorite (a toggle), Tags (one toggle per tag), New tag with Add tag (Enter in the field adds), Comment |
| 04 Advanced | Terminal type, Keepalive |
| Rail | Forget password, Forget passphrase (when one is stored) |
| Footer | Cancel, Save host, Save & connect |

A field is checked when you leave it. Save host with a problem focuses the
first field that has one and announces "Fix N fields before saving". Leaving
with unsaved changes (the Hosts crumb, a nav item, Cancel, closing the window)
asks first: the question focuses Keep editing; Escape keeps editing.

### Folders

Order: New folder, the folder tree, the list (a Move select per host, Choose
hosts), the rail.

| Control | How | Operate |
|---|---|---|
| Folder tree | Tab through every folder (the selected one has `aria-current`) | Up and Down move the focus; Enter or Space selects. The tree is always open |
| Move | the select of a host row | Arrow keys choose the folder; the host moves at once |
| Choose hosts | Tab | Opens a search field and a list of hosts; "Move here" per host |
| Rail form | Tab | Name, Parent folder, Cancel, Save folder; an impossible parent says why (cycle guard) |
| Delete folder | Tab to the rail | The confirmation says what moves and focuses Cancel |

### Identities

Order: New identity, the list, the detail (Choose key…, Unbind, Copy, Edit of
a host), the rail.

| Control | How | Operate |
|---|---|---|
| List | Tab through the identities | Enter or Space selects |
| Rail form (new) | Tab | "Where the key comes from" (two radio buttons), Name, Comment, the passphrase and "Remember the passphrase on this device", or "Choose key file…" |
| Rail form (edit) | Tab | Name, Comment, Cancel, Save identity |
| Delete identity | Tab to the rail | The confirmation says what the deletion affects and focuses Cancel |

### Sync

Signed out: Server address, Email, Password, Device name, Sign in (errors on
blur; a bad field takes the focus on submit). Signed in: Sync now, the
conflicts (for each: Take the server's copy, Keep this device's copy), Change
server or account, Sign out. The status line is a polite live region: it
announces "Talking to the server", "Up to date", "Sync offline" and "Last sync
failed" when they change.

### Preferences

Order: Restore defaults, then Font family, Custom font family (when "Other…" is
chosen), Size, Line height, Theme; Cursor, Cursor blink, Scrollback, Copy on
select, Confirm multi-line paste; the shortcut table; Reduce motion,
High-visibility focus, Routine notices, Preview notice.

| Control | Operate |
|---|---|
| Selects | Arrow keys change the value; it applies and saves at once |
| Number fields (Size, Line height, Scrollback) | Type; checked when you leave the field or press Enter; an out-of-range value says the range and saves nothing |
| Switches | Space or Enter flips it (`role="switch"`, `aria-checked`) |
| Shortcut table | A row has Record, Unbind and Reset buttons, each named for its command. Record says "Press the new keys" (a status); press the chord; Escape stops. A chord that is taken says which command has it |
| Restore defaults | A confirmation that focuses Cancel |

### Dialogs, in short

Connect (password), host key, sign-in question, close session, paste,
discard changes, delete, restore defaults: each is `role="dialog"` or
`"alertdialog"` with a title (`aria-labelledby`) and, where there is one, a
description (`aria-describedby`). Tab and Shift+Tab stay inside; Escape cancels
(for the host key question and the sign-in question that answers "no"); Enter
submits a form dialog. The focus returns to the control that opened it.

## Screen reader notes

- The Hosts table is an ARIA table with `aria-rowcount` and `aria-rowindex`
  (10,000 hosts are 10,001 rows) although only about twenty rows are in the
  page at a time. A screen reader's own row navigation and the webview's
  find-in-page reach the rendered rows only; use the search field to narrow
  the list.
- The session list says which session is open with `aria-current`; a session
  with panes says "/ 2 panes" in its name. The state word (Connected,
  Reconnecting…) is text in the same button.
- A connection that fails or drops puts a `role="alert"` in the pane; the
  status bar line and the pane footer ("INPUT READY · UTF-8", "CONNECTION
  FAILED · INPUT OFF") are text, not colour.
- Notices are `role="status"` (saved, copied) or `role="alert"` (a failure,
  a warning). They take no focus; Preferences, "Routine notices" turns the
  routine ones off, failures always show.
- The tab strip is a list of buttons, not a `tablist` (a tab may not contain
  the close button, and the strip has no arrow-key behaviour). Mind it if you
  expect "tab 1 of 3": you will hear a list of three items.

## Known gaps

- **No test with a real screen reader.** The checks are axe in jsdom and a
  read of the markup. Orca, NVDA and VoiceOver on a real desktop have not been
  used; the announcements above are what the markup asks for.
- **The terminal itself is not read.** xterm's screen reader mode is not
  switched on, so what the remote shell prints is not announced. A screen
  reader user can copy the output and read it elsewhere, but cannot follow a
  session live. This needs a preference and a decision on what to announce.
- **Reordering session tabs is a drag only.** There is no keyboard way to move
  a tab.
- **`role="toolbar"` without arrow keys.** The session toolbar and every
  button in it is a tab stop (about five); the toolbar pattern would make it
  one stop with Left and Right inside.
- **No skip link.** Six nav items (and the window buttons on Windows) come
  before the page content.
- **Contrast and target size are not measured in a browser.** The palette
  pairs are computed from the tokens; hover and selection tints (the accent
  tint of a selected row under muted text) are not.
- **Forced colours (Windows High Contrast) are untried.** The selected row, a
  pressed filter and the active page use a tint and weight; in forced colours
  the tint disappears and only the weight and the underline are left.
- **Zoom.** The narrow layout (760 px) is checked in screenshots; browser zoom
  at 200 % and a larger system text size have not been.
- **The folder tree is a list of buttons, not an ARIA `tree`.** It is always
  open, so there is nothing to expand; the indent is visual, and the tooltip
  and the detail path carry the hierarchy.
- **Real desktop webviews.** Focus rings, the select arrow and the shortcut
  recorder were only looked at in headless Chrome (see `TODO.md`).
