# UI design

How dm's window is drawn, and the rules a window is checked against. The window is the GPUI
one (`crates/dm-gpui`), one code for desktop and phone. Its colours are in
`crates/dm-gpui/src/window/theme.rs`, its sizes in `crates/dm-gpui/src/widgets/native.rs` and
`widgets/space.rs`, and its look is held by the pictures in `docs/behaviour-baseline/gpui/`
(contract row B25). A rule below that the window does not yet follow names the gaps row that
brings it there.

An AI agent working on the window starts from the `dm-design` skill
(`.claude/skills/dm-design/SKILL.md`): it maps the general interface skills beside it
(`better-interface`, `better-ui`, …, written for web pages) onto these rules and tokens, and
these rules win where they differ.

## The rules

The eye must have somewhere to start. Every window is checked against these, at 360, 700 and
1280 points wide, light and dark, before its commit.

1. **One filled button in a window: its main action.** OK, Start now, Add URL. Every other
   button is plain. A window with no main action (a list of facts) has no filled button.
   *(The toolbar has none yet: gaps E3. The progress window's: E9.)*
2. **The accent marks three things only:** the filled button, what is chosen (a row, a
   place, a picked item), and progress (a bar). Links take the accent's text shade
   (`accent_text`). Nothing
   else is drawn in it: not a heading, not a count, not a picture.
3. **The error colour marks two things only:** a failure (a row, a field, a message) and a
   button that destroys (Delete, Cancel download), which is plain with its words in that
   colour. Never a filled red button.
4. **Three ranks of text, no more:**
   - *Figure*: what a window is for, read first: the progress window's "7% · 380 KB/s · 2 min
     left", a dialog's title. Larger type (`native::TEXT` × 1.3 or more), semibold.
   - *Body*: everything read in turn: labels, fields, rows (`native::TEXT`).
   - *Hint*: what explains or is secondary: a help line, a size beside a name, a column of
     dates (`native::SMALL_TEXT`, the `muted` colour).
   Bold alone is not a rank: it is for a group's heading inside a form.
5. **A state is a mark and a word, never colour alone.** Running, paused, waiting, done and
   failed each have a mark told by its shape, then its word; a file's kind by its picture's
   shape (B45). Under High Contrast every colour is the system's, so a state colour cannot be
   the only sign. *(The list's Status is words alone: E4.)*
6. **Labels in one column.** In a group of fields the labels form a column as wide as its
   longest label in the language shown, the fields start on one line after it. A label wider
   than 40% of the window goes above its field; on a phone every label is above. A hint sits
   directly under the field it explains. A switch's line has its name at the start and the
   switch at the end. *(E10.)*
7. **Nothing is cut while room stands empty.** A text is cut (with "…") only when the window
   has no more room to give it; the whole text is then on hover and in the detail. A column
   that says the most (Status) takes spare width first. No text starts outside the window or
   runs past its edge, at the window's least width, in the longest language. *(E2, E4.)*
8. **Rank by place, too.** In a toolbar: the main action at the start, then what acts on the
   chosen rows, the rest at the end, going under "More" as the window narrows, the last
   first. In a dialog: the name or question first, the figure, then the facts, then what can
   be changed, each behind a line that opens; the buttons last. *(E3, E9.)*
9. **One message bar at a time.** Its mark first (a ring with "!" in the error colour for a
   failure, "i" in the hint colour for a note), the sentence in the text colour, its buttons;
   where more wait, "+N more" at its end opens them (`bars::framed`).
10. **Colours and sizes come from the tokens.** Read colours from `Theme`; read sizes from
    `native` and `space`, through `zoom::px` so the text size setting reaches them. Never a
    hard-coded colour in a widget.
11. **Check by a picture, not by reading code.** `python3 scripts/ui-shot.py out-dir
    [command …]` starts the real binary on a throwaway home with rows in every state and saves
    a PNG of the window and of each dialog a command opens. Look at it in each look.

## Words

The words are part of the design: short words rank themselves.

- **Sentence case** everywhere: "Stop all", "File name", never "Stop All". *(V3.)*
- **A button is a verb**, one or two words: "Start now", "Retry". The verb says what happens,
  so "OK" only where nothing else fits.
- **A label is under 40 characters; a help line under 90, one idea.** More goes to the
  setting's page in `docs/guide/`, behind "Learn more". *(V2.)*
- **No "please", no "!".** Say "you", never "the user".
- **A number carries its unit:** "380 KB/s", "2 min left", "3 connections".
- **A failure in one shape:** what happened, in a few words, on the row ("Server refused the
  sign-in"); what to do, in the details ("Check the user name and password in Properties").
  *(V4.)*
- **One word per thing.** One name for each thing across the window, the menus and the docs;
  the list is kept here once U20 is decided (gaps row U20).
- **A translation fits the same room:** it is checked at the dialog's least width, and one
  over 1.4 times its English is cut back. *(V6.)*
- **No other download manager is named** (`CLAUDE.md`): "reference A", "reference B",
  "reference C".

## Looks

| Look | Setting word | What it is |
|---|---|---|
| As the computer is | `system` | The default: light or dark as the system is set, followed as it changes |
| Light | `light` | The system's light colours |
| Dark | `dark` | The system's dark colours |

Chosen in Options › General › Look and kept as `theme` in the settings; an older `classic` is
read as `system`. On top of the look, the window follows the system: its accent colour (where
the words on it stay at 4.5 to 1), Windows' Mica under the frame, and Windows' High Contrast,
whose colours replace dm's whatever the look.

## Colours (`window/theme.rs`)

Each system's programs are drawn in that system's colours, so dm keeps a palette for each:
macOS's AppKit colours, Windows 11's WinUI (Fluent) colours, GNOME's libadwaita colours on
Linux, and under High Contrast the system's own. A widget reads the role, never the value:

| Role | What it colours |
|---|---|
| `window` | A dialog's, a sheet's and the window's own background |
| `background` / `stripe` | The list's background, and every other row's |
| `sidebar` | The side panel |
| `menu` | A menu |
| `text` / `muted` | Body and figure text / hints and secondary text |
| `separator` | Lines between parts |
| `control` / `control_border` / `field` | A button's face and edge, a text field's inside |
| `accent` / `on_accent` | A fill: the default button, a switch on, a ticked box; the words on it |
| `accent_text` | The accent drawn on a surface: a running download's state, a link, "+N more", a bar, a focus ring; at 4.5 to 1 on the list, a stripe, a dialog, the side panel and a menu |
| `selection` / `on_selection` | What is chosen; the words on it |
| `warning` | A failure and a destructive button's words |
| `success` | What is in order (an extension that has spoken to dm) |

Contrast is tested, not eyeballed (`theme/tests.rs`): words at 4.5 to 1 on every surface they
sit on, in every palette.

## Sizes (`widgets/native.rs`, `widgets/space.rs`)

Each system's own sizes, in points, macOS / Windows / Linux, and a phone's:

| Size | macOS | Windows | Linux | Phone |
|---|---|---|---|---|
| Body text (`TEXT`) | 13 | 14 | 14 | 15 |
| Hint text (`SMALL_TEXT`) | 11 | 12 | 12 | 13 |
| A control's height (`CONTROL`) | 24 | 32 | 34 | 44 |
| A window's margin (`MARGIN`) | 20 | 24 | 18 | 16 |
| Between rows of a form (`ROW_GAP`) | 8 | 8 | 10 | 14 |
| A control's corners (`RADIUS`) | 5 | 4 | 6 | 10 |

Spacing steps (`space`): 2, 4, 6, 8, 10, 12, 16, 20, 24; a touch target is at least 48. The
font is the system's (`.SystemUIFont`); dm ships none. Everything is multiplied by the text size setting (`zoom.rs`, B66: 60% to 120%).
Pictures are dm's own SVGs (`assets/icons/`).

## The main window

- **Width:** under 600 points (`narrow::NARROW_BELOW`) the window is the phone's: cards, a
  bottom bar and a floating Add button; 360 points is the least (`narrow::LEAST_WIDTH`).
- **Menus:** Tasks, File, Downloads, View, Help (`menus.rs`; on macOS also dm's own menu and
  Edit).
- **Toolbar:** pictures with their names under them; View › Customize toolbar sets the style
  and which buttons.
- **Side panel:** the places (Everything with its kinds, In progress, Done, the queues) beside
  the list (Name, Queue, Size, Status, Time left, Speed, Date, Note).
- **Search:** one box in the top row over name, address and note; where the row has no room
  for it, the filter keeps its place above the list.
- **Messages** are one bar above the list at a time (`bars.rs`, rule 9).

## Interaction

| Surface | What it does |
|---|---|
| Row, double-click | Running, waiting or stopped: its progress window (B46). Finished: what Options › General says. Failed or cancelled: Properties |
| Row, right-click (phone: long press) | The row's menu, in the groups of `reach::ROW`: what opens the file (Open, Open with, Preview, Open folder, Extract, Move/Rename, Redownload), what runs it (Resume or Retry, Stop download, Update download link), then copying, Delete, Move to queue, Delete file after and Properties; on a phone also Share and Select |
| Place, click | The list shows that place; right-click (long press) opens its menu |
| Ctrl (⌘) with + / − / 0 | The text size a stop larger / smaller / 100% (B66) |
| Browser panel | See `docs/video-panel.md` |

## What the visual checks found

- **Every finished download froze the window for about 2 seconds** (several at once, 25
  seconds). The notification for it was posted from the window's thread, and macOS's call
  waits on the system. Found by `ui-shot.py` timing out on the control protocol, and pinned
  down with `sample` on the stuck process. Notifications are now posted from a thread of
  their own, and `app-check.py` times adds while downloads finish.
