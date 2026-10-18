# UI design system

How dm's window looks, and the rules that keep it consistent: the look is
written down as tokens, every widget draws from them, and every visual change
is checked by looking at a picture of the real window. The pictures are dm's
own, drawn in code (`crates/dm-desktop/src/icons.rs`).

## Themes

| Theme | Word | Intent |
|---|---|---|
| Light | `light` | A command bar with one filled accent button, a navigation pane with an accent indicator, the list on a rounded card, thin progress bars |
| Dark | `dark` | The same in dark tokens |
| As the computer is | `system` | The default: Light or Dark, as the operating system is set |
| Classic | `classic` | A plain light window: big coloured toolbar pictures, yellow folders, percentages |

Chosen in Options › General › Look; saved as `theme` in the settings.

## Tokens (`crates/dm-desktop/src/theme.rs`)

| Token | Light | Dark |
|---|---|---|
| `frame` | `#f5f5f5` | `#141414` |
| `pane` | `#fafafa` | `#1f1f1f` |
| `list` | `#ffffff` | `#292929` |
| `border` / `stroke` | `#e0e0e0` / `#d1d1d1` | `#3d3d3d` / `#666666` |
| `text` / `text2` / `weak` | `#242424` / `#424242` / `#616161` | `#ffffff` / `#d6d6d6` / `#adadad` |
| `selection` / `hover` / `pressed` | `#ebebeb` / `#f5f5f5` / `#e0e0e0` | `#383838` / `#3d3d3d` / `#2e2e2e` |
| `brand` / `brand_hover` / `accent` | `#0f6cbd` / `#115ea3` / `#0f6cbd` | `#115ea3` / `#0f6cbd` / `#479ef5` |
| `row_selected` / `row_selected_text` | `#0f6cbd` / `#ffffff` | `#1c76d0` / `#ffffff` |
| `focus_inner` (the focus ring's inner line, inside the `accent` one) | `#ffffff` | `#ffffff` |
| `radius` / `card_radius` | 4 / 8 px | 4 / 8 px |

Type: Inter (SIL OFL 1.1, `crates/dm-desktop/fonts/LICENSE-Inter.txt`), which covers Latin, Cyrillic and Greek in regular and semibold:
body 14, caption 12, titles 18 semibold; sizes, speeds and times in Inter's
tabular figures, so they line up down a column. A soft shadow under dialogs, a
smaller one under menus.

## Rules

- **Layout:** surfaces meet without lines; the list sits on a rounded card;
  the command bar keeps as many names as fit and shows the rest as pictures;
  a dialog's main action is the filled accent button.
- **Contrast is tested, not eyeballed** (`theme.rs`): a selected row
  stands out from the list at 3:1 or more and its words on it at 4.5:1
  (Classic's row is `#0072d0`); the focus ring is two lines, the accent one
  for the list and the frame and a white one inside it for whatever is
  itself in the accent colour (a primary button, a selected row).
- **Scroll bars** in the list and Options take their own room and are
  drawn whether or not the pointer is over them (`theme::steady_scroll`),
  the handle in the text colour; the focus ring is kept inside the list's
  frame. Classic's toolbar buttons are as wide as their longest name, and
  its progress bar's track is drawn in the border colour.
- **Right-to-left names** (Arabic, Hebrew) are put into reading order
  before egui draws them (`bidi.rs`), which lays out a line in stored
  order; a screen reader is given the name as stored.
- **Colours come from tokens.** Read them with `Tokens::get(ctx)`; never
  hard-code a colour in a widget.
- **Window grammar:** menu bar (Program, File, Downloads, View, Help), then a
  toolbar of pictures with their names under them, then the category tree on
  the left (Everything with its kinds, In progress, Done, Queues) beside the
  list (Name, Queue, Size, Status, Time left, Speed, Date, Note).
- **The Status column says words:** "Complete", "Error", a percentage while
  running. A bar appears only in the dark look and in the progress window.
- **Verify every visual change** with `python3 scripts/ui-shot.py out-dir
  [command …]`. It starts the real binary on a throwaway home, with rows in
  every state, and saves a PNG of the window (and of each dialog a command
  opens). Look at the picture, in each theme.

## Interaction

| Surface          | Behaviour |
|------------------|-----------|
| Row, double-click | Properties, for any download; a finished file is opened with its Open button |
| Row, right-click | Open, Open with, Open folder, Move/Rename, Redownload, Resume, Stop, Update download link, Remove, Move to queue, Properties |
| Category tree    | A click filters the list; each place shows its count |
| Toolbar          | View › Customize toolbar: style (icons and text, small, text only), which buttons, their order |
| Browser panel    | See `docs/video-panel.md` |

## What the visual checks found

- **Every finished download froze the window for about 2 seconds** (several
  at once, 25 seconds). The notification for it was posted from the window's
  thread, and macOS's call waits on the system. Found by `ui-shot.py` timing
  out on the control protocol, and pinned down with `sample` on the stuck
  process. Notifications are now posted from a thread of their own, and
  `app-check.py` times adds while downloads finish.
