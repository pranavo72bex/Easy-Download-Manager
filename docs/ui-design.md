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

The modern looks are WinUI 3's Fluent 2 brushes (decisions.md, 2026-10-19),
each laid over the surface it sits on; see-through ones are given as WinUI
gives them.

| Token | Light | Dark | WinUI brush |
|---|---|---|---|
| `frame` / `pane` | `#f3f3f3` | `#202020` | SolidBackgroundFillColorBase |
| `list` / `header` | `#f9f9f9` | `#282828` | LayerFillColorDefault |
| `dialog` | `#ffffff` | `#2b2b2b` | LayerFillColorAlt |
| `border` | `#e5e5e5` | `#3a3a3a` | CardStroke / DividerStroke |
| `stroke` | `#8a8a8a` | `#9e9e9e` | ControlStrongStroke |
| `edge` | `#e5e5e5` | `#3e3e3e` | ControlElevationBorder |
| `text` / `weak` / `disabled` | `#1b1b1b` / `#616161` / `#9a9a9a` | `#ffffff` / `#cfcfcf` / `#787878` | TextFillColor Primary / Secondary / Disabled |
| `hover` / `pressed` / `selection` / `row_selected` | black 3.5% / 2.4% / 3.5% / 3.5% | white 5.9% / 3.9% / 5.9% / 5.9% | SubtleFillColor Secondary / Tertiary |
| `control` / `control_hover` / `control_pressed` | `#fbfbfb` / `#f6f6f6` / `#f5f5f5` | `#373737` / `#3c3c3c` / `#323232` | ControlFillColor |
| `brand` / `brand_hover` / `brand_pressed` | `#005fb8` / `#1a6fbf` / `#337fc6` | `#60cdff` / `#5bbdea` / `#55acd4` | AccentFillColor |
| `on_brand` | `#ffffff` | `#000000` | TextOnAccentFillColorPrimary |
| `accent` (links) | `#003e92` | `#99ebff` | AccentTextFillColorPrimary |
| `focus_outer` / `focus_inner` | `#1b1b1b` / `#ffffff` | `#ffffff` / `#0b0b0b` | FocusStrokeColor Outer / Inner |
| `error` / `success` / `warn` | `#c42b1c` / `#0f7b0f` / `#9d5d00` | `#ff99a4` / `#6ccb5f` / `#fce100` | SystemFillColor Critical / Success / Caution |
| `radius` / `card_radius` | 4 / 8 px | 4 / 8 px | ControlCornerRadius / OverlayCornerRadius |

Type: Segoe UI, Fluent's face, taken from Windows where it is installed
(it is never shipped); elsewhere Selawik, Microsoft's open stand-in made to
its measure (SIL OFL 1.1, `crates/dm-desktop/fonts/LICENSE-Selawik.txt`);
Inter behind both for Greek and Cyrillic (SIL OFL 1.1). Icons: Fluent UI
System Icons (MIT, `fonts/LICENSE-FluentSystemIcons.txt`), 20 px regular,
cut down to those drawn (`icons::fluent`). Sizes and weights: the WinUI ramp's
Body 14, Caption 12, Body Strong 14 semibold for group headings, Subtitle
20 semibold for a dialog's title; sizes, speeds and times in tabular
figures. Sizes: WinUI's compact density (24 px fields and drop-downs, 32 px
rows and places, buttons padded 11×5, a drop-down as tall as a button
beside it), 8 px between controls, dialogs padded 24, a check box's box
20 px (grey when ticked but disabled). Fields and number boxes carry the
bottom line at rest, the accent's 2 px while typed in; a drop-down's
picked item carries the accent pill, as a list's selected row does.
Shadows: Fluent's 28 under dialogs, 16 under menus.

## Rules

- **Layout:** surfaces meet without lines; the list sits on a rounded card;
  the command bar keeps as many names as fit and shows the rest as pictures.
  A dialog is laid out as WinUI's ContentDialog: its title at the top left
  (`dialogs::Dialog`), its buttons on a footer band at the right, the main
  action first and filled with the accent (`dialogs::footer`).
- **Contrast is tested, not eyeballed** (`theme.rs`): a selected row
  carries the accent pill at 3:1 on the list and its words at 4.5:1 on its
  fill (Classic's row is a solid `#0072d0`); the focus ring is two lines,
  one in the text colour and one in the background's, so one of them shows
  on any fill.
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
