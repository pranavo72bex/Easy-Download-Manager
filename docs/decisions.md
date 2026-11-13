# Implementation decisions

Decisions that shape dm and are not obvious from the code, newest first. Each says what was decided and why, so a later change
can see what it is undoing.

## 2026-11-11: After a review: the top bar's drag areas, a window on its own display, one pass of tidying

From a code review and a cleanup pass over the GPUI work of 2026-11-10:

- On Windows the whole top bar was a drag area, which Windows' hit test put over the window's
  buttons, the menus and the search box: only the bar's empty spaces move the window now
  (`titlebar::drag_space`). The menus' titles are measured as drawn (their words are the
  language's) rather than assumed 330 points wide, so the search box never covers them, and they
  are a fixed list (`menus::WINDOW_MENUS`): the menus are built on a click, not on every frame,
  which had looked for the When-done programs on the disk each frame (now looked for once a run,
  `power::installed`).
- The window's frame is kept with its display: macOS counts it from that display's corner, and a
  window left on a second display came back on the first.
- On Linux only the window's size and zoom are kept, not where it was: Wayland gives a program no
  position, and X11's (GPUI's ConfigureNotify) is the drawing area's or counted from the desktop's
  frame, which, asked for again, puts the frame there and the window a title bar lower at each
  start (review, 2026-11-11). It opens in the middle of the main display, where the desktop may
  place it otherwise.
- Bringing dm's window forward (the Dock, a second launch, the port) brings a dialog open over it
  forward after it, rather than burying it under the main window. `human-qa.py` still waits a
  moment after asking for the window before opening Add URL: macOS brings the app forward on its
  own time, and without the wait the GPUI window's B01, B25, B42 and B44 failed again (tried,
  2026-11-11). The script selects the field before typing an address again, rather than looking
  for any address on screen.
- `make install`, `make qa` and the Makefile's `BINARY` build the window each system ships; a test
  holds dm-gpui's version to dm-desktop's, which the release tools read.
- The question window is as wide as its buttons at each system's measures. The menus are built by
  name, not taken out of a list by position; the probe reads the screen once a click; the macOS
  palette takes `dark` as the others do and its test uses `dm_desktop_base::contrast`; one PNG
  writer for Windows' and X11's pictures; one helper for the links in dropped files.

Not done, for later: one binary name for both windows (`dm`) so scripts need no per-system crate;
a shared PNG writer and tray-icon decoder for both windows; the macOS pasteboard readers shared
by both windows; Linux's `--hidden` window drawn once before it is put away (GPUI's X11 windows
ignore `show: false`); a reused X connection.

## 2026-11-10: Each system's own colours, and one way in for what is dropped

At the maintainer's words of 2026-11-10 ("even Windows and Linux should look native on their
platform"; "make the overall architecture scalable, maintainable and easy to follow"):

- The GPUI window's palette is each system's own (`theme.rs`): macOS's AppKit colours as before,
  Windows 11's Fluent ones (Mica's grey, the list on the layer above it, the default blue accent,
  black text at 89%; in dark, the light-blue accent with black on it) and GNOME's libadwaita
  ones on Linux. `on_accent` names what is drawn on the accent, which is not white everywhere.
  A test holds every palette, light and dark, to readable contrast (text 7:1, the rest 3:1).
- The dialogs' measures are each system's too (`native.rs`, `pick`): macOS's as measured on its
  own dialog (13 pt text, 24 pt controls, 20 pt margins), Windows 11's WinUI ones (14 px text,
  32 px controls, 24 px margins, 120 px buttons, 4 px corners, the default button first and
  Cancel last) and GNOME's (14 px text, 34 px controls, 18 px margins, 6 px corners).
- What is dropped on a window waits in one place for both windows, `dm_desktop_base::drops`, and
  Windows' OLE drop target (a browser's link as well as files) moved there out of the egui crate,
  with its tests: the GPUI window puts it on each of its windows on Windows, and its macOS drag
  methods feed the same queue.
- What is said when the system makes no tray icon is one sentence for both windows
  (`dm_desktop_base::tray::unavailable`): the GPUI window said the toolkit's error alone.
- `human-qa.py` brings dm's window in front before Add URL, as a person does: behind the browser,
  the egui window drew no frames, and its clicks waited for one (B10 and B63 failed so).

## 2026-11-10: On macOS dm ships the GPUI window (G9 for macOS)

D5 recommended switching the default window once release packages held 64 of 64 on all three
systems. The maintainer's later words of 2026-11-10 ("first migrate on macos, all steps, then we
will move for linux"; "complete the migration") take every step on macOS first, so on macOS the
GPUI window is now the one built and shipped: `make app`, `make build-macos` and `make run` build
dm-gpui there (`WINDOW` in the Makefile), the release workflow's macOS job packages and
smoke-tests it (`matrix.crate`), and `human-qa.py` and `app-check.py` drive it by default. It
was proved first from its own release .dmg: `smoke.py`, and `human-qa.py` 64 of 64. Linux and
Windows keep the egui window until theirs hold the contract in GPUI, and egui is removed only
then, in a change of its own. Nothing reaches `main` before the full gate (Windows' and Linux's
runs included). An installed dm's browser registrations and login item name the old binary,
`dm.app/Contents/MacOS/dm-desktop`; the new one re-points both at its first start (the browsers'
manifests as they go stale, `register::ensure_if_claimed`; a login item whose program is gone,
`autostart::takes_over`). `make app-egui` still builds the egui bundle for comparing.

## 2026-11-10: The GPUI window on Windows and Linux: its title bar, the tray, the port

At the maintainer's word of 2026-11-10 ("make coding part of migration of windows and linux
here"), the GPUI window's system-specific code is written here for all three, and run on each
system's own computer: `syswindow.rs` (was `appkit.rs`) does what GPUI cannot ask of a window, each
system its own way. Windows: put away into the tray with `ShowWindow(SW_HIDE)` and shown again (it
was only minimized), moved with `SetWindowPos` for `ui.move_window`, pictured with `PrintWindow`
for `ui.screenshot` (a PNG). Linux on X11: unmapped into the tray and mapped back, moved and pictured (`GetImage`) through a
connection of dm's own to the X server (`x11rb`; each request waited on, since one left in the
buffer as the connection closed was lost); on Wayland minimized, and the port says it can neither
place nor picture the window. GPUI's X11 windows are shown however they were asked to open, so a
login launch's window is put away after its first frame. GPUI's X11 and Wayland windows do not say
their title back, so every dm window opens through `screen::open_window`, which keeps its title as
its name for the port. The top
bar is the title bar on macOS and Windows (with Windows' own buttons); on Linux the desktop draws
its title bar (server-side decorations) and the top bar is the row under it, as a code editor's is
with the system's title bar, since a program drawing its own on Linux must draw its frame, shadow
and resize edges too. The tray shows dm's own icon on Windows and Linux (the template picture is
macOS's). Checked here: macOS's build, tests and `human-qa.py`; Windows by `windows-check.sh
clippy` (Docker, the whole workspace); Linux in `linux-check.sh`'s Debian container: clippy, and
the GPUI binary under Xvfb with no GPU passing `smoke.py` and `app-check.py` in full. Running them
with a person, and Windows at all, is for those systems' own computers.

## 2026-11-10: The GPUI window's menus are in its top bar, on every system, beside the search box

At the maintainer's words of 2026-11-10 ("make the search bar and options like Tasks File
Downloads View Help to top bar like in vs code", "in windows too", "in linux too"), the GPUI
window's top bar holds, as a code editor's title bar does, the sidebar's button, the egui
window's five menus by title (Tasks, File, Downloads, View, Help; Exit last in Tasks, Options in
Downloads, About in Help, as in the egui window), and the search box, centred on the window where
the menus leave room. A title opens its menu as the system draws one (an `NSMenu` on macOS,
Windows' own, a drawn one on Linux: gpui-component's `NativeMenu`), with the menu bar's items and
ticks (`menus::window_menus`). On macOS the system's menu bar keeps them too, with dm's app menu,
Edit and Window. On Windows the bar is the title bar and ends with the window's buttons, whose
clicks the system takes (`WindowControlArea`, so Snap layouts work). B25's GPUI pictures were
drawn again for it. The port opens a title's menu as it reads one (and a dialog's words of the
same name, Options' Downloads tab, still go to the dialog).

## 2026-11-10: dm brings no font: the system's interface font, and its monospaced one

At the maintainer's word ("use system fonts or os fonts, no need to provide fonts explicitly"):
the GPUI window draws every word in the system's interface font (`.SystemUIFont`: San Francisco,
Segoe UI, the Linux desktop's) and its monospaced text (a digest, links) in the system's own,
gpui-component's choice for each (Menlo, Consolas, DejaVu Sans Mono, else the next installed),
where it named Menlo, which only macOS has.

## 2026-11-10: Add URL's address selected as the field is asked to take the keyboard

In the egui window, Add URL offers a link on the clipboard selected, so what is typed replaces
it. It was selected once the field had the keyboard, a frame after it asked; a window behind
another, drawn only as input arrives, took the first keys in that same frame, beside the link
(`human-qa.py`'s B63 and its walk failed so, 2026-11-10). It is selected as the field asks for
the keyboard now (`select_on_focus_asked`); a person's typing is not selected again once it has
begun. `human-qa.py` types an address again only after two seconds without it on screen.

## 2026-11-10: The GPUI window comes back where and as large as it was left

The egui window keeps its frame across launches (eframe's `persist_window`); the GPUI window
opened in the middle at 900 by 600 every time, which a Mac program does not. It keeps its frame
in `window.json` in dm's home as it quits (a `DM_HOME` run its own, so a test or a check starts as
a fresh install does) and opens there next time, unless that frame's title bar is on no display
now (one unplugged since) or it is smaller than the window's least size: then in the middle.
Zoomed comes back zoomed; full screen comes back as the window it was in, not as a Space of its
own at launch. Driven: zoomed, quit, started again on the same home, the same frame.

## 2026-11-10: The GPUI window's View menu is the egui window's, in the menu bar's own items

The egui window's View menu holds controls a menu bar cannot: a checkbox with move-up and
move-down arrows beside each column and each toolbar button, and a zoom with a percentage. The
GPUI window has the same choices as the system's menu items: View › Columns ticks each column
shown (the file name always stays), Customize toolbar the style (Icons and text, Small buttons,
Text only, now drawn as chosen; `dm_desktop_base::toolbar`, moved out of the egui crate) and each
button shown, Language each language with the one in force ticked, and Sort by the sort in force.
A column is moved as a macOS table's are, by dragging its heading onto another (after it from the
left, before it from the right), and a toolbar button the same way. View › Text size is not in the
GPUI window: GPUI has no zoom, and every size there is in points (gaps row G7).

## 2026-11-10: `--headless` is a loop of its own in dm-desktop-base, with no GPUI

The GPUI binary had no `--headless`, which `smoke.py` (the release workflow), `app-check.py`,
the site checks and a server need. GPUI has a headless platform, but dm's list view cannot exist
without a window (its text field, its window's wake loop), and a GPUI application on macOS is
still an application. So `dm-gpui --headless` starts no GPUI at all: `dm_desktop_base::headless`
runs the application's rules (`dm_shell::app`) in a loop of its own, woken by the engine and the
control port and once a second by its clock, as the egui binary's headless mode runs `DmApp` with
no window. It settles what a window would ask as egui's does: hand-offs go straight in
(`Asking::Nobody`), Exit when done is at once, Resume on held rows waits and says so, Exit over
the port ends dm, `ui.screenshot` is refused, `ui.texts` is empty. Two differences, both for a
script's sake: a step (`ui.click`, `ui.type`, `ui.key`, `ui.scroll`) is refused at once, saying
there is no window, where egui's queued it for frames that never came; and `command.run` of a
command that needs a window (a dialog, the clipboard) is refused, naming it, where egui's opened
the dialog unseen. With nobody to see it, the weekly update check is not made and the clipboard
is not watched. `app-check.py` (98 of 98) and `smoke.py` pass with dm-gpui, and
`dm-gpui/tests/quit.rs` signals it headless as `dm-desktop/tests/quit.rs` does.

## 2026-11-10: The GPUI window carries out When done, and its menus tick as the egui window's

Downloads › When done's choices did nothing in the GPUI window: the command reached no branch
and fell through. Fixed, and the menus now say what is in force, as the egui window's do: When
done's choice ticked (and greyed where the computer cannot do it), Limit speed ticked while a
limit holds and the preset it is, View's Categories, Toolbar and Drop target ticked while shown.
The menu bar is built again when any of them, or the queues, change. An item that opens a dialog
ends with "…", as the egui window's and macOS's own menus say it (found by `app-check.py` looking
for "Add numbered series…"). The port lists a menu it opened as the egui window shows one, and
Escape closes it. The window forgets `control.port` as it quits (it was left naming a port nobody
answered: nothing after GPUI's run is reached on macOS).

## 2026-11-10: The GPUI window's menu bar has macOS's own Edit and Window menus

As the plan's "Native feel and speed" asks of the menus on macOS ("the standard app, Edit and
Window menus and their usual shortcuts"), the GPUI window's menu bar has Edit (Undo, Redo, Cut,
Copy, Paste: the text fields' own actions, given as the system's, so its services and dictation
reach them) after File, and Window (Minimize ⌘M, Zoom, Bring All to Front) before Help. The list's
Select all stays in File, as in the egui window. Their words are in every catalog; Redo, Window,
Minimize, Zoom and Bring All to Front are new there, in each system's own words for them.

## 2026-11-10: GPUI takes links dropped from a browser through its window's own drag methods

GPUI's macOS window registers for dragged files alone, so a link dragged from a browser (an
address, not a file) never reached it. As the egui window does with winit's, `drops.rs` gives
GPUI's window class (`GPUIWindow`) dm's own `draggingEntered:`, `draggingUpdated:` and
`performDragOperation:`: a drag that carries a link and no files is taken (its URL, else its text,
read for links) and the window woken as at any change; every other drag goes on to GPUI's own
method unchanged, files included, which reach the window as `ExternalPaths` and are read for the
links they hold (`dm_shell::app::links::links_at`: a `.torrent` is itself the download). Each
class is given dm's methods once: given twice, a method would keep dm's own as GPUI's and call
itself. The floating drop target (View › Drop target) is a small always-on-top window of its own
(`droptarget.rs`): it takes drops the same way, shows how far the downloads are along its foot and
how fast on hover, moves when dragged, brings the window back on a double-click, and has Show dm and
Hide the drop target on its right-click menu, as the egui window's card has. The GPUI app is
bundled beside the egui window's (`make app-gpui`, `desktop/target/release/bundle-gpui/`), with the
same name, id, icons and the types it opens, until GPUI is the window dm ships (Phase E).

## 2026-11-10: `human-qa.py` drives the GPUI window as a person does, through the same port

The control port answers every `ui.*` method in the GPUI window as it does in the egui window,
so `human-qa.py` runs against either (`DM_BINARY`): `ui.inspect` names the dialogs open (by the
dialog's name: a progress window is "Download progress" whatever its title says), the status
line, the problems, and each row by the name the list shows and its status in English;
`ui.texts` holds only the words in view (a click aimed at words scrolled out of a page's view
landed on whatever was there) and what fields hold (not a password's); `ui.scroll` turns the
wheel of the window the words are in; `ui.screenshot` is the system's own picture of the window
(`screencapture -l`) and `ui.move_window` moves it as a drag by its title bar would (AppKit,
`appkit.rs`). The system's menu bar and its pop-ups are modal and cannot be clicked by the port,
so the port walks them as a person does: a click on a menu's title, where no words on screen are
exactly those, opens it and holds its items; a click on a submenu holds its items; a click on an
item runs its action in the main window. A click on a pop-up holds its choices, and a click on
one picks it. The first full run against the GPUI window held 54 of 64 rows; the failures were the
port's (the menus, the rows' words, the dialogs' names) and three of the window's (Delete
Completed said nothing; Options did not hold the window, which B42 asks of it; Resume did not
open the progress window as the egui window's does), all put right. B25's pictures are the GPUI
window's own: they are kept under `docs/behaviour-baseline/gpui/`, beside the egui window's, until
GPUI is the window dm ships (D3, D5).

## 2026-11-10: The GPUI window's tray is a status item; its close buttons do what Cancel does

GPUI has no tray, so the GPUI window takes D4's fallback, `tray-icon`, as the egui window does:
the status item in the menu bar with the egui window's menu (Show dm, Start Queue, Stop All, the
speed limiter ticked while a limit is in force, Speed with its presets and Settings… (B52), the
Scheduler, Exit dm) and its tooltip (`dm_desktop_base::tray`, both windows). Its picture is a
template image drawn by dm (an arrow into a tray), as the system's own status items are, so it
takes the menu bar's colour; the egui window's coloured icon was not. The close button follows
Options › "When I close the window": into the tray (the window put away with `orderOut:`, not
closed, so it comes back as it was), Exit, or asked once ("Keep running in the tray?", a sheet);
with no tray it ends dm, asked first while downloads run. The window comes back from the tray,
from the Dock icon, from a second launch and from the browser's button (the inbox's `show`,
which the GPUI window had ignored); a login launch starts in the tray, with no window shown.
File › Close (⌘W), as every macOS program has it, does what the close button does.

Every GPUI dialog's close button now does what its Cancel does (B42), through one hook
(`native::close_is_cancel`). It had simply closed the window: Download details or Download links
closed that way left the hand-off unanswered, and every later hand-off waited behind it, and
"Download complete" lost its "Don't show this window again" tick. ⌘W in a dialog does the same.

## 2026-11-10: GPUI's Options is macOS's settings layout over the shared rules; a sidebar button

Options' rules are `dm_desktop_base::options` for both windows: its six pages (B43) and what
each page's Restore defaults puts back, the lists typed as words, and OK, which reads the lists
into the draft, refuses a proxy with nowhere to connect to (on Advanced, at its proxy) and a
category made or renamed that cannot be a folder on every computer (on Folders), and keeps only
what Options changed over what is in force. The GPUI window draws the pages as macOS's settings
are: the pages as a tab view's segments at the top, the settings in sections under bold headings,
each check box with what it does under it, the system's pop-ups and fields, one size for every
page with the page scrolling. Its Theme offers the computer's look, Light and Dark, and forces
the application's appearance for the last two (`set_window_appearance`); `classic` is the egui
window's own look and is drawn as the computer's. Text size is the egui window's zoom; a macOS
program follows the system's text, so the GPUI page has none. Mica, who opens magnet links and
Dial Up / VPN are Windows' and are drawn there by the egui window until the Windows work is done
on a Windows computer. The clipboard is watched as the egui window watches it ("Offer to download
links I copy": the bar above the list; "Take a copied link at once").

The maintainer, 2026-11-10, looking at the GPUI window: "no option to close side pannel here".
The categories could be hidden only from View › Categories. The title bar now has the system's
sidebar button just past the traffic lights, where Finder, Mail and Notes put it, and ⌃⌘S (macOS's
own keys for Show and Hide Sidebar) does the same.

## 2026-11-10: The GPUI window keeps the script guard; its own actions are registered once

The GPUI window had no script guard: a control client could double-click a finished row (with
Options' double-click set to Open) or click Install now, and the file ran. As the egui window
does (security review of 2026-11-07, gaps row 159 f), each click, key, typing and scroll the
control port gives the GPUI window marks the moment (`ScriptGuard::acted`), and File › Open,
Open with… and Preview, Download complete's Open and Open with…, Properties' Open and Install
now refuse for a moment after, saying why. The probe's refusal of a click on an Open word stays
as well. Help's pages, Hide, Hide Others and Show All are application actions registered once
at start: the menu bar is built again whenever the queues change, and each build had added
another handler for Hide (so a Help page would have opened once a build).

## 2026-11-10: The row dialogs' rules are one module; GPUI's take the system's ways of a field

What OK does in Update download link, Move to queue, Move/Rename and File properties (a fresh
address and the retry of a download that failed for want of it; the queue picked or named, in
dm's own name; each file moved, then renamed, and a name already taken said; a torrent's files
and priorities, the note, the address with its sign-in, and the retry with a sign-in given) is
`dm_desktop_base::row_edits`, which both windows call, so they cannot differ. In the GPUI
window each is a window of its own, as Add URL is, and keeps the system's ways where the egui
window had none: Move/Rename opens with the name without its ending chosen, as Finder's rename
does (typing replaces the name and keeps the kind); Move to queue opens with the keyboard in
"Or a new queue"; the finished file's SHA-256 is text that can be selected and copied, as the
system's labels can be (`gpui-base`'s `SelectableText`, the base gpui-component itself pins at
the same version, added as a window crate in `cargo xtask layers`), not a field one can type
in. These follow the maintainer's word on native feel (2026-11-10); no contract row names them.
The window's double-click on a row does what Options › General says (B46), as the egui
window's. Dropped: the digest in a text field whose typing was undone (it took a Tab stop
between Note and Check against, so Tab put the keyboard where nothing could be typed).

## 2026-11-10: The GPUI window's top bar holds the search box, as a code editor's title bar does

The maintainer, 2026-11-10, looking at the GPUI window: "search is missing and also top bar is
missing. take reference from vscode and make similar way." The GPUI window's top row is a title
bar of its own (`dm-gpui/src/titlebar.rs`): the system's traffic lights at its left and, centred
on the window, the search box that narrows the list as one types (name, address or note, the
egui window's top-row box and its words), its magnifier at the left as the editor's command
centre and the system's search fields draw it, "N shown" and a clear mark once it holds words;
⌘F puts the keyboard in it with its words chosen, Return finds the first row it leaves, Escape
clears it and gives the keyboard back to the list, Find next and previous go round the rows it
leaves (`dm_desktop_base::find`, both windows). The toolbar is a row of its own under it, no
longer in the title bar. Dropped: the toolbar in the title bar beside the traffic lights (the
unified style of Finder), which left the search nowhere and the window no top bar.

## 2026-11-10: The ways dm runs without a window are one module for both windows

`dm_desktop_base::modes::without_window` runs the browser's host, the warm page reader,
`--version`, `--help` (its text byte for byte as before), the command-line client, MCP,
`--register` and `--unregister`, and refuses a typo, for both windows' `main`; macOS's "open with"
Apple events (`dm_desktop_base::opened`) likewise. The GPUI window had none of them, so a browser
could hand it nothing (`--native-host` opened a window). Notifications stay each window's own, as
the decision on `Desktop` below says (notify-rust is a window crate's): the GPUI window's notifier
is `dm-gpui/src/notify.rs`, and it claims dm's bundle id for them after gpui-component has started
its own notification centre, since claiming it first made macOS look for a bundle a bare binary
does not have, and dm stopped at launch.

## 2026-11-10: GPUI dialogs take AppKit's measures and macOS's order of buttons

The maintainer, 2026-11-10, looking at the GPUI dialogs: "margin padding is also off. fix it on
macos too". Every GPUI dialog and sheet is built from one set of measures (`dm-gpui/src/native.rs`),
AppKit's regular controls: 13-point system text, 11 for what is said under a control and an
alert's informative text; a text field, a push button and a pop-up all 22 points tall; 20 points
from the window's edges, 8 between rows and from a label to its control, 12 between buttons. A
form is laid out as macOS lays one: the labels right-aligned in a column of their own, every
control, check box and warning in a second. A row of buttons is in macOS's order: the default
rightmost, the button Escape presses beside it, any other action at the leading side (Download
details: Add for later to a queue at the left; Cancel, then Start now, at the right; a question's
Yes / No reads No, Yes). The main window takes the same sizes: rows and the side panel's places at
13 points, rows 24 points tall; toolbar labels, column headings, section headings and the status
bar at 11. The words are unchanged. B02 and B42 list buttons in the egui window's order; their rows
change for the GPUI window on macOS when it becomes the default (D5), in that commit, naming this
word, as for the sheets. Dropped: gpui-component's own sizes (32-point fields, 14-point text),
which read as a web page's form beside the system's pop-ups. The measures were taken from the
system's own dialog on this Mac (`display dialog` with a text field, measured pixel by pixel):
push buttons and fields 24 points tall, OK and Cancel 80 points wide, 12 between them, 20 from the
window's edges, corners of 5 points, a dark dialog's background #2e2c2b (tinted by the wallpaper)
and its push button white at 8% with no edge, the default button #007aff.

A question is asked as a sheet on dm's main window only, once that window is in front
(`Downloads::sheet`): GPUI begins a sheet on whichever window AppKit calls main, so "Already in the
list", asked as Add URL closed, went with Add URL, and a question raised while dm was in the
background (a 404 while the person is in the browser) was never shown, and every later question
waited behind it. A question waits, in order, and is asked when the main window is activated, as
a Mac program's alert waits for its person to come back.

## 2026-11-10: GPUI pickers open the system's menu; dialogs fit what they hold; loops use their own window

Under the maintainer's native word of 2026-11-10. A picker in a GPUI dialog (Download details'
Category, Delete file after and queue; Add URL's Recent) is a pop-up button whose click opens the
system's own menu, the chosen item ticked: gpui-component's `NativeMenu`, an `NSMenu` on macOS and
Windows' own menu there, drawn by GPUI on Linux (`dm-gpui/src/popup.rs`). Dropped: a list drawn by
GPUI inside the window, which a person on a Mac meets as a web page's menu, clipped at the window's
edge. The cost: the system's menu is modal, so while one is open the control port waits, as the
app does; `human-qa.py` needs a way to choose in a pop-up without opening it before it drives
these rows. A dialog's window is as tall as what it holds (`fit.rs`: laid out at its own height,
the window resized on the next frame), as the system's dialogs grow when a warning or a sign-in's
fields appear; a fixed height put Download details' buttons past its bottom edge once the name's
clash was shown. A task that updates the main view from its window (the wake loop) runs in that
window by the async context (`AsyncWindowContext::update`), never by `WeakEntity::update_in`:
GPUI runs that in whichever window last drew the entity, and Download details draws the main
view's rows for the name's clash; when it closed, the entity had no window, `update_in` failed and
the loop ended, so the main window stopped reading the queue. Quit is an action of the
application's (`menus::Quit`, heard from any window in front), the list's commands (`menus::Run`)
the main window's, as on a Mac Quit reaches the application from any window while a list's
commands belong to its window; and Exit closes every dialog before it ends dm, since AppKit holds
`terminate:` while a sheet is up.

## 2026-11-10: On macOS the GPUI window asks its questions as sheets

At the maintainer's word of 2026-11-10 ("focus on native improvements … it should feel native to
users"). A question (Remove, Delete file, Cancel download, the server's name, a page for a file, a
404, Exit while downloading) is asked on macOS as a sheet on the dm window: GPUI's
`WindowKind::Dialog` is NSWindow's `beginSheet`, which is how macOS's own programs ask, holds that
window until answered, and comes back with it when it is brought forward. The dialogs that are
work rather than a question (Add URL, Download details, Options, the Scheduler, Properties) stay
windows of their own, movable anywhere, as B42 says. B42's row and `human-qa.py`'s B42 checks are
changed for the GPUI window's questions on macOS when it becomes the default (D5), in that commit,
naming this word; until then the egui window, the one shipped, keeps B42 as written. Dropped:
questions as windows of their own on macOS too (B42 as it reads), which a person on a Mac meets as
a stray panel they can lose behind the window.

## 2026-11-10: The GPUI work is proved on macOS first; D1 go for macOS

The maintainer, 2026-11-10: "or we can skip the linux for now", then "focus on macos". The GPUI
shell's Phase B checks and its slices are built and checked on macOS; the Windows and Linux runs
are deferred, not dropped: owed before a phase merges into `main` (`docs/gaps.md`). The code is
still written for all three systems, so deferring their runs defers checking, not rewriting.
Then: "first migrate on macos, all steps, then we will move for linux. windows part will be handled
on windows so no need to do here." Taken as D1 (go) for macOS, Phase B's check 1 having passed
there, with D2 the recommended pin (`gpui-pre` and `gpui-pre-platform` 0.3.12); Phases C to E run
on macOS first, Linux after, and Windows on a Windows computer.

## 2026-11-10: The GPUI window is built native to each system, and measured

The maintainer, 2026-11-10, mid-migration: "while migrating focus on native improvements and
optimization too. it should feel native to users." So the GPUI slices are not a pixel copy of
the egui window: menus in macOS's menu bar, the systems' own title bars, fonts, text input,
scrolling, choosers and accessibility, where GPUI or the system offers them
(`docs/migration/GPUI_MIGRATION_PLAN.md`, "Native feel and speed"). The features stay as they
are. A contract row or B25 picture a native way changes is updated in the same commit, naming
this word, as an approved change (CLAUDE.md). Speed is judged on numbers against G5's egui
baseline, a slice slower on any of them not done. Dropped: matching egui first and making it
native after (every slice would be drawn twice).

## 2026-11-10: G3 leaves `apply_settings`, the sounds and the dialogs' drafts in the window

At the maintainer's word of 2026-11-09 (take the recommended option). Of the list in
`docs/migration/CURRENT_UI_ARCHITECTURE.md` 4.2, three stay where they are, each for a reason a
second window would share: settings coordination is already the application's
(`Backend::set_settings_from`, with the script rule `Settings::keep_programs_of`), and what is
left of `DmApp::apply_settings` is the window's (its sentences for a kept program, the language
it draws in, re-placing its rows, its own timers); notifications and sounds are which words and
which sound, the presentation's by the row's own wording ("wording, sounds and which dialog opens
stay in the window"); and a dialog's draft (Download all links' list and ticks, Properties' edits)
is view state, its rules moved (`links::looks_like_page` and the rest, `rules::replacing`). The
rows' verbs a dialog calls (`backend.pause`, `retry`, `resume`) are the application's API already.
Every other entry moved, one rule per type or function, each with a dm-shell test.

## 2026-11-10: `Desktop` is the base crate's, and the window supplies what its crates do

At the maintainer's word of 2026-11-09 (take the recommended option). The survey counted egui
in `platform.rs`'s `Desktop` but not the crates `cargo xtask layers` lists as GUI beside it:
`arboard` (the clipboard), `rfd` (the file choosers) and `notify-rust` (notifications away from
Windows). Rather than let the toolkit-free crate use them, `dm_desktop_base::desktop::Desktop<W>`
is the application's `Platform` from everything else (the login item, with `login.rs` moved too;
the Trash; notifications held while the person is busy; the sounds; opening and showing files;
When done; Windows' own toasts; Linux's file-manager and portal look-ups), and takes those four
from a `WindowServices` the window implements (dm-desktop's `EguiWindow`). A GPUI window
supplies its own (its clipboard and path prompts; a notifier). `drag_out` stays in dm-desktop,
needing the window's handle. Dropped: allowing those crates in `dm-desktop-base` (a crate
"without a toolkit" pulling in a toolkit's file dialogs), and leaving `Desktop` in the binary
(a second window would copy 300 lines of system calls).

## 2026-11-10: G2's code moves are gated per batch; `dm-desktop-base` is the toolkit-free crate

At the maintainer's word of 2026-11-09 (take the recommended option). G2 moves the desktop
program's modules that draw nothing into `desktop/crates/dm-desktop-base`, which `cargo xtask
layers` forbids every window crate; `dm_desktop` keeps each at its old path with a `pub use`, so
no caller, test or script changes. Each batch of the survey in `docs/gaps.md` is a commit (the
first, `quiet`, came with the crate; a batch's manifests share lines, so a commit per module
would be hand-split and its middle states never built), held to the gate's steps 1 and 2 (fmt, clippy, `cargo test --workspace`; `scripts/windows-check.sh clippy`); the two
visible QA runs (`human-qa.py` and `linux-human-qa.sh`, about 20 minutes each) are run once per
batch of the survey in `docs/gaps.md` (the first after G1 and batch 1), since a move with a
re-export changes no code path and a QA run per module would cost a day for the same evidence.
A row that fails sends the batch back module by module. The gate's step 2 is
`scripts/windows-check.sh`: a plain `cargo check --target x86_64-pc-windows-msvc` stops at
`ring`'s C without the Windows headers that container carries.

## 2026-11-10: A file a range where the download folder is on another disk

At the maintainer's word of 2026-11-10 (gaps row 166), asked whether dm may stage a download as
reference A does where its download folder is on another disk than its parts folder: each range is
now written front to back into a plain file of its own (`range-0000`, `range-0001`…, beside the
ranges' ledgers in the parts folder), and the finish appends them into the download folder in file
order, each freed once copied (`assemble::copy_parts_blocking`). On one disk nothing changes: one
staged file, every range at its own offset, finished by a rename (2026-10, "Written in place,
finishing is a rename"). Across disks the finish is a copy whichever way the bytes are staged, so
the reason the part files went in 2026-10 (every byte written twice) does not hold there, and on
Windows one sparse staged file was the slower thing to write and to copy: in a probe of eight
threads writing 16 KiB at a time, 512 MiB took 1.8 to 1.95 s into one sparse file and 0.42 s into
eight plain ones, and 1.6 s against 1.1 s to copy. Measured the same day beside reference A
(`bench.mjs`, dm's home on the SSD, C:, both apps saving to the hard disk, D:; every file checked
whole): 512 MiB in 2.09 to 2.87 s from the first request (2.3 to 2.9 s from the address given),
reference A 3.14 to 4.88 s (3.7 to 6.0); after the last byte dm 0.72 to 0.96 s, reference A 0.80 to
1.09 s. With the one staged file, forced in the same session, dm took 5.56 to 6.05 s, 2.9 to 3.3 s
of it after the last byte. 64 MiB at 1 MiB/s a connection, 8.58 and 8.60 s against 8.95 and 9.31.
What decides is what an earlier run left (`files::stage_by_range`): a resume goes on in the layout
its bytes are in, a staged file or an earlier dm's part files keeping the staged file; a download
not fetched by ranges, or of no known length, keeps it too. A range's ledger still says how much of
it is on disk; on a resume it is held to the length of the range's file, which a power cut can leave
shorter than the ledger (`ranged::adopt_ranges`). The resume's check of a range's end, the seams and
the overlaps read the range's own file (`Job::held`, `assemble::bytes_at`); a preview of a partly
fetched video is its first range's file. `DM_STAGE_BY_RANGE=1` or `0` decides instead, for tests
(`tests/by_range.rs` runs each of its tests again in a process of its own with it) and for measuring
the two. Not done: the parts staged on the download folder's disk (slower on a hard disk, in the
entry below); appending a range to the finished file while the others are still fetched, as most
ranges end together.

## 2026-11-10: The background and popup have no JavaScript; the plain scripts' rules are named in Rust

At the maintainer's words of 2026-11-10 to move the extension's JavaScript into Rust and then to
"make it fully rust" (gaps row 169). A Manifest V3 worker must register its listeners on its first
turn (2026-10-12), before WebAssembly loaded the usual way can have; so `scripts/package.sh` now
writes the WebAssembly into the background's script (`pkg/background.js`, as `pkg/panel.js`
already carried it), which starts it at once (`initSync`), and Rust registers the listeners
itself (`web/listen.rs`). `extension/background.js` and `extension/popup.js` are gone; the two
scripts `package.sh` writes only start the Rust. A rebuild in place can no longer pair the glue
with another build's WebAssembly (gaps row 114), so the digest check and reload went with the
loader. Before that, the same day, the loader only registered the listeners and forwarded each
event: the form a
page posts is handed over whole (`on_before_request`) and Rust decides which bodies to keep and
writes them as the browser would (`decide::posted_body`: the fields as `URLSearchParams` writes
them, a raw body as `btoa` does, 64 KiB at most), and the page script's request for the panel is
answered in Rust (`web/page.rs`), which puts `pkg/panel.js` into the asking frame. A test holds
the loader to forwarding. `content.js` and `document.js` must stay plain JavaScript (a page that
forbids WebAssembly, which Firefox applies to content scripts, 2026-10-17; the page's own world),
so their rules are named once in `decide/scripts.rs` and each script is held to them by a test,
as the message names already were; the full panel takes the five phrases it shares with the plain
one, and the click-as-drawn limits, from there too. Kept JavaScript, at the maintainer's word the same day when the cost was
put to them: the logic of `content.js` and `document.js`. They must run where a page forbids
WebAssembly (in Chromium as in Firefox, `chrome-check.py`), so Rust could run there only compiled
to plain JavaScript; a prototype of one of `document.js`'s hooks, built with binaryen's `wasm2js`
(Rust's features lowered for it, the page's functions wrapped through a `Proxy`, so no JavaScript
by hand), came to 110 KB of JavaScript (19 KB gzipped) and 1.1 MB of memory in every page and
frame before any of their logic, against 22 and 30 KB for the scripts themselves. Not done either: generating their rules into them at
packaging rather than holding them by a test, which changes what the extension loads; and the
two panels' wording made one ("dm did not answer…" against "dm did not answer in time…"), a
change a person sees.

## 2026-11-10: Completed before the flush; the end of a part checked once a run; connections a ceiling

Measured beside reference A on Windows on 2026-11-09 (gaps rows 166 to 168), dm was slower in
three cases; the maintainer said "fix the download speed problem" on 2026-11-10 and, asked, chose
the first option offered for each of the two that undo a decision.

Completed before the flush (row 166, undoing the sync before Completed of 2026-11-08 below). A
file saved to another drive than dm's home spent most of its finish waiting for the flush to a
hard disk: 512 MiB took 10 to 12.7 s where reference A took 5.7 to 6.4 s. The finished file is
now in place (renamed, or across drives copied by the system's own copy: CopyFileEx on Windows,
`std::fs::copy`) and its digest checked before the row says Completed, and it goes to the disk
after, with nothing waiting for it (`assemble::sync_after`, called by the queue's runner once its
renames and the mark of where the file came from are written). On macOS and Linux that is an
fsync on a thread of its own. On Windows it is the system's own write-behind, as for reference A's
files, and no flush is asked for: measured on 2026-11-10, a flush of a file (`FlushFileBuffers`,
or `NtFlushBuffersFileEx` with the data-sync flag) holds it against every other open while it
runs, 4 to 8 s for 512 MiB on this hard disk. Started after Completed, it kept the mark's stream
waiting (6.9 s, 0.06 s with no flush) and the window's look at the finished file's size, made on
its own thread (`app/sync.rs` `saved_sizes`), so the window froze for the flush; Explorer or a
double-click on the file would have waited the same. Asked again, the maintainer chose the
write-behind that day over a flush put off about 30 s (still a lock on a slow stick) and over the
flush before Completed. Measured the same day on the 2026-11-09 setup: 512 MiB in 5.4 and 6.4 s
from the first request (10.0 and 12.7 s before), reference A 3.5 and 4.4 s; saved on dm's own
drive, Completed 0.09 s after the last byte. What is left is the copy (row 166): dm's of its one
staged file took 1.4 to 3.2 s whether by Windows' copy or a 4 MiB buffered one (six traced runs),
reference A appends its eight part files at about 450 MB/s; in a probe of eight threads writing
16 KiB at a time, eight plain files were written in 0.42 s and one sparse file in 1.8 to 1.95 s,
copied in 1.1 s on average against 1.6, and any copy made right after the writing swung from 0.4
to 3.8 s with what the system still had to write. Given up for it: a power cut in the seconds after Completed can leave the
file's last blocks unwritten, as reference A's can. Not done: staging the
parts on the download folder's drive, which makes the finish a rename but was slower on a hard
disk (13.6 to 14.9 s, eight connections writing all over it); keeping the flush with only the
faster copy, which would have left dm 1.5 to 2 times slower there.

Tried and dropped later the same day, at the maintainer's word to make dm faster than reference A
and to verify it: copying the file to its folder's drive while it downloads, so little is left to
copy after the last byte. First each range's bytes at their own place in a sparse copy: whole
0.16 to 0.47 s after the last byte in a probe, but a file that was sparse while it was written
could not be opened afresh on the hard disk for 5 to 13 s after (512 MiB; the first open of any
kind waited, a rename included, and clearing the sparse flag took 12 to 16 s itself), where a
plain file written front to back opened in 0.6 to 0.9 s and reference A's at once. Then the
pieces fetched in file order (four a connection) and the whole front appended to a plain copy:
in one session beside the committed build and reference A, 512 MiB took 5.25 to 5.85 s against
the committed 2.31 to 5.77 s and reference A's 3.65 to 3.94 s. On this two-core laptop, a local
server at 180 MB/s outruns the hard disk, so the copying during the download slowed the download
by about a second and still left 1.3 to 1.9 s after the last byte; at 18 MB/s the two tied
(28.9 and 29.3 s against 28.3 and 29.3 s). Not tried: reference A's layout, a plain file a range
appended together at the end, which would make the finish on one drive a copy where it is now a
rename (Completed 0.09 s after the last byte, dm's file opened at once).

The end of a part checked once a run (row 167). The 64 KiB a part's end is fetched again and
compared with on a resume (`RESUME_CHECK`, 2026-10-15) was asked for on every reconnect within
one run as well, where nothing written can have been lost: on a server cutting connections every
megabyte or so it was most of what dm fetched twice. A range's end is now checked once in a run,
on its first attempt, when its ledger came from an earlier one (`Live::trusted`); a resume, after
a pause, a restart or a power cut, checks as before. A file changed during the run is still
caught by the validator (`If-Range`), the total in `Content-Range`, each range's checked last
bytes and the seams. Measured the same day (64 MiB at 8 MiB/s, each connection cut after 0.25 to
3 MiB): dm 8.50 and 8.63 s, 1.1% fetched twice (4.8 to 6.7% before); reference A 8.34 and 8.27 s.

Connections a ceiling (row 168; contract rows B06 and B08 edited). dm opened every connection
it was allowed; allowed 16, on a server sharing 8 MiB/s among them and cutting each, that took
10.1 and 11.1 s where reference A, opening 8 or fewer, took 8.4 and 8.2. A download now opens at
most 8 at once (`OPENED_AT_ONCE`) and plans its ranges for them; past 8, `ranged::grow` lets in
one, then two, then four, each time the last ones added raised the total rate by at least half of
what a connection had brought on average (measured over a second, once their first bytes are in),
and takes the last ones back, after the pieces they hold, when they did not, trying one again after
10 s, then 20, 40. Too little left to judge one (under 2 s at the rate seen) ends the growing. At
the default of 8 nothing changes. Not done: trying connections past 8 one at a time each time,
slower to reach 16 on a server that holds each connection down; opening them all and closing the
ones that add nothing, which is what cost the time. Measured the same day with both set to 16: on
a server holding each connection to 1 MiB/s dm opened 16 and took 6.58 and 6.87 s, reference A
6.74 and 6.65; on the cutting server dm had at most 10 open and took 8.46 and 8.42 s, reference A
8.26 and 8.27.

## 2026-11-08: A finished file is synced once; a server with no version is not taken for a changed one

From a review of the engine. A finished file and its folder are synced before the row is called
Completed (`assemble::durable`): the 2026-10-15 decision repairs a power cut at resume, and a
finished file is never resumed, so its repair never came. One sync per file costs little next to
the download; syncing a download's own writes is still not done. A 200 to a range from a server
that named no version, at the probe or now, is ranges refused (`Error::RangesIgnored`: the
parts are wiped and the file fetched on one connection), not "changed": with nothing to compare,
a whole answer is no sign of a new file. Tried and dropped the same day: sending If-Range with the
spot check of a range's last bytes without reading a 200 as a change; a file republished during
the download was then saved as a mix of both (the zoo's changes-midway, 2 runs in 6). A 200 to it
under the validator is now a disagreement.

## 2026-11-08: A panel choice's session goes only where the tab's own requests went

A video chosen in the panel (and a video page handed over) carried its host's cookies for that
download, whatever host the page's player named: a page could name another site's file and have
dm fetch it with that site's sign-in (security review, 2026-11-07, row 159 c). Asked to choose
on 2026-11-08 between the tab's own site, the listed sites, and the hosts the tab's player
fetched from, the maintainer answered "implement it and verify it" without naming one, so
the one that keeps every site working was taken: the session now goes to the page itself, its
site or a host under it, or a host the browser itself sent one of that tab's requests to
(`decide::session_goes`; `RequestHeaders::reached`, kept per tab apart from the addresses and in
the session's storage). The last keeps TikTok's video servers, on a domain of their own, which
refuse a signed address without the session. Not done: the tab's own site alone, which would
refuse those; the listed sites alone, which would make every such site a setting first. Left as
is: a site the person listed and the app's "every site" switch, which the person chose. A file
the page names but the browser never fetched from that host goes without a session; a player
whose page is open long enough for its host to fall out of the 2000 pairs kept would too.

## 2026-11-08: A stream is renewed only from the tab it was handed from

The extension offers the app a newer signed address of a stream it handed over (gaps row 121)
when the page's player asks for that stream again. It matched across every tab, so another
tab's request for the same stream on the same host counted (security review, 2026-11-07, row 159
d). Each hand-off now keeps its tab (`Handed`'s `tab`, the request log's `tabId`), and only that
tab's requests renew it, at the hand-off (`renewed`) and later (`due`). Given up for it: a
hand-off from no tab (a caught download, which the browser's downloads API names no tab for) is
not renewed by the browser, and a player whose requests a service worker makes (no tab, -1) is
not renewed either; the app's own wait for the browser's next hand-off of the same file
(row 57, `fresh_link`) still covers both. The handed address itself was not used to find the
tab: YouTube's player never asks for it whole (each request names a byte range), so it is not
in the request log.

## 2026-11-07: dm posts its own toasts on Windows

notify-rust's Windows toasts each hang a click and a dismissal handler that are never taken
off, and Windows keeps three handles for each for the life of the process (BUGLOG BUG-012).
dm now makes the toast itself through WinRT (`dm-desktop` `toast.rs`), as notify-rust made it
(the same XML, PowerShell's app id), and takes the handlers off once the toast is clicked or
dismissed; at most eight toasts are heard at once, as eight threads waited before. notify-rust
stays for Linux and macOS. Not done: a fix to notify-rust or tauri-winrt-notification
upstream, which dm would wait on, and a toast that opens its file through the shell by
protocol activation with no handler at all, which would change what a click on an executable
does. An app id of dm's own (its toasts say PowerShell) is a change a person sees, not part
of this fix.

## 2026-11-07: The staged file is sparse on Windows

dm writes every range into one staged file at its own offset and makes that file as long as
the download first (2026-10, "Written in place, finishing is a rename"). On NTFS a file made
long that way is not sparse: the first write far into it makes Windows write zeros up to it,
so a 4.5 GiB download spent minutes writing zeros (BUGLOG BUG-011). The file is made sparse
before its length is set (`FSCTL_SET_SPARSE`). Not done: writing each range to a part file of
its own and joining them (reference A's way, and dm's before 2026-10): every byte would be
written twice again. Not done: `SetFileValidData`, which needs a privilege dm does not have and
would expose whatever the disk held before. A finished file stays marked sparse; with every
byte written it takes the same room as any other.

## 2026-11-07: A 429 to the browser's name moves that server to dm's own

dm sends Internet Explorer's name first, as reference A does (2026-11-01), and moved to its own
names only for a 403. Wikimedia's media servers answer a browser's name from a program with 429
and `Retry-After: 600` and serve dm's name at once (measured with each name, with and without a
`Referer`; BUGLOG BUG-010), now on the first request and now only on the ranges after it. So a
429 to the browser's name asks once more by dm's own at the first request (`probe.rs`) and, on
any later request, moves the server to dm's name for the rest of the run (`agents::limited`,
called from `Error::from_response`, where every refusal passes). Not done: waiting the 429 out
under the same name (the limit was on the name, so it never lifted; dm sat at 9 MB of 36 for 30
minutes), trying every fallback name after a 429 (a server that limits everyone would be asked
three more times at once; it is asked once), and moving a person's own choice (Identify as dm
only) to another name on a limit. What stays as it was: a server that limits every name is waited
out with `Retry-After` as the least wait (2026-11-04), one request more than before.


## 2026-10-31: The external video tool removed; videos as reference A gets them

Decided by the maintainer on 2026-10-31, asked which way to replace
the external video tool: "exactly like reference A". Measured the same day (reference A
6.43 build 16 on Windows 11): handed a YouTube watch address on its command
line, reference A made one connection, saved the page itself (6 KB) and
stopped; it read no video. Reference A reads no page: it gets a video only
from the requests the browser's player makes, caught by its extension.
dm already does that (`tests/caught_pair.rs`, no the external video tool present). So
the external video tool goes entirely: YouTube is read by dm's own reader (`youtube.rs`),
every other site only through the player's caught requests, and a site
only the external video tool could read is no longer offered, as reference A does not offer
it. What reference A does on a playing YouTube page, where the player's
requests are SABR and carry no plain address (2026-10-29), is to be watched
from outside (its connections and the files it writes), not by opening its
binaries, which its licence forbids. This replaces the earlier gaps entry's "the
maintainer names the sites" and the 2026-10-26 decision to keep the external video tool
behind the player's requests.

## 2026-10-31: dm and the other macOS manager, run side by side again

Asked to run both and make dm equal or better. The same cases, one app
after the other, on this Mac (the other through its local interface, dm
headless, release build of baa0972); the person's files and jobs in the
other app left as they were (only what the run added was removed):

| Case | Other macOS manager | dm |
|---|---|---|
| 168 MB, each connection held to 1 MB/s | 68.3 s | 21.2 s |
| Server down 40 s at 30% | moving 21.4 s after it returned, done at 41.3 s | moving 16.4 s after, done at 38.1 s |
| A 19 s YouTube video | 21.8 s | 3.6 s |

The folder-not-writable case was left out: on the other app's side it
needs the person's own download folder made read-only. Its clip, GIF,
MP3 and vertical modes are reached only through its window, so dm's were
checked on real YouTube videos instead (decisions above).

## 2026-10-31: YouTube's streams are asked for when YouTube serves them; MP3 on every core

Gaps row 55. A song's sound (dQw4w9WgXcQ, itag 140) was refused (403) to
dm's engine for both dm's reading and the external video tool's, and came only through
the external video tool's own slow download (46 s). Watched with the external video tool's own
`--print-traffic`: it asks with the same TV-client address and no token,
and is answered. The reading says why: `available_at`, four seconds after
it is made; the external video tool waits for it, dm asked sooner. dm now waits for a
reading's `available_at` (at most 10 s), and a stream refused within ten
seconds of its reading (dm's own reading carries no such field) is asked
for once more when six seconds have passed. The song's sound: 10 s.

The MP3 of it then took 46 s of encoding. Now: each table's bits counted
once per band (a split's cost is sums), the families with linbits priced
by their base codes and the values needing linbits, each granule's gain
searched from the last, the lines' 3/4 powers worked out once, and the
frames encoded on every core (no bit reservoir, so frames stand alone;
each run's encoder takes the two frames before it first and counts from
its start, and the bytes are the same as one run's, which a test holds).
3:33 of sound to MP3 in about 2.5 s; the song from link to MP3 in 12.4 s
(57 s before).

## 2026-10-31: Qualities named by their class, as people know them

From the comparison with the other macOS manager: its list says "4K,
1440p, 1080p" where dm said "MP4 3840×2026", the exact picture of a film
not shot 16:9. Each picture is now named by its class and its container
("4K MP4", "1080p MP4", "720p WEBM"): the larger of its shorter side and
its longer side's 16:9 height, snapped to the nearest of 144p ... 4K, 8K,
or its own lines where it is not near one (`videopage::class_name`). The
quality word a line carries ("2026/mp4") is as before, so nothing
downloads differently; the panel's list and Download details' chips say
the same. `youtube-check.py` reads the container from the line wherever
the word is.

## 2026-10-31: A part as a GIF: the system's decoder, dm's GIF writer

Gaps row 54, with no package and no ffmpeg. The part's H.264 samples
(read by dm's MP4 reader) are decoded by VideoToolbox, called directly
(`frames.rs`), twelve pictures a second kept, each made at most 480 across
and as tall as it is decoded (box averaging), within the part its edit
list plays and inside the stream's crop, so a vertical or square part
gives a vertical or square GIF. `gif.rs` writes them: one 256-colour
palette by median cut over a sample of their pixels (every pixel of small
pictures, else every seventh: every eighth missed a four-pixel pattern),
Floyd–Steinberg dithering, GIF's LZW (tested against a reader written to
the format), and a GIF89a that loops, its delays rounded so the whole
keeps time (8, 8, 9 hundredths for twelve a second). Checked on real
YouTube parts: 10 s at 480x270 and 270x480, 120 frames each, 10 MB, made
in under 3 s; a 6 s part downloaded square and saved as a 480x480 GIF.
In Download details, "Part as: Video / GIF" shows with Part, on macOS.

## 2026-10-31: MP3 by an encoder of dm's own; the system's AAC decoder in front

Asked for MP3 as dm's own code, with no ffmpeg and no added package.
`mp3.rs` is an MPEG-1 Layer III encoder (32, 44.1, 48 kHz; 192 kbit/s
stereo, 128 mono; constant bit rate): the standard's polyphase analysis
filterbank, a 36-point MDCT (over 9, which the decoder's unnormalized
inverse multiplies back: measured exactly 9.00 at five frequencies),
alias reduction, a binary search on the global gain for the finest
quantization that fits, Huffman tables chosen per region. No
psychoacoustic model and no bit reservoir. Its tables (`mp3/tables.rs`)
are generated from PDMP3's (public domain, Unlicense), read as data: each
decoding tree walked to its codes, every table checked to be a complete
prefix code. minimp3 (CC0) was tried first; compiling it here to probe
its window was refused by the session's permissions, and PDMP3 holds the
window in its natural order. The AAC is read by the system's own decoder
(`mp3/decode.rs`: AudioToolbox's ExtAudioFile on macOS, linked directly;
Windows and Linux are gaps row 53). Checked: tones through the system's
MP3 decoder at over 30 dB SNR, an AAC made by the system converted and
played back at its level, and a real video's sound (3:33) saved as a
valid 192 kbit/s MP3. Offered as "MP3 audio" after "M4A audio" in every
quality list dm hands out (`videopage::with_mp3`), so the browser's panel and
Download details show it with no change to the extension; its quality
`audio/mp3` fetches the M4A and converts it.

Also from the comparison with the other macOS manager's screenshots: its
4K and 1440p files are twice dm's (414 against 209 MB) because it takes
YouTube's VP9 where dm takes the MP4 stream, which there is AV1, as sharp
at half the bits; from 1080p down the sizes match. When YouTube refuses
the addresses of dm's own reading, dm now fetches the external video tool's reading on its
own engine before the external video tool downloads on its few connections. For one video
(dQw4w9WgXcQ's sound, itag 140) both readings' addresses are refused to
any fetch dm or curl makes, and the external video tool's own download succeeds; finding
why was stopped by the session's permissions (gaps row 55).

## 2026-10-31: Vertical and square parts by the stream's own crop, no codec

Asked for the other macOS manager's clip shapes (vertical, square, GIF,
MP3), and, as there is no ffmpeg in dm, for dm's own code rather than an
added package. Vertical and square need no codec: an H.264 stream's
sequence parameter set names the rectangle of its coded picture to show
(the frame cropping every decoder honours, the way 1080 lines are coded as
1088), and dm rewrites it to the middle 9:16 or 1:1 of the picture, with
the sample entry's and track header's sizes to match (`shape.rs`, applied
to picture tracks in `mp4mux::join_cut`). Nothing is decoded or
re-encoded, so it is instant and loses nothing; the file keeps the whole
coded picture and is as large as the part as it is. Checked on a real
YouTube part: macOS's own decoder (QuickLook) draws it 336x600 and 599x600.
Only H.264 can be shaped (VP9 and AV1 are refused in words); a part is
always MP4 already. In Download details, Shape (As it is, Vertical 9:16,
Square 1:1) shows once Part is ticked.

Left as gaps rows 53 and 54: MP3 needs an AAC decoder and an MP3 encoder
of dm's own (written from the standards, several thousand lines); GIF needs
decoded frames, which means a full H.264 decoder of dm's own (not taken:
a project of its own) or the system's (VideoToolbox on macOS, Media
Foundation on Windows; none on Linux), with a GIF writer of dm's own.

## 2026-10-31: Ideas from the other macOS manager's window, kept within dm's own

Asked for: run both side by side, take the other macOS manager's slick
window as inspiration (the maintainer: "do not completely modify" dm,
"just take good inspiration"), and better words. Its window could not be
captured here (no screen-recording permission), so the maintainer took 17
screenshots of it; dm's own were taken through its control port. Only
what was seen was used; no code, asset or wording was copied.

What it does better, and what dm took, each inside dm's own layout:

- **A failed download is hard to miss.** It counts failures above the
  list with one button to retry them all. dm now does the same
  (`failed_bar`: "{} downloads didn't finish.", Retry all, Hide; hidden,
  it stays away until another fails).
- **A link copied at a moment** (`&t=34`) is offered as a clip from that
  moment. dm's Download details now says "This link starts at 0:34." with
  a "From 0:34" button that ticks Part and fills its start
  (`youtube::start_of`).
- **Turbo Mode explains itself in steps.** dm's Second network now has
  "How to use your phone" (three steps) and names each interface as the
  system does ("iPhone USB (en5)", `second_path::system_name`).
- **Each browser says whether it is connected.** Options › Browser now
  shows Connected or Add for Edge, Firefox and the Chrome family, from
  the browsers whose extension has said hello (`host::browsers_seen`).
- **Its words** are short, say what happened and what to do next, and
  speak to the person. 42 of dm's messages were rewritten that way (the
  longest settings lines and most failure reasons); the translations
  kept their meaning and moved with them.

Not taken, and why: its library layout (a sidebar of kinds, rows grouped
by day with thumbnails, a paste bar and an inline card instead of a
dialog), which would replace dm's window, pinned by the behaviour
contract and declined by the maintainer; its conversions (vertical,
square, GIF, MP3), which need codecs dm does not have; its notch, plans
and invites, which are not a download manager's.

## 2026-10-31: A long list saves only what changed, to a journal

Gaps row 18. Every save wrote the whole list (9 MB at 20 000 rows), so
saves were spaced to ten times a write's length, up to 5 s, and a dm
killed in between lost what changed. Past 500 rows (the length written
compact already) a save now appends the changed, added and removed rows to
`queue.journal` (`queue/journal.rs`). A changed row is one that is not the
same allocation as at the last write: the last write's list shares every
row, so the queue copies a row before changing it (`Slot::entry_mut`).
`queue.json` is written whole again when the journal would pass half its
size, when rows are reordered, and at every start, which replays the
journal and folds it in. The journal's first line names the file it was
written over (its length and an FNV-1a hash), so a journal left beside a
newer file is not replayed; a line cut by a stop ends the replay. Logins
and sessions stay out of it, in `secrets.json`, written whole when one
changes. A list of 500 rows or fewer is written whole at every save as
before, which keeps `queue.json` complete for the check scripts that read
it. Measured at 20 000 rows (release): killed 0-50 ms after five quick
adds, 4 of 5 kept (1 of 5 before); small files 123/s (115/s); restart 1 s.
Not done, the row's other half, "a smaller row": `queue.json` and the
control protocol's `queue.list` are one shape (`tests/wire.rs`), and
leaving empty fields out would change what outside readers get.

## 2026-10-31: The queue's rows are shared with the window, not copied

Gaps row 25. Each read of the list (four a second while anything moves)
copied every row out of the queue, and the visible list copied every shown
row again; `Backend::queues` took the queue's lock and walked every row
several times a frame. Now a row is held as `Arc<Entry>`: the queue hands
out the same allocation until the row changes (`Slot::entry_mut`, which
copies it first if a list read earlier still holds it, so a list already
read never changes under its reader), the window keeps those
(`Queue::list_shared`), the visible list and its sort hold pointers, and a
row whose pointer is the one seen before is not hashed again to decide
whether to place it. The queue names are worked out once per read from the
list the window holds (`DmApp::queues`, by the same `queue_names` the queue
uses), with no lock. The control port's "is this queue known" check still
asks the queue. Measured with a release build, 5000 rows waiting and 20
downloads moving, the window thread's CPU time over 20 s (`ps -M`): busy
4.5 % before, 3.3-3.4 % after; memory 312 MB before, 128 MB after (the
copies a read made stayed resident). Not done: a read that returns only
the rows that changed. With the rows shared, a read is a pointer copy per
row under the lock, and the busy time left is drawing.

## 2026-10-29: The video helper fetched at the first start, its bytes told

`tools::freshen`, which runs at every start, now also fetches the external video tool (and
QuickJS where no Deno is installed) when neither is installed nor fetched:
twenty seconds after the start (`DM_PREFETCH_AFTER` for tests), so a dm
that runs for a moment does not fetch 36 MB, and never for a named
its environment switch. `tools::preparing_progress` tells the bytes so far of what is
being fetched (`Counting` in `fetch_once`); the row's "Getting ready for
videos" status appends them in units alone, so no catalog gained a string.
The one string that changed, the failure's, names both ways out in all
thirteen catalogs. Weighed and not done: a fetch at install time (the
installers are the same for every system, and fetching is the person's
setting to turn off).

## 2026-10-29: The external video tool's portable build where the release has no standalone one

`tools::fetch_into` reads the release's checksums first and takes the
standalone build only where the release lists one for this computer; else
the portable build (its file, one file of Python) checked the same way,
with the Python to run it noted beside it (its file), which
`videopage::writable_to` starts it through. `videopage::python3` is `DM_PYTHON`, or
the first `python3` on `PATH` or in the usual places that says it is 3.10
or newer, asked once a run. Unix only in practice: every Windows has a
standalone build. `tests/videopage_portable.rs` publishes only the portable
build and downloads through it.

## 2026-10-29: The external video tool's failures in dm's words, from a corpus of its lines

its file holds the external video tool's error lines as its readers
print them (YouTube's, Vimeo's, X's, Instagram's, TikTok's, the generic
one, the downloader's), each with the sentence dm says; `tests/videopage_words.rs`
checks every line is rewritten and that none of the external video tool's advice (flags,
its issue tracker, `the external video tool -U`, links) reaches a person. A line with no
sentence of its own says the fallback, the site named as people write it
(`site_name`) and the external video tool's words in brackets, cut before its advice
(`last_error`). The download's own failure line keeps its old shape,
"the external video tool could not download that: …", because the window's `format.rs`
reads the external video tool's words out of it to show after its own sentence; the window
was not changed. Private and age-restricted are judged before the sign-in
catch-all: The external video tool's words for both carry "Sign in".

## 2026-10-29: A failed the external video tool run fetches the newest the external video tool once and tries again

the external video tool's fix for a site change is usually out within days; dm's weekly
`freshen` left everyone waiting up to a week. Now a run that fails in words
that are not a known refusal (`videopage/refusal.rs`) asks the release's
`SHA2-256SUMS` whether a build other than the one dm noted at its fetch
(its file) is published, fetches it if so, and runs once more
(its file, used by its file and `read_choices_as`).
Once an hour at most, so a site that stays broken does not have dm asking
GitHub at every download; never for an external video tool the person installed, which is
theirs to update; never where fetching is off. Stable releases only:
nightly carries breakages of its own. its environment switch names a test's
server as the release (`tests/videopage_heals.rs`: a broken build, then the
fix, then nothing more within the hour).

## 2026-10-29: Like reference A without the external video tool, asked again and measured

Asked for: videos the way reference A gets them, with no the external video tool, and no
upkeep. dm already does what reference A is documented to do: the
extension reads the player's own requests, takes the picture's and the
sound's addresses, fetches both on many connections and joins them itself,
with no helper (`tests/caught_pair.rs` proves it with no the external video tool present).
On YouTube that finds nothing today: in Edge 156 on a playing watch page,
13 of 13 media requests were `POST videoplayback?sabr=1` with a binary body
and no `itag`, `mime` or `range`. So "no the external video tool on YouTube" means a client
of YouTube's SABR protocol (protobuf requests, UMP answers, proof-of-origin
tokens) written into dm and kept in step with YouTube, which is the upkeep
asked to be avoided; reference A does it with a paid team and a new build
each time, reference B does not do it at all. Kept as decided on
2026-10-26: the player's requests first on every site, the external video tool reading the
page only where they cannot be used, fetched by dm and current within a
week, so dm itself has nothing to keep up. Weighed again and not taken:
saving what the player plays through the browser's media source (only the
quality it picked, only as fast as it plays).

## 2026-10-27: The queue file is written at most every 150 ms

Found by the stress test (`scripts/stress-check.py bigqueue`): every state
change rewrote the whole list, pretty-printed and synced, and the caller
waited for it, so a long list made every operation slow in proportion to its
length (cancel 2 000 of 20 000 rows: 60 s). Now the first change after 150 ms
of quiet is written before the call returns, as before, and changes inside
the window are folded into one write when it ends (`Queue::save`,
`Queue::flush`); the write is made on a blocking thread. The window and the
headless loop flush as they end. What this costs: a dm killed within 150 ms of
a change forgets that change (the staged parts stay on disk). The file stays pretty-printed: tests and
people read it. Not done: a journal, which would also cut the cost of
each write on a very long list.

## 2026-10-27: Where the time between a YouTube click and its first byte went

Measured in Edge with the video file's real size on disk (not the space
reserved, which an earlier measurement counted). After a click with a reading
ready, 4-5 s went on: The external video tool runs for each of the two streams dm's own
request is refused for (YouTube answers dm 403; the refusal was handed to
the external video tool as a page, `download::is_stream` now stops that), and then the external video tool
*sleeping*: formats from YouTube's tv client carry an `available_at` four
seconds after the read, which the external video tool waits for ("as required by the site")
and which is not shortened here. The default read merges that client with
visionos; asking for visionos alone (`FAST_YOUTUBE_CLIENT`) lists the same
heights and no wait. It is the external video tool's own, documented client choice, and falls
back to the default read where it gives nothing. The read for the download
starts when the list is asked for, in parallel with the page's own data
(`mark_reading` before the helper starts, `kept_choices` where the page data
gives no list), not after it.

## 2026-10-27: The reading behind the panel's list is made by a process of its own

Measured in Edge on a real YouTube video: the quality list was already shown
at the click (0.02 s: it is asked for when the panel appears, and read from
the page's own data in about a second), so reading it inside the page would
have gained nothing. What took 5-8 s was after the click: the reading that
the host started behind the list (a thread) was ended with the host, which
the browser stops within seconds of its answer, and the download read the
page itself (3.6 s) and then each stream (`-O`). Waiting for the thread in
the host did not help (the browser ended the host anyway). The reading is
now made by `dm-desktop --warm-reading`, started detached from the host's
process group with the page and session on its standard input, so a click
after a second or more finds it (`wait_for_reading` waits for one still being
made). Not done: reading the player response in the page (an isolated
content script cannot see the page's own variable; the inline script is
stale after YouTube's in-page navigation).

## 2026-10-27: What a failed YouTube read says; cookies stay the browser's session

Checked against a report that YouTube downloads fail in dm: dm already runs
theexternal video tool it finds only if under 120 days old (else its own fetched copy,
refreshed weekly), hands the external video tool a JavaScript engine by path (Deno, else the
QuickJS dm fetched) on every read and download, and sends the page's session
or a chosen cookies file. What was wrong: the list read dropped the external video tool's
stderr, so no failure could be told from another. Now `said_by` gives a known
refusal in words, else the external video tool's last `ERROR:` line. Only the external video tool's own words
for a failed player script ("solving failed", "challenge solver", "no
supported JavaScript runtime", "only images are available") are called that;
"Requested format is not available" is not, as it has many causes. Not done:
`--cookies-from-browser` (dm passes the extension's session for that page
alone, so no browser profile is read and no cookie is kept) and
`--remote-components` (it has the external video tool fetch code from GitHub to run).

## 2026-10-27: Every type is caught out of the box

A download the browser starts is already a download (a page it navigates to
is not), so the type list only left files with the browser (`.txt`, `.json`,
an unlisted type) that a person wanted in dm. The default is now no list,
which `CatchRules::takes` already read as every type. A saved list that is
exactly the old defaults was never chosen and becomes empty
(`bring_up_to_date`, edition 2); a changed list, an Alt-click, a skipped site,
the size floor and a private window still leave a download with the browser.

## 2026-10-27: The panel's list for YouTube is read from the page's data

Measured: The external video tool takes 2.6-4.5 s over a watch page (its `player_skip=js` saves
about a second but drops the addresses the download needs; `android_vr`,
`tv_simply` and `mweb` list one to five formats, and `web` and `tv` fail with
"page needs to be reloaded"). The watch page's own `ytInitialPlayerResponse`
carries every picture and sound with its size, in one request of 0.8 s. The
panel's list is read from that (`videopage/page_data.rs`, same labels, qualities
and order as `choices`, checked against the external video tool on three videos); the SABR
streams carry no addresses, so the download is still the external video tool's reading of the
page, started behind the list (`warm_reading`). Any page that is not
`status: OK`, is live or lacks the data goes to the external video tool as before. Dropped: a
`player_client` pick for the external video tool (fewer formats, and fails by video).

## 2026-10-26: The external video tool reads a video page, dm fetches it

the external video tool downloaded what it read on one connection a stream, so a video ran at
one connection's speed (1 MB/s from a server holding each connection to
that) while dm's engine sat idle. Now `videopage::download_page` asks the external video tool
only what it would fetch (`-O "%()j"`, nothing downloaded, with the same
format choice, name template, sign-in and proxy as its own download) and
dm fetches each stream that is a plain file (`http`/`https`) with its own
engine, with the agent the external video tool gave it (`prefer_agent`; a site can bind its
addresses to the client it gave them to), and joins picture and sound as
before (`finish_dm`). Measured with the real the external video tool against that server,
40 ms away: 8 MB 8.69 s → 1.78 s, 32 MB 32.69 s → 4.77 s, the page's
reading included (`tests/throughput.rs`, `video_helper`).

Nothing that downloaded before can fail now. Streams in pieces (HLS, DASH
fragments), live ones and a site's own protocols stay the external video tool's; where dm's
fetch is refused, the external video tool downloads the same reading (`--load-info-json`),
so the page is not read twice, and from the page again if that reading no
longer serves. A refusal is remembered in the staging folder, so a resume
goes to the external video tool at once. A pause keeps each stream's parts under its format
id; a resume reads the page again (addresses expire after hours) and the
engine adopts them by size and validator. Subtitles are fetched from the
reading's addresses and carried in as before. The reading can hold the
site's cookies: written for this account alone and removed after.

The page is read once, not twice. The panel's list (`list_choices`, in the
browser's host) keeps the external video tool's whole `-J` reading for ten minutes in
`<home>/readings` (a file per page, named by a hash of its address, for
this account alone, the stale ones deleted at each write), and the
download a click starts has the external video tool choose its format from it
(`--load-info-json`, 0.23 s here with the server gone) instead of reading
the site again (1.7 to 6 s on YouTube, measured 2026-10-22/25). A reading
that no longer serves is passed over for a fresh one. A fresh reading
skips YouTube's HLS manifests and translated subtitles, as the list's
does: dm fetches neither, and they were about a third of a reading's time.

A picture and sound the extension caught from a player and that refuse dm
(YouTube's 403 after about a megabyte without the player's token) now fall
back to their page through the same route, rather than failing the row.

Why not without the external video tool, as some download managers are: they read the player's own requests in
the browser, which dm's extension already does (2026-10-25), and keeps up
with each site by shipping fixes every week or two (30-odd builds in a
year, most "resolved video recognition problems"). YouTube's web player
has since moved to SABR, whose requests name no format and cannot be
fetched by range; taking it over would mean writing YouTube's streaming
protocol (protobuf requests, UMP answers, its redirects and policies) into
dm, using the page's proof-of-origin token, and keeping it working alone.
The external video tool is kept current by many and fetched afresh by dm weekly. Recording
what the player plays was weighed too: only the quality the player picks,
and only as fast as it plays.

## 2026-10-26: A speed pass, measured (`tests/throughput.rs`)

The engine was measured before anything was changed: 1.1 GB/s on one
connection and about 1.5 GB/s on eight over loopback on a two-core machine,
server included. Nothing between the socket and the disk holds a real link
back, so the write path, buffers and locks were left as they are. What did
hold downloads back was waiting:

- **Small files on one connection.** `MIN_SEGMENT` (1 MiB) holds a file under
  2 MB to one range, and with `workers = connections.min(planned)` nothing
  was free to halve it. Cutting the file finer at the start (256 KiB, as a
  first version did) took a 1.5 MB file from a server that holds each
  connection to 1 MB/s, 40 ms away, from 1.55 s to 0.40 s, but cost every
  fast distant server a round trip, since each extra range opens a
  connection (80 ms away, no cap: 512 KB and 1.5 MB 0.27 s → 0.35 s). So the
  ranges are planned as before, and the connections beyond them are
  *helpers* (`ranged::run`): each waits 100 ms, watches the bytes arrive for
  80 ms, and halves a range only when more than 0.3 s of the file is left at
  that rate (`slow_enough_to_help`). A fast server finishes before they
  decide and never sees them (the test counts connections); a slow one is
  helped within a fraction of a second. Against the same two servers, in the
  same benchmark (`DM_BENCH_SMALL=1`), before → now: capped, 1.5 MB 1.55 s →
  0.80 s, 3 MB 1.14 s → 0.80 s; uncapped, 0.27 s → 0.27 s and 0.35–0.45 s →
  0.44 s. The rate is watched over a window rather than since the start,
  which counts the wait for the first byte as slowness (a first try, with
  "nothing yet" taken as slow, made 3 MB and 6 MB at 80 ms slower). Two FTP
  tests that paused a 4 MB download at 0.4 s found it finished and fetch 8 MB
  now.
- **A hand-off to a running dm waited for the inbox poll** (every 500 ms).
  dm now writes the port of a loopback UDP socket to `inbox/bell`, and the
  host sends one empty datagram after filing a request (`host::ring`); the
  watcher looks at once. The datagram carries nothing, so anyone else
  sending one only makes dm read its inbox early, and a flood is held to a
  look every few milliseconds. The poll stays for an older host, a missing
  bell, or a socket that could not be had. A notifier on the directory was
  weighed again and dropped for the reason it was dropped before. From
  `download.start` to the server's first request (`scripts/launch-latency.py`,
  median of nine, macOS debug builds): 520 ms → 59 ms with dm running (Linux
  under Xvfb, median of five: 509 ms → 13 ms); with
  dm closed, starting it took 16 ms before and 20 ms after (noise).
- **A download added while others run waited for the scheduler's next
  tick** (50 ms). An add, a resume, a retry or a queue start now wakes it
  (`Queue::kick`); the tick stays as it was.
- **The probe's byte is read** when its answer is small, so its connection
  goes back to the pool for the first range. Most servers send that byte
  with the headers, and hyper reused the connection already; one that
  sends the body apart had its connection closed and cost the first range
  a handshake.
- **One timer per body** in the range loop (`hls::Reader`) instead of a
  `timeout` made and dropped for every chunk; a pause is still seen within
  a quarter of a second. Within the benchmark's noise on loopback; kept
  because it does less per chunk and changes nothing else.

Weighed and not done: fetching the first range on the probe's own request
(`bytes=0-`) would save a round trip before the first byte, but a server
that answers a range of the whole file with 200 (Wikimedia does) would then
read as one without ranges and lose every connection but one, and a second
request beside the probe meets servers that allow one connection with a
429 on the probe. A caching DNS resolver: Windows, macOS and systemd all
cache lookups already.

## 2026-10-25: Update manifests are signed; logins are exact-host and https

`latest.json` is signed with ed25519 (`latest.json.sig`, made in the release
job by `cargo xtask sign-update`); the public key is built into `update.rs`
and checked with `ring`, already in the tree. The private key was generated
by `cargo xtask update-keygen` and is kept by the owner, never committed: it
must be set as the repository secret `UPDATE_SIGNING_KEY` or the release
job fails. Saved site logins no longer cover subdomains by default (a leading
dot opts in) and are never sent over plain http except to localhost.
Quarantine on macOS is set through `setxattr` (libc), on files only.

## 2026-10-25: TorBox built, then removed at the owner's request

A TorBox integration (own API key, Add URL › Through TorBox, a row command;
`torbox.rs`) was built and tested against a stand-in and against the live API
with a bad key, then reverted: no account was available to prove a real
fetch, and the owner judged it not worth carrying yet. It is in git history
(commits `cbfa3c8`, `291fc09`) if it is wanted again. The 2026-10-24 drop of
debrid providers stands.

## 2026-10-25: Sharing a finished file was built twice and removed both times

**Share on Gofile** (a guest upload, its page link copied) was built on the
owner's request (commits `0d505d1`, `f4089e0`, `020e3dc`) and a real upload
worked. It was removed the same day because it did not do what it was for: a
Gofile link cannot be downloaded by dm. The link is a JavaScript page (3 KB of
HTML), and Gofile's content API answers `error-notPremium` to a guest
account: listing or fetching a folder through the API is closed to free
accounts, and the website gets round it with a token taken from its own
script. Using that token would be getting round a restriction Gofile put
there, and it would break whenever they change the site. Gofile's terms had
also never been readable.

**Share on this network** (commit `81d9e00`: one finished file served on the
computer's own address, for an hour, with byte ranges and an unguessable link)
was built in its place, tested (a container fetched it byte for byte, by
range and eight at once; real clicks in the window; a security review that
found and fixed two weaknesses) and then removed at the owner's word: sharing
is not part of a download manager's job, and it was the one feature that
opened a port for others to reach. Neither feature is left in the code, menus,
strings, privacy text or allowed addresses; both are in git history.

Not built, and not wanted unless the owner says so: sharing beyond one's own
computer. dm downloads; it does not host.

## 2026-10-24: The work list is empty: what was built, and what was dropped and why

`docs/gaps.md` held 58 open rows. Each is now either built (below) or dropped
here with its reason, so nothing is left half-listed. The rule for building
was the project's own: free, legal, nothing that needs a server of ours, and
nothing on by default that listens or sends. A dropped row is not rejected
for ever; it names what is missing — a person, an account, money, a security
design — so it can be picked up when that exists.

**Built** (each has its own entry below or above): ZIP extraction, a command
line with an opt-in setting to find a running dm, a watched folder, per-site
rules, batch rename and a file's SHA-256 in Properties, a recording time
limit, the FTP torn-part repair, and the Wayland close fix, checked under
sway and KWin in Docker.

**Dropped because it needs a person's own machine, account or release**, which
no automated run can stand in for: dm on a real Windows desktop with a GPU
driver, tray and SmartScreen (`windows-checklist.md` lists the 17 checks); a
real login on any system; VoiceOver and NVDA; a drag by hand on macOS or
Windows; Brave's fresh-profile automation; a Chrome context menu clicked with
real input; the updater against a published release; the x86_64 AppImage and
each installer on a person's desktop; the two CI jobs only a release tag starts;
the Firefox store listing, which waits in Mozilla's review; waking a sleeping
computer on macOS and Linux, which `pmset` and `rtcwake` do only for an
administrator.

**Dropped because it costs money**, and dm is free and stays free: Apple's
notarisation ($99 a year, no free route); Chrome Web Store's one-time fee, so
Chrome-family browsers load the extension unpacked or from Edge Add-ons. A
free Windows signing certificate exists (SignPath Foundation, for open
source) but is applied for by the owner, not by code; until then the README
says how to open an unsigned installer.

**Dropped for law, terms or privacy.** Debrid providers and Gofile: their API
terms could not be read, and the reference project the owner named uses keys
from each person's own accounts, one of them possibly paid; dm will not claim
a provider it has not tested under terms it has read. Cutting advertising or
sponsor segments out of a video: it touches a service's terms and
technical protections, detection from the video alone is unreliable, and a
lossless MP4 cutter is a project of its own. Catching downloads other programs
start would need a system-wide hook, which is invasive and not free of
privacy cost.

**Dropped because it needs a server or a new security design.** Sending a link
from a phone: only a LAN listener fits (a relay for a phone off the network
would be a hosted server). It would add an address and nothing else —
no queue control, files or credentials — with explicit pairing, a per-device
token that can be revoked, TLS or a pinned key before any token crosses the
network, a rate limit and a confirmation in the window. That design is
written down here so it is not reinvented, and not built.

**Dropped as technically not worth it, or not possible.** Kerberos (needs the
operating system's GSSAPI; Basic, Digest and NTLM are done); Arabic and
Hebrew layouts (egui lays text out left to right only, so it needs
bidirectional ordering inside its layout and a mirrored window); the "Download
video" panel on a site whose security policy forbids WebAssembly (inherent:
the menu still works); telling an advertisement from the programme on a
player (needs a site's own knowledge, which dm does not keep); rewriting
the external video tool's site extractors and QuickJS (site logic is not dm's to write); a
browser that fetches a file again when it keeps a download dm refused
(Firefox's pause drops the connection); the one-name-for-every-package
command (`dm-desktop` in the .deb, `dm` elsewhere; the README says which).

**Left for later though free and possible**, because nothing needs them yet:
a schedule, retry count and When done for each queue (one for all today); a
button over a selection of links in the page; a stop time or start time for
one recording and a readout of its elapsed time and size; the hundred small
polish findings of the internal QA rounds (a column's arrow, a tooltip, a
window's size at 300% text), none of which loses a download or hides a
control. A defect found again is a new row.

## 2026-10-24: The command line finds a running dm by an opt-in setting

`dm-desktop list|status|pause|resume|cancel` reach the running dm over the
loopback control port. That port used to exist only under `--control`. Options ›
General now has "Let the command line list and steer downloads", off by
default, which opens the same port, with the same token only the account can
read, at every start. Off, dm listens on nothing, as before, and the command
says to start dm with the setting or `--control`.

## 2026-10-24: A watched folder

Options › Downloads names a folder dm reads every five seconds (even in the
tray or headless). A `.url`, `.webloc`, text file of links, or `.torrent` saved
there is read exactly as a drop is read (the first 64 KB of a text kind; a
torrent by its path), its links are added, and the file is moved into a `done`
folder inside it, so nothing is added twice and nothing is deleted. A link
already in the list is put aside without being added again. A file less than
two seconds old is left for the next look; symbolic links, hidden files and
other kinds are never read; a file with no link in it, or one that cannot be
moved, is said once and then left alone until dm restarts. Empty is off.

## 2026-10-24: Site rules apply when a row is added

Options › Network lets a host (and its subdomains, the most specific winning)
have its downloads saved to a folder and held to a speed of their own. The rule
is applied at the doors every row comes through (typed, clipboard, drop,
browser hand-off, the command line, the launch-time inbox) by one function,
`Settings::with_site_rule`, and a folder or speed a person chose for that one
download stands over the rule. The speed is the row's own limit (`speed_limit`
on `AddOptions`), so it is kept with the row and shown where a row's own limit
already is. A relative folder in a rule is ignored.

## 2026-10-24: Batch rename by a pattern, and the SHA-256 in Properties

Move/Rename of several finished files takes a name pattern: `{n}` counts from 1
in the order listed, `{name}` is the old name, the ending (and `.tar.gz`) is
kept, and empty keeps the names. Properties of a finished file shows its
SHA-256, worked out on the app's runtime so the window never waits, and a box
that checks it against a digest the person was given (MD5, SHA-256 or SHA-512,
told by its length, in any case).

## 2026-10-24: FTP parts are checked at their ends when a download resumes

HTTP re-reads a part's last 64 KiB on resume and cuts the part back to the last
byte that agrees; FTP trusted the part's length, so a part torn by a power cut
(zeros at its end) was kept and the file came out the right size and wrong in
the middle. FTP now does the same once per range per run, over a short extra
session (`REST` to 64 KiB before the end), and not at each retry of its own
writing. A server that cannot be asked is left to the fetch to report.

## 2026-10-24: The ZIP crate is 9, and the advisory check gates it

`zip` 8.6 carried three advisories about `ZipArchive::extract`, which dm does
not call (it reads and judges each entry itself), but `cargo audit
--deny warnings` fails the Audit workflow on any. It is `zip` 9, whose entry
names are fallible: a name that is not text refuses the archive. The same run
brought `tokio` and `clipboard-win` past a new advisory and a yanked version.

## 2026-10-23: ZIP extraction refuses whole archives and never replaces

Every entry is judged before any is written: a path that leaves the folder
(`..`, absolute, drive, backslash, a `:`), a link or device file, a password, a
repeated name in any case, a file that is also a folder, and totals of
entries, bytes and compression ratio past the limits fail the archive. The
bytes actually written are counted against the sizes the header claims. The
folder is always new beside the archive (or inside a folder the person picks),
so there is no overwrite question to ask; the archive is kept and nothing
extracted is opened or run; a failure or a cancel removes what was made. Only
Deflate and Stored entries: the crate is built without its C-backed codecs.

## 2026-10-23: Closing the window on Wayland minimizes it

A program cannot hide or unminimize its own window on Wayland (winit's
`set_visible` does nothing there), so close-to-tray cannot hide it. Marking the
window hidden anyway stopped dm drawing a window the compositor still showed
(a frozen ghost, KDE Plasma). dm minimizes on a Wayland display and keeps
drawing; the task manager restores it, which the tray's Show cannot. Checked
under sway and under KWin 6.8.1 in Docker (`linux-kde-close-check.sh`), where
the check fails on the original code.

## 2026-10-23: A recording's time limit is one setting

Not a field of each row: that would be a new column of the saved queue and a
migration, which one number in Options does not need. It stops the recorder
through its own control so the file is closed, instead of dropping the task
mid-write.

## 2026-10-23: Source is split by concern, and a file has a size limit

`app.rs`, `dialogs.rs`, `download.rs`, `queue.rs`, `main.rs` and the window
tests were each one file of thousands of lines, found by reading the code
rather than by any rule. They are now directories of files by concern
(`src/app/`, `src/dialogs/`, `dm-core/src/download/`, `queue/`, the window
tests under `tests/window/`), moved verbatim: the only edits are `pub(super)`
on what now crosses a file, and rustfmt's rewrapping of the longer lines.
Behaviour, the window, and the tests' names are unchanged.

Two rules keep it that way. A source file over 2500 lines fails `cargo xtask
layers`. A module's tests go in `tests.rs` beside it, not inline, and the
privacy and translation scans skip a file by that name (before, they stopped
reading at `#[cfg(test)]`); a scan that read a moved test file as shipped code
would have taken its made-up addresses and strings for the product's. Struct
definitions stay in the parent module, so the fields their children use need
no widening.

## 2026-10-22: Arabic and Hebrew wait for bidirectional text in egui's layout

egui 0.37 shapes Arabic (harfrust) but lays every line out left to right in
stored order, and has no paragraph direction (epaint's `unicode.rs` still
says "TODO: heed bidi characters"). dm reorders single-line names itself
(`bidi.rs`). An interface in Arabic or Hebrew cannot be done the same way:
a message that wraps must be reordered line by line after it is broken,
which only the layout can do, and reordering strings before they reach
egui would hand screen readers the words backwards. The way there is a
vendored epaint, as winit is vendored: the Unicode bidirectional algorithm
run per laid-out row (reorder its glyph runs by level), right-aligned rows
for a right-to-left paragraph, then the window mirrored (the places on the
right, the toolbar and columns from the right) and the two catalogs. Until
then dm offers the eleven left-to-right languages it has.

## 2026-10-22: Mica is opt-in, off by default, from the next start

Options › General offers "Mica" on Windows 11 22H2 and later (build
22621, read from the registry) and nowhere else. Chosen, the next start
makes the window see-through, clears its frame (`theme.rs` makes the
`frame` token transparent; the list, dialogs and fields stay solid to be
read on), asks DWM for the main-window backdrop and extends the frame over
the client area (`chrome.rs`). Read once at start, since a window is made
see-through only when it is created. Off by default for the reason below:
an old GPU driver or a desktop with no compositor can draw the window
black, and it has not been watched on a real Windows 11 desktop. Someone
who sees black turns it off in Options; deleting `"mica": true` from the
settings file does the same.

## 2026-10-22: The title bar takes the window's colour; no Mica backdrop yet

Fluent's windows read as one surface from title bar to content. On Windows
dm tells the window's frame its look through DWM: dark mode where the look
is dark, and the caption in the window's own background colour, its words
in the text colour (`chrome.rs`). Mica itself (`DWMWA_SYSTEMBACKDROP_TYPE`)
shows only through a see-through window, which means drawing egui's
panels with a transparent clear colour; where the compositor or an old GPU
driver cannot, the window comes out black. That stays off until it can be
watched on a real Windows 11 desktop.

## 2026-10-22: dm does not get around copy protection

dm has no DRM remover and never will: no licence-server requests, no key
extraction, no content-decryption-module hooks. What it decrypts is HLS's
own AES-128 scrambling, whose key the server hands to any player that asks
(as every player and standard decoder reads it). A stream whose key belongs to a rights
system is refused as copy-protected: SAMPLE-AES, any `KEYFORMAT` other than
`identity` (FairPlay, Widevine, PlayReady), or a key address dm could only
reach through one (`skd://`, a `data:` licence). The external video tool is never given
`--allow-unplayable-formats`, and its own "DRM protected" refusal reaches the
row in plain words. In the browser, a player the page plays through EME (its
`mediaKeys` set) gets a "Protected video" bar that says dm does not download
protected video and hands nothing over.

## 2026-10-17: A key is taken by the command that asks for the most modifiers

egui matches a shortcut ignoring an extra Shift, so ⌘N matched ⌘⇧N and Add
URL opened in place of the numbered series. Rather than match modifiers
exactly (which would break a key whose character needs Shift on some
keyboards), every command's keys are tried with the most modifiers first,
and the first that matches consumes the press. Remove follows the platform:
⌘⌫ (shown) and ⌫ on macOS, whose keyboards have no Delete without Fn, and
Delete elsewhere. Keys that act on the selection do nothing while a dialog
is in front; the per-download progress and completion windows do not count
as in front. A dialog asked for while it is open is raised, not rebuilt.
Window state (`persist_window`) goes to `$DM_HOME/window.ron` when `DM_HOME`
is set, so test and screenshot runs leave the person's own alone.

## 2026-10-17: A frame carries the oldest protocol version that reads it

`download.start` gained `cookies`, `mime` and `deadline` while
`PROTOCOL_VERSION` stayed 1, and serde ignores keys it does not know: a
newer extension (stores update it on their own) talking to an older app
had `mime` ignored, so a sign-in page served in a caught file's place was
taken and the browser's copy erased. Bumping the number alone was not
enough: a version-1 host does not answer a frame of another version at
all, so a hello at 2 would read as "no app" and the menus would stop
working too. So a frame's `protocolVersion` is the oldest version that
reads it correctly: a `download.start` for a download the browser holds
(`held`) goes at 2; everything else, a file chosen from a menu or the
panel included (checked, but the browser holds nothing to lose), and every
reply, at 1. Both sides read 1 and 2. The hello says the newest
version the app reads (`protocol`, absent from a version-1 app), and the
extension hands a download the browser holds only to an app that says 2 or
more; an older app keeps getting what a person asks for from the menus and
the panel, which the browser never started, and the popup and one
notification say "update the dm app". A caught download sent to an older
app anyway is not answered and is given back after the wait. A frame of a
version outside 1..=2 now names the side to update, not `--register`.

## 2026-10-17: Alt-click and the page's links are plain JavaScript

They were registered by the WebAssembly page script, so on a page whose
policy forbids WebAssembly (where Firefox applies it to content scripts)
Alt-click no longer kept a download in the browser and "Download all
links" said to reload. They are a few lines each and read nothing
WebAssembly decides, so `extension/content.js` registers them before it
imports anything; the panel stays Rust. A unit test holds the message
names in the script to the constants in `decide.rs`.

## 2026-10-17: A lock dm cannot read stops dm, and says so

`instance.lock` that cannot be opened or locked (a directory in its place,
a file system without locks) used to read as "another dm is running", and
dm handed its links to nobody and exited without a word, every launch.
Starting anyway without the lock was weighed and dropped: the next launch
would start a second dm the same way, writing the same `queue.json`. So
dm exits with the reason on stderr and in a dialog (none when headless),
and the native host treats the lock as unreadable rather than as a running
dm: it refuses the hand-off, and the browser keeps its download. A lock
file left by dm run as another user is opened to read instead, which an
advisory lock allows, so `sudo dm` once does not cause this. The host's
probe still takes the lock for a moment; a dm starting then retries for
half a second before believing another is running.

## 2026-10-17: Waiting connections to the control port are evicted, not refused

Connections that have not presented the token wait in their own list,
apart from the 16 authenticated slots, and for at most 5 s. When the list
is full the oldest is closed for the newcomer, so a program filling the
port with idle connections cannot keep out the owner, whose connection
authenticates in a round trip. A first line that is not a valid `auth`
closes the connection.

## 2026-10-17: A handed-over sign-in belongs to one download and one server

The first version kept handed-over cookies in a process-wide map by host,
filled when a row started and never emptied: a later download from that
site, one never signed in, or one whose address was edited, carried them
too. Now a row's cookies travel in that download's options as a `SignIn`
(the server they came from, the header) and are sent only to requests for
that same server, so a mirror, a CDN redirect or another row never sees
them, and an address edited onto another server drops them. A row keeps its sign-in until it is deleted from the list: dropping it
when the download finished was tried and broke Redownload, and left the
recheck queue probing signed out, where a sign-in page of another length
reads as a changed file. dm's files are written readable by the account
alone instead. The external video tool gets
them in a private cookies file of its run (mode 0600, removed after),
not as a command-line header any local `ps` could read. The control
protocol and MCP say only whether a row has a sign-in.

## 2026-10-16: Released unsigned, with the free signing there is

dm earns nothing, so nothing that costs a yearly fee is bought. Releases
go to GitHub unsigned for every platform, and the README says how to open
one. Where signing is free it is taken: Mozilla signs the Firefox build at
no cost without a store listing (self-distributed), which ends the
"temporary add-on every session" problem; SignPath Foundation signs
open-source Windows builds through CI at no cost, and dm applies. Package
managers that cost nothing carry it by name: winget and Chocolatey on
Windows, a Homebrew cask on macOS (no quarantine warning there), the AUR
and Flathub on Linux. The Chrome Web Store's one-time five dollars is the
one paid item worth it, when it can be spared; Edge Add-ons is free. Apple's
notarisation has no free route and is not bought: the dmg stays unsigned
with right-click-Open instructions.

## 2026-10-16: A row remembers the folder its parts began in

The folder for unfinished parts is a setting now. The obvious reading —
"takes effect for downloads started afterwards; the rest start over" —
loses progress silently, so instead each row records the folder it first
staged under and resumes there whatever the setting says since. The
queue's parts helpers (discard, connections, resumable) read the row's
folder first. A `.ts` stream is remuxed, not re-encoded: `-c copy` into
an MP4, kept as `.ts` when no converter is installed, a digest names the `.ts`
bytes, or the remux fails — a stream a person can play beats no stream.

## 2026-10-16: Escape is one key, and the menu's before the dialog's

An Escape that closes a menu, a combo box or a right-click menu is theirs:
egui closes them on it without consuming the key, and they draw before
the dialogs, so by the time the dialogs see the key the popup is already
gone. The window remembers whether one was open at the end of the last
frame and lets that Escape pass. A text field with the keyboard lets it
go on the same Escape that closes its dialog, as dialogs generally do;
the two-step the code once described was never what the code did.
Dialogs close in the order a person means — a question first, Find and
the other small ones over the large ones, then the large — since nothing
records which was opened last. Found by a code review of 2026-10-16.

## 2026-10-16: A site search stays on its site and stops at a hundred pages

"Files on a site" can now read pages past the first, but only pages on
the start page's host, at most five deep and a hundred in all, and a
path can be skipped by a word in it. The bounds are the point: a search
that follows every link is a crawler, and a person wanting a site's
files wants that site's. Saved searches run from the window's own tick
(once a minute it looks for one that is due), through the same client as
the downloads, so the proxy and the timeout apply; what they find is
added for later, never started, and never twice.

## 2026-10-16: A proxy that is off is no proxy; the computer's settings are a kind

reqwest's default client reads the environment's `HTTP_PROXY` and friends,
and with its `system-proxy` feature the macOS network settings too. Turning
that feature on to offer "the computer's proxy settings" made every client
dm builds take over a second on this Mac (`SystemConfiguration` is slow to
answer), and a test that paused a download at 400 ms found the download had
not started. So the system's settings are read only for the one proxy kind
that asks for them (`builder_with(read_timeout, system_proxy)`); a proxy
that is off goes out directly, whatever the environment says, and a proxy
of dm's own covers every address but its bypass list. A proxy dm uses is
one chosen in its Options. PAC scripts are not read: they are JavaScript.

## 2026-10-16: Digest is answered; NTLM and Kerberos are not

A 401 with a `Digest` challenge is answered in `auth.rs` with the login the
address carries (RFC 7616, MD5 and SHA-256), and the challenge is kept per
server so the ranges sign in from the start. Negotiate, NTLM and Kerberos
are not attempted: they authenticate a TCP connection, not a request, over
several round trips, which a pooled HTTP client does not promise to keep
on one connection, and dm has no Kerberos ticket cache to draw on. A server
asking only for those gets the 401 said with what it asked for.

## 2026-10-16: Attempts, timeout and per-site connections are settings; one ceiling still

Five attempts and a 30-second read timeout were constants. A person on a
link that drops for minutes wants more attempts, or none at all ("keep
trying": zero), and a server that assembles an archive for a minute is not
dead to the person who knows it; both are Options › Network now, with
bounds (100 attempts, 600 s). A server that refuses a second connection
gets a limit of its own by host or suffix, which never raises the setting.
The speed ceiling stays one for everything (see 2026-10-12): per-download
limits were asked about and are still not wanted, because a shared ceiling
is what a link has.

## 2026-10-16: Rows are clicked by rectangle, not by the accessibility tree

The Linux real-input check (`scripts/linux/gui.py`) finds the toolbar, menus
and dialogs through the AT-SPI accessibility tree and clicks them where it
says they are. Table rows are the exception: egui's accessibility tree
reports a virtualized table's cells at the wrong screen position (a row at
y=244 was announced at y=431), so a click there selects nothing. Rows are
instead clicked by their own rectangle, which dm reports over the control
protocol (`ui.inspect`), plus the window's on-screen origin. The same
protocol is the check's oracle: on any failed check it prints dm's view of
the window (open dialogs, selection, rows) so a failure says why.

A running or queued download cannot be deleted until it is stopped
(`enabled(Command::Remove)` excludes those states): dm keeps an active
download safe, and the test follows that — Stop, then Delete.

## 2026-10-16: An installer is installed and run before it is released

Building an installer proves nothing about installing it: the .deb named
too few libraries for a month without a failing build, because GTK (gone
with the tray's switch to D-Bus) had been bringing winit's X11 libraries
along. So the release workflow installs each installer on its own system
the way a person would (apt, dnf, the AppImage itself, the Windows
installer silently, the app copied out of the dmg), runs
`scripts/smoke.py` against what was installed — the native host's answer,
the window opening under a display, a download byte-exact — removes it,
and drafts the release only when every one passed. The libraries winit and
glow open are named in every package, by package name for .deb and Arch
and by library (soname) for .rpm, which Fedora and openSUSE both resolve.
Test runs use nextest's `ci` profile, a hung test killed at five minutes.

## 2026-10-15: The Linux tray is a StatusNotifier item

Linux uses `tray-icon`'s KSNI backend over the session D-Bus instead of its
GTK/AppIndicator backend. Niri, GNOME and KDE expose StatusNotifier hosts;
requiring a separately installed AppIndicator library made the tray vanish
on this CachyOS/Niri machine even though the session already had a host.
KSNI owns its service thread and exports the same icon and menu without
GTK-specific startup. The test machine's StatusNotifier registration and
menu callbacks are verified.

The tray is made whether or not a watcher is up, since at login dm can start
before the panel and KSNI registers when one appears. Whether the window may
hide is asked on closing, and the answer is whether the watcher lists dm's
item, not merely whether a watcher exists: a watcher that has just started
can take its bus name before it answers, and drop a registration that lands
in that gap without a reply (seen in the Linux check after a watcher
restart, which is what reloading waybar does). So a thread checks every five
seconds and registers the item again when the watcher does not list it, and
a login launch that finds no tray showing it after ten seconds opens its
window rather than run unseen.

## 2026-10-15: dm's own names, list format and remote control

dm is its own program and names no other. Its menus, dialogs, columns,
categories and themes use dm's own words (Program menu, "Download details",
"Add numbered series", "Files on a site", Videos/Audio/Apps/Archives/
Pictures, Light/Dark/Classic). Export and import use dm's own `.dmlist`, a
JSON list of rows; a plain list of links still imports. The only remote
control is dm's own token-protected control protocol (`--control`) and its
MCP server (`dm-desktop mcp`); the extra JSON-RPC server that spoke another
program's protocol was removed, with its setting. The window's controls are
held by window tests that click every menu item, toolbar button and
row-menu item by its name, not by a catalog of another window.

## 2026-10-15: FTP by a client of dm's own

A download needs six FTP commands; a library would bring TLS and a second
async runtime's worth of choices for them. Passive only (active needs the
server to reach the client, which a home router stops). The data
connection goes to the host the control connection reached, never the
address a PASV names, as curl does: right behind NAT, and no way for a
server to point dm elsewhere. FTPS is not done. FTP goes through a SOCKS5,
SOCKS4 or HTTP CONNECT proxy (2026-10-16), both connections, with the
server's name handed to the proxy for the data connection; an HTTPS proxy
is refused, not sent around.

## 2026-10-15: Ranges are halved under the connection fetching them

Pieces alone (about four per connection) left the last slow piece to one
connection while the rest sat finished. Now, when a connection frees up,
dm halves the largest range left and gives it the second half. Each range has a
small lock of its own holding where it ends and how far it has claimed;
its connection claims a chunk before writing it, and a split moves the end
only past the claim. Splits append ranges, so part order is no longer file
order: assembly sorts by start, and the overlap byte is checked against the
range before in the file. The plan file is rewritten before a new range
fetches, which keeps "the plan beside the parts is the plan a resume
finishes" true. Halves stop at 256 KiB, and a download has at most 256 ranges.

## 2026-10-15: A video is an MP4 unless WebM is asked for

Left to itself, the external video tool picks YouTube's best stream, which is AV1 or VP9
in WebM with Opus sound: files many players (QuickTime, Windows' Films &
TV, phones) open badly or not at all. dm now
asks for MP4 picture and M4A sound, H.264 ahead of AV1 at the same height,
joined into an MP4, whenever no WebM line was picked. Above 1080p YouTube
has no H.264, so 4K is VP9 in an MP4: the height asked for wins over the
codec.

## 2026-10-15: The window's language is per thread, and held by a walk

`i18n` keeps the language in force per thread. The window is drawn on one
thread, so it is the window's; and the window tests, each on a thread of
its own, can put one window in German while another runs in English. A
process-wide value made them fail at random. Words for a script or an
agent (the control protocol, MCP) stay English on every machine; the status line's sentence is kept in English
with its values (`i18n::Said`) and worded only where it is drawn, so
`said` reaches them in English too. What is translated is held two ways: a scan of the source for every string passed
to `t`/`tf`, and a walk of the window in each language that records every
string asked for and missing, which the scan cannot see for words chosen
at run time. A translation may name its values `{0}`, `{1}` where its word
order differs.

## 2026-10-15: A file dm is refused goes back to the browser, from the panel too

A caught download dm cannot fetch was already left to the browser. A file
from the panel or a menu was not: dm was refused (W3Schools' 403 on one
Linux network) and the person got nothing. Such a hand-off is now checked
the same way, and when dm is refused or absent the extension starts the
browser's own download of it and the panel says so. A player page (handed
to the external video tool) is not sent back: the browser cannot download a page as a video.
The probe also tries a browser's agent, with `dm/<version>` after it, as a
last rung after dm's own and the plain `dm/<version>`: it is still named.

## 2026-10-14: Browser drags can be URL-backed temp files, not just regular files

The window reads dropped files through the egui file handle, but some browser
sources hand over a temporary file or a URL-shaped path when they cannot expose
readable bytes. dm now falls back to the dropped path and parses URL-like text
from it before giving up, which fixes the common browser-drag case without
claiming a full live-browser drag has been proven end to end.

## 2026-10-14: More pieces than connections, taken in turn

One fixed range per connection let one slow connection decide when a
download finished. Splitting a range under a connection that is still
filling it would need part files to change
meaning mid-download, which dm's resume and its one-byte overlap check rely
on not happening. Instead the file is cut into about four pieces per
connection, and connections take the next unstarted piece in file order:
the same effect for any slowness short of the very last piece, with no
change to resume or assembly. 8.36s became 2.26s in `tests/speed.rs`.

## 2026-10-13: The installed app registers itself with the browsers

A person who installs dm from a release should not need a terminal. Each
time the installed app opens, it checks that every installed browser's
host manifest names this copy, and writes those that do not, keeping any
extension ids already allowed there. A development build, a dm with
`DM_HOME` set (every test), and a copy running from the mounted disk image
do not, so none of them can take the browsers from the installed one. An
AppImage registers the AppImage file, not the program mounted inside it.

## 2026-10-13: A range answered whole is not always a changed file

A 200 to a range used to mean the file was republished, and the download
failed. Wikimedia's caches answer several ranges at once that way under the
same ETag, and a range of every byte too. Now the validator decides: the same
one means the server stopped serving ranges, and the download starts again
on one connection (a whole answer to a range of every byte is the file);
another one still means the file changed. dm's user agent also names the
project's address, which Wikimedia requires.

## 2026-10-12: Light or Dark by default, following the computer

dm's window draws every colour from `theme.rs` tokens. The default look is
Light or Dark as the operating system is set; Classic stays as a choice.
An openly licensed typeface is embedded (Inter since 2026-10-18; see below). Fonts given to egui take
effect a frame later, and egui panics on a family it has not loaded, so
semibold is used only from that frame on.

## 2026-10-12: Notifications are posted off the window's thread

On macOS, posting a notification waits on the system for about two seconds.
From the window's thread, each finished download froze the window that long.

## 2026-10-12: A video from the panel asks first

The app opens "Download details" for a panel video (`ask_panel_downloads`,
on by default). The picked quality travels with its container, and the external video tool is
told to prefer that container, so an "MP4" line arrives as .mp4.

## 2026-10-12: One speed ceiling for everything

A limit per download would let N downloads use N times the limit. dm's
is one ceiling, shared by every connection and split with torrents and
the external video tool runs. A limit is set to leave the link usable, and that is a
statement about the link, not about one download.

## 2026-10-12: The browser extension is Rust compiled to WebAssembly

Only two small loaders are JavaScript, because a Manifest V3 background and
a page script must start before WebAssembly can load. Its decisions live in
`decide.rs`, tested on the host.

## 2026-10-11: The window is egui, not a web view

dm's window is drawn in Rust. A Tauri
web view was replaced: one language, one process, and a window the tests
drive through its accessibility tree.

## 2026-10-11: Player pages go to the user's the external video tool

dm writes no site logic, token minting or signature work of its own. It runs
theexternal video tool the person installed and places what it produces. Nothing behind
DRM is attempted.

## Always: free, with nothing reported

No analytics, crash reporting, telemetry or paid service. `cargo xtask
privacy` fails the build on an address that would phone home.

## 2026-10-15: Power cuts are repaired at resume, not prevented by fsync

A part's length is its resume ledger, and after a power cut the length can
be on disk while the last bytes are not. Syncing every write would prevent
it at the cost of throughput; instead a resume fetches each part's last
64 KiB again, compares, and cuts the part back to where it agrees. The
queue and settings are synced before they replace the old copy, since an
empty queue file forgets every download. The plan is not: it is rewritten
at every split, a flush each time halved 8-connection speed, and a stale
plan still covers every byte while an empty one only costs a refetch.

## 2026-10-17: A recheck replaces a file only with a whole one

A recheck that finds a file changed fetches the new version beside the
old, which the row keeps pointing at (`replaces`), and moves it over the
old one only once it is whole; a failure leaves the old file and says so.
Deleting first, a server that failed halfway left nothing. What is compared
is what the row was fetched from: a Metalink row by the file's length
alone, since its ETag is one mirror's of many; an external video tool or stream row not
at all, because its address is a page or a playlist and nothing it serves
describes the file made from it.

## 2026-10-17: SOCKS5 resolves names at the proxy; FTP paths are the login's

A SOCKS5 proxy is handed out as `socks5h://`, to dm's client and to
the external video tool, so a name is resolved by the proxy: resolved locally, a name only
the proxy's network knows cannot be reached, and every site is told to the
local resolver. librqbit reads only `socks5://`, so the swarm is given that
spelling; its peers are addresses already. An FTP address's path is relative
to the login directory, as RFC 1738 has it, and `%2F` reaches the root.

## 2026-10-17: A page named as a document is that document

A link whose last segment names a document (`.html`, `.xml`, `.txt`, not a
script's `.php` or `.aspx`) is fetched by dm's own client, with the row's
referrer and cookies, and never handed to the external video tool. It used to go to the external video tool
first, falling back only on "unknown site"; any other refusal (a 403 to a
request without the page's sign-in, a 429) failed a page dm could save.
Some video sites do end watch pages in `.html`: a quality picked in the
browser's video panel (which carries a container) still goes to the external video tool. A
watch address with no extension, or a script's, is the external video tool's or refused.

## 2026-10-17: How far a power-cut repair looks back

A resume compares a part's last 64 KiB with the server; where the disk has
zeros the server does not, the part is cut back to where the zeros begin.
That scan stops 4 MiB back: far more than a disk block or a write buffer,
and little to fetch again where the zeros were the file's own (a disk
image, a sparse archive), which unbounded meant gigabytes. A longer loss is
still found, 4 MiB at a time: the window at the cut opens on zeros the
server does not have and is cut again. Each step goes straight to the next
attempt rather than spending one of the range's retries.

## 2026-10-17: A stream's segments are named by their bytes

The first segment's first 4 KiB (from the staged part on a resume) decide
TS, fMP4 or packed ADTS audio; the playlist's addresses decide only where
the bytes say nothing or are encrypted. Tokenised segment addresses have no
extension, and ADTS from them was called `.ts` and sent to the remux. A
changed initialisation segment is decided once, in `beyond_dm`, on resolved
addresses, and goes to the external video tool.

## 2026-10-17: Secrets stay off the external video tool's command line

The proxy (with any user name and password) goes in a 0600 config file of
the run's own, passed with `--config-locations` and removed when the run
ends, as the cookies already were: any local user's `ps` reads a command
line. The process group is ended with `libc::killpg`, not by running
`kill`, which a minimal `PATH` may not find.

## 2026-10-17: egui 0.37, for screen readers on at-spi2-core 2.62

at-spi2-core 2.62 (Arch since 2026) dropped the bus's `ScreenReaderEnabled`
property and keeps only `IsEnabled`. egui 0.36's accesskit_unix 0.21 waited
for a change to `ScreenReaderEnabled` before putting the window on the
accessibility bus, so there dm was invisible to Orca and to every AT-SPI
lookup; accesskit_unix 0.24 (egui-winit 0.37) listens to `IsEnabled`. A
`[patch.crates-io]` of accesskit_winit alone was the fallback, not needed.
What 0.37 changed under dm, kept as it was: eframe's clipboard and egui's
emoji fonts became opt-in features (`clipboard`, `monochrome_emoji_fonts`),
both turned on; a screenshot is now a callback (`request_screenshot`), not
an input event. 0.37's test harness panics on a glyph no font has, which
found the table's sort arrows (↑ ↓) drawn as boxes all along: Hack, already
bundled, is now the last fallback for the proportional families.

## Free space is checked once, at the start (2026-10-17)

A download whose stated length exceeds the free space is refused before a
range is fetched: the parts' disk must hold what is not yet staged, and the
destination's disk the whole file when it is another disk (on one disk,
assembly frees each part as it is consumed, so the length is enough). Read
with `statvfs` on Unix (`libc`, already a dependency) and
`GetDiskFreeSpaceExW` on Windows through `windows-sys` — already in the
tree through tokio and others, only its `Win32_Storage_FileSystem` feature
named; a crate of its own for one call was not worth it. Where the space
cannot be read nothing is refused. Space taken by other programs during the
download is not watched: a disk that fills midway fails the write as before.

## A refusal's body is said only when it says something (2026-10-17)

"the server answered 404 Not Found" carries the canonical reason. The body
is read down to its text as before, and then dropped when, without the
status, its reason phrase, filler words and a server signature, nothing is
left — the boilerplate pages of Python's http.server, nginx and Go. A body
with words of its own (a rate-limit sentence) is kept.

A file system that reports no total size (some FUSE and SMB mounts answer
0 free of 0) is taken as unknown and not checked.

## A folder name: usable here to load, portable to make (2026-10-17)

`one_folder` decides whether a saved category or kind's folder is loaded
and used: one folder, not `.` or `..`, and on Windows a name Windows can
make. `portable_folder` adds every computer's rules (no `<>:"|?*` or control
characters, no trailing dot, no device name) and is applied only to a name
being made or renamed. Holding saved names to it dropped "Live: 2024" made
on a Mac at the next launch, and the next save deleted it with its types
and sites.

## When done: what counts as done (2026-10-17)

"When done" fires when nothing is left that dm will finish by itself. That
means nothing Running or Queued and nothing the schedule or download limit
holds, since those start again when the window reopens. Something must also
have finished or failed since the queue last ran dry. A row a person stopped
is theirs: it is left alone, not waited for, so it does not keep the action
from firing once the rest has finished. A Stop all that finished nothing is
not done, and the choice stays armed for the next stretch. Failed rows are
settled, because nothing retries them without a person and their parts stay
for Retry.

## Login item and host manifests follow the installed copy (2026-10-17)

On launch an installed release dm (no `DM_HOME`) rewrites the host
manifests when they name another copy or do not allow this version's
extension ids. A development build fixes only a missing one. The ids
already allowed are kept, including Firefox's retired
`dm@pranavo72bex.github.io`: that is harmless, and it keeps an old copy of
the extension working until it is replaced.

The login item is taken from another copy only when that copy's program is
gone. Otherwise two installed copies (a .deb and an AppImage) would take it
from each other at every launch. `--unregister` removes only this copy's
item, one whose program is gone, or one it cannot read, and it names the
copy it leaves alone. On Windows it calls `disable()` unconditionally,
because an item turned off in Task Manager reads as not enabled.

On macOS a copy is transient when it runs from AppTranslocation, or from a
read-only mount under `/Volumes` (a disk image). A writable external disk
is an install like any other. A transient copy, or one in Downloads,
Desktop or Documents, is never registered with the browsers or as the
login item.

## Free space is shared between downloads under way (2026-10-17)

The free-space check is made against a ledger shared through
`Options::room`, not by each download alone. Each download that passes
claims what it still needs. A part's claim falls as its bytes arrive,
because the file system's free space then counts those bytes. A finished
file on another disk claims its whole length until the download ends. The
check and the claim happen under one lock, so two downloads starting
together cannot both pass against the same space.


## The shown byte count follows the speed limit (2026-10-17)

Under a limit, a chunk is counted as the limiter lets it through, 16 KiB
at a time (`Limiter::take_counting`), not as it arrives. A connection
reads what the socket holds, up to ~256 KiB at once, and eight connections
counting on arrival put two megabytes in the count before the ceiling had
passed any of it: the speed and ETA showed twice the limit for seconds.
The count is still a ledger of what is on disk: a wait cut short by a
pause or cancel counts the rest of its chunk. Smoothing the meter over a
longer window was not chosen: it would hide the error, not remove it, and
slow the number's response to a real change.

## Linux openers are waited for (2026-10-17)

`xdg-open` exits 3 or 4 at once when it has nothing to hand a file to; a
spawn that succeeded was taken as opened. Each opener is now waited for up
to 1.5 s, in place, so `open_file` keeps its `Result`. A non-zero exit
tries the next opener; one still running at the deadline is taken as
opening the file and reaped on a thread. Its stderr goes to a temporary
file rather than a pipe, so a program it runs in the foreground is not
killed writing to a closed pipe.

## A browser is registered when its program is installed (2026-10-17)

Besides a browser whose profile folder exists, `register` and `ensure`
take one whose program is installed (Linux: on `PATH`; macOS: a bundle in
`/Applications` or `~/Applications`) and make its `NativeMessagingHosts`
folder. A browser with neither gets no folder. Windows writes every
browser's key regardless, as before.

## The toolbar keeps every name (2026-10-18)

The modern toolbar used to keep one row by dropping names from the right,
leaving pictures to guess at a narrow window or in German. Now each button
keeps its name: beside its picture where one row has room, under it in
smaller type where it has not, and on a second row only when even that does
not fit. The classic looks wrap the same way.

## Progress windows for downloads the window did not start (2026-10-18)

A row first seen running or queued that the window did not add itself (a
browser hand-off through the inbox, a second launch's link) opens its
progress window when "Show a progress window" is on. Rows the window adds
(Add URL, series, lists, imports) are marked as its own and follow the old
rule: one window for a single Start now, none for a batch.

## Exit asks; scripts do not (2026-10-18)

Exit, Cmd+Q and the tray's Quit ask while anything is Running or Queued;
the downloads resume at the next launch either way. `program.exit` over the
control protocol quits without asking, since nothing there can answer.

## A download the browser has already finished is still caught (2026-10-18)

`first_look` used to leave a download whose state was `complete` with the
browser ("already finished, usually from the cache"), and one that could
not be paused and was no longer running ("it finished first"). Chromium
finishes a small file served fast before the extension can hold it — a PDF
almost always (it is not held for a danger check the way a `.bin` is), and
a pause does not stop what it has already received — so those PDFs never
reached dm. A cache hit and a fast fetch look the same from the extension,
so the rule could not be kept for one and not the other, and it guarded
nothing a person asked for: the catch rules said dm takes the type. Now a
finished download is handed over by its address like any other, still
`held` (protocol 2), so dm probes it, compares the type the browser got,
and refuses what it cannot fetch itself (a one-time link, a POST result, a
sign-in page). Only once dm has taken it is the browser's copy removed:
`downloads.removeFile` for a finished one (cancelling it would leave the
file), `cancel` for one still running, then `erase`. Refused, the finished
copy stays, and `give_back` does not start it again. The size floor in the
catch rules is the setting for leaving small files to the browser.
Considered: leaving the browser's copy and handing over too (two copies of
every fast PDF, which is what the hold-then-complete race already did);
keeping the rule and lowering latency (the race is lost before
`onCreated` runs).

## Chrome's file name answer is held while dm decides; `scripting` added (2026-10-18)

Chrome asks where to save a download (its Save dialog) when the name is too
long for the disk, or when "Ask where to save each file" is on, and it asks
only after every `downloads.onDeterminingFilename` listener has answered. A
dialog opened while dm was being asked stayed open after the extension
cancelled the download it had taken, and caught the next clicks. So the
extension now listens there (Chrome only; Firefox has no such event) and
holds its answer until `onCreated` has decided: a download dm takes is
cancelled before Chrome asks anything, one it leaves is answered with no
suggestion, as if dm were not installed. The hold lasts at most
`DETERMINE_WITHIN_MS` (handshake, settings and hand-off times plus 5 s) and
is then answered whatever happened. Considered: suggesting a shorter name
(fixes only the long name, not the "ask" setting); cancelling sooner (the
dialog can open before the app has answered).

"Download all/selected links" asks the page script for the page's links;
Chromium puts declared content scripts only into pages loaded after the
extension, so a tab open before dm was installed, updated or reloaded had
nobody to answer. The extension now puts `content.js` into that frame with
`scripting.executeScript` and asks again. That needs the `scripting`
permission (both manifests); with `<all_urls>` already granted it adds no
install warning in Chrome, but the Firefox listing needs a new version.
`content.js` replaces a predecessor cut off by a reload rather than
deferring to it. Pages no extension may enter (the browser's own, stores)
get a message saying so.

## Dialogs in front are modal; Remove always asks (2026-10-18)

The dialogs that stand in front of the list (every one but Find, the
progress windows and Download complete) are egui's modal layer: Tab goes
round their own controls and the menu bar and list behind take no clicks
until they close, which is what "focus stays in the dialog" takes in egui.
Find stays beside the list, since finding selects rows a person may then
click. Enter with none of the dialog's controls focused presses its default
button; a focused button of its own takes Enter itself. Remove now asks for
finished rows too, whichever way it was asked for — the Delete key took
them off at once while the toolbar asked — except headless, where nobody
can answer. Considered: asking only from the key (two behaviours for one
command).

## Non-Latin names use the computer's fonts, read on demand (2026-10-18)

Inter and egui's fonts have no Chinese, Japanese, Korean, Arabic, Hebrew,
Thai or Devanagari. Bundling Noto CJK would add tens of megabytes to a
7.9 MB installer, so the computer's own font for the script is read the
first time a row's name, path or note needs it (fontconfig's answer on
Linux, the usual files on macOS and Windows) and appended after dm's fonts.
A computer without one draws what it can, as before. The files are read
into memory (no memory-mapping crate in the window), once a process.

## Starting one row outside the schedule (2026-10-18)

`Queue::start_outside_schedule` starts the row at once and marks its slot so
the schedule and the download limit do not put it down; a pause, a cancel
or the row going back in line clears the mark. The mark is not saved: after
a restart the row is the schedule's again. Considered: turning the schedule
off for a while (holds every other row's start too).

## In progress is what is under way (2026-10-18)

"In progress" lists running, queued and schedule-held rows only, matching
the status bar's count. Failed and stopped rows are under All and their
kind, not here.

## Every opening of dm registers with the browsers (2026-10-18)

Registration ran only in the copy that opened the window, so opening dm
again while it ran (what the README says to do after installing a browser)
handed off and exited without telling the new browser. Now a launch that
hands off registers first, and the running dm checks again whenever it is
asked to show itself. `ensure` is idempotent and reads a few small files,
so it costs nothing when nothing changed. A failure is said in the window
once until it changes. The extension with no host cannot know whether dm
runs, so its words cover both: open dm, again if it is open.

## Firefox's downloads panel is left to Firefox (2026-10-18)

Firefox opens its panel for every download it starts. Measured: cancelling
and erasing inside `onCreated`, before asking anything, keeps it shut, but
dm may cancel only after the app has taken the download (never lose one),
and the app's answer comes later than the panel. No extension API closes
the panel or sets `browser.download.alwaysOpenPanel`. Considered and
dropped: cancelling at once and restarting the download if the app refuses
(a restart is a second request — a one-time link or a POST is lost, and the
restart is caught again); erasing at once and cancelling later (an erased
download leaves the list the extension's pause, cancel and resume act on,
so it could not be given back; reasoned from Firefox's code, not run). The README says the panel may show and dm has the download.


## Right-to-left names reordered before egui draws them (2026-10-18)

egui 0.37 has no bidirectional layout: a line is laid out in stored order,
and its shaper (harfrust, guessing each font run's direction) turns each
Arabic or Hebrew word round, but not the order of the words. dm does not
wait for egui: `bidi::visual` runs the Unicode bidirectional algorithm
(`unicode-bidi`, a left-to-right line, as dm's window is) and hands egui the
runs in drawing order, keeping each right-to-left word in stored order so
the shaper's own turn is not undone, and mirroring brackets. It is applied
only where a name or a path is drawn; text a person edits is left alone,
since a reordered string in a text field would be edited backwards. A
screen reader is given the stored name. The one case it gets wrong is an
Arabic-Indic number touching an Arabic word in the same font, which egui
shapes as one run; a name with one is rare.

## A selected row is filled with the accent, its words white (2026-10-18)

The light grey selection (235 on 255) was invisible on many screens. The
list's selection is now a 3:1 fill with 4.5:1 words, the way desktop file
lists draw it, and every coloured word on a selected row (red Error, faded
File missing, the progress bar) takes the selection's text colour. Text
selection in fields keeps the soft tint. The focus ring gained a white inner
line, which is what shows on a primary button or a selected row, both in the
accent colour; the colours are tested as numbers in `theme.rs`.

## Scroll bars that take their room and can be seen (2026-10-18)

egui's default scroll bars float over the content and, in dm's looks, are
invisible until the pointer is over them: their handle is drawn in a
button's fill, which the looks keep clear, and at zero opacity while
dormant. A tester found Options › General cut off with no sign of more, and
End leaving the list's last row under the sideways bar. The list and the
Options tabs use `theme::steady_scroll`: solid bars that take their own
room, the handle in the text colour at a low opacity even when the pointer
is elsewhere. Other scroll areas (small dialogs that rarely scroll) keep
egui's.

## An enabled proxy with no host is a refusal, in the engine too (2026-10-18)

`Proxy::url` is `None` for a proxy turned on with no host or port, and
`client_with` used to build a direct client from that: every download went
out around the proxy someone had asked for. `client_with` now refuses it,
which is what the launch path already turns into "the saved proxy could not
be used, so it is turned off" for every route (dm's own client and the
tools), said in the window in its language. Options refuses it before it
gets that far. The native host's pre-check (`host.rs`) still falls back to a
direct client when its proxy cannot be built; it only asks the server
whether a caught download can be fetched, but it is a direct request, and
is left for a change of its own.

## Link drags from a browser: a hint, not a fix (2026-10-18)

winit delivers dropped files only, and reports nothing while a link is
dragged over the window, so dm cannot tell that a link was dropped on it.
What it can do: a drop holding no link says how to add one (Add URL, the
browser's right-click menu, or a dropped `.url`/`.webloc` file), and the
empty list says the same.

## A hand-off of a listed address is said, not asked (2026-10-18)

Add URL asks before adding an address already in the list. A browser's
hand-off cannot be asked about the same way: the native host files it in the
inbox, the inbox adds it to the queue (`shell::watch_inbox`, before the
window hears of it), and the browser has already cancelled and erased its
own download. A question then could only throw the download away, and a
window in the tray would hold it unanswered. So the window says it in the
status line — "Added file.zip again: that address was already in the list."
— where a person who did not mean it can remove the second row.

## Plurals: forms divided by `||` (2026-10-18)

A sentence about a count holds its forms in one catalog key, divided by
`||` ("Added {} download.||Added {} downloads."); the translation holds its
language's own forms the same way, and `i18n::form` picks one by CLDR's
integer rules: English, German, Spanish, Italian, Dutch two (one, other),
French and Portuguese two with 0 singular, Polish and Russian three, and
Indonesian and Turkish one. Separate keys per form would not have served
Polish and Russian. `tests/i18n.rs` holds every form to the English's
placeholders and every language to its number of forms.

## The list draws its own up-and-down bar (2026-10-18)

egui_extras' table has no sideways scrolling, so the list sits in a sideways
scroll area, and the table's up-and-down bar was at the columns' far edge,
out of sight whenever they ran past the window. The table's bar is hidden
and the list draws one of its own in a strip at the visible right edge: it
follows the table's offset, and a drag or a wheel on it is handed to the
table on the next frame. The sideways offset egui keeps across launches is
reset to the left edge on the first frame.

## Only Exit quits (2026-10-18)

The Exit question used to close dm by itself once nothing was running,
reasoning that there was nothing left to ask about. A person reading the
question, or reaching for "Keep downloading", saw dm vanish. dm now quits
only on the person's Exit (or a script's or a signal's explicit quit):
when everything finishes under the question, it changes to "Nothing is
downloading any more. Exit dm?" with Exit and Stay. The question keeps one
width and the room of three lines, because a dialog anchored by its middle
that shrinks moves its buttons, and Exit had slid under the pointer.

## A `.bin` is Other (2026-10-18)

`.bin` was sorted under Apps. It is as often firmware, a disc image, a
model's weights or a raw dump as a program, and the extension cannot say
which; the server's type (`application/octet-stream`) cannot either. Other
is the honest place. A name with "setup" or "install" in it says it is an
installer, and goes to Apps. The browser's catch list still catches `.bin`.

## Key names are catalog words with a `key:` prefix (2026-10-18)

German keyboards say Strg, Umschalt and Entf. The shortcut text is built by
dm rather than egui on every system without the Mac's symbols, from
catalog keys such as `key:Delete`: the prefix keeps "Delete" the key
("Entf") apart from "Delete" the command ("Löschen"), and is taken off
where no catalog has the word (English).

## The pointer over controls is the desktop's, not a web page's (2026-10-18)

The arrow over buttons, checkboxes, menu items, the toolbar and rows; the
text cursor over a field; resize arrows over a column's edge and the rail's
splitter. A hand over every clickable thing is a web page's convention, and
egui can only give it to plain buttons, so turning it on would have left
checkboxes, menus and rows with the arrow beside buttons with the hand —
the inconsistency a person notices. `the_pointer_is_the_same_kind_over_the_same_kind_of_thing`
holds it. Addresses are drawn in the window's one font, not a second
monospace one.

## Inter is the window's typeface (2026-10-18)

It replaces the earlier embedded face, which had no Cyrillic or Greek: a
Russian window drew those letters in egui's fallback, a second face with no
semibold, so headings lost their weight and one label could mix two faces.
Inter (SIL OFL 1.1, `fonts/LICENSE-Inter.txt`) covers Latin, Cyrillic and
Greek in regular and semibold, and is an independent design rather than a
stand-in for any vendor's system font. Scripts it lacks still fall back to
the computer's own fonts.

## Numbers are drawn with tabular digits (2026-10-18)

Inter's digits are proportional, so a column of sizes was ragged and a
speed jittered as it changed. Inter has tabular figures, but behind its
`tnum` feature, and egui applies no optional font features. So
`scripts/tabular-digits.py` (fonttools) makes those figures the drawn
digits in a copy cut down to digits and the punctuation numbers use,
named "Inter Tabular Digits" (9.5 KB; the OFL allows modified versions and
Inter reserves no name). It leads the `numbers` family, Inter behind it for
letters and units, used for the list's Size, Time left and Speed, the
status words and the progress window's figures. Making every digit
tabular was not chosen: in names and sentences tabular "1" reads gappy.

## Links that work once are named by the person, not guessed (2026-10-18)

The native host asks a caught download's server once before taking it, so a
link only the signed-in browser can fetch stays with the browser. For a site
whose links work a single time, that question spends the link. Detecting
such links from their address (tokens, signatures) guesses wrong both ways,
and reusing the check's connection for the download would mean the host
fetching files. So the person lists those sites in Options › Browser, and a
hand-off from one is taken without the question, knowing a sign-in-only link
from it then fails in dm instead of finishing in the browser.

## Only questions dim the window (2026-10-18)

Remove, Exit, Cancel this download and the other questions dim the window
behind them and take every click until answered: an answer meant for one
must not land on something else. Dialogs that set things (Options,
Scheduler, Speed limiter, Properties) do not: the list stays readable
beside them, and nothing is lost by clicking past one. A tester asked for
the Scheduler and Speed limiter to dim like the questions (UI round 8);
they are settings, so they stay as Options is.


## The Queue column starts at one width (2026-10-19)

The list's columns fit themselves to the window at each launch until a
person drags one. Queue started at 96 points in a list where no row had a
queue and at 140 in one where some did, so the columns after it moved
between launches and "Time left" was cut to "T" (UI round 11). It now
starts at 116 either way. An empty Queue column no longer gives its room to
the name: a list that stays where it was is worth more than 24
points of name. Widths a person dragged are kept until the text size
changes, when the list fits itself again: kept at 140% text, Status and
every column after it were out of view.

## Behind a proxy, a torrent finds peers through http(s) trackers only (2026-10-19)

A SOCKS5 proxy carries TCP connections; the DHT, local service discovery
and `udp://` trackers are UDP, and the torrent engine sends them directly.
Left on, they told the swarm the info-hash from the real address on a
machine set up to make no direct connection. Behind a proxy they are off,
and a torrent's `udp://` trackers are dropped from it (its info dictionary
kept byte for byte, so it stays the same torrent). A magnet or a torrent
that lists only `udp://` trackers is refused with that reason rather than
sent around the proxy. An HTTP proxy is still refused for torrents
altogether, as before.

A torrent shares the whole speed ceiling while none of dm's own downloads
is running; with one running, the ceiling is split as before, one part for
dm's own downloads and one for each torrent or the external video tool run.

## The modern looks follow WinUI 3's Fluent 2 (2026-10-19)

Asked for twice: the window follows Fluent 2 as WinUI 3 draws it, with the
values read from WinUI's own open-source control templates
(microsoft-ui-xaml's `*_themeresources.xaml`), the ones the WinUI 3 Gallery
renders with. dm is not presented as a Microsoft product and does not ship
Segoe UI (not redistributable); Inter stands in.

What was taken, and where it was bent:

- Brushes: the base, layer and dialog surfaces, text primary/secondary/
  disabled, the strong and soft strokes, the accent fills from the default
  accent (#0078d4), status colours, and the subtle fills for hover and
  selection, kept see-through as WinUI's are. Two were darkened a shade to
  hold dm's contrast floors on the list: the strong stroke (#8d8d8d to
  #8a8a8a, 3:1) and disabled text (#9f9f9f to #9a9a9a, 2.5:1). The hover test
  now asks only that hover changes something, since WinUI's subtle fill is
  about 1.1:1 by design; the selection is told apart by the accent pill.
- A selected row is the subtle fill with the accent pill, not the solid
  accent of the selected row: the pill is 3:1 on the list and the words keep their
  colours. Selected words in a field stay on the accent (#0078d4), white.
- Density: WinUI's compact sizes (24 px fields and drop-downs, 32 px list
  rows and places), which WinUI offers for information-dense apps; the
  standard 32/40 px pushed Options' and Properties' content out of reach at
  large text.
- Dialogs: no egui title bar; the title painted at the top left in the
  Subtitle style, a subtle close button, 24 px padding, and the buttons on a
  footer band at the right. The title is painted rather than a label so a
  screen reader is not given the dialog's name twice; a scrolled body
  scrolls under the title, which keeps the close button in reach.
- Shadow: Fluent's 28 for dialogs rather than 64, which is large for a
  window this small.
- Radio buttons stand in a column under their group's name, as WinUI's
  RadioButtons do; side by side, egui shrank a choice's words to a letter a
  line at 200%.

Tried and dropped: egui's experimental style providers (they cannot tell a
ticked box from an empty one, and they sit behind a feature flag), and
right-aligning the footer from last frame's window size (it fed back on
itself and grew Options past the window at 200%).

## The page script loads its WebAssembly only where there is media (2026-10-20)

The "Download video" panel is the only thing the page's WebAssembly does,
and it was loaded into every page and every frame: on a page of advert
frames, one compile of half a megabyte each. `content.js` now loads it once
the page shows media: a `<video>` or `<audio>` in the document (checked as
the page changes, at most once a second), one in a component's shadow root
under the pointer (checked at most twice a second), or a play. The
key-held clicks, magnet links and "Download all/selected links" stay plain
JavaScript and work from the start, as before.

## Segoe UI from Windows, Selawik elsewhere (2026-10-20)

Fluent's face is Segoe UI (Segoe UI Variable on Windows 11), which
Microsoft licenses with Windows: dm may not ship it. Chosen by the person:
on Windows dm draws with the computer's own Segoe UI and its semibold
(`%WINDIR%\Fonts\segoeui.ttf`, `seguisb.ttf`, mapped, as WinUI uses the
system's); on macOS and Linux, and on a Windows without them, with Selawik,
the open stand-in Microsoft made to Segoe UI's measure for other systems
(SIL OFL 1.1, Reserved Font Name, shipped unmodified, 44 KB a face). Selawik
has Latin only, so Inter stays behind it for Greek and Cyrillic (Russian,
in the translations); its digits are one width, like Segoe UI's. The
static Segoe UI is used rather than the variable one, whose weights egui
cannot pick. Not yet seen on Windows.

## Fluent UI System Icons for the window's pictures (2026-10-20)

Segoe Fluent Icons, WinUI's icon font, comes with Windows and may not be
shipped. Chosen by the person: Microsoft's Fluent UI System Icons (MIT), the
same icons published for other platforms, as a font cut down to the 26 the
window draws (`fonts/FluentSystemIcons-dm.ttf`, 5 KB, made with fontTools'
`pyftsubset`; `icons::fluent` names each by its `ic_fluent_<name>_20_regular`).
The modern look draws the toolbar, every place (each its own, as the WinUI 3
Gallery's pane has them), the pane's group chevrons, drop-downs' and
submenus' chevrons from it; the classic look keeps its drawn pictures. The
progress window's Connections opener keeps its drawn chevron, which turns
with egui's opening animation where a glyph would jump.

## Inter's tabular digits removed (2026-10-20)

`Inter-TabularDigits.ttf` led the numbers family while Inter, whose digits
are proportional, was the window's face. Segoe UI and Selawik, which lead
it now, have digits of one width themselves, so it was never reached and
is gone; the numbers family is the window's face and what follows it.


## A login launch makes its window only when asked (2026-10-20)

eframe makes the window and its GL context as it starts, about 40 MB of
the 43 a dm waiting in the tray held. A `--hidden` launch now runs winit's
event loop itself (`run_hidden`, `eframe::create_native`), with dm's logic
on a stand-in egui context (`run_logic`: no fonts, no drawing) whose
repaint observer wakes the loop; the first ask for the window passes the
loop to eframe, the window's state to its creator, and the stand-in's
pending viewport commands (a control request's screenshot, the focus) to
eframe's context, which every later wake of the stand-in is forwarded to.
A normal launch keeps `run_native`. Tried and dropped: the wgpu renderer,
which reached the first frame no sooner and held as much.

## 16 KiB write buffers (2026-10-20)

tokio's file keeps its own copy of each write, so a connection held its
`BufWriter` twice: 256 KiB cost 20–30 MB over ten downloads of eight
connections against 16 KiB, which fetched 2 GB over loopback no slower
and with no more CPU (measured on 2026-10-20).

## NTLM sign-ins (2026-10-20)

NTLMv2 is written in dm (`ntlm.rs`, MD4 from RustCrypto, HMAC-MD5 over the
`md5` already used for Digest) rather than taken from a crate: the
algorithm is short, its worked example in MS-NLMP pins it, and the crates
on offer bring signing and sealing dm has no use for. It is answered under
NTLM and under Negotiate, which Windows servers accept. Only the NTLMv2
answer: LM and NTLMv1 are broken. NTLM signs in a connection, which
reqwest does not expose; each reply is read to its end so the connection
returns to the pool, whose next request takes it again, and an answer
that still lands elsewhere (ranges signing in at once) is tried once more.
Kerberos is not offered: it needs the system's GSSAPI or SSPI and a domain
to be tested against.

## RTMP, RTSP and MMS through an external converter (2026-10-20)

These are media servers' protocols, not files: dm has no client for them
and does not write one. The converter dm already finds for joining
streams reads all three, so an address of these schemes is handed to it
with `-c copy`: nothing is re-encoded. MP4 is written fragmented
(`frag_keyframe+empty_moov`), so a live stream stopped by hand still
plays; MMS, which carries Windows Media, goes into its own container. A
stream has no length and no position to resume from: progress is the
bytes written, and a resume starts it again. That converter cannot go through
dm's proxy for these, so with one in force the download is refused.

## The external video tool and a video tool fetched by dm itself (2026-10-21)

YouTube is to work out of the box, with nothing installed by hand. dm still
writes no site logic of its own: it runs the external video tool. Where the computer has no
the external video tool (or converter), the first video page that needs one has dm fetch it
into `<dm home>/tools`, through dm's proxy like any download: The external video tool's own
standalone build from its GitHub releases, checked against the
`SHA2-256SUMS` published with it (on macOS the unpacked build, which starts
in 0.3 s where the single-file one took 8–14); the converter as one file from
a static-build project's b6.1.1 release on GitHub, checked against digests written
into dm, since that project publishes none. Fetched the external video tool is fetched
afresh once a week. One the person installed is preferred, wherever it is.
Not bundled in the installers: a bundled the external video tool goes stale between dm
releases, and both would add 100–150 MB to every installer. Not written as
dm's own extractor: that is the site logic this project does not write.

## A video page's session goes with its video (2026-10-21)

Measured on 2026-10-21: YouTube's streams stop with a 403 after about 1 MB
unless the client holds a proof-of-origin token, and an address that has
asked too often is told "Sign in to confirm you're not a bot". The external video tool gets
past both with a JavaScript runtime (Deno, now fetched with it) and the
browser's session. So the extension sends the page's cookies with every
video handed over (the panel, "Download media with dm") and with the
panel's quality list, whether or not the site is in the popup's sign-in
list, and the `cookies` permission moved from optional to required (it
carries no install warning in Chrome or Firefox). Bounded the way a kept
sign-in is: to the page's own server, in a private file of the external video tool's run
removed when it ends, and — unlike a kept one — dropped from the row once
the video is downloaded (`Start.cookies_once`, `Entry.cookies_once`).
Nothing from a private window. Dropped: writing a token generator of dm's
own, which is the site logic this project does not write.

## dm joins a video's picture and sound itself (2026-10-21)

A separate video tool was fetched (40–80 MB) for every video page only to join a picture
and a sound that the external video tool had already fetched. `mp4mux.rs` does that join in
Rust: each input's samples are read from its index — a plain `moov`, or the
`moof` fragments a DASH server's files carry — and written as one plain MP4
with its index first and picture and sound interleaved a second at a time,
the samples copied byte for byte. A DASH picture with B-frames and no edit
list is given one that starts it at its first shown frame, as the converter's own
join does (it played 67 ms behind its sound without). With no converter, the external video tool
is asked for an MP4 picture and an M4A sound (`bv[ext=mp4]+ba[ext=m4a]`),
leaves them side by side (its default `ignoreerrors='only_download'` turns
the missing merger into a warning), and dm joins them under the name the external video tool
reported. Measured: 120 MB joined in 96 ms where the converter's stream copy took
174 ms; the decoded frames and sound match the inputs' (`tests/mp4mux.rs`),
and AVFoundation reads the result. The converter is still fetched where dm does not
do the work: a WebM picked in the panel, embedded subtitles, RTMP/RTSP/MMS.
Not written: a WebM (Matroska) muxer.

Later the same day, the HLS side: `tsmux.rs` reads a transport stream's
tables, its H.264 pictures (start codes turned into lengths, the picture
size read from the SPS) and its ADTS AAC frames, and hands them to the same
writer, picture and sound kept the distance apart they had in the stream.
It runs before the converter for dm's own HLS downloads, and on a file the external video tool saved
as `.mp4` that is a transport stream (the external video tool's HLS downloader leaves that to
its converter's fix-up). A stream with any other picture or sound kind (HEVC, MP3,
AC-3…) is refused whole, so nothing is dropped from it: a converter does it
where there is one, else the `.ts` is kept. When dm joins, the external video tool's joiner-location option is
pointed at nothing, so it never uses a converter dm
did not choose. Tests that would pass with a converter the external video tool found itself
were checked to fail with dm's join turned off.

## WebM joined by dm too (2026-10-21)

`mkvmux.rs` joins a WebM picture (VP9, AV1) and sound (Opus) the way
`mp4mux.rs` joins MP4: tracks and blocks read from each input's EBML, written
as one WebM with clusters starting at the picture's key frames, `Cues` and a
`SeekHead` so it seeks, the frames copied as they are (laced or not, block
groups kept). A WebM line picked in the panel no longer fetches the converter. A
real YouTube 720p VP9/Opus pair (21 MB) joins in 44 ms and decodes identical
to its halves. Reads move within one megabyte buffer rather than seeking
(seeking threw the buffer away per block: 1.47 s for the same file).

## Subtitles put in by dm (2026-10-21)

With no converter, the external video tool is asked for subtitles as WebVTT beside the video
(`--sub-format vtt`, no `--embed-subs`), and dm carries them inside what it
writes: in an MP4 as `tx3g` text tracks, one per language in one alternate
group, each cue a sample and each gap an empty one, with a closing empty
sample (readers time a subtitle by the sample after it; without one the
last cue ended with the picture); in a WebM as `D_WEBVTT/SUBTITLES` tracks,
each block "id line, settings line, text" as the WebM spec has it (a standard
decoder drops a block without the two lines; Matroska's newer `S_TEXT/WEBVTT`
is not read by the version 7 of it tried). A file that was already one stream is written
again with them; one that will not take them keeps its picture and sound.
A real YouTube video's English and German subtitles read back exact. A
block before zero (Opus's delay) now keeps its time, the first cluster at
zero and its offset negative, rather than moving everything 7 ms on.

## More of a transport stream put into MP4 by dm (2026-10-21)

`tsmux.rs` now also carries HEVC pictures (an `hvc1` with its `hvcC` built
from the VPS, SPS and PPS: profile bytes as they are, sub-layers, chroma
format, bit depths and size read from the SPS), MPEG-1/2 audio (MP3, MP2:
frames whole, object type 0x6B or 0x69) and AC-3 (`ac-3` with its `dac3`,
frame sizes from the standard's table). Each was made by a standard encoder, put into an
MP4 by dm and decoded identical to the stream; AVFoundation reads the HEVC
and AC-3 one. E-AC-3 too (`ec-3` with its `dec3`; one independent
substream). Still refused, and left to an installed converter or kept as `.ts`: MPEG-1/2
and MPEG-4 Part 2 pictures and LATM AAC, which HLS does not use.

## RTMP spoken by dm (2026-10-21)

`rtmp.rs` is a client of dm's own for `rtmp://` and `rtmps://` (TLS through
tokio-rustls with the webpki roots reqwest already uses, the ring provider
named since two are compiled in): the simple handshake, the chunk stream
(all four header forms, extended timestamps, the server's chunk size,
acknowledgements at its window, pings answered), and AMF0 `connect`,
`createStream`, `play`. H.264, HEVC (both RTMP extensions) and AAC or MP3
arrive as FLV tags, aggregates included, and go into `fmp4.rs`: a
fragmented MP4 written a fragment a second, so a stream cut short plays.
When the stream ends it is rewritten plain by `mp4mux`, which now keeps a
fragmented track's start (`tfdt`) as an edit, so sound that began after the
picture still lines up. Checked against a real streaming server sending a clip over RTMP: the
saved file decodes identical to the clip, picture and sound, with dm told
there is no converter. RTMPE (Adobe's encryption) and RTMPT (HTTP tunnelling)
stay with the converter; neither is in use any more.

## RTSP recorded by dm (2026-10-21)

`rtsp.rs` records `rtsp://` with the `retina` crate (MIT/Apache, a
maintained RTSP client used for IP cameras): DESCRIBE, SETUP with RTP
interleaved on the RTSP connection (passes NAT and firewalls), PLAY, and
retina's H.264, H.265 and AAC depacketizers giving frames already in MP4
form, which go into the same fragmented MP4 as RTMP. A sign-in in the
address goes to retina separately (it refuses an address carrying one).
The initial-timestamp policy is permissive: mediamtx, and servers like it,
leave `rtptime` out of the PLAY reply, which retina's default rejects.
Checked against mediamtx 1.12 fed by a standard encoder (H.264 and AAC; the recorded
pictures are the clip's frames 75–198 in order) and, in the suite, against
a one-client RTSP server in `tests/live.rs` sending real H.264 as single NAL
and FU-A packets (all 75 pictures identical). RTSPS and MMS remain the converter's.

## MMS over HTTP by dm (2026-10-21)

`mms.rs` asks an `mms://` (or `mmsh://`, `mmst://`) address over HTTP as a
Windows Media player does: a first request for the ASF header, from which
the packet size and every stream's number are read, then `xPlayStrm=1`
with every stream switched on; the `$H`, `$D` and `$E` chunks are read
across reads and each data packet padded back to the header's packet size.
What is saved is the ASF the server streamed: in the suite an encoder-made
Windows Media file served this way comes back byte for byte. MMS's own TCP
protocol (MMST) is not spoken: servers that had it also answered HTTP, and
none are known to be left. With this, the converter is fetched only for RTMPE,
RTMPT, RTSPS and a transport stream whose picture is MPEG-1/2 or MPEG-4
Part 2.

## The same formats as the converter route (2026-10-21)

Without the converter dm had asked the external video tool for an MP4 picture and M4A sound, which
would have given a lower picture for a video whose tallest came only as
WebM. `mkvmux::mp4_tracks` now reads a WebM's VP9 (`vp09`, its `vpcC` from
the first key frame's header), AV1 (`av01`, WebM's `av1C`) and Opus (`Opus`,
`dOps` from OpusHead) as MP4 tracks, and `mp4mux` takes a WebM input beside
MP4 ones, so dm asks for what the converter route asked for (`bv+ba/b` and its height
forms, with the same sort) and puts it into an MP4 as the converter did; a WebM is
made only when one is asked for. Checked: on six YouTube videos at best and
at 720p, the external video tool picks the same format IDs for the old and the new selector,
and `scripts/out-of-box-check.py` gets a 4K video's best (format 625, 2160p
VP9 over HLS) as one MP4 with its AAC sound that plays.

## Links dragged in and files dragged out, around winit (2026-10-21)

winit 0.30 takes only files in a drop, and on Linux the `drag` crate needs a
GTK window. Each system is met where winit stops, rather than moving off
winit:

- macOS: `draggingEntered:` and `performDragOperation:` on winit's window
  delegate class are replaced at run time; a pasteboard with files goes on to
  winit's own, one with a URL or text holding a link is taken by dm.
- Windows: winit's drop target is revoked and dm registers its own OLE
  `IDropTarget`, which reads files, `UniformResourceLocatorW` and text.
- X11: winit is vendored (`desktop/vendor/winit`, `[patch.crates-io]`) with a
  small patch, marked `dm:`, choosing `_NETSCAPE_URL` or UTF-8 text when no
  `text/uri-list` is offered — Firefox offers none for a link. Dragging out
  is dm's own XDND source on a connection of its own (`xdnd.rs`): winit's
  pointer grab (core and XInput 2) is let go so the drag can take it, and
  dm's window is told the button is up afterwards, since winit never sees
  that release.
- Wayland: neither. winit has no data-device support there; a hint says so.

Tried and dropped: asking for the selection with type `ANY` (X servers
answered TypeMismatch; the chosen target's own atom is asked for).
Replacing the vendored copy goes away when a winit release reads links.

## Out of the box for someone new: nothing to install but the extension (2026-10-21)

dm no longer fetches a separate video tool for anything. What it was still fetched for is
dm's own now, or refused: RTMPT through an HTTP tunnel in `rtmp.rs`, RTSPS
through a loopback TLS bridge in front of retina (which speaks plain RTSP
only: patching it was the alternative, and the bridge needs no vendored
copy), and a video in a container dm does not join asked of the external video tool as one
file that needs no joining. RTMPE is refused: it is Flash's encrypted RTMP,
a protection dm does not undo. A converter the person installed is still
used where it helps.

Bounded, as everything read from a server is: an RTMPT answer is read in
pieces and refused past 16 MiB. The RTSPS bridge takes the first
connection to a random loopback port: another program on the same
computer that got there first would reach the camera without dm's login,
which retina sends only on its own connection, and dm's recording would
then fail rather than go anywhere else. Accepted, as a program already
running as the person can read dm's files anyway.

The external video tool stays the one thing fetched, with Deno. Written into dm instead it
would be the site logic this project does not write (see "Player pages go
to the user's the external video tool"): YouTube's player code changes weekly and is answered
by running it, which is why the external video tool needs a JavaScript runtime, and the external video tool's
thousands of sites are kept working by hundreds of people. Fetched and kept
current is what makes video work for someone who installs nothing; the row
says "Getting ready for videos (first time only)" while it happens.

Someone who installed only the app is shown how to add the extension: a bar
over the list, until a browser connects (the native host leaves
`browser.seen` in dm's home on the extension's hello) or they say not now,
with the same buttons in Options › Browser. Messages never say "install
the external video tool" or tell the person to install a converter: a failed video says dm could not get ready for
videos and to check the connection, and an extension talking to an older
app says to update the app. The two store addresses are allowed in shipped
code by `cargo xtask privacy`, since they open in the person's browser only
when clicked.

## The external video tool may use a joiner the person installed (2026-10-22, later)

The option that pointed the external video tool at a joiner that is not there was removed:
it was the last place dm named a tool it does not use, and the owner chose
to drop it. On a computer with no joiner of its own nothing changes: The external video tool
leaves picture and sound apart and dm joins them. Where one is installed,
the external video tool may join them itself and dm takes the one file as it is (putting
subtitles inside where it can, as it does for any single file). The cost:
the file can differ from computer to computer, and that route is tested
only where the test machine has such a joiner. dm still fetches nothing
of the kind and runs nothing of the kind itself.

## No separate video tool, not even an installed one (2026-10-22)

dm joins picture and sound (`mp4mux`, `mkvmux`), carries subtitles, puts
transport streams into MP4 (`tsmux`) and records live streams itself, so a
converter on the computer is no longer used for any of it either: The external video tool is
pointed at a joiner that is not there and always leaves picture and sound
for dm, an HLS stream dm cannot convert (MPEG-2 pictures) is kept as the
`.ts` it came as, and RTMPE is refused. The same file on every computer,
whatever is installed, and one route to test instead of two.

The external video tool stays, and is not rewritten inside dm. It is what reads a video
site's page: YouTube's player answers with signatures and throttling codes
that change week to week and are worked out by running the player's own
JavaScript (which is why the external video tool needs Deno), and the external video tool covers well over a
thousand sites, kept working by the many people who maintain it. dm's own
version would be site logic this project does not write (see "Player
pages go to the user's the external video tool"), would break within weeks of being
written, and could not behave as the external video tool does across its sites. The
person never installs it: dm fetches it, checked, and keeps it current.
User-facing words call it dm's video helper; PRIVACY.md names it and Deno,
since people are owed knowing what program dm downloads and from where.

## QuickJS rather than Deno for the external video tool's JavaScript (2026-10-22)

the external video tool answers YouTube's player challenges by running JavaScript, with
Deno, Node, QuickJS or Bun. dm fetched Deno (85 MB) for it; it now fetches
QuickJS-ng 0.17.0 (1–3 MB), pinned, each build checked against the digest
written into dm since the release publishes no checksums file. Measured
first on 2026-10-22: the same formats chosen at every quality, a 108 MB
1080p stream in 9 s either way, a page read in about 6 s against Deno's 3.
`scripts/out-of-box-check.py` passes every case on it. The first video
now waits on about 36 MB instead of 120. A Deno the person installed is
still used, as it reads a page a little faster.

Not shipped in the installers: with 36 MB fetched once, bundling the external video tool and
QuickJS would add OS-specific programs to six installer formats (the Arch
package builds from source), invite antivirus false alarms on Windows, and
go stale between releases, for a few seconds saved once.

## Drag and drop on Wayland, in the vendored winit (2026-10-22)

winit 0.30 has no Wayland drag and drop, and a Wayland drag can only be
started on the connection that owns the window, with the serial of the
press that began it, so dm cannot do it from a connection of its own as it
does on X11 (`xdnd.rs`). Both directions are added to the vendored winit
with sctk's data-device support, which winit already depends on: a drop is
read through a pipe on winit's event loop; a drag out is asked for by dm,
and winit starts it on its next turn, woken at once so the press is still
held. Tested under sway (headless) with a virtual pointer written for the
test (`scripts/linux/vpointer.c`, wlroots' virtual-pointer protocol), since
a headless compositor has no pointer of its own and `swaymsg seat … cursor`
reaches no client without one.

## 2026-10-24 Find keeps 24 points from the row it found (was 4)

The top-row search box moved the list up 30 points, and Find's placement,
which reads the found row's rectangle from the frame before, then left the
dialog 6 points over the row (`find_stays_clear_of_the_row_it_found`). A wider
margin absorbs that one frame of lag rather than chasing the layout; the test
that caught it covers the margin. Tried and dropped: reading the row's
rectangle again after the list's own layout, which needs a second pass over
the frame for one dialog.

## 2026-10-25 Gaps found by a behaviour walk-through, and what was left

Done: tracking parameters are ignored when comparing addresses (`lists::bare`,
used for every "already in the list" question; the address that is fetched is
untouched), and the tray gained Start queue and Stop all.

Left as it is, on purpose:
- A certificate that cannot be checked stops the download with the reason
  said. A "continue anyway" would be the one place dm fetched over a link
  anyone on the path can read and swap; nothing a person is waiting for
  justifies it. Considered: a per-row override, dropped.
- Taking a download the browser would keep is Shift-click, and the keys are
  options; a second default combination adds nothing.
- A floating button over selected links, with a checklist of them: the
  right-click "Download selected links" hands over the same set, and dm's
  own Add links window has the checkboxes. A page overlay needs a real
  browser to check, which this project's shell does not have, so it is not
  built unseen.

## 2026-10-25 Closing the window quits on a tiling Wayland desktop

Close-to-tray on Wayland minimizes (2026-10-23), and sway, Hyprland, river,
niri and similar have no minimize: Super+Q (their kill-window key) changed
nothing on screen. Where `XDG_CURRENT_DESKTOP` names one of them
(`minimize_is_ignored`, unit-tested) a close takes the no-tray path: dm exits,
asked first while downloads run. Not seen on a real compositor from this
shell; `swaymsg kill` in `linux-wayland-close-check.sh` sets no desktop name,
so it still checks the minimize path. Considered: a setting for what closing
does, dropped until someone needs the other choice.

## 2026-10-25 DASH is dm's own; the external video tool is left the pages

A DASH manifest went to the external video tool, so a stream whose address was a plain `.mpd`
needed a 40–80 MB helper fetched first. A manifest is a published format with
no site logic in it: it says where the pieces are. So `dash.rs` reads it
(templates with `$Number$`/`$Time$`/`$RepresentationID$`/`$Bandwidth$` and
widths, `SegmentTimeline` with repeats, `SegmentList` and `mediaRange`,
single files), `download/dash.rs` fetches the chosen picture and sound, and
`mp4mux`/`mkvmux` (which already read fragmented MP4 and WebM) join them.
Decided: one period only (an advertisement break would join two initialisation
segments, the case `beyond_dm` refuses for HLS); live (`dynamic`) refused
rather than half-recorded; anything under `ContentProtection` skipped, and
named if nothing else remains; a byte range answered with anything but
`206` and the right length is an error, since a server that ignores the range
and sends the file would be written in as a piece. Sound is a main-role track
over a louder commentary, in the picture's container where there is one.

Not changed: a *page* (a video site's watch address) still goes to the external video tool,
because the media's address is minted by the site's own player and reading
it is site logic, which this project does not write (2026-10-11, 2026-10-21).
The way past that without site logic is to take the address from the player
in the browser, which already ran the site's code: see docs/gaps.md.

A picture and sound that an HLS master keeps in playlists of their own
(`#EXT-X-MEDIA TYPE=AUDIO` with a `URI`) are dm's too when both are plain
fragmented MP4: fetched as the two tracks of a DASH stream and joined by
`mp4mux`. Encrypted, byte-ranged or transport-stream pairs are still
`Beyond`, because joining them wrongly saves a silent film as complete.

## A picture and a sound the player fetched apart (2026-10-25)

The extension reads a video site's player requests (`webRequest`, and
`extension/document.js` in the page's own world, which tells the same address
through `content.js`) and, where the request is a `videoplayback` of a
`googlevideo.com` host, keeps the file's own address (the per-piece `range`,
`rn` and `rbuf` taken off) as a picture or a sound by its `mime`/`itag`. A
tab that has loaded both shows one line, "1080p video + sound", whose hand-off
carries a second address (`Start.sound`, `Entry.sound`, `Options.sound`). dm
fetches both at once in 8 MiB byte-range pieces, as many at a time as the
connection setting, and joins them with its own muxer
(`download::dash::download_media_pair`). A server that does not answer a
range is refused, not fetched whole.

Known limit, measured on 2026-10-21 (see "A video page's session goes with
its video"): such addresses stop with a 403 after about 1 MB where the site
wants a proof-of-origin token the page's player holds. This route is tried
where the player's addresses work; where it fails the row fails with the
server's refusal, and the same page remains one click away through the video
helper. Not tested against the live site (no browser network in the build
environment): tested against local servers that answer ranges as a CDN does.
Hostnames stay out of the manifest (the privacy check forbids them), so the
page script is registered for every page and itself ignores any host but
`googlevideo.com`; it only reads an address and leaves the request alone.

## Reading a page's qualities faster (2026-10-25)

The panel's list took 2 to 39 seconds, nearly all of it the external video tool reading the
page. Three things, kept in this order of effect:

1. The extension asks as soon as the panel is raised over a player (on
   play), not at the click, and a click made sooner joins the read under
   way (`content/handoff.rs`, `reading`). Measured in Edge 156 on a
   YouTube watch page: hovering, waiting eight seconds, clicking: the list
   in 0.03 s.
2. `list_choices` passes `--extractor-args youtube:skip=hls,translated_subs`:
   the same heights are in the DASH manifests, and the list needs neither
   the HLS ones nor the translated subtitles. 2.4 s to 1.7 s on the same
   page, the same eight heights and sizes. The client choice was tried and
   dropped: `android_vr` and `mweb` read in about 1.6 s but offered five
   formats, nothing above 338 p; `tv`, `ios` and `web_safari` were refused.
   Downloads are untouched.
3. A reading is kept for ten minutes in `formats.cache.json` in dm's home
   (48 pages), keyed by the page and the *names* of the cookies sent: a
   site restamps some cookie values on every request, and keyed by value a
   page was never found again (measured: the second read took 1.6 s).
   A page opened again, in another tab or by another host process, lists
   in 10 to 20 ms. Only a non-empty reading is kept.

## HLS byte ranges and encrypted pairs (2026-10-25)

`#EXT-X-BYTERANGE` (and a `BYTERANGE` on `#EXT-X-MAP`) are read into
`Part::range` and each piece is asked for by its bytes, exactly or refused
(`fetch_segment_range`). A range dm cannot read is an error, not ignored: an
ignored one saved the whole source repeated and called it done. A pair
(picture and sound in playlists of their own) may now be AES-128 as well:
`dash::Piece` carries a `Lock` (key and the vector its segment number
makes), undone as each piece is fetched. Still `Beyond`: a transport-stream
picture with packed-AAC sound (gaps.md row 7).

## 2026-10-26: A transport-stream picture with packed-AAC sound is joined by dm

An HLS master that keeps its sound in `.aac` segments beside a `.ts` picture
went to the external video tool. dm now reads the ADTS frames as `tsmux` reads a transport
stream's, takes the sound's start from the first segment's ID3
`transportStreamTimestamp` (the picture's 90 kHz clock), and writes both as the
two tracks of one MP4 (`tsmux::join_packed`). The join is chosen by sniffing
the gathered files (0x47 sync byte; ID3 or ADTS), not by a new flag on the
track, so the DASH and fMP4 paths are untouched. A stream `tsmux` refuses
(HEVC with odd sound, MPEG-2 picture) fails the join with the reason rather
than falling back; dropped: asking the external video tool after the fetch, which would fetch
the stream twice.

## 2026-10-26: A browser hand-off starts at once; "Download details" is opt-in

`ask_panel_downloads` and `ask_caught_downloads` now default to off (this
reverses "A video from the panel asks first", 2026-10-12). A click on the
floating "Download video" button, or a download dm catches, was handed over
and then waited in "Download details" for a "Start" press, which read as the
browser having downloaded it and dm opening late. Measured in Edge 156: the
browser's copy is paused 0.02 s after it is created; the 1.5–1.9 s before dm's
first request is the browser starting the native host the first time in a
session (22 ms after), which `on_startup` already warms. The two switches stay
in Options for anyone who wants the dialog. Existing settings files keep what
they hold. Side effect: with both off, "Download all links" queues the links
without the picker (the links dialog follows the same switch).

## 2026-10-26: A file asked of dm by name is never started in the browser instead

The floating "Download video" button and the menus' "Download with dm" handed a
file as `caught`, so the host fetched it first and, if dm was refused (a 403
where the browser has the site's session), the extension gave it back and the
browser downloaded it: the "download started in the browser" the person saw.
They are handed over as asked for now (`caught: false`); dm takes the file and
a refused one shows as a failed row to retry or sign in for. The browser still
takes over only when no dm app can be reached at all. Dropped: probing first,
which is what let the browser start.

## 2026-10-26: A caught download dm is refused is retried with the session, not left in the browser

A link only a signed-in browser could fetch was refused by dm's pre-check and
stayed in the browser. The extension now hands it over a second time with the
page's cookies for that one download (`cookies_once`, to the download's own
server, dropped when it ends), and the browser keeps it only if dm fails again.
A caught download of an address already in dm's list opens "Already in the
list" (Show it / Download again) instead of being fetched a second time
unasked. When the browser does keep a download because dm could not take it or
could not be reached, it says so in a notification. Left with the browser by
design: Alt-click, a type dm does not catch, catching switched off.

## 2026-10-26: Smaller ways a download ended up in the browser

- "No app" is believed for 2 s, not 10: one slow or failed start no longer
  sends the next ten seconds of downloads to the browser.
- The default catch list gained Office documents (doc, docx, xls, xlsx, odt,
  ods, odp), disk images (vmdk, vdi, vhd, vhdx, ova), jar, whl, ipa, msix,
  more video (rmvb, vob, m2ts, mts, asf) and archives (lz, lzma, tbz2, txz).
  A settings file that already lists types keeps its own list.
- A caught link of an address in the list asks (Show it / Download again) only
  while that row is worth keeping: a failed row, or a finished one whose file
  is gone from the disk, is fetched again without asking.

## 2026-10-26: "Don't show this window again" ticks, then saves when the window closes

The box in "Download complete" applied the setting on the frame it was ticked
and shut the window under the pointer, so it never showed as ticked. It now
stays ticked (`Dialogs::complete_never`), the window stays, and the choice is
saved when the window closes by Close, ×, or the last Open; a later download
that finishes opens nothing (tested through the engine's own settings).
Escape closes the window as Close and × do, and the ticked box is kept on that
path too (`Dialogs::close_top` empties the set; the window saves the choice on
its next frame). Rule for dismissal in dm: a dialog's check box is a choice made
in the dialog, kept ticked while it is open and saved when it is dismissed by
any route, never applied (and the dialog shut) under the pointer.

## 2026-10-26: Everyday controls where the eye is

A pass against a list of what a download manager should do without being
asked twice:

- The row under the pointer shows Stop or Resume (Retry when it failed) and
  Remove on itself (`table/hover.rs`), through the toolbar's commands; their
  names carry the file's, so they are not the toolbar's twins to a screen
  reader.
- The status bar's speed-limit text is a button: a click turns the cap off or
  on, and opens the limiter's settings when none was ever set (turning on a
  cap of 1 KB/s was what the toggle did then).
- Downloads that finish in one pass are one toast ("N downloads finished."),
  a single file keeps its toast with Open and, on Linux, an Open folder
  button. Not done: the Windows toast's buttons and a batch spread over many
  seconds (each pass is judged alone).
- The floating drop target (View › Drop target) shows the overall progress of
  what is downloading as a bar along its foot, and the speed on hover. Not
  done: a separate resizable mini window.
Already in place and unchanged: categories, Filter and sort, drag in and out,
the clipboard bar, the context menu and extensions, the tray, the scheduler.

## 2026-10-26: A saved list of caught types is brought up to the defaults once

A person who never opened Options kept the list of file types an older version
had saved, and documents (.doc, .docx, .xls, .xlsx and others), added to the
defaults later, stayed in the browser for good; a saved value always beats a
new default. `catch.defaultsVersion` (0 when absent) says which edition of
`DEFAULT_EXTENSIONS` a list has been brought up to; below the current one, the
types it lacks are added once, in the app as it loads settings and in the host
the browser asks (`CatchRules::bring_up_to_date`). Dropped: adding the defaults
on every start, which would put back a type a person took out; and telling an
untouched list from an edited one, which the file cannot say. An empty list is
every type and is not touched. The same applies to the two "ask first"
settings, which are saved as they were: an older file keeps asking before a
caught download starts, until the person turns it off in Options.

## 2026-10-26: A click on a file the browser shows is caught by the page script

A link to a `.pdf`, `.mp4` or `.mp3` opens in the tab: the browser starts no
download, so `downloads.onCreated` never fires and a type on dm's list was
read, not caught. The page script now sees the person's plain left click
(`isTrusted`, no key held, no `download` attribute, an http(s) link to one of
the browser-shown types that is also on the app's list), keeps the page from
opening it, and asks the background (`dm.inline`); the background applies the
app's rules (`CatchRules::takes`, the master switch, the popup's switch, not a
private window) and hands the file over. Any "no" (no app, a site to leave
alone, a failed hand-off) makes the page open the link itself, so a click is
never swallowed. Dropped: `declarativeNetRequest` setting
`Content-Disposition: attachment` on those responses, which would also take
the Alt key's way out and every address typed or bookmarked, and make a PDF
impossible to read in the browser at all; and `webRequest` blocking, which
Manifest V3 does not give an extension. Not covered: an address typed into
the bar, a bookmark, a link opened from another app, a PDF in an embedded
viewer: those are the browser's page loads, not clicks the page can see.

## 2026-10-26: A waiting dialog comes forward; a saved "ask first" is not migrated

A download the browser handed over and dm asks about has already left the
browser, so a dialog behind another window or in the tray looked like a lost
download. Opening Download details, the page's links or "Download again"
from a hand-off now sets `show_asked`, which the window already honours (it
shows itself, unminimized and focused), and a hand-off that starts at once
does not. Dropped: migrating a saved `ask_caught_downloads` or
`ask_panel_downloads` to off, as the catch list was brought up to date: the
file cannot tell an old default from a person's choice, and wrongly turning
off a question someone wanted is worse than a question they can switch off
in Options › Browser. The 404 text was reworded for a person (not the
server's words) in all thirteen catalogs; the other plain-wording rows stay
open (gaps row 9) since each is a string in every catalog.

## 2026-10-26: Remove from the list asks nothing for a finished download

The standards separate "Remove from list" (safe, no prompt, Undo for five
seconds) from "Delete file" (a question, the Recycle Bin, never permanent).
A finished row's removal loses nothing, so `Remove` now takes it off at once
and the queue keeps its snapshot (`Queue::finished_rows`) for `put_back`,
which restores it in its place; Undo is a link in the status bar for five
seconds, not a toast with its own window. An unfinished row still asks: its
fetched bytes go with it and cannot be put back. `Delete file` reuses the
removal question with the file ticked, and `Platform::trash` (the `trash`
crate on a desktop, a plain delete in the tests) moves the file out instead of
erasing it. Dropped: undo for an unfinished row, which would mean keeping its
parts folder for five seconds and the disk it holds. The new dependency,
`trash` 5.2.9, is the one crate for the three systems' recycle bins.

## 2026-10-26: The first run is the welcome; hints are said once; paused sites are the extension's

The standards ask for a first run of at most three skippable steps. dm already
opened a welcome on a new install (the extension for each browser found, start
with the computer); it now also shows the folder downloads go to, which can be
changed there, and ends with Done (Later still skips it). A hint is said once
ever and kept in the settings file (`hinted_big`, for a download of 5 GB or
more; the Scheduler is what it names): said again, it is a nag. The close
button asks once (`on_close`: ask, tray, exit) and the answer is the setting,
changeable in Options, so no answer is hidden. "Pause capturing on this site"
is a list in the extension's storage (`dm.catch.paused`), as "Take every
download from this site" is, not a setting in the app: the app's skip list
(Options › Browser) is the app's own, for every browser, and a popup that
wrote into it would need the host to be running. Restore defaults copies a
tab's saved settings from `Settings::default()` by key, as `changed_only`
merges them, so a setting the tab shows but not a key is a test failure
(`tab_settings`); Sign-ins has none, since a person's logins are not a
default to go back to. Dropped: a Windows quiet-hours check written without
being able to compile it (gaps row 14), and hiding the sideways extra columns
(Last try, Description, Referrer, Folder) by default, which changes what every
saved window shows and which fifteen tests read.

## 2026-10-26: Windows modules are checked from here, and run on Windows later

`cargo check --target x86_64-pc-windows-msvc -p dm-desktop` stops at the C
build of `aws-lc-sys` without a Windows toolchain, so Windows-only code could
not be compiled on a Mac, and was left unwritten. The new Windows code
(`quiet.rs`, `contrast.rs`, `jumplist.rs`) is written to need only `std`,
`windows-sys` and `windows`, which are plain Rust, and
`scripts/windows-modules-check.sh` includes each file in a crate of its own and
checks it for the Windows target; their logic (what is quiet, what is held, the
palette's contrast) is plain code the tests run anywhere. What it cannot say is
whether the system answers as expected, so gaps rows 11 and 14 stay open as
checks to make on Windows. Dropped: a `windows` crate behind a feature in
dm-core, to check the whole of dm-desktop (a second build for one check), and
leaving the rows blocked (a person can only verify what exists).

High Contrast is not a fifth choice in Options: Windows says it is on, and
every program is expected to follow, whatever it was set to; the debug-only
`DM_HIGH_CONTRAST=black|white` shows the look elsewhere. A jump list is
published once at start, in the language then in force.

The list's vertical bar was a scroll area under egui's 64-point minimum, in a
strip of one row it could not reach as far as the list and pulled it back each
frame. Controls at 32 made that strip one row at 300% text with the clipboard's
bar, which is how it was found; it is fixed by giving the bar the strip's own
height (`min_scrolled_height`), and held by a test.

A dialog's shadow follows the room: Fluent's 128 where the window is 1000
points tall, 64 from 800, and 28 below (`dialog_shadow_blur`), since egui
counts a shadow in a window's size. The menu has 32; tooltips share egui's one
popup shadow and have 32 too, not 16.

## 2026-10-26: `app-check.py` reads the window after waking it

The real-binary check failed one run in three or four, on builds from before
this day's changes as well as after (4 of 12 and 2 of 12), at steps that click
and then read the window's words once after half a second. A request for the
words (`ui.texts`) reads the last frame drawn and does not make the window
draw one; any other control call (`ui.inspect`) does. The steps now wait up to
three seconds for what they expect, waking the window each time round
(`soon`), and a batch finishing no longer opens a Download complete window
over the dialog being typed into (`show_complete` off in the check's own
settings). 16 of 16 runs pass. The app was not at fault: a person's pointer
makes frames all the time.


## 2026-10-27: Update-manifest signing removed

The 2026-10-25 ed25519 signing of `latest.json` needed a private key kept as
the repository secret `UPDATE_SIGNING_KEY`; the owner has none and will not
set one up for now, so the 0.8.0 release job failed at the signing step. The
signing step, `cargo xtask sign-update`/`update-keygen` and the check in
`update.rs` are removed; the installer is still checked against the SHA-256
in the manifest. It is in git history (commit `89a66d6`) if wanted again.

## 2026-10-27: Compressed bodies are decoded by dm

reqwest's gzip/brotli features decode inside the client and report no count
of the wire bytes, so a ratio limit was impossible. They are off; `decode.rs`
wraps flate2 and brotli-decompressor writers, counts the wire bytes, and
stops a body past 64 MiB that unpacks to over 100 times what was sent. A
download with a content coding has no known size or resume, as before.
Tried and dropped: handing freed memory back to the system on a timer
(`memory.rs`): RSS did not move, so it was removed.

## 2026-10-28: Speed audit, what was changed and what was not

Changed: a low-speed drop for ranged connections (`ranged/helping.rs`
`Window`), concurrent HLS segments, DASH pieces by a shared counter, the external video tool
`--concurrent-fragments`. Measured with `tests/speed.rs` and `tests/hls.rs`.
Not changed: the 16 KiB write buffer (loopback 512 MB runs at 480-640 MB/s, so
the disk write is not the limit here; Windows/NTFS and 10 GbE are unmeasured),
the second probe after the host's own (needs the probe result carried in the
inbox and a freshness rule), HTTP/1.1 pinning (kept: separate congestion
windows per range). The stall drop is limited to a range's first two tries so
a server that serves one range slowly for good is waited out, not retried to a
failure.

## 2026-10-28: The video route follows what the external video tool's reading says, per stream

Four changes to the route that fetches a video page's streams itself
(`videopage/fetch.rs`), each driven by a field of the external video tool's reading and none by a
site's name.

**A request is as long as the reading says a server allows.** A format's
`downloader_options.http_chunk_size` (10 MiB on YouTube) becomes the most one
request asks for: a connection asks for the next slice of its range when one
ends (`ranged.rs`, `attempt_segment`, `fetch_segment`). Segments, ledger,
splitting and the resume check are those of the whole range; no figure means
the code that was there. The figure is raised to 1 MiB at least, so every
slice brings bytes beyond the resume check. It travels as `StreamHints`
(a task-local beside `STREAM`) and `Job.chunk`, not as a field of `Options`,
which is built field by field in every test and by callers. A local server that
slows each response after 1 MiB: 16 MiB in 6.4 s without, 0.55 s with.

**An address that runs out is renewed once.** A 403 or 410 from a stream, when
its `expire` has passed or bytes were already fetched, reads the page again and
continues if the streams are the same (format id, name, stated size). The saved
plan holds no address, so nothing on disk changes. A refusal at the first ask
on a live address keeps the old route (the `REFUSED` marker, the external video tool downloads),
so a site that refuses dm does not cost a second reading. Pause and cancel
during the second reading stop the download, as anywhere.

**A stream's headers are that stream's.** All of `http_headers` goes with its
probe and ranges (`download::Headers`), carried on `Job`, not kept by host as
the agent is (`prefer_agent` stays for the agent, as before). Not forwarded:
the agent, `Range`, `If-Range`, `Accept-Encoding` (the engine needs the file
as the server keeps it), the connection's own headers and `Authorization`;
`Cookie` and `Referer` only where dm has none of its own. Values are never
printed (`Headers` has a `Debug` of names alone).

**The external video tool, when it downloads itself, gets `--http-chunk-size 10M` and not
`--throttled-rate`.** Run against the external video tool 2026.10.21: chunking a server that
ignores ranges gave an identical 25 MB file, `--limit-rate 3M` held (25 MB in
8.7 s) and the `@dm@` progress lines were unchanged. `--throttled-rate 100K`
against a server held to 40 KB/s: the 600 kB file never arrived in four minutes
(a 0-byte `.part`; the external video tool starts over each time it reads the link as throttled),
where without it the same file took 16 s. Tried and dropped: it would turn a
slow connection into a download that never ends, on the path that downloads
today.

## 2026-10-28: The external video tool's unpacked builds on every system

its file, its file and its file (all in the
release's `SHA2-256SUMS`) replace the one-file builds, as its file
did for macOS. Each unpacks to its file, the program beside `_internal/`;
macOS keeps `ditto` (it keeps what a signed program's files carry), the rest use
the `zip` crate (`unpack_portable`: a name that leaves the folder is refused,
modes are kept, at most 600 MB). musl is told by a `/lib/ld-musl-*` loader.
An earlier one-file install is replaced at the next start (`freshen`) and
removed once the unpacked build is in; until then it is still found. Measured
on Linux aarch64 (Debian 12, a container, files on its own disk), `--version`:
one-file 0.59-0.66 s, unpacked 0.22-0.24 s, five runs each, twice. That is the
whole gain on Linux (0.4 s a run); the 8-14 s of macOS does not happen there.
Windows was not measured.

## 2026-10-28: Routing `m3u8_native` to dm's own HLS (design only)

Measured on 2026-10-31, and not built. Dailymotion's 1080p HLS stream
(`hls-1080`, 47 segments, 80.7 MB, the same bytes both ways), three runs
each from one reading of the page: The external video tool with `--concurrent-fragments 8`
and `--load-info-json` took 6.2 to 6.6 s; dm's `download_stream` on the same
media playlist, 8 connections, 5.6 to 5.7 s. About 0.7 s (12 %), most of it
the external video tool's start, on a link that the stream fills either way: not the clear
gain the route would need to pay for a second path, its fallback and the
picture-and-sound join. Measured with a throwaway example that called
`dm_core::download` on the playlist address; to repeat, build it again.

Not built. The external video tool already fetches HLS fragments concurrently (`--concurrent-
fragments`, since 2026-10-27), so the gain over today is unmeasured and may be
small; a measurement on a recorded run comes before code.

Which formats: `protocol` is `m3u8_native`, the reading is not live, `has_drm`
is not true, the playlist's keys are `AES-128` or none (`SAMPLE-AES` and
`EXT-X-MAP` shapes dm's `resolve_stream` calls `Beyond` stay the external video tool's), and
`http_headers` are what `Headers` carries (applied in `fetch_segment_range`,
which does not yet). The format's `url` is already the chosen variant's media
playlist, so dm takes it as given and does not choose a rendition again.

How: `Read::plan` gives such a stream a kind beside the plain-file one;
`fetch_stream` calls `hls::download_stream` into the same `.dm-f{id}` folder
(segments resume as whole segments, as they do anywhere), and the existing
`merge_progress` adds the streams' bytes up (no total, so no ETA, until every
stream has one). A separate audio playlist is a second stream, joined like the
two files of a plain pair, which needs each to be an MP4 first (`remux_ts`);
that join is the part to prove on real streams.

Fallback: any failure but a pause or cancel (including `Beyond`) forgets the
parts, writes `REFUSED` and has the external video tool download the same reading, as for a
plain file; an expired playlist address is renewed as above. Not wired through
`hls::download_stream`'s own `Beyond` arm, which hands the segment-playlist
address to the external video tool as if it were a page.

## 2026-10-28: A finished download wakes the scheduler

The scheduler fills free places on a fifty-millisecond tick and only an *add*
woke it earlier, so a place freed by a download that took five milliseconds
sat empty for the rest of the tick: eight places refilled every ~60 ms is the
131 files a second `stress-check.py many_small` showed on every run, whatever
the files or the machine's speed (identical to 0.1 file/s with and without
other changes, which is how it was found: sampled with `sample`, the process
spent its time in file calls, and removing some of them changed nothing).
`start()` now calls `kick()` when a download settles. Measured on a release
build, macOS: 2 000 small files 131 -> 740 files/s, byte-exact; 200 small files
with 20 000 queued 37 -> 116 files/s; the concurrency limit, pause storm, crash
resume and leak phases unchanged. Debug build, one place, 60 tiny files: 3.1 s
without, 0.22 s with (`queue.rs`).

Two smaller changes in the same pass, not separable by measurement while the
tick was the limit: the ledger is read for the slice loop only when a stream is
capped (my own change of earlier the same day read it twice per range for every
download), and the end-of-download cleanup (`assemble.rs`, `discard_staged`) is
one blocking task instead of a hop to the pool for each file. Looked at and
left: `plan.json` is written by temporary file and rename for every download
(about a seventh of the file calls of a small one); writing it in place would
leave a torn file the resume then reads as no plan, and what a resume does
without a plan was not worth changing for a few per cent.

The wake-up exposed a race the tick had hidden: `run` returns as soon as
nothing is busy, and the finishing task registered its last queue save a moment
later, so a `flush` right after `run` found nothing waiting and wrote nothing
(the full workspace run caught it once: `a_finished_download_is_still_finished_
after_a_restart`; the app's equivalent is quitting just after the last download
settles). The save is now owed (`unwritten`) before the scheduler is woken. The
test for it (`the_last_state_is_on_disk_when_run_returns`, 40 rounds) passes
without the fix on an idle machine and fails 4 runs of 4 with the CPU saturated,
so it only means something under load.

## 2026-10-28: The window reads the queue when it changes

The window keeps a copy of the list (`DmApp::rows`) and read it every quarter
of a second while it thought something moved, every 30 seconds otherwise; a
change to the queue only asked for a repaint in a quarter of a second. A frame
that came before that (a control request, a click) read nothing, because a read
was less than a quarter of a second old, and ended by asking for the next
repaint in 30 s from a list in which nothing moved, which replaced the earlier
request. Measured with a line in `refresh()`: a download resumed over the
control port from idle was not read again for 4.3 s, until it finished.

`QUEUE_CHANGED` is set by the queue's change callback and cleared by a read;
`refresh_plan(changed, since_read)` reads now when a quarter of a second has
passed and otherwise, if the flag is set, asks for the next look a quarter of
a second after the last read. The same run: a read at +264 ms and then every
255 ms for as long as it moved. The cost is at most one read more per change,
only within a quarter of a second of another.

Control calls (`downloads.pause`, `resume`, `cancel`) take the row from the
queue (`Backend::row`) and not from the copy: the copy is read at most four
times a second, and a script that paused and resumed was told "it is not
paused". The in-app commands were already right, because each ends with a read.
Left as it is: everything else the window draws is from the copy, up to a
quarter of a second old; that is the price of not asking the queue on every
frame, and no run found it longer than 0.7 s.

A row that is not Running has no speed or time left (`Slot::stopped_moving`,
on hold, requeue, cancel, the end of a run, and for a report that arrives after
one of them), as `persist` already said of a restored one.

## 2026-10-28: Per-frame work in the window, measured and cut

Following the optimisation guide (baseline, profile, smallest change, measure
again). Gates first: `cargo fmt --check`, `cargo check`, `cargo clippy
--all-targets --all-features -D warnings`, `cargo test --workspace` (1164) and
`cargo audit` (both workspaces) were already clean; `unwrap`, `expect` and
`panic` are denied outside tests in every crate.

Baseline, release build, macOS, 5000 rows and 20 downloads moving, nothing
else running: idle 0.3-0.5 % of a core and 190 MB; with the downloads moving
the process used 15-17 % (mostly the engine's threads, so not a measure of the
window). Measured on the window's own thread instead, by sampling it for
about 6 s (`sample`, a symbolized release build): busy 17.0 % and 19.2 % of
the time. The profile named what a frame did that it need not:

- `dialogs::clip_links` and `dialogs::site_files` worked out
  `listed_addresses` (a URL parse of every row; `site_files` also built an
  HTTP client) before checking that their dialog was open: about a quarter of
  the busy time, on every frame, for dialogs that are almost never open.
- `Queue::queues()` copied every row to read each one's queue name, and the
  window asks several times a frame (side rail, tree, dialogs).
- `targets()` (the toolbar's enabled states call it several times a frame)
  copied and sorted every shown row to find the selected ones, usually none.

Each now does only what its answer needs: the dialogs return first,
`queues()` reads the rows in place, and `targets()` returns at once with
nothing selected and otherwise copies and sorts only the selected rows (a
stable sort of some rows is those rows in a stable sort of all of them; a test
compares it with `visible()` filtered by the selection under every sort).
After: busy 10.5 % and 8.9 % (11.1 % with the first two changes alone),
URL parsing in frames 0, `Backend::queues` 208 -> 21-27 samples, `visible()`
200 -> 48-63. About 46 % less work on the window's thread.

Not changed, and why: idle was already 0.3-0.5 %, so the repaint cadence
(`request_repaint_after`) stays; the whole-process CPU figure is too noisy to
show this (it includes the download threads and moved 9-16 % between runs of
one binary), which is why the thread was measured on its own.

The shown rows, the sidebar's counts and each row's kind and filter text are
kept from one frame to the next (`DmApp::visible_shared`, `count`,
`place_rows`), each under what it is made from: a generation counter bumped
when the list is read or the places are made again, and for the rows the
place, the filter, the sort and how many sizes a dialog has told. A place is
made again for a row only when its address, name, note or kind changed (a
hash of those), and for every row when a person's own categories do. Per frame
at 5000 rows sorted by name (release; `frame_cost_of_a_long_list`, run by
hand): copy and sort 370 us, cached 7 ns; a place's count 98 ns. The window's
thread under 5000 rows and 20 downloads moving: 17-19 % busy at the start, 6.0
and 4.9 % now. Checked as a person would use it (`qa.py`, 28 checks: click the
headings, each place and its count, search in any case and clear it, select,
Stop, Resume, add and remove with a sort and a filter on), all passing, and
failing 8 of 28 with the generation never advanced, so the session can see a
stale cache.

## 2026-10-28: Google's Rust style guide, what was taken and what was not

Read at <https://google.github.io/styleguide/rustguide.html> and checked rule
by rule, with the guide's own lints (`clippy::wildcard_imports`,
`needless_for_each`, `undocumented_unsafe_blocks`, `must_use_candidate`,
`fn_params_excessive_bools`, `string_add`, `missing_panics_doc` and
rustc's `missing_docs`) and greps for the rest. Its Chromium and Fuchsia
rules (flat `rust_library` layout, `chromium::import!`, `git cl format`,
`rust_gtest_interop`, `fuchsia_sync::Mutex`, no `async`, no std threads)
belong to those build systems and were not applied.

Already as the guide asks, found by checking: no `unwrap`, `expect` or `panic`
outside tests (denied in every crate); no `ctor`, `#[rustfmt::skip]`,
`#[doc(hidden)]`, `get_` getters or `Ext` traits; the lints for `ptr_arg`,
`borrowed_box`, `should_implement_trait`, `new_without_default`,
`len_without_is_empty`, `wrong_self_convention`, `unnecessary_unwrap`,
`fn_params_excessive_bools` and `missing_panics_doc` have no findings.

Changed: every `unsafe` block in the macOS and test code has a `// SAFETY:`
comment that says what makes it sound (28 had none), and a lint keeps it so
(`undocumented_unsafe_blocks`, not on Windows, whose blocks are not written up
yet); three strings built with `+` are built with `format!`; two `for_each`
are loops, and `string_add` and `needless_for_each` are kept clean by a lint;
`rustfmt.toml` says the width of 100 the guide asks for (rustfmt's default,
stated); the cache key added the same day is a struct with named fields, not a
tuple; one test that set `DM_HOME` in every case of a property test now sets
it once (the standard library calls `set_var` unsafe because another thread may
be reading, which a comment cannot make false: the exposure that remains is
named in the comment).

Kept, on purpose:

- `lock()` (`dm-core/src/lib.rs`) takes the data of a poisoned mutex
  (`PoisonError::into_inner`), where the guide says never to. dm's plain mutexes
  guard a problem list, a schedule and the window's settings, which stay whole
  after a panic elsewhere, and one panic turning into a crash at every later
  lock was judged worse; changing it would put a `Result` on hundreds of calls.
- `tray.rs` calls `catch_unwind` around the tray's creation, where the guide
  allows it only if never relied on. It is: a panic in the tray library on some
  desktop would otherwise end the program, and dm runs without a tray instead.
- `use super::*;` in a file module (121 uses) and `use submodule::*;` in a
  parent that only re-exports: the guide allows the glob in inline modules and
  in non-leaf modules that re-export; a file module extending its parent is the
  same shape, and replacing them is churn with no behaviour.
- `Rc<Vec<Entry>>` for the shown rows (`visible_shared`): the guide prefers
  unique ownership, and the alternative is a copy of every row in every frame.
- Not taken: `imports_granularity = Module`, `hex_literal_case` and
  `format_macro_matchers` are nightly-only rustfmt options.

## 2026-10-28: Three CI failures, and what each was

CI on `36bec4b` failed on Linux and Windows. Read from the job logs, fixed, and
checked on Linux in the repo's container image (fmt, clippy `-D warnings`, the
layering check and 1167 tests, none failing).

- Linux: clippy refused three `unsafe` blocks in `xdnd.rs` that had no
  `// SAFETY:`. The lint added that morning is on for every non-Windows build;
  those blocks are Linux-only and did not compile on the Mac it was written on.
  Written up; a lint that applies to code the author cannot compile has to be
  tried on that code before it is made a rule.
- Windows: `fetched_tools_are_kept_only_when_they_match` planted the old
  single-file the external video tool as its file, where Windows names it its file; the code
  was right and removed its file. The test names it as the system does.
- Windows (it passed on the retry): `removing_many_rows_writes_the_queue_once`
  saw 2 saves for 1. `Queue::flush` looked at "is a save owed" before taking the
  save lock, so the trailing flush and a caller's both saw it owed and each
  wrote the list. It now looks again once it holds the lock. A test of eight
  concurrent flushes fails against the old one (4 runs of 4) and passes now.

Also found by running the same suite again: window tests of that day took the
settings-file lock only inside `add_url`, then went on driving a window that can
write the settings file, and an Options test then read someone else's defaults
(about half the runs of `cargo test`; nextest's process per test hid it in CI).
They hold it for their whole run, as the others do. An earlier note here that
this was "load" was wrong.

The stress suite's `edge` phase ran Chrome for Testing, not Edge: it blanked
`CHROME_PATH`, so the browser check downloaded Chrome and failed three runs in
a row (connection closed, timed out, reset). It now uses the installed
Microsoft Edge (macOS, Windows, Linux paths) and says when it has to fall back:
38 of 38 checks pass with Edge 156, forty simultaneous downloads all taken by
dm and none kept by Edge.

## 2026-10-28: Microsoft's Pragmatic Rust Guidelines (the part given)

Checked against what was pasted (it stopped partway). Already as asked: no
`panic!`, `assert!`, `todo!` or `unreachable!` in shipped code (so every one
has no message to be missing); `unsafe` has its reason written beside it; no
macros of dm's own; module and crate docs are about what a thing does.

Not applied, and why:

- Items reachable by one path (`M-SINGLE-ITEM-PATH`): `dm_core::category` is
  public and its types are also re-exported at the crate root (about twenty
  items), so each is reachable two ways. One path means changing it in every
  crate that uses the other; a public-path change for you to decide.
- `M-PANIC-IS-STOP` calls `unwrap()` on a poisoned lock valid, where
  `dm_core::lock` takes a poisoned mutex's data; and `M-PANIC-CONTINUATION` asks
  for a restart after a caught panic, where the tray's `catch_unwind` goes on
  without a tray. Both are the choices already recorded above; the two guides
  disagree about the first (Google: never reach inside a poisoned mutex).
- `M-TARGET-CPU` is for server applications; a desktop program has to run on
  the oldest computer it is installed on. `M-MIMALLOC-APPS` was tried and
  not kept: two release binaries from one tree, differing only in the global
  allocator, on `stress-check.py many_small` (2000 small files, three runs
  each): 735-749 files/s with mimalloc and 736-740 without (the same), and a
  peak of 112-117 MB resident against 63-64 MB (80 % more), for a C dependency
  in every platform's build. `M-FAST-HASHER` was not tried: the profile of
  2026-10-28 does not show the hasher.

## 2026-10-28: A portable Windows build, and a video failure on CachyOS

**Portable.** dm already kept its data beside the program when an empty file
named `portable` sat beside it (`settings::enter_portable`), but nothing
shipped that way: the Windows checklist told a tester to make one by hand, and
the release held only the NSIS installer. The Windows bundle job now zips
`dm-desktop.exe`, an empty `portable` and a readme under a folder of its own
(`dm_<version>_x64-portable.zip`), the `installed` job unzips it and runs
`scripts/portable-check.py` (headless, with every usual data place pointed at a
decoy: `dm-data` appears beside the program with its port and its list, and the
decoy stays empty), and the release job ships it with the rest. The check was
run here on a Mac and fails, as it should, when the marker is left out (dm then
writes to `Library/LaunchAgents` and `Application Support`). Not run on
Windows: the zip step and the Windows run of the check first happen in CI.
`latest.json` lists every file, but the updater takes the Windows installer by
its `-setup.exe` ending, so the zip is never offered as an update.

**"dm could not get the video from this page", CachyOS, AppImage, Brave.** One
way it happens found and shown; that it is this person's way is not confirmed.
The released x86-64 AppImage cannot run in Docker on the Apple Silicon machine
this was tried on (Exec format error even for `--appimage-version`), so the
aarch64 AppImage of the same release was run on an arm64 Arch Linux image, started
the way an AppImage runs, with no `DM_HOME` set by hand. With `/tmp` as on a
default system the page downloads. With `/tmp` mounted `noexec` the single-file
the external video tool that dm 0.8.3 fetches cannot start (it unpacks itself into `/tmp` on each
start: "error while loading shared libraries: libz.so.1: failed to map segment
from shared object"), and the row ends as the person's sentence: dm's state was
`the external video tool could not download that: …/tools/the external video tool: error while loading shared
libraries…`, which the status table turned into "dm could not get the video from
this page" for every the external video tool failure it had no words for. The current tree, which
fetches the unpacked build (written for startup speed), fetched
its file, ran, and reached YouTube under the same `noexec` `/tmp`
(its only failure there was this network's HTTP 429, from repeated tests).
`TMPDIR=<a folder programs may run from>` makes the released one-file build start
too (checked on 0.8.3). `scripts/linux/noexec-tmp-check.sh` shows the single
file failing and the unpacked build starting, from the external video tool's own release files.
Also found: the AppImage bundles no libraries (`usr/lib` is empty, `AppRun` sets no
environment), so on a minimal system it needs the host's `libxkbcommon-x11` and
the like; and with `/tmp` noexec an AppImage run without FUSE
(`--appimage-extract-and-run`) cannot start either, which a desktop's FUSE mount
does not do. The status now says the external video tool's words after the vague sentence
(`format::videopage_said`: the last `ERROR:` line, extractor and id taken off, no
address, 120 characters), so the next report carries its cause.

Reproduced on CachyOS itself with Brave (`scripts/linux/cachyos-brave/run.sh`):
CachyOS's own image (x86-64, under emulation), Brave 1.98.62 as Brave publishes it
(it runs a page, and `dm-desktop --register` connected dm to
`BraveSoftware/Brave-Browser`), and the released 0.8.3 AppImage unpacked. A page
with a video in it is handed to dm over the native host exactly as the extension's
Download video panel sends it (`engine.hello`, then `download.start` with a
quality). With `/tmp` as normal it completes (`A video (1) [page-1].mp4`, 600000
bytes); with `/tmp` `noexec` the row fails with the `libz.so.1: failed to map
segment` loader error and the Status column reads "Error: dm could not get the video
from this page", the person's sentence; with `noexec` and `TMPDIR` pointed at a
folder that allows programs the same page completes. The external video tool's own two builds on
that CachyOS under `noexec`: the single file fails, the unpacked one starts. What
is not shown: that this person's `/tmp` is `noexec` (asked of them), the
extension's own UI (the hand-off is its message, sent by script), and the current
code's x86-64 build (its aarch64 build was run on Arch earlier).

## 2026-10-28: The external video tool gets a temporary folder of dm's own when /tmp is noexec

The unpacked the external video tool (0.8.3's follow-up) does not unpack at each start, but a
single-file the external video tool a person installed, or one fetched by an older dm, still does,
into `TMPDIR`. On a `noexec` mount it cannot start. dm reads `/proc/self/mounts`
once (`videopage::runnable_temp`); if the system temporary folder sits on a `noexec`
mount it sets `TMPDIR` for the external video tool to `<tools folder>/tmp` (made on demand, `_MEI*`
older than a day removed). Where `/tmp` allows programs nothing changes. Tried and
dropped: asking the person to set `TMPDIR` themselves (they see only "could not get
the video"); remounting is not ours to do.

## Programs dm starts do not get the AppImage's folders

The AppImage's `AppRun` sets `PYTHONHOME`, `PYTHONPATH`, `LD_LIBRARY_PATH`,
`PATH` and others to point into the AppImage, for dm's own sake. A person's
the external video tool run by the system's Python (CachyOS's `pacman -S the external video tool`) inherited them
and died at start ("Fatal Python error … <no Python frame>"). `no_window` and
`no_window_async`, which every helper dm starts already goes through, now give
each of those variables without the entries under the AppImage's folder
(`APPDIR`, or the folder dm's program sits in as `usr/bin/dm-desktop` beside an
`AppRun`), and unset it where nothing is left. Outside an AppImage nothing
changes. Dropped: clearing the variables outright, which would also drop a
person's own `LD_LIBRARY_PATH` entries (CUDA and the like).

## The panel's code is injected by the browser, not imported by the page

`content.js` used `import(runtime.getURL('pkg/dm_catcher.js'))` and the module
fetched its `.wasm`, both listed as web-accessible. Brave on YouTube logged
"Unsafe attempt to load URL chrome-extension://… Domains, protocols and ports
must match" for those loads (reproduced by `stress.sh` on real YouTube, on tabs
opened after the first), whichever address the manifest gave (`use_dynamic_url`
on or off). Blink logs that when a fetch is checked against the page's origin
rather than the extension's. Now `content.js` sends `dm.panel-load`; the
background calls `scripting.executeScript({ files: ['pkg/panel.js'] })` for
that tab and frame; `panel.js` is wasm-bindgen's `no-modules` glue with the
WebAssembly written into it as base64 (`scripts/package.sh`), so the page's
frame fetches nothing and no file is web-accessible. It leaves its exports in
`__dmPanelCode`, not `__dmPanel`, which the panel itself keeps to start once.
Cost: 726 KB of script read per media frame instead of a 500 KB fetch; the
panel still loads only where a page shows media. Tried and dropped: removing
`use_dynamic_url` alone (the lines stayed, with the stable address).

## A certificate that is not trusted is an answer, not a bad moment

reqwest reports a refused certificate (expired, for another name, an issuer the
built-in roots do not hold) as a connect error, and `retry::transient` retried
every connect error: ten tries over two minutes before the row said why
(BUGLOG.md, BUG-002). `retry::untrusted_certificate` reads the error's chain
and makes it final, as `unreadable_response` does for headers that cannot be
read. dm still trusts only the built-in roots (webpki); trusting the system's
store, for a company's own authority, would be a separate decision.

## The external video tool is ended when it stops answering

an external video tool that said nothing kept its row "running" for as long as dm ran
(BUGLOG.md, BUG-004). Its two output streams are watched: nothing before its
first word for 120 s (as long as a page reading may take, `READ_WITHIN`), or
nothing for ten minutes once it has spoken (a merge or a slow server is quiet
for a while), and its process tree is ended with "the external video tool stopped answering".
its environment switch sets the first limit for tests, as its environment switch names the
program. Dropped: a limit on the whole run, which a long video on a slow link
would hit while working.


## 2026-10-30: dm follows redirects itself

reqwest followed a redirect with the request's headers, dropped the sign-in
and cookies when the host changed, and wrote the address each hop came from
into `Referer` on every hop (`referer(true)`, its default), which is how a
page's path and query, or a signed link's, reached whatever other host a
redirect led to (`docs/security-report.md`, L2). Its policy sees the hops
and can refuse one but cannot change a header. So dm's clients follow
nothing (`Policy::none()`, `referer(false)`) and every request goes through
`redirects::send` or `redirects::execute`: ten hops at most, none from
`https` to `http`, none from a public address inward, the sign-in and
cookies dropped across hosts as reqwest dropped them, a 303 (and a 301 or
302 to a POST) answered by a GET, and `Referer` cut to its origin across
hosts, dropped from a secure page to a plain host. The ranges that go
straight to where the probe landed carry the same (`referrer_for`). The
first host still gets the page whole: the extension hands over the page,
not what the browser sent, and a hotlink check that wants the page on the
first request is the common one. Dropped: sending the origin alone from the
first request (those checks), and a task-local flag read by reqwest's policy
to stop on a cross-host hop and re-issue (the logic in two places, and the
policy is per client). Clients built elsewhere with reqwest's own builder
(a page read, RTMPT) are not affected and follow as before.

## 2026-10-30: A row's secrets go beside the queue file, not yet in the keychain

A login written into an address and the cookies the browser handed over were
written into `queue.json` as the row carries them (L1). They now go to
`secrets.json` beside it, by row id, taken out on every write and put back
on restore, so the queue file can be shown or sent without them; both files
are 0600, as the settings file holding the Sign-ins' and proxy's passwords
already is (2026-10-17). The operating system's keychain was looked at and
set aside for now: on macOS an item made by an unsigned or ad-hoc-signed
build belongs to that build's code hash, so every rebuild is asked for the
password in a dialog that the QA loops cannot answer, and on Linux the
Secret Service needs a session daemon CI's containers do not run; a keyring
dependency would also pull D-Bus into the Linux build. The row stays open
for the signed release (gaps.md row 39). Dropped: encrypting the file with a
key kept beside it, which hides nothing from whoever has the folder.

## 2026-10-30: The host answers the extensions it knows

The native host served any `chrome-extension://` origin (L3, accepted as the
standard model: the browser lets only the extensions its manifest names
start the host). Added as defence in depth, since it costs one file read:
`shell::launcher_known` takes the id from the origin Chrome passes (or from
the id Firefox passes after its manifest path, in the versions that do) and
answers only the ids built in and those a manifest dm wrote allows, read
from the files whether or not their browser is installed, so a manifest a
person edited for a build of their own still counts. A `moz-extension://`
origin is a profile's own name for an extension, not its id, and is let
through. Refused, dm says so on stderr (the browser's log) and exits.

## 2026-10-30: The slice cap timed on a long video over a real connection

Row 22 asked for the slice size taken from the external video tool's `http_chunk_size`
(2026-10-28) to be timed on a long video against a server that actually
throttles. Big Buck Bunny (10 minutes, 181 MB at 720p) through the release
build, headless, on this connection: 16.1 s and 14.1 s with the cap (11.2
and 12.9 MB/s), 44.2 s and 44.2 s without it (4.1 MB/s). The cap stays. It
is switched off by `DM_NO_CHUNK` in the environment, kept so the
measurement can be taken again (the scratch script drove dm over its
control port, as `scripts/real-sites.py` does).

## 2026-10-30: A scripted step goes to one painted pass, and the window comes to the front for it

`ui.click` queued the pointer move, the press and the release as one step
each, given out one per call of eframe's raw-input hook, which was taken
for one per frame. Since egui 0.37 (2026-10-17) eframe also runs passes
that only run the app's logic, between paintings, calls the hook for each,
and carries that pass's input over to the next painted one. A step given
to each call piled up there: the move, the press and the release reached
egui in one painted pass or many times over, and a dialog's button (OK,
Start now) took the press and not the release. It showed only when the
window was not being painted between steps, which on macOS is a window
covered whole by another (mine, this afternoon; the QA passed in the
morning with the window in the open) — the system sends such a window no
redraw at all, so the steps waited in eframe's carried-over input for a
painting that never came, and a screenshot, which forces one, let them
all in at once. Traced with the hook's raw input printed per pass. Taken:
a step is given out only once the window has been painted since the step
before (`DmApp::painted`, counted where the window is drawn), and a step
that has waited 400 ms for a painting asks the system to bring the window
to the front (`ViewportCommand::Focus`), as a person driving the window
would have it in sight. Also kept from the chase: each step finds its word
where it is drawn when its pass comes (a dialog still settling, or one
closing whose title also reads "Add URL", no longer takes the click), and
egui's click-duration limit is 10 s rather than 0.8, as every other program
here counts a press let go late as a click. Tried and dropped on the way:
the press and the release in one frame (a dialog's button never took it);
blaming the screenshot, the clipboard offer (a link the QA had put back on
the clipboard did open its offer over the dialog, a separate confound) and
the window's focus. The driver with the window covered and a screenshot
before each click: ten of ten, from none of ten.

## 2026-10-30: The external video tool's legal position, and what the other managers do

Asked for: whether using the external video tool can affect this project, and how the
reference managers get YouTube "legally".

- **Licence.** The external video tool, its its file challenge solver and QuickJS-NG
  are published under the Unlicense (public domain) and the MIT licence;
  nothing in them constrains dm's MIT licence, and dm could even carry
  their code. dm fetches them at run time from their own releases rather
  than shipping them, so dm's packages and repository distribute none of
  that code at all; the licence question does not arise for a distribution
  that has nothing to distribute.
- **What the law is about** is not the tool but the use: YouTube's terms
  forbid downloading without permission, and the 2020 takedown of
  an earlier video tool claimed circumvention of a technical measure (the "rolling
  cipher"); GitHub restored it, finding that claim did not hold, and the
  EFF's letter on it is the public record. The external video tool solves the player's `n`
  and signature puzzles, which YouTube serves in the clear to every
  browser; it breaks no encryption (DRM-protected formats are not
  downloaded by it, and dm leaves them alone too). Every download manager
  that gets YouTube is in the same position: reference A documents reading
  the player's requests and keeps a paid team shipping fixes; the other
  macOS manager looked at today (1.1.10, installed on this computer) ships
  the external video tool 2026.10.26 inside its bundle with QuickJS, ffmpeg, ffprobe and
  the downloader, points the external video tool at its QuickJS through a config file, and updates
  the external video tool to the latest release through its own manifest every four hours.
  So none of them "does it legally" in a way dm does not; dm fetches the
  same the external video tool afresh and does not ship it.
- **What dm keeps apart:** no YouTube code of its own to defend, the tool
  fetched from its publisher and refreshed, the person's own session only
  with their consent, no DRM, and the policy of each site's terms left to
  the person, as with any downloader. The README's "Use it responsibly"
  says so.

## 2026-10-30: How YouTube is got today, watched, and what a reader of dm's own would be

The maintainer asked for YouTube without the external video tool, as the other macOS
manager was believed to do it. It does not (above). What the external video tool itself
does today (2026.10.26, watched with `--print-traffic` on a public
video; a public-domain tool, read as a black box): one GET of the watch
page as a browser (for the visitor id, the player's script id and its
signature timestamp), one POST to `/youtubei/v1/player` as a Samsung TV
client (`TVHTML5` 5.20260707, client name 7; plain `https` DASH formats,
no proof-of-origin token needed), one more as a visionOS client for HLS,
one GET of the player script (`/s/player/<id>/player_ias.vflset/en_US/base.js`),
and one run of the its file solver (v0.8.0, two scripts, Unlicense)
in Deno or QuickJS, which turns each format's `n` value into the one the
server accepts (and decodes a `signatureCipher` where a format carries
one). Its own source marks the web client as SABR-only, the Android VR
client refused since 2026-08-17, and the iOS and Android clients as
needing a token: the TV client is the one that works without tokens, as
of today. A reader of dm's own is therefore well defined: the same four
requests, the same solver scripts fetched from their releases and run in
the QuickJS dm already fetches, the result written in the shape of
the external video tool's reading so `download_page` and the panel's list consume it
unchanged, and the external video tool kept behind it for every other site and for the
day YouTube changes the client. Its cost is the one already recorded
(2026-10-29): the client's name and version, and the solver's version,
are dm's to keep current, as the external video tool keeps them for itself. Taken on, at
the maintainer's word of 2026-10-30, as gaps.md row 41.

What the other manager offers that dm does not, worth taking on the same
evidence (its bundle and settings, not its code): a download of a part of
a video (the external video tool's `--download-sections`, which cuts with ffmpeg; dm has
no ffmpeg and its own muxer joins whole streams, so a part would be its
own fetch of the byte ranges the picture's and the sound's index names,
row 42), and the external video tool refreshed within hours rather than a week (dm already
refetches a newer the external video tool after a failure, once an hour).

## 2026-10-30: YouTube read by dm itself, done

Row 41 closed the same day: `youtube.rs` makes the four requests above
and writes the reading in the external video tool's shape; `download_page` takes it first
on a watch page and the external video tool behind it, the panel's list stays the page's
own data, and the host no longer warms an external video tool reading of a YouTube page.
A YouTube page does not wait for the helper's first fetch either: that
goes on behind dm's own reading, for the videos it cannot read. Two
things the stand-in could not show and the real video did: the TV
client's answer names no title (the page's own is used) and a
picture-with-sound format chosen beside a sound made a file with two
sound tracks (the formats apart are the only candidates now, the muxed
ones alone and only where nothing is apart). Checked: the stand-in test,
the real video through `real-sites.py` (4 s, the right name, one picture
and one sound) and the Edge check 49 of 49 with the panel's download
through the reader. The solver's scripts are pinned to release 0.8.0 by
digest; when YouTube's player changes past what that release solves,
dm's reading fails in words and the external video tool (which carries the newer solver)
reads the page, so a stale pin costs the first request and nothing else;
refreshing the pin is a release of dm, as QuickJS's is.

## 2026-10-30: The other macOS manager watched at run time, and what dm takes from it

Asked for: how the other macOS manager (1.1.10) gets YouTube and other
sites, found by running it, and the same in dm without the external video tool. Its engine
is a Node program packed into one file with its code compiled, so it reads
as nothing (a static JavaScript analysis tool was run on it and on its Edge
extension; only the extension is source). What cost nothing was going
round that: its engine listens on 127.0.0.1 with a token it writes to its
own folder for its extension, so a script drove it over that interface as
the extension does, and every process it started was recorded with its
command line and the hosts it reached (`ps`, `lsof`).

What it does, from the record:

- **YouTube.** First `the external video tool --dump-json --no-download --no-playlist
  --ignore-errors --prefer-free-formats <url>` (its config file adds
  `--js-runtimes quickjs:<its QuickJS>`), which ran its QuickJS for the
  player's challenge: 7.6 s. Then, after the job had waited 36 s in its
  queue, `the external video tool --ffmpeg-location <its ffmpeg> -f <selector>
  --merge-output-format mp4 -o <file> --no-playlist --no-warnings --newline
  --retries 10 --fragment-retries 10 --retry-sleep http:exp=1:20
  --retry-sleep fragment:exp=1:20 --abort-on-unavailable-fragments
  --progress-template … --concurrent-fragments 8 --load-info-json <its
  reading>`: The external video tool fetches the two streams, one connection each (the
  concurrent fragments are for streams in pieces), ffmpeg joins them, and
  `ffprobe` reads the result. Its selector asks, per quality step from the
  one chosen downwards, for H.264 first, then H.265, then any MP4, then
  anything, with M4A sound; and it measures a quality by the picture's
  shorter side (`[aspect_ratio<1][width<=720]` for a portrait video).
- **Other video sites** (Dailymotion tried): the same the external video tool download
  with no reading first. **A plain file:** its the downloader over RPC, sixteen
  connections (`--split=16 --max-connection-per-server=16
  --min-split-size=1M`).
- **Its extension:** a button on every `<video>`; on YouTube the quality
  list is asked from the page's own player (`getAvailableQualityLevels`,
  in the page's world) and elsewhere from its engine's reading; a site's
  cookies go only for Vimeo and Instagram; a clip (a part of a video) is
  cut by the external video tool's `--download-sections` and ffmpeg (row 42).

So it does not get YouTube without the external video tool: it is the external video tool, ffmpeg and the downloader
behind a window, and refreshes the external video tool every four hours. dm, as of today,
goes further than it on YouTube: the page is read by dm itself in four
requests (`youtube.rs`, no the external video tool run at all when the reading holds; proved
by a run with its environment switch naming a program that does not exist), the
streams are fetched on dm's connections and joined by dm's own muxers, with
no ffmpeg, and the same video that took it 45 s took dm 6 s from a cold
start, dm's own reading included. The external video tool stays behind dm's reading, as
decided on 2026-10-29 (rows 6 and 32), for the videos the reader cannot
read and for every other site, where the other manager uses it too.

Taken from it: a quality is the picture's shorter side (`format_for`'s
selector and `youtube::choose`), so a portrait video at 720p is 720×1280,
not the 405×720 picture that is 720 tall; dm's own HLS and DASH picks
still go by height (row 43). Not taken: the downloader (dm's engine fetches plain
files on its own connections), ffmpeg and ffprobe (dm's muxers join without
them), the four-hour refresh (dm refetches a newer the external video tool after a failure
it has no words for, within the hour), and `--prefer-free-formats` (dm
chooses H.264 in MP4 by its own rule).

## 2026-10-30: What was taken from the other macOS manager, and how it was measured

Asked for: the other macOS manager's good things, its slick window, its
settings and its paid tier's features, taken into dm as a clean-room
implementation in Rust and egui, and dm made to perform as well. Its window
is a native Swift app with a Tauri window beside it; none of its code or
assets were read or copied (its engine is compiled bytecode, its window
could not be captured here for want of screen-recording permission). What
it does was taken from its public pages, its own words in its binaries
(feature and setting names), and its engine driven over its local
interface; dm's versions were written from that description alone.

What it offers, and what became of each:

- **A part of a video** (its "Clip", paid): taken, `clip.rs`. Its clip is
  the external video tool's `--download-sections` with ffmpeg re-encoding; dm's is the
  streams' own pieces chosen by their index (`sidx`), fetched by ranges,
  and cut to the second by the joined file's edit lists, with nothing
  decoded. Its vertical, square and GIF clips re-encode, which dm has no
  codec for: not taken.
- **The quality list with sizes** in its save sheet: taken into Download
  details for video pages, as chips four to a row.
- **Playlists** (paid): taken for YouTube, read by dm itself from the
  playlist page and the browse API; other sites' playlists are gaps.md
  row 46 (the external video tool's flat listing would give them).
- **Connections per file up to 32, ten downloads at once** (paid): dm
  already allows 32 and 10.
- **Turbo Mode** (an iPhone's connection beside the Mac's): not built; the
  design (a second client bound to the second interface by its address,
  ranges shared by measured speed, tested on loopback against a second
  interface) is gaps.md row 47.
- **Auto-delete** of a file after a chosen time, **conversion** to MP3,
  WAV, GIF, JPEG (needs codecs), **a Google Drive folder**, and its notch
  (music, a shelf, clipboard history, AirDrop, weather, calendar, Messages
  replies): not taken; auto-delete is row 48, the rest are outside a
  download manager.
- **Its settings** (connections per file, downloads at once, where to
  save, ask where to save from the browser, sort into folders by kind,
  notify when done, sounds, open at login, show in the menu bar, offer
  copied links, auto-updates): dm has each of them already, and many it
  has not (queues, schedules, proxies, sign-ins, site rules, host limits,
  a speed limiter).

Measured, the same files one after another:

- A local server holding each connection to 1 MB/s, 168 MB: dm 8
  connections 20.5 s (8.2 MB/s), 16 connections 10.1 s (16.6 MB/s), 32
  connections 5.4 s (31.1 MB/s); the other manager at its own setting
  (the downloader, 16) 40.6 s, 16.4 s of it waiting in its queue.
- A 100 MB file on a far server over this slow link: the other manager
  failed once at 16.6 s and completed once in 167 s (0.6 MB/s); dm on 8
  connections completed in 88 s (1.2 MB/s) and on 16 failed twice (362 s
  and 496 s, "the server did not answer in time"): that server leaves
  connections past its limit unanswered until dm's read timeout, and after
  dm's tries the row fails where fewer connections would have finished it.
  Row 44.
- YouTube, a 19 s video: dm 3.9 s whole with no the external video tool run; the other
  manager 45 s (7.6 s of the external video tool reading, 36 s in its queue, 1 s
  fetching). A 30 s part of a ten-minute 720p video: dm 5.0 s, 8 MB.

Also seen: its extension asks YouTube's player for the quality list in
the page's own world (`getAvailableQualityLevels`); dm's panel reads the
page's data instead, which gives the sizes too. Its the external video tool is refreshed
every four hours; dm refetches after a failure, within the hour.

## 2026-10-30: The maintainer's later asks on the other macOS manager, and what was done with each

Asked for, in turn: its window's close icons for the side and top panels
and its popup dialog; how it ships the external video tool, since dm's has given trouble;
a core as good as its or better; the way its extension gets a video's
qualities quickly, since dm's panel was slow to; and, last, to run a decompiler
on its app and implement its core and its extension in
dm. Also: Microsoft Edge alone for the browser checks.

- **A decompiler is not on this computer** (nothing at the paths rea and the
  shell look in), so that cannot run here; and decompiling its
  binaries to re-implement them would not be the clean-room work asked
  for earlier the same day. Its core is the external video tool, the downloader and ffmpeg driven
  by a Node program: there is no native engine in it to take, and dm's own
  engine measured faster (the entry above). What was taken is behaviour,
  observed: the items below.
- **The instant quality list.** Its extension asks YouTube's player, in
  the page's world, for `getAvailableQualityLevels()`. dm's panel now does
  the same (`document.js` answers a `dm-ask-qualities` message with the
  player's levels and its video id; `handoff.rs` lists the heights at
  once) and lets dm's reading, with sizes, replace that list when it
  comes. The contract's row B34 still ends with the sizes listed.
- **How it ships the external video tool.** Inside its bundle, the unpacked build, beside
  QuickJS, ffmpeg and the downloader, refreshed from its own manifest every four
  hours. dm now looks for an external video tool shipped in its own package
  (`videopage::bundled`) and the packaging fetches one (its file,
  the desktop workflow); dm's fetched copy still comes first, so the
  after-failure refresh keeps working. The earlier note that dm's packages
  distribute no the external video tool no longer holds for packages built that way; the
  Unlicense allows it, and the maintainer asked for it.
- **The core.** The one measured shortfall (16 connections failing on a
  server that holds extra connections open) is closed in the engine: a
  timed-out request steps aside like a refused one while others are
  served (`ranged.rs`, `busy`). The trade-off: on a link where timeouts
  are random, a download settles to fewer connections rather than
  retrying each; the range goes on either way.
- **Its popup dialog** ("Save download", "From Edge", Save As, Where,
  "Always save here without asking", Auto-Delete, Don't Download /
  Download): dm's Download details for a caught download now says the
  browser (the host names its parent process) and offers "Always save
  here without asking" (Options › Browser's switch, from the dialog).
  Auto-Delete stays row 48.
- **The close icons.** dm's View menu already folded the categories
  away; the toolbar now has the switch at its left end, as its window
  has at top left. Its right-hand inspector toggle has no counterpart in
  dm (the progress window is a window of its own).
- **Edge alone.** The check scripts take Microsoft Edge where it is
  installed; Chrome for Testing is fetched only without it.

## 2026-10-30: The reference work reviewed and stressed, and what changed

A review of commit 8772014 by five lenses (the part, the engine's step
aside, untrusted input, the window, the reader) found 23 candidate
defects; its adversarial verification and three of its four stress runs
stopped at a usage limit, so each finding was judged by hand against the
code. Fixed: the step-aside rule narrowed to a request that got no byte
while others did (a whole-link stall would otherwise have retired every
connection but one for the rest of the download) with the last connection
given another try instead of failing; the part's slices retried, checked
against `Content-Range` before reading, written beside their name, kept
on resume, its index arithmetic checked; MP4 alone for a part; sound
alone M4A first; portrait chips by the shorter side; the playlist reader
filtering unplayable and malformed entries and capping page size; the
host's `ps` by path. A guard added for a part asked of a plain file first
refused YouTube pages too: caught by running six real edge cases, and
placed after the probe. Left: the held-connections test still passes on
the old rule (it asserts the new one's count of held requests, which
the old would exceed, but a stricter shape needs paced served responses
at the timeout; it has them now), a forged page message can silence the
panel's "did not answer" note (harmless otherwise), and the packaged
the external video tool is not pinned to a digest in the repository (gaps row 50).
Network drop (25 s, 100 s) and a 45 s freeze of dm complete byte-exact.

## 2026-10-30: The external video tool out of the box, as the clean-room specification recommends

From `docs/clean-room-download-engine-spec.md`: the other macOS manager
ships the external video tool in its bundle, signed with its own Developer ID, and runs a
copy in its support folder that carries no quarantine mark. dm now ships
the pinned build in every package and copies it into its tools folder on
first use (`tools::adopt`, `ditto --noextattr --noqtn` on macOS, then any
quarantine attribute removed and its absence checked), running it from
there. Found while checking it: `ditto --noqtn` alone kept the quarantine
attribute, and the quarantined copy hung the download for four minutes;
and the shipped-copy lookup missed the build's own program name
(its file) beside the program. A Developer ID for dm stays the
maintainer's to buy; until then the copy is what keeps macOS from asking.

Retry timing: 15 s apart after the fifth try instead of 30, with 15 tries
by default instead of 10, so the give-up horizon stays near three minutes
while a returning server is found within 15 s. Existing settings files
keep the 10 they saved, which now outlasts about 90 s.

## 2026-10-30: The external video tool shipped as its zip, not unpacked

Found by packaging a real dm.app before the release: cargo-packager signs
every program in the bundle, and codesign refuses the external video tool's embedded
`Python.framework/Python` ("bundle format is ambiguous"), so the macOS
package failed. The package now carries the release's zip as it is
(its file), which codesign leaves alone and the bundle's
seal still covers; `tools::adopt` unpacks it into the tools folder on
first use (`ditto -x -k` on macOS, then any quarantine mark removed; the
zip crate elsewhere). The app is 75 MB (the dmg 62 MB) instead of about
150 MB. Checked end to end: the packaged dm.app, marked as an Edge
download and approved by a person (an ad-hoc-signed app downloaded by a
browser is killed until it is: the known limit without a Developer ID),
with nothing installed and fetching off, read a non-YouTube video page in
34 s from its shipped zip.


## "Delete file after" is the row's, and goes to the Trash (2026-10-30)

A finished file can be given a time after which it goes (10 minutes, an
hour, a day), as the other macOS manager's Auto-Delete does. Decided:

* It is set from the row's menu ("Delete file after ▸"), before or after the
  download finishes, and counts from the finish. Download details is left
  as it is: its layout is in `docs/behaviour-baseline/download-details.png`,
  and the row's menu reaches the same rows.
* The file goes to the Trash or Recycle Bin (`Platform::trash`, as Delete
  file does), never erased, and the row is removed with it. A file already
  missing, or one a later row has written over, is not touched; its row goes.
* A file that cannot be moved stays with its row, is said once in the status
  line, and is not tried again (the time is taken off the row).
* The window sweeps (`DmApp::delete_due_files`, from `logic`), so the time
  is kept while dm runs, hidden or not, to within its 30 s idle wake; a time
  that passed while dm was closed is acted on at the next start.

## The external video tool in macOS's own sandbox (2026-10-30)

the external video tool runs a site's player script, so on macOS it now starts under
`sandbox-exec` with a profile made per run (`videopage/sandbox.rs`): everything
allowed as usual but writing, which is allowed only in the run's staging
folder, the temporary folder, `~/.cache` and `~/Library/Caches` (the external video tool's
and Deno's caches) and `/dev`; the browsers' profiles, Safari's and the
system's cookies, the keychains, `~/.ssh`, `~/.gnupg` and the cloud logins
are neither read nor written, listed last so a writable folder cannot
uncover them. `sandbox-exec` is deprecated but present on every macOS; it
is tried once a run, and where it cannot make a sandbox the external video tool starts
plainly, as on Linux without bubblewrap. `DM_SANDBOX=0` turns it off.
Checked: a hidden file reads as not permitted, outside its folders nothing
writes, its staging writes; Dailymotion (HLS, through dm) and YouTube (with
Deno) downloads complete with both the Homebrew and the shipped build.

## One reqwest and one axum (2026-10-31)

`cargo tree --duplicates` listed crates twice, some from dm's own manifests.
What reached the shipped binary was reqwest: 0.12 from `dm-core` beside the
torrent library's 0.13. `dm-core` now asks for 0.13 built without a TLS
provider of its own (`rustls-no-provider`), and every client is handed the
set-up the stream protocols already use (`with_tls`, `rtmp::tls_config`):
ring and the public roots, as 0.12's `rustls-tls` had them, so which
certificates are trusted is unchanged. Release binary 24,687,328 to 24,405,792
bytes (275 KB less); duplicate crates in a normal build 66 to 64; the 14 real
sites pass. axum 0.7 was a test dependency only, beside the 0.8 the torrent
library brings: the tests moved to 0.8 (`{capture}` paths), one axum built.
`dirs` 6 comes from `auto-launch`, not from a manifest of dm's. Clean build
time was not measured.

## A second network path (2026-10-31)

Options › Network › Second network names an interface; every other
connection of a ranged download goes out on it (`second_path.rs`). The
client is dm's usual one, bound to the interface's address and, on macOS and
Linux, to the interface itself (reqwest's `interface`, `IP_BOUND_IF` /
`SO_BINDTODEVICE`), so it leaves by that network rather than the default
route. No share is planned: connections take ranges as they free up and
split the largest when none are left, so the faster path takes more on its
own. Off while a proxy is on: a connection around the proxy would show the
site this computer's own address. A chosen network that is not connected
is said once and the download uses one path.

## The probe's response kept as the first piece (2026-10-31)

The probe now sends `Range: bytes=0-` (`probe_get`) instead of `bytes=0-0`,
and its response body is threaded through to segment 0's first attempt
rather than drained. This reverses the decision of 2026-10-24 ("Weighed and
not done"), asked for by the maintainer on 2026-10-31 as part of matching
reference A's engine behaviour (the engine gap of 2026-10-31, now closed).

The Wikimedia concern (a server answering 200 to `bytes=0-`) is handled:
the probe reads the status code, and a 200 sets `resumable = false`, so the
download falls to one connection and the response body is that connection's
stream. A server that limits connections: the probe's response is kept
open only on the connection that carries segment 0; every other segment
makes its own request, the same as before. On a resume where segment 0
already has bytes on disk, the probe body is dropped and a fresh range
request is made. On a capped stream (HLS/DASH chunks), the probe body is
also dropped because each chunk makes its own small request. It is dropped
too under a speed limit when the file is planned into several ranges: kept,
the shown speed overshot the limit near the end of the file
(`tests/speed.rs`, `the_shown_speed_keeps_to_the_limit_from_the_first_second`);
with one range it is kept, so a limited small file still uses one connection.
The excess-bytes check (`DishonestRanges`) is skipped for the probe body only,
since it legitimately carries the whole file and is clipped at the range's end.

Found on the way, on Windows, with the same `HEAD`: the control port closed a
connection that had been idle for the authentication limit (5 s), because the
longer timeout was set on a clone of the socket and did not reach the
reader's own. It is now set on the reader (`control.rs`).

## Video from other sites comes from what the browser's player loaded (2026-11-01)

With the external video tool gone, a site other than YouTube is downloaded from the requests its
player made, which the extension hears (`webRequest.onResponseStarted`): HLS and
DASH manifests, whole media files, and, for Facebook's and Instagram's CDN
(`fbcdn.net`, `cdninstagram.com`), a file asked for in pieces. Checked live in a
visible Edge with no sign-in on Vimeo, Instagram, TikTok and X.

- A piece of that CDN is named by `bytestart` and `byteend`; the file is the
  address without them. Which pieces are the picture and which the sound is in
  the `efg` parameter's `vencode_tag` (`…dash_ln_heaac_vbr3_audio` is sound).
  Their size is not known from pieces, so the latest picture seen is the line's.
- A `<video>`'s own fetch of a small whole file is offered above 64 KB, not the
  1 MB floor of what a script fetches: X's GIF-style clips are the post's media.
- A stream picked from the panel carries the page's cookies for that download
  alone, as a video page's session does: TikTok's CDN answers 403 to a signed
  address without them (tested with every agent and referer; only a session
  was missing).
- The panel's hit-test accepts a layer of the player's own size up to eight
  ancestors out, not four: TikTok's and Instagram's overlays hang off a
  container five levels up and hid the panel. The size bound
  (`layer_fits`) still refuses a dialog over the page.
- An HLS playlist is named by the height in its address and a sound-only one is
  not listed beside a picture, so no line saves the sound alone.

## Each dialog a window of its own, and the vendored egui-winit (2026-11-01)

The maintainer asked (2026-11-01, "make the UI improvements in gaps.md"; the
request of 2026-10-31 that dm behave like reference A, whose dialogs are
windows of their own that a person drags anywhere on the screen) for the
dialogs to stop being drawn inside the main window. `Dialog::show`
(`dialogs/frame.rs`) now hands its body to `dialogs/apart.rs`, which opens it
in an egui immediate viewport, as `app/drops.rs` opens the drop target: the
system's title bar with the dialog's title and close button, placed over the
middle of the main window to begin with and left wherever the person puts it,
sized to what the body measures. Immediate, not deferred, because the dialogs'
bodies are closures over `&mut DmApp`. Where the backend has no window but the
first (`Context::embed_viewports`, which the test harness is), the dialog stays
a window in the main window, as it was, so `tests/window` keeps its tests. A
dialog that keeps the clicks to itself holds the main window through a layer
over it that egui treats as the modal one (`dialogs/mod.rs`, `hold_the_window`).
Closing a dialog's window, like Escape, closes that dialog and no other
(`Dialogs::close_window`, `state.rs`).

How the control port reaches them. `ui.click`, `ui.key` and `ui.type` are
how the QA and AI clients drive dm: events fed to egui as a person's pointer
and keyboard would. eframe 0.37 offers the app the first window's input
(`App::raw_input_hook`, `epi_integration.rs`) and no other window's: an
immediate viewport's input is taken by egui-winit and handed straight to egui.
So a dialog's own window could not be clicked. Weighed and not done: keeping
the dialogs embedded whenever a control client is attached
(`set_embed_viewports(true)`), which would have the QA check a look nobody
sees and make B42 impossible to check; and the system's own input
(`SendInput` and its kin), which is a different program for each system and
moves the person's real pointer. A plain setting does not do it; there is none.

So `desktop/vendor/egui-winit` is the published crate **egui-winit 0.37.0**, the
version eframe 0.37.0 depends on, licence **MIT OR Apache-2.0** (its
`Cargo.toml`; the published package carries no licence text of its own, the
project does: github.com/emilk/egui), used through `[patch.crates-io]` as
`vendor/winit` is. The one change is in `src/lib.rs` (search `dm:`): a
`pub static OTHER_WINDOWS_INPUT: OnceLock<fn(&mut egui::RawInput)>` before
`State`, and `State::take_egui_input` calls it with the input of every window
whose viewport is not the first, just before returning it. Its `Cargo.lock` is
left out, as a path dependency uses the workspace's. dm sets the function once
at start (`main.rs`) to `dialogs::apart::feed`, which gives a window the events
a control client queued for it (`app/human.rs`). Nothing else differs from the
published crate (`desktop/vendor/README.md`); carry the change over when eframe
is upgraded, or drop the copy when eframe offers a hook of its own.

Also found: eframe's glow integration answers screenshot commands for the first
window only, so a dialog's own window is pictured from the screen, over the
rectangle `ui.windows` reports (`scripts/human-qa.py`, `screen_region`), and
the Windows set of pictures for B25 is kept beside the macOS one
(`docs/behaviour-baseline/windows/`, the maintainer's approval of 2026-11-01).

Found by the maintainer's report that they still could not drag a dialog out of
dm's window (2026-11-01), and by dragging every kind of dialog with the system's
mouse (`SetCursorPos`, `mouse_event`) and reading its window back from Windows
(`GetWindowRect`, `GetParent`, `GetWindow`): a dialog window was put back over
the middle of dm's window whenever its measured size changed, so one whose body
changes (Properties, a progress window, Download details) was pulled back while
it was dragged. Placement is now latched once the size has settled (`Own::placed`).
Sizing had two faults of its own: a panel's `min_rect` is the whole room it was
given before anything is drawn, so a window could never measure smaller than its
first guess (the body's height is now read from the layout's cursor), and Options
fills a height it is given (`fixed_size`), which is now passed through
(`Spec::height`). A dialog that opens centred on a dm window that reaches past the
bottom of the screen is kept above the taskbar (`on_screen`). The control port
chooses, among words that two dialog windows share, the one in the window that has
the keyboard (`apart::windows`).

## Options: one Advanced page for the connection and the proxy (2026-11-01)

The maintainer asked for the UI improvements of gaps.md ("make the UI improvements in gaps.md") and
row 16 had waited for that word: the expert tabs behind one Advanced page. Options now has six tabs:
General, Browser, Folders, Downloads, Advanced, Sign-ins. Advanced is the old Network tab's body
under a "Network" heading, then the old Proxy tab's body under a "Proxy" heading, on one scrolling
page; every setting, its explanation line and the settings file's keys are as they were
(`dialogs/options/tabs.rs`, `tab_advanced`). Restore defaults on it puts back every setting of
both (`tab_settings(Tab::Advanced)`). The one new word, "Advanced", is in all thirteen
catalogs; the two sentences that sent a person to "Options › Proxy" (the proxy that could not be
reached, and the saved proxy turned off at launch) say "Options › Advanced" now, their
translations changed in the tab's name only.

Where OK refuses a proxy (no host, an address where the host goes, port 0) Advanced is opened at
its proxy section (`OptionsDialog::to_proxy`, consumed by the page with `scroll_to_cursor`), where
the Proxy tab was selected before. Behaviour contract row B43; tests in `tests/window/options.rs`.

Not changed, as they are other crates' words: the English notices of dm-core and dm-shell that
still say "Options › Proxy" or "Options › Network" (`download/mod.rs`, `live.rs`, `shell.rs`);
they are shown to a person only through the window's own sentences above, or as they are.

## The YouTube challenge scripts stay fetched from their project's release page (2026-11-01)

Replaced on 2026-11-08: "YouTube's checks are answered as reference A answers them" below. The
scripts are no longer fetched, and the weekly pull request is gone.

dm reads YouTube itself (`youtube.rs`). To answer the signature and speed-limit checks in YouTube's
player it runs two small scripts in QuickJS or Deno, pinned by version and digest (`SOLVER_VERSION`,
`SOLVER_LIB`, `SOLVER_CORE`) and fetched once from the release page of the project that publishes
them; the address is the one place the old external video tool's name is left in the repository.
dm does not run that tool.

The maintainer chose on 2026-11-01 to keep it so. Copying the scripts into dm's own release assets
would still need the same attribution and the same upkeep, and a script that no longer matches the
player is the most likely reason YouTube stops working. The way to notice is a weekly run of
`scripts/real-sites.py` (a real YouTube video); the cure is a new version and digest. Writing dm's own
solver was declined as upkeep (the YouTube proof-of-origin gap, gaps row 37, is the same kind).

### The pin is kept current by a weekly pull request (2026-11-01)

`.github/workflows/solver-update.yml` runs every Monday (and on demand): `scripts/update-solver.py`
reads the project's address out of `youtube.rs`, asks its releases for the latest tag, and if it is
newer downloads both scripts, refuses anything that is not a script, works out their SHA-256 and
rewrites `SOLVER_VERSION` and the two digests; the offline YouTube tests run with the new pin and a
pull request is opened. It never merges: the offline tests use stand-in scripts, so a person runs a
real video (`SITES=youtube python3 scripts/real-sites.py`) before merging, and a checksum only
protects if somebody looks at the change. `scripts/test-update-solver.py` tests the script against a
stand-in releases server (a newer release, the same or an older one, a missing script, an error page
in place of a script, a server that is down, a changed layout of `youtube.rs`).

The repository needs "Allow GitHub Actions to create and approve pull requests" switched on in
Settings > Actions > General; a pull request opened with the workflow's own token does not start the
other workflows, which is why the YouTube tests run inside this one. A failed run (the release
page unreachable, a script that is not one) is the signal that something needs a person.

## Secrets on Windows are sealed under a key Credential Manager keeps (2026-11-01)

A row's login and its browser cookies were written whole to `secrets.json` beside the queue file, readable
by this account alone, so a copy of the home carried them in clear (gaps row 39). A Windows credential is
limited to 2560 bytes and a cookie jar is not, so the file is sealed instead (ChaCha20-Poly1305, a fresh
nonce on every write, `secretstore.rs`) under a random 32-byte key that is the credential, named
`dm:queue-secrets-key` and scoped to this computer's profile. A file written in clear moves into the sealed
form the first time it is read where the store works; where it does not (an error from the API, the
store switched off by `DM_NO_SECRET_STORE`, any other system) the file is exactly what it always was and a
notice says so. A file that cannot be opened (a copy made on another account) is not guessed at: the
downloads that had logins or cookies ask for them again. Not done: the Sign-ins' and the proxy's passwords
in the settings file, and the macOS and Linux stores (the row says why).

## Site rules are served by the app and added to the extension's tables (2026-11-01)

A fix for one site (a host to leave alone, a request whose answer names the stream) used to need a new
extension in the browser stores, because the tables were compiled into it (gaps rows 68 and 69). The hello
now carries an optional `rules` document, `Rules` in `dm-protocol`: `format` (how it is written, 1) and
`revision` (how new it is), the lists `exclude` adds to `excluded.rs`, and `sites`, each with the requests
whose JSON answers hold streams (`url_contains`, and the `keys` whose strings are addresses). The app serves
the newer of `dm-shell/resources/rules.json` (built in) and a `rules.json` in dm's home, so a fix can be put
in place by a file and not a release. Decisions:

* Additive only. The extension adds what it is served to its own tables and nothing served removes a built-in
  rule, so a missing, older, oversize (256 KiB, one native-messaging frame holds 1 MiB), malformed or
  other-format document costs nothing: the extension decides as it was built to.
* The served rules are kept in the extension's storage (`dm.rules`) and put back when the worker wakes, since
  waking is when a download arrives and asking the app takes longer than a small download.
* The page hook reads only JSON, at most 1 MiB, only for requests the rules of the page's site name, and
  the extension re-checks the site and the request before it finds anything, so a page cannot make the
  extension read an answer the rules do not name. What counts as a stream (an `http(s)` address ending in a
  media extension, not on the never-caught list) is decided in Rust (`decide/answers.rs`), not in the page.
* The shipped document is empty. The profiles are written from what the real pages show at the final QA; guessing
  at sites' answers from memory would add rules nobody checked. Not done: carrying `rules.json` in the app's
  signed update manifests.
* Considered and not chosen: serving the document through `settings.get`. The hello is answered first and once per
  handshake, and the decisions that need the rules (the never-caught list) run before the settings arrive.

## 2026-11-01: Site rules ride in the update manifest, checked by digest

`latest.json` may carry `rules`: `{url, sha256, size}`, the address under dm's own releases, size at most 256 KiB. The weekly or manual update check fetches it, requires the exact size and digest, parses it as `Rules` and keeps it as `rules.json` in dm's home only when its `revision` beats what is served (the shipped document is the floor); anything else is passed over silently and the check itself does not fail. Trust rests on the manifest being fetched over TLS from the project's releases plus the digest; this tree has no separate signature check on `latest.json` to lean on (the 2026-10-25 entry describes one), so none is claimed here.

## 2026-11-01: The Linux AAC decoder is written but gated off until its tables are supplied

`dm-core/src/aac/` holds everything of an AAC-LC decoder that is logic (bit reader, config, frame syntax, dequantisation, pulses, mid/side, TNS, FFT-based IMDCT, windows, overlap-add), tested against the defining formulas. It holds none of the standard's Huffman books (121 scale-factor and eleven spectral codebooks) nor band layouts beyond 44.1/48 kHz: they could not be reproduced reliably, copying them from another decoder is not allowed, and a wrong code word yields noise with no error. `tables::COMPLETE` is false, so `aac::decoder_ready()` is false and Linux offers no MP3 exactly as before. Dropped: guessing the tables, and returning a short or silent file. To finish: supply the tables from the standard, set `COMPLETE`, decode `sound.f140.m4a` and compare with a reference decode.

## The settings passwords go to the same store; macOS and Linux reach theirs through the system's tools (2026-11-01)

The Sign-ins' and the proxy's passwords (gaps row 39) are kept in memory as before; `Settings::save` puts each
into the store under its own name (`dm:login:<home>:<host>\n<user>`, `dm:proxy:<home>`, the home being the
short hash `secretstore::namespace` gives) and writes the field empty, and `Settings::load` fills an empty field from
the store. A password still in the file in clear is moved in on its first read and the file written again. A
store that refuses leaves the password in the file, as before, with a notice. A login removed in Options leaves
its stored password behind (no listing of the store); an emptied password is deleted. macOS uses `security`
and Linux `secret-tool`, started as processes (no new crate, no linking to libsecret, which a build on CI would
need): a value is base64 text; `secret-tool` takes it on standard input, `security` only on its command line,
which is said in the code. A missing tool is no store. Written without a Mac or Linux computer to compile or
run on: proved by hand at the final QA.

## Page renditions are read by one scan, not by a reader per site (2026-11-02)

The panel's list of qualities adds the renditions a page names in its own data. The first
three readers (Facebook, Instagram, TikTok) were each written for one site's field names;
with a thousand sites that cannot be kept up. `decide/page_lines.rs` now names no site: it
finds the strings of the page's scripts that hold a file the player fetched, takes the
innermost list of objects around it as the renditions, reads each entry by the names of its
fields (an address, `*height`, `*bitrate`/`*bandwidth`, a short `quality`/`label`), reads any
string holding a DASH manifest for its representations, and looks in the objects around for
the video's other lists. The old per-site tests pass unchanged, and a made-up site's shape has
one of its own. Dropped: a table of site profiles (needs a release for each site).
Known limits: field names with none of those words (a height named `h`), a list kept outside
a script (a `data-` attribute), and pages that hold no list at all (the list then shows only
what the player fetched).

Added 2026-11-02 (Facebook's feed showed no list): the scan also reads what the page's own
answers said. A feed loads each video's renditions through its API after the page, so the page's
markup never held them (the file the player fetched appeared nowhere in it). `extension/document.js`
keeps, on every site, the text of each fetch or XHR answer (text, JSON, XML; at most 2 MiB each) that
mentions `.m3u8`, `.mpd`, `<MPD`, `.mp4` or `.webm`; `content.js` holds the newest 40, at most 12 MiB,
in memory; the panel's list scans the page and then those answers, newest first. They are sent nowhere.
Tried and dropped: offering the page's one stream when the player has fetched nothing (on a feed it
was another video's: a 2-hour file landed), so a stream is offered only with a file the player fetched.

## EDM-V1 and EDM-E1: what was built, and what was not (2026-11-03)

Checked against the code before writing any: most of EDM-E1 already exists. Midpoint halving of the
largest range is `ranged::split_largest`; a connection far slower than the rest is dropped and
asked again (`window.stalled`, a coarser rule than the spec's: under 1/64 of the whole download's
bytes over a window, not an EMA below 0.15 of the peers' for 3 s, so Part 2 is only partly met); a first retry after a cut is at once (`retry::quick_nap`); the seam
is compared over 64 bytes (`LEAD_BYTES`, row 96); a zip's directory is read from the file's tail
(`zipinfo.rs`). Not built, with the reason:

- **Part 8, HTTP/2 multiplexing.** The client is pinned to HTTP/1 (`lib.rs`, `http1_only`) on
  purpose: over h2 the ranges share one connection, one congestion window and one throttle, so
  eight ranges download at the speed of one. The spec asks for that to be undone; it would make
  the multi-connection feature do nothing on most CDNs.
- **Part 6, full pre-allocation (`fallocate`, `SetFileValidData`).** The staged file is sparse
  (`set_len`); allocating every block up front costs the whole write before the first byte arrives
  and `SetFileValidData` needs a privilege and exposes old disk contents. Not a gain.
- **Part 3, size tiers.** Contradicts row 95 (a 100 KB file on 4 connections, measured on reference
  A); the floor stays at 32 KiB.

EDM-V1: built are the page hook (`document.js` now relays manifests, playlists and whole media
files on any host, not only googlevideo, leaving segments and byte ranges out; the MediaSource and
`URL.createObjectURL` calls are observed, their codecs kept in `content.js` and read by nothing yet)
and the shipped `rules.json` (revision 2) with answer rules for Facebook, Instagram, Vimeo, TikTok,
X/Twitter and Reddit, in the document's real shape (`sites[].answers[]`; the spec's `profiles` schema
is not the one the code reads). None of the site rules has been run against a live site (rows 62, 63
and 71: logged-out pages refuse an automated browser). Not built: the `AdaptivePair` stream kind, the
itag table (YouTube's heights come from the player response, not from itags), the extra headers in
`Start`, and the `RefreshStream` request; `Entry::fresh_link` / `adopt_fresh_link` already cover the
expired-address resume (row 57).

Row 111 closed 2026-11-06 without new test files: the regression guards the row asked for already
exist. Halving and stealing are `tests/speed.rs` (`one_slow_connection_does_not_hold_the_download_up`,
`a_download_paused_after_its_ranges_were_halved_resumes_whole`), the 64-byte seam
`tests/seam_check.rs`, and the zip tail `tests/zip_contents.rs`.

## The worker checks its WebAssembly against a digest built with its code (2026-11-03)

A browser keeps an unpacked extension's worker scripts (`background.js` and wasm-bindgen's glue)
from when the extension was loaded, but reads `pkg/dm_catcher_bg.wasm` from the folder each time the
worker starts. A rebuild of `extension/pkg` in place therefore paired old glue with new WebAssembly,
which failed on its first call ("wasm.__wasm_bindgen_func_elem_1846 is not a function", gaps row
114). `package.sh` writes the WebAssembly's SHA-256 into `pkg/build.js`, which the worker imports
beside the glue, so the two are cached together; the worker fetches the bytes itself, compares, and
on a mismatch reloads the extension, which loads glue that matches. At most once in two minutes
(`dmReloadedAt` in `storage.local`, which a reload keeps): a folder caught halfway through a
rebuild cannot set off a loop. Tried and dropped: naming the file by its digest, which would do the
same with a 404 but changes a name seven scripts look for. The page's panel needs none of this: its
glue and WebAssembly are one file (`pkg/panel.js`).

## Reference A's extension, rows 116 to 119 (2026-11-04)

* **Headers (116).** The worker listens at `onSendHeaders` (read only) and keeps the last request
  headers per address in memory, never in storage or a log. They travel in `Start.headers`; the host
  and the queue filter them again (`dm_core::cookies::forwardable`: nothing dm sets itself, no
  `Cookie`, bounded), keep them with the row's secrets like cookies, and send them only to the
  row's own server (`SignIn::headers_for`), so a bearer token never follows a redirect or a mirror.
  `Authorization` is forwarded here although `Headers::of` (the stream hints) drops it: those hints
  come from a playlist, these from the browser's own request for the same file.
* **Answers (117).** The page hook no longer clones every answer. It wraps the `Response` reading
  methods and the `body` getter (a `tee()` for answers that may be text), and XHR `json`,
  `arraybuffer` and `blob`; bytes are decoded only when the first 64 are text.
* **Finders (118).** A small selector-chain and property-path language from `rules.json`, evaluated
  read only in the page's world (32 steps, 5 ms). The id is asked at the panel click; the lines come
  from the answers already kept (`page_lines_for_player`). No site's finder is shipped yet.
* **Pairing (119).** `ranged_of` reads any site's byte-range pieces by `Content-Type`; a lone one
  is offered as a whole file, so a site with video only loses nothing.
* **Not built: renewing (121).** The app cannot ask the extension anything (the native host exists
  only while the browser holds a port), so there is no way to ask for a newer address.

## Media types and adverts audited against reference A (2026-11-04, rows 126 and 127)

* **Added:** a playlist or manifest recognised by its type alone (`application/octet-stream-m3u8`,
  `video/vnd.mpeg.dash.mpd`), and `audio/mp2t` as a segment, not a file.
* **Not offered, on purpose:** HDS (`video/f4m`, `video/f4f`, `Seg1-Frag1`): Flash-era, neither a
  file; it is now kept out of the list. Subtitle types (`text/vtt`, TTML, DFXP) are not media.
  `application/vnd.yt-ump` is YouTube's, read from the page by `youtube.rs`, not from the list.
* **Adverts:** already excluded by host, path, `Server: DCLK-AdSvr` and a Windows-media stub of
  100 KB or less (`excluded.rs`); an MP4 loaded as an image is never heard, since the worker does
  not listen to the `image` type. Tests added for each.

## The made-up `Referer` is dropped for a server that refuses it; `Retry-After` is a floor (2026-11-04, rows 133 and 134)

Found by running reference A and dm on the same real sites (`docs/gaps.md` row 132).

* **`Referer`:** dm keeps sending the file's own folder when no `Referer` was given (reference A does
  the same, and some hosts want one). Only when the probe is still refused with 403 after the agent
  ladder is it asked once more with none; if that is served, the server (host and port) is
  remembered for the session and every later request to it goes without. A `Referer` that came with
  the download (the browser's page) is never dropped. Tried and dropped: never sending one, which
  would lose the hosts that want it.
* **`Retry-After`:** the server's value is the least wait, and dm's own wait still grows under it,
  doubling past the 15 s a dropped connection is held to, up to the 60 s ceiling as before.
  Wikimedia's limiter answered dm each second for minutes and let a request through only after
  about 20 quiet seconds; following the header exactly cannot get past one. Checked by
  `tests/errors.rs` and a unit test in `retry.rs`. Not re-checked live: after the day's
  side-by-side runs Wikimedia answered this computer `Retry-After: 600` with a request to be less
  disruptive; dm then showed "Too many requests (retry n …)" and asked once a minute. Do not
  re-test against Wikimedia's originals in a loop.

## Measured and not changed: FTP's sign-in, the ramp after a restart (2026-11-04, rows 138 and 139)

* **The 11.7 s against 8.2 s for GNU's 1 MB file over FTP** was mostly the harness: `sites.mjs`
  sampled connections with a blocking PowerShell call each 400 ms, which held Node's loop and added
  seconds to both apps' times. Traced without it, dm took 7.3 s on three runs; it signed in twice
  (once to describe the file, once to fetch it) where curl signs in once (7.4 s). Changed: the
  describing session fetches the first range; about 6 s now.
* **The slow ramp after a hard kill** was the server: Hetzner's Ashburn mirror swings from 5 MB/s to
  16 KB/s within a fresh download as well. On a local server holding each connection to 256 KB/s,
  dm was back at full speed in the first second after a restart (1.5 MB, against 262 KB in a fresh
  start's first second). Not changed. Use a local steady server (`D:\compare\steady\steady.mjs`)
  for engine timings; real servers only to see that a thing works.

## dm works exactly like reference A, with its labels (maintainer, 2026-11-04)

Asked, in the maintainer's words: "I want to clone" reference A; "it should exactly work same";
"if the user from" reference A "use our app user should be comfortable"; Windows first, macOS and
Linux tested later; Microsoft Edge for testing. Answered the same day to three questions:

* **Approval:** "It should work exactly like" reference A. This is the approval
  `docs/behaviour-contract.md` asks for, for every change that makes dm behave as reference A does
  (the link chooser, File Info before a panel download, the quality list, connections on small
  files, a pasted page saved, the queue drop-downs, the Options controls). Each such commit names
  this entry and edits its contract row and `docs/behaviour-baseline/` in the same commit.
* **Labels:** reference A's own words for menus, buttons, categories and columns (a Tasks menu,
  Delete, Delete Completed, All Downloads, Unfinished, Finished, Grabber; the Q, Last Try Date and
  Description columns), in every catalog. Its icons, logo and artwork are still never copied.
* **The window:** dm's own (Rust and egui), reshaped to reference A's layout; no WinUI rewrite.

## The window as compact as reference A's (maintainer, 2026-11-04)

Asked: "the opening screen size, font size and overall compactness" of dm and reference A "should be
same. It should not look extremely big." Measured on this computer (1366 by 768, 100%): reference A
opens 800 by 458, its text the system's 9-point face (about 12 pixels), its list rows 17 apart, its
tree about 145 wide with items 16 apart, its toolbar 66 tall with every button named (a long name
shortened, "Delete C…"). dm opened 962 by 692, past the bottom of the screen, with 14-point text,
rows 28 apart, tree items 32 and a tree 230 wide; at that width its toolbar fell to pictures alone.

Now (`theme::BODY` 12, `CAPTION` 11, `HEADING` 16, `CONTROL` 22, `NAV_ITEM` 22, `ROW` 20): a new
window 820 by 500; the tree from 170; toolbar buttons with their name under the picture at most 64
wide, a longer name shortened with "…" and whole on hover; columns in reference A's widths, so its
eight fit. The larger text sizes in Options › View still scale all of it.

## Maintenance pass: one settings reader, lib.rs by concern, borrowing over copying (2026-11-05)

Asked: code easy to maintain, without changing what dm does; then a performance review on three
rules (borrow in parameters, no copy before the end, references out where only read).

* **One settings reader.** `Settings::read_only` (the window's lenient reading and its upgrades, no
  file moved, no secret asked) is what the browser's host reads; it read strictly, so one odd field
  put every default in force there while the window kept the rest.
* **`dm-core/src/lib.rs` by concern** (1214 lines to 397): `error.rs`, `files.rs`, `http_client.rs`,
  `agents.rs`, `processes.rs`, each re-exported whole, so every path (`dm_core::Error`,
  `crate::lock`, `dm_core::agent_for`) is where it was.
* **CI off Node 20**: checkout v5.0.0, upload-artifact v6.0.0, download-artifact v7.0.0, the first
  of each on Node 24 (later majors not taken: more change than the runtime).
* **Borrowing over copying**, found with clippy's `needless_pass_by_value`, `ptr_arg`,
  `redundant_clone`, `assigning_clones`, `needless_collect`, `unnecessary_to_owned` on top of the
  usual set: parameters only read now borrow (`add_sized`'s options, `remember_folder`'s folder,
  `open_add_url`'s clipboard, the MCP relay's params, a font's bytes, a page's player id, the
  journal's changed rows); fifteen `x = y.clone()` reuse `x`'s buffer (`clone_from`); clones of
  values not used again are gone; `queues()` hands out its cached list (`Rc<[String]>`) where it
  copied every name several times a frame, and the window's read-only users of the visible rows
  share the table's list (`visible_shared`) rather than copying it.
* **Left as they are, on purpose:** `mp3.rs` collects its threads' handles before joining them (not
  collected, the threads ran one after another); `Arc::clone` spelled `.clone()` (no cost, a
  matter of style); a `web_sys` event or `JsValue` taken by value (that is how wasm-bindgen hands
  them over, and a clone is a handle); `tree_action`'s place (used after it is stored); `Spec`
  (copying it is free; deriving `Copy` only quiets the lint).

## The Scheduler by queue, and the queue lists on Start Queue and Stop Queue (2026-11-05, gaps rows 79 and 90)

Built at the maintainer's word that dm "should work exactly like" reference A (2026-11-04), as
contract rows B64 and B65.

* **The Main queue's hours are the shared ones.** The settings already had one schedule for
  every queue (`schedule`) and an entry per named queue (`queue_schedules`), a queue with none
  following the first. The Scheduler shows that as it is: the Main queue edits `schedule`, and
  any other queue either follows it ("This queue follows the Main queue's hours.") or is given
  hours of its own. An entry for the Main queue itself, which only a hand-written file can
  have, is not shown and so not kept.
* **Once on a date** is `once_at` with the date typed and the time taken from Start at, so a
  person types one time, not two. The days are greyed then: `open_now` does not read them for a
  once-only run.
* **"Force processes to terminate" is not offered.** `Schedule::force` exists in the settings
  but nothing acts on it; a switch that does nothing would be worse than none. It waits until the
  window's close stops processes.
* **Start now and Stop are Start Queue and Stop Queue for the queue chosen.** Start now does
  not open a closed schedule: it starts what the queue's hours allow and says when it waits for
  them, as Start Queue does; the per-row Start now (a held row's) is how a person goes past them.
* **Side by side where there is room.** The queues sit at the left of the hours when the window
  leaves 700 points for the dialog (it then opens 660 wide, as a body that scrolls keeps the
  dialog's least width and the days ran past its edge); at large text or in a small window the
  queues stand above the hours. A refusal under a body that scrolls is scrolled into view.
  The choice is made once, as it opens: a dialog that is a window of its own reads its own size
  in its own frames. The line between the two is drawn as tall as they are: a separator in a
  row takes all the height it is given, which in a window of its own is the window's, and the
  window grew to it frame after frame until it had to scroll, then shrank, and over again,
  so that Cancel moved under the pointer (found by `scripts/human-qa.py`, B50, B56, B65).
* **The toolbar's arrow is a second button beside the first**, not a split button drawn as one:
  egui has none, and two plain buttons keep each one's click and name for a screen reader
  ("Choose the queue to start"). Its tooltip is not shown while its list is open: it lay over
  the list and took the pointer from the first item.

## A BitTorrent client's features: peers, files, a blocklist; slower hours; sound files (2026-11-05, gaps rows 144 to 148 and 82)

From the BitTorrent client's feature list the maintainer supplied (its name left out, as for the
reference apps); the sound files are reference A's.

* **Peers: connected and known, no seeds.** librqbit counts peers by state (`live`, `connecting`,
  `queued`, `dead`) and every address found this run (`seen`), not which of them have the whole
  file, so the progress window says "Peers: {} connected, {} known" and claims no seed count. The
  upload speed is measured from `uploaded_bytes` between samples, as the download speed is.
* **A torrent's files are chosen while it is stopped.** librqbit can change `only_files` on a
  running torrent, but the queue owns the session's life (a pause ends it), so the choice is
  kept on the row and given to `add_torrent` at each start; Properties greys the ticks while it
  runs. A file left out is deleted from the staging folder before the rest is moved: pieces it
  shares with a wanted file leave it partly written, and moved it would look fetched.
* **The blocklist is fetched by dm, not librqbit.** librqbit's own fetch would go around dm's
  proxy, so dm fetches a list on the web with its own client, keeps it a day beside the parts
  (named by a hash of its address) and hands librqbit a `file:` address. A list that cannot be
  read stops the torrent with the reason: going on unprotected is not what was asked for. One
  address on a list does not cover a peer's others: on this computer local peer discovery found
  the test's seeder on the network address as well as on loopback.
* **Slower hours every day, applied by the window.** The limit in force is worked out from the
  clock (`Settings::speed_limit_bytes_now`) when settings are applied and at each refresh of the
  window, which puts the other limit in when the hours begin or end; with nothing moving a refresh
  can be up to half a minute away, so the change can come that late.
* **Sound files per event** are kept in the `sounds` map the engine already read (`complete`,
  `failed`, `queue_started`, `queue_done`); an emptied field removes its entry, so the usual
  sound is back. Only `.wav` is offered in the chooser, as only `.wav` is promised.

## Who opens magnet links: shown, taken at a click, given back (2026-11-05, gaps row 106)

* **Read from Windows' keys, beside the window.** Options › Browser asks `reg.exe` four
  questions as the page opens (each takes a moment), on a thread of its own. Windows' own
  choice (`UserChoice`, set in Default apps) is asked first: it decides over every program's
  registration and no program may write it, so dm says so and offers no button there.
* **dm takes them only at a click, and keeps what it replaced.** At start dm still writes its
  keys only where nobody has them (row 100). "Use dm for magnet links" is the person's own
  choice to replace another program's: that program's key is first written out with
  `reg export` to `magnet-before-dm.reg` in dm's home, then removed, then dm's written. "Give
  magnet links back" removes dm's key and imports the kept one. The same for `.torrent`, whose
  extension key is kept and dm's ProgId removed on giving back.
* **dm does not appear in Default apps.** That needs a `RegisteredApplications` entry with
  `Capabilities` (spec T5), a change to how dm shows in Windows, left for the maintainer's word.

## Choices one size (maintainer, 2026-11-05)

Asked: the toggle switches were bigger than the rest, and the controls "should be consistent".
Check boxes and radio buttons kept WinUI's 20-point box, and the switch its 40 by 20 pill in a
36-point row, sizes made for WinUI's 14-point text; the window has had 12-point text and
22-point controls since 2026-11-04. Now `theme::CHOICE` (16) is the box, the ring and the
switch's height (32 wide, its row `CONTROL` + 6), and the theme picker's buttons are a control's
height with their 16-point drawings. The Windows baseline pictures were taken again from the QA
run that showed the change (Add URL 8 points shorter: its sign-in switch's row); contract row
B25 says so.

## "Download links" as reference A's, and where dm keeps its own way (2026-11-05, gaps row 94)

At the maintainer's word that dm "should work exactly like" reference A (2026-11-04).

* **Hide images and Hide repeated files are on**, as reference A's are. A picture is told by the
  name its server gave, or before it answers by its address (`classify`).
* **A repeat is an address the page gives more than once**, the first kept. Reference A also
  counts one already downloaded; dm already shows such a link unticked and says "(already in the
  list)", which tells the person more than a missing line, so that stays.
* **Save To's one category** is any of the kinds or a person's own categories; the folder it
  names is the one dm would sort that kind into. One folder must be named before Start now or
  Add for later take it.
* **Edit is a button on each line**, opening the link's name and address under the list: egui's
  grid has no row to select for one Edit button below it, as reference A's list does.
* **Link texts come with the links**, as a map of address to words (`Batch.texts`), not a list
  beside the links: the browser and the host each drop links (a fragment, a page's own address,
  what dm cannot fetch), and a map stays right where two lists would fall out of step. Words are
  a link's text, else its title, an image's `alt` or title, spaces closed up and cut to 200
  characters; an older extension sends none and the column is empty. With the dialog off they
  are the queued links' descriptions too, as the dialog keeps them by default.

## A download that is a form's answer is fetched by sending the form once (2026-11-05, gaps row 128)

* **Only the answer itself.** The extension keeps the body of each form a page sends (main frame
  and frames, two minutes, twenty at most, 64 KB at most). A download is given its form only
  where it comes from the form's very address with no redirect: a form answered by a redirect
  leaves the file at an ordinary address that a GET fetches, as before.
* **Sent once, never resumed.** The form travels with the download's sign-in (`SignIn::posting`)
  to its one address, so every request builder sends it as a POST (`download::with_form`); the
  probe asks once, with no range and none of its second tries (each would send the form again),
  and the answer is fetched whole on one connection. A failure starts over with the same form,
  as the browser would.
* **Not asked about first.** The host's check that dm can fetch a caught download would send the
  form, so a form's answer is taken as a link that works once is.
* **Fields re-encoded.** A form the browser reports as fields is sent as
  `application/x-www-form-urlencoded`; a raw body goes as its bytes with the type the browser's
  request named. A form uploading a file (multipart with a file's bytes) is not reproduced.
* **In memory only.** The row keeps the form in memory and writes only that it is a form's
  answer; after a restart it fails saying to start it again from the page.
* **Fields in the browser's order.** Chromium reports a form's fields as a dictionary sorted by
  name (`format=csv&report=7` for a form written `report`, `format`), so dm sends them in that
  order. A server reads a form by field name; one that checks the body's bytes (a signed body)
  would refuse it. Reading the form in the page as it is submitted would keep the order, at the
  cost of a listener on every page's submit.

## A proxy configuration script is asked once per site and fails closed (2026-11-05, gaps row 80)

* **Asked inside the request, waited for on a thread of its own.** reqwest's custom proxy is a
  synchronous callback on the shared client, so the script cannot be awaited there. The first
  request to a site (`scheme://host:port`, a redirect's new site too) fetches the script and runs
  it in QuickJS on a thread with a runtime of its own, and the answer is kept for the session
  (`pac::way_to`). On a runtime with several threads the wait is `block_in_place`; elsewhere the
  thread waits, which nothing the script needs depends on. Asking before the first request instead
  (the plan of 2026-11-03) missed redirects to other sites and every request made outside a
  download (the video page reads, the probe of Add URL).
* **The first way only.** A script's answer lists ways to try in turn; dm takes the first. Trying
  the next on failure would need a client per way.
* **No path, kept per site.** The script is asked about `scheme://host:port/`, as browsers ask
  about secure addresses: a path can carry a session, and the answer is kept for the whole site.
* **Fails closed.** A script that cannot be fetched or read, fails, runs past five seconds or
  answers nothing usable sends the request to an address nothing answers (`pac::NO_WAY`), so it
  fails, and the row says the script's trouble in place of "refused" (`pac::refusal` in
  `error::network`). Never direct.
* **Fetched directly.** The script's own address is fetched with no proxy, as browsers do: it is
  what says where the proxy is.
* **The tools take its answer.** `url_for_tools` gives `script:<address>`, and `proxy::route_for`
  asks the script about each site where the address is used (FTP, a torrent, the video page
  reads, the YouTube reader), so they go its way too, and a site it gives no way to is refused.
* **The login goes with it.** The proxy section's user and password go to the proxies a script
  names: a script names no login, and the proxy that needs one is the usual case.
* **It decides over the proxies above it.** Filled in, the script chooses in place of the host and
  port and the per-protocol proxies; the bypass list still sends its sites directly.

## A height at two frame rates is two YouTube lines (2026-11-05, gaps row 72d)

* **Two lines only where both exist.** A height with a picture above 30 frames a second and one at
  or below gets a line for each, the faster first ("720p MP4 60 fps", then "720p MP4"); a height at
  one rate keeps its one line and its old quality word, so a hand-off from an older list means what
  it meant.
* **The rate in the quality word.** `720/mp4/60` and `720/mp4/30`: a third part read by
  `Quality::frame_rate_of` into `Options::frame_rate`, which keeps `youtube::choose` to that side
  of 30. A word with no rate still takes the fastest picture, as before. Where the rate asked has
  no picture at that height (the page changed), the other is fetched rather than nothing.
* **The rate said is the picture's.** "60 fps" is the format's own rate (50 for a 50 fps video), as
  reference A says it.

## Windows' firewall question is said, not answered (2026-11-05, gaps rows 101, 107)

* **Told from Windows' own rules.** The question at the first torrent leaves a firewall rule naming
  the program, allowed or blocked, in `HKLM\...\FirewallPolicy\FirewallRules`, which a person's
  account can read. No rule with `App=` dm's own path means Windows has not had an answer: the
  row of a torrent finding peers says Windows may be asking, until there is one
  (`dm_shell::firewall::may_ask`, read on a thread of its own every few seconds while asked).
  Looking for the question's window by its title was the other way: its title is in the
  system's language and its owner is not dm.
* **Never answered for the person.** Adding a rule needs administrator rights and is the
  person's choice; dm says where to change it later (Windows Security, Firewall) and that it
  still downloads without it.
* **The magnet chooser likewise.** Windows' own "how do you want to open this" at a first
  magnet click is said in Options beside who opens magnet links; dm cannot answer it.

## The browsers dm takes from, and its menu items, are chosen in the catch rules (2026-11-05, gaps row 86)

* **The extension knows its browser; the app does not.** Native messaging tells the host an
  extension's id, which is the same in Chrome, Edge and Brave for one store. The extension names
  its browser from `navigator.userAgentData`'s brands (Firefox from its own URL scheme) and checks
  it against `CatchRules::off_browsers`, which the app sends with the rest of its rules.
* **An unticked browser keeps its downloads, not its menus.** Its right-click menu and the
  video panel still hand things over when asked, as with catching switched off (B31).
* **Menu items by id.** `dm_protocol::MENU_ITEMS` names them for Options and is checked against
  the extension's own list by a test; the extension makes its menu again when the rules it is
  sent leave out other items than the ones it kept.
* **The panel's buttons.** Reference A's panel has buttons to choose from; dm's has one (Download
  video) and its close, and the panel itself has its switch (`video_panel`), so there is nothing
  more to choose.

## Caption files are list lines of their own (2026-11-05, gaps row 72c)

* **YouTube: the page's tracks, as TTML.** Each track the watch page's player data names is a
  line ("TTML file, EN subtitles", "(ASR)" after the speech-recognised one, as reference A says
  it); the line's quality word is `caption/<language>[/asr]` (`Quality::caption_of`), which the
  runner turns into `Options::caption`, and the reading is that one track's address with
  `fmt=ttml`, a file named after the video and the language. Only YouTube's own caption address
  is taken. TTML because reference A saves TTML; the "Subtitles" setting that puts WebVTT into
  the video stays as it was.
* **Any site: the caption files in the video's record.** The page lines read the record around
  the video's files for addresses ending `.srt`, `.vtt`, `.ttml` or `.dfxp`, or under a key
  naming a caption or subtitle with no type in their last part (taken as SRT), from the innermost
  record that has any (a wider one can hold other videos'); each is a line after the video's
  files, downloaded as the file it is.

## Starting while Download details asks (2026-11-05, gaps row 85)

* **A row of its own from the start.** With the setting on, a plain file (one web address, not a
  video page, a torrent or a form's answer) is added to the queue as Download details opens, so
  it shows in the list and fetches into its parts folder like any row. The dialog keeps its id.
* **Placed, not added again.** Start now and Add for later give that row its folder, name,
  description and queue (`Queue::settle_early`): a row still under way is stopped a moment,
  given its place (its parts moved with `relocate` when the folder changed) and put back in
  line keeping what it has, or held for later; one that finished meanwhile is renamed and moved
  as Move/Rename does. Cancel and the close box remove the row and its parts.
* **Off by default**, as reference A ships it, and so the dialog behaves as before (B63).

## The Edge check keeps its torrents off the network (2026-11-05, gaps row 108)

* The check copies dm into a new folder for each run, and a torrent listening for peers from a
  program at a new path raises Windows' firewall question every time. `DM_NO_SWARM`, set only by
  the check's host shim, makes a torrent fail at once before any connection; the seven magnet
  variants are checked by the rows they make, which is what the check is about.

## A started stream renewed from the open page (2026-11-05, gaps rows 57 and 121)

* **Found: the expired-address resume was never reached.** `AddOptions::fresh_link_ok`, which
  lets a browser's hand-off take the place of a row whose signed address ran out, was set by its
  test alone; every hand-off from the inbox left it off, so the waiting row stayed failed and the
  browser's next address of the file became a second row. The host now marks every hand-off it
  files `from_browser`, and the inbox offers it to a waiting row first (`shell::take_from_browser`).
* **Download details still asks first (B63).** A hand-off the window asks about is asked about
  as before: taking the waiting row in its place would change what contract row B63 says, and
  waits for the maintainer's word. The waiting row takes it where nothing asks.
* **The extension offers what it sees, the app decides.** The app has no way to ask the
  extension (the browser starts the native host, not the app). So the extension remembers the
  addresses it handed to dm (`decide::Handed`, in the session's storage across the worker's
  naps), and when the page's player asks for one anew with a fresh signature
  (`RequestHeaders::renewed`: the same path, query names and `itag`, no byte range) it sends
  that address marked `Start::renews`. The app gives it to a row waiting for it and drops it
  otherwise; the host never starts dm for one and never asks about one.
* **Bounded.** Looked for at most every 2 s (every request the browser sends reaches the
  worker), offered at most every 30 s and 20 times for one hand-off, 50 hand-offs remembered.
* **Not a protocol version.** Version 3 would make today's app unable to read the new
  extension's frames. The app's hello says `renews: true`; the extension sends renewals only to
  an app that says so, as an older one would add each as another download.
* **Dropped: an alarm to poll the app.** It needs the `alarms` permission, which every person
  would be asked for on update.

## A fetched tool loses macOS's quarantine mark (2026-11-06)

* **Found by the human QA (B35):** every YouTube download from Edge failed with "the solver took too
  long". The QuickJS dm fetched carried `com.apple.quarantine` ("Edge"): a program the browser
  starts is a quarantining one, so what it writes is marked, and Gatekeeper rejects an ad-hoc
  signed program so marked; `qjs` waited, never ran, and the solver's 45 s ran out. A copy without
  the mark printed at once.
* **dm takes the mark off** what it fetched and held to the SHA-256 it ships (`tools::place`), and
  off a copy fetched before this (`tools::fetched`, when found). The digest says more than the mark;
  a tool the person installed is theirs and is left alone.
* **Not done: signing QuickJS.** dm does not re-sign a program it did not build.


## The rows that waited for the maintainer's word, decided for the person using dm (2026-11-06)

The maintainer's word, 2026-11-06, asked about gaps rows 70, 80, 84, 95, 120, 125, 136, 147, 154 and
155: "implement which is better for user and user experience, what the user expect from program".
Each was decided by that rule:

* **Built: 155, stream lines say how long.** Two videos' playlists on one page read alike ("1080p HLS
  stream"), so a person could not tell the film from the trailer.
* **Built: 136, a web page typed into Add URL is asked about, not refused.** The question contract
  row B40 already asks for an address that ends like a file is asked here too (Yes saves the page as
  `.html`, No keeps nothing): a person who typed it may want it, and one who pasted it by mistake
  says No. Reference A saves it without asking; asking is what B40 already does in dm.
* **Built: 147, "Keep sharing a finished torrent", off by default.** A person who downloads torrents
  expects to be able to give back; one who does not is not asked to (uploads cost a metered
  connection, battery and privacy), so it is off until turned on.
* **Built: 154, the macOS pictures of contract row B25 taken again**, of the 820 by 500 window the
  maintainer approved on 2026-11-04, with dm picturing its own dialog windows (no screen-recording
  permission is needed for the check).
* **Instead of 80 (the browser's proxy as a fallback): dm follows the system's proxy from the
  start.** The extension would need the `proxy` permission, and a browser disables an extension on
  update until each person accepts a new permission: every user would lose the catching for a
  click. A browser takes the system's proxy itself, so a new dm does the same (Options › Advanced ›
  Proxy, "system"); with no proxy set in the system it goes direct as before. A saved setting is
  kept.
* **Not built: 95, small files in reference A's 14 KB pieces.** A file under a megabyte arrives in
  well under a second on one connection; ten connections for it cost the server more than they
  save the person, and servers that limit connections answer them with refusals. dm keeps one piece
  under 1 MiB, with helpers only when the first is slow (contract row B36 stays).
* **Not built: 120, the app fetching through the open page.** The extension already hands the app
  what the page loaded (its answers, its renditions, the session's cookies for that download), which
  is what the fetch would be for; a way for the app to make the browser fetch addresses with the
  person's session is a door another local program could use.
* **Not built: 125's three rules** (the caught address's player first, `object-fit: cover` loops
  passed by, the largest of several). Reference A raises its bar unasked and must pick a player; dm
  shows its panel on the player the person points at, plays or focuses, which is the one they mean.
  Instagram's and TikTok's own videos use `object-fit: cover`, so passing those by would hide the
  panel where it is wanted.
* **Not built: 70, a button drawn by the app over the page.** It would follow player windows, zoom
  and scrolling from outside the browser; the panel inside the page already moves under a fixed
  header and out of a clipped parent (row 125's fixtures), and the cost is out of proportion.
* **Not built: 84, dial-up and VPN redial.** Dial-up is gone and a VPN is the system's to connect;
  a person expects dm to use the connection the computer has.

## 2026-11-06: Release profile, what was not changed

`panic = "abort"` would shrink the binary but breaks `tray.rs`, which survives a
panic with `catch_unwind` (lines 154 and 164), and tokio tasks' panic isolation:
not done. `opt-level = "s"` is now the release default, with sha2, md-5, crc32fast,
miniz_oxide, ring, epaint and egui kept at 3 (the CPU-bound work: digests,
unpacking, TLS, drawing). Release binary 25,895,328 to 20,516,304 bytes
(all at "s": 20,152,704). `tests/throughput.rs`, two runs each on this Mac:
512 MB over 8 connections 1398/2003 MB/s at 3, 1981/1964 at "s"; every other
line the same within noise. `human-qa.py` on the new release binary: 63 of 64
rows hold; B25 not pictured (no Screen Recording permission, gaps row 154).
The 27-finding audit was not kept.

## 2026-11-06: Linux AAC decoding by symphonia (gaps row 53)

At the maintainer's word (2026-11-06, asked with the choice "Symphonia crate"),
Linux decodes AAC with `symphonia-codec-aac` 0.6.1 (MPL-2.0, pure Rust, used
unmodified as a crates.io dependency, as `colored` and `option-ext` already
were). dm's own frame decoder (`aac/decoder.rs`, `huff.rs`, `transform.rs`),
which lacked the standard's Huffman tables, is removed; dm keeps its own M4A
reading (AudioSpecificConfig, sample tables, edit list), which refuses HE-AAC,
more than two channels and 960-sample frames in words before decoding. A rate
MP3 lacks is resampled by a Lanczos-windowed sinc (16 taps at the cut-off) to
44.1 kHz for the 11025 family and 48 kHz otherwise. Checked: 72.7 dB against
macOS AudioToolbox on a stereo 44.1 kHz fixture. macOS and Windows keep their
system decoders. Row 54 (GIF on Linux) stays: no H.264 decoder, by the same
day's answer.

## 2026-11-06: GPL-3.0-or-later, with an attribution term (gaps row 151)

At the maintainer's word ("if I can go to GPL v3, implement it, and make sure
anyone who uses the project or code mentions this project in their readme").
Checked first: every dependency's licence allows it (MIT, Apache-2.0 (v3
only, hence not v2), BSD, ISC, Zlib, Unicode, BSL, CDLA-Permissive, MPL-2.0
not marked "Incompatible With Secondary Licenses"; the few that offer LGPL or
GPL-2.0 also offer MIT or Apache), and every commit is the maintainer's apart
from dependabot's version bumps, so no one else's consent was needed.
`LICENSE` is GNU's text (sha256 3972dc97…6986); `NOTICE` holds the copyright,
the "or later" grant and a section 7(b) term: keep `NOTICE` and the files'
notices, and show "Based on Easy Download Manager (dm) by pranavo72bex" in the work's Appropriate Legal Notices and wherever it shows
its licence information, such as its README. The README is named as an
example rather than as the only place: 7(b) covers preserving attributions in
the material and in legal notices, and a term beyond it could be removed by a
recipient as a "further restriction" (section 7, last paragraphs). Releases
up to desktop-v0.8.7 and extension v3.2.1 stay MIT for those who have them.

## 2026-11-06: Release build at "z", with the CPU's work at 3

Measured on this Mac (M-series), release builds, the same day:

| Build | Binary | MP3 encode (test, best of 5) | 20,000 rows over the control port | Window (`hidden`) |
|---|---|---|---|---|
| all 3 (until today) | 25.9 MB | 0.17 s | | |
| "s", digests/unpacking/TLS/drawing at 3 | 20.5 MB | 0.18 s | 0.27 s | 104 ms, list 0.11 s |
| all "z" (same overrides) | 16.6 MB | 0.37 s | | |
| **"z", and dm-core, symphonia at 3 too (chosen)** | **17.7 MB** | **0.13 s** | 0.47 s | 104 ms, list 0.11 s |
| "z", dm-core, dm-shell and dm-desktop at 3 | 27.0 MB | | 0.30 s | |

Throughput (`tests/throughput.rs`, two rounds each) and memory at rest
(`memory-check.py`: 38.5 MB private against 38.8) were the same within
noise. With fat LTO the binary crate's level applies to the merged program,
so dm-desktop cannot be raised alone. Accepted: listing 20,000 rows over the
control port (the command line, MCP) takes 0.2 s longer. Not done:
`panic = "abort"` (the tray survives a panic with `catch_unwind`), jemalloc
(no measured need), removing aws-lc-rs (the torrent library's every TLS
option brings it or OpenSSL; it would need a patched copy for about 0.5 MB).

## 2026-11-06: Builds hardened, not obfuscated; nothing of the builder in what ships (gaps row 151)

At the maintainer's answer (keep the repository public; harden, do not
obfuscate) and word ("keep security the highest priority; do not leak anything
about the Mac or information"). Found: the release app held 1,139 strings
with the builder's home path and user name, and the extension's WebAssembly 34
(panic messages name their source file). `scripts/release-flags.sh` gives
release builds `--remap-path-prefix` for the home folder, Cargo's crates and
the checkout, through CARGO_BUILD_RUSTFLAGS so that the Windows target's own
flags (+crt-static) are untouched; Cargo's `trim-paths` is still unstable on
1.98. Both are now at 0, and CI's packaging fails if the runner's paths are in
the binary. Obfuscation was not done: the Chrome, Edge and Firefox stores
refuse obfuscated extension code, and the app is already compiled and
stripped. In the repository: the README's pictures showed the per-user
temporary folder (`/var/folders/…`); they are rendered again by the window
test `shots` with the folder shown as ~/Downloads, and `ui-shot.py` uses
/tmp. The security report's checkout path, an e-mail address in the hand-off
notes, the Cargo `authors` and PKGBUILD e-mail, and the full name in
`NOTICE`, the README and the Windows file properties now say `pranavo72bex`.
Kept: the Code of Conduct's contact address (the maintainer's to change).
Not touched: git history (author e-mails, the old pictures), which only a
rewrite and force-push would change.

## 2026-11-06: Torrent file priorities as tiers (gaps row 149)

librqbit 9 has no priority between files, only a file in or out
(`only_files`), which it can change on a running torrent
(`Session::update_only_files`). dm asks for the High files alone, and when the
library reports them finished adds the Normal ones, then the Low ones
(`torrent::tiers`, `TorrentOptions::tiers`). A priority is an order, not a
share of the bandwidth. The row's total is every wanted file's, as the
library counts only the files asked for. Found on the way: the step that
removes files left out of a torrent read `only`, which is now the first tier,
and would have deleted the Normal and Low files at the end
(`TorrentOptions::wanted`). The priority is set while the download is stopped,
as the choice of files is; a priority of Normal is not written to the queue
file, so files from before read as Normal.

## 2026-11-06: The browser's server address is not passed to the app (gaps row 129, measured)

Measured with a YouTube stream address, which carries an `ip=` token bound to
the address that asked for it: asked for over IPv6, the same address fetched
over IPv4 is turned away (302) and over IPv6 served (206). dm's own reader asks
and fetches over the same connection, and a download handed to dm reaches the
address the browser was given: dm's client prefers IPv6 as the browser does
(the address, asked for over IPv6 and handed to a running dm, completed, 433
KB). A token bound to the browser's address would fail only where the browser
and dm leave the computer differently (a VPN or proxy in the browser alone).
No site was found where it fails, so `Start.ip` is not built; it is what to
build if one is (reqwest's `resolve`, falling back to DNS). Row 122 measured
the same day: without a sign-in nothing failed for want of the page's data.

## 2026-11-06: A GIF on Linux from the AV1 stream (gaps row 54)

At the maintainer's word ("implement it"), reversing the earlier "not
proposed": Linux has no H.264 decoder dm can use, and one of dm's own was not
wanted (patents; openh264 built from source has no licence from Cisco). AV1 is
royalty-free, and YouTube offers most videos in it inside MP4, which dm's
cutter already handles. A part asked for as GIF on Linux takes the AV1 picture
(`youtube::read_for`, codec `av01`, at most 720 tall: a GIF is 480 across and
a decoder without assembly is slow on more), and `frames/av1.rs` decodes its
samples with rav1d 1.1.0 (BSD-2-Clause, a Rust port of dav1d) built without
its assembly, so no assembler is needed and the build stays pure Rust; it is a
dependency of Linux builds only (and of the tests everywhere). Its dav1d calls
are `unsafe`, each with its reason. Pictures are made RGB with BT.601 or
BT.709 as the sequence header says, limited or full range. "Part as GIF" is
offered on Linux for YouTube addresses only (`gif::possible_for`): a stream
caught elsewhere is H.264. Checked on Linux: a real YouTube part to a GIF in
8 s (release).

## 2026-11-06: Encrypted peer connections in a patched librqbit (gaps row 150)

At the maintainer's answer ("build it in a patched copy"). librqbit 9.0.1 has
no MSE/PE; `desktop/vendor/librqbit` is the published crate with
`src/mse.rs` and the hooks for it, used through `[patch.crates-io]` as winit
is. Policy: plaintext first, as before, and encrypted again where a peer hangs
up on the plaintext handshake (an encryption-only peer does exactly that), so
peers that took dm before see no change and the extra connection is made only
for those that refused; incoming connections of both kinds are taken
(`require_encryption`, off in dm, makes and takes encrypted ones only; the
tests use it). Ruled out: encrypted first for every peer (a second round trip
to every plaintext-only peer, and a change for the peers that worked).
Chosen to check against: libtorrent (the library most clients are built on),
not a client application, so that no other product is named. MSE with RC4 and
a 768-bit group hides BitTorrent from simple traffic shaping and reaches the
peers that insist on it; it is not strong encryption, and the window does not
call it that. Cost: the patch is carried over at each librqbit upgrade until
upstream has MSE (`desktop/vendor/README.md` says where each change is).

## 2026-11-07: Windows' memory budget at rest (gaps row 137)

Measured on the maintainer's Windows 11 computer (four cores), the release
build at rest, `scripts/memory-check.py` three times: working set 64.4 to
76.1 MB average (76.2 MB peak), private bytes 30.5 to 30.7 MB, 25 or 26
threads, 478 to 483 handles. Set as on macOS, a third above what was
measured: the working set's budget stays 100 MB and the private bytes' goes
from 60 to 40 MB, so a regression of a third fails it and the system's own
share does not. Beside reference A's numbers in row 137 (35 MB working set
average, 13 MB private, 9 threads, 991 handles) dm keeps about twice the
working set and private bytes and runs about three times the threads, with
half the handles; the three runs agree to within a megabyte of private bytes.
What makes up the difference was not measured on Windows (on macOS: the
window's drawing surfaces, fonts and the runtimes, row 137's old text).

## 2026-11-07: macOS opens links and files with dm through Apple events (gaps row 105)

The first click on a Mac with dm.app installed showed that a `magnet:` link
or a `.torrent` file reached nothing, with dm closed or running. dm had added
`application:openURLs:` to winit's application delegate at run time, as
`dock.rs` adds the Dock's method, once AppKit said the app would finish
launching; winit's class has no such method, so it was added, and still was
never called: most likely because AppKit reads which methods its delegate
answers when the delegate is set, and winit sets it before dm can add
anything (not proved; the replacement was checked instead). Chosen: the Apple event manager's own
handlers for `GURL` (a link) and `odoc` (files), on an object of dm's, put in
at `NSApplicationWillFinishLaunchingNotification`, which Apple names as the
place for them: after AppKit's defaults (which would replace handlers put in
earlier) and before the launch's own event is read. Ruled out: setting
winit's delegate again after adding the method (it relies on AppKit's cache
being read afresh, which is not documented), and a delegate of dm's own
(winit 0.30 panics on any other delegate).

## 2026-11-07: Whole MP4 files fetched by ranges are told apart by their first bytes (gaps row 155)

Bilibili's player fetches its picture and its sound as two whole fragmented
MP4 files (`…-1-30032.m4s`, `…-1-30216.m4s`), each in byte ranges its
requests' `Range` header asks for, and its server answers both `206
video/mp4`: the type cannot say which is the sound. Chosen: the extension
takes such a file (a partial answer, an MP4 type, an address ending `.m4s`,
`.mp4`, `.m4v` or `.m4a`, its whole length after the slash of
`Content-Range`, 64 KB at least; only where the older rules drop it) as a
picture or a sound by its type, and reads its first 64 KB once, with the
browser's session, for its first track's handler (`vide` or `soun`) and
height (`decide::mp4_track`). The pair is then one line, as a picture and a
sound a player fetched apart already were, and the app joins them as before
(`download_media_pair`). Beside a playlist or manifest these files are not
lines (they may be its single-file renditions). Ruled out: a site rule for
Bilibili (its file ids, or its `playurl` answer through the page hook): it
would work for one site, and reading the file's own header works for any
player that fetches files this way; guessing by size (the sound is the
smaller): wrong whenever two pictures are loaded. Cost: one more request per
such file, of 64 KB at most, refused for a private host the page is not on
(`may_reread`).

## 2026-11-07: Shared hosts are named in dm, not read from the public suffix list

A waiting row adopts a fresh address of the same "family" of servers (gaps
row 57), and the family was a host's last two labels, so two customers of
one shared host (`victim.s3.amazonaws.com`, `evil.s3.amazonaws.com`) were one
family. Chosen: 30 names of the hosts files are served from (from the
public suffix list's private part: object stores, CDNs, page and app hosts)
under which a host is its own family (`queue/family.rs` `SHARED`). Ruled
out: the whole public suffix list as a crate (a dependency of some size for
one comparison, and its private part changes weekly); the same host always
(a site's signed link moves between its own servers, `cdn1.example.com` to
`dl.example.com`, which is why the family exists). Cost: a shared host not
named stays as before; the list is added to when one is found.


## 2026-11-08: Dial-up and VPN connections as reference A's, a download held until it is up (gaps row 93, G84)

The maintainer's word of 2026-11-07 for G84: "If reference A has then implement it". Reference A's
last Options tab was read and run beside dm with a VPN entry that refuses
(`docs/reference-behavior.md`, 2026-11-08). Chosen: its switch, fields, words and defaults
(off; zero redial attempts, endlessly; 30 seconds between) as a section of Options ›
Advanced on Windows, where dm's connection settings already are (contract row B43's six tabs
stay six); and its rule: a download that starts while the connection is down waits while it is
dialled and is never sent without it, failing with "cannot connect to the Internet" when every
try failed. The wait is a hook in the engine (`dm-core` `connect_first.rs`) that every download
passes before its first request, and the dialling is dm-shell's (`dialup.rs`, Windows' RAS):
downloads that start together wait on one dial, a pause or cancel while one waits is heard
within a fifth of a second and the dialling stops once nobody waits, and a setting turned off
while it redials lets the download go as it would have.

Ruled out: dialling once when the queue starts and letting the downloads go whatever came of
it (the first draft): through a VPN a download sent around it would come from the address the
person chose it to hide, and reference A sent nothing. Left out, and why: reference A's box
asking for a user name and password when Options has none (dm dials with the ones Windows keeps
for the connection, and says Windows' reason when that fails), and its Apply button, which
hands the sign-in to Windows (dm keeps its own: with Save password on in the system's store
like the proxy's, off nowhere at all, forgotten when dm closes). Cost: a connection with no
saved sign-in and none in Options fails where reference A would ask.

## 2026-11-08: The engine-wide settings are put in force at the start too (BUGLOG BUG-014)

"Identify as dm only", the second network and now the dial-up connection live in the engine,
not on the queue, and only a change of settings put them in force: after a restart the switch
read on while dm sent the browser's name. One `engine_wide` in `shell.rs` now serves the start
and every change, at the maintainer's standing word of 2026-11-07 ("if found bug or improvement
just implement it"); contract row B51 ("the change applies at once and is kept") is what it now
does, so its words stand.

## 2026-11-08: eframe patched so each dialog's window has its own AccessKit adapter (gaps row 160)

Every dialog became a window of its own on 2026-11-01 (contract row B42), and eframe 0.37's
glow backend gives AccessKit to the root window only, so a screen reader read none of them.
Chosen: a copy of eframe 0.37.0 in `desktop/vendor/eframe`, used through `[patch.crates-io]`
as winit and egui-winit are, with one change in `src/native/glow_integration.rs` (search
`dm:`): the proxy the root's adapter was given is kept, and a window made after it (a dialog,
or the root made again) is created hidden, given an adapter, and shown as asked (AccessKit
panics on a window already shown). Only the root's adapter may turn AccessKit off, since the
switch is egui's one context's. Behind `ADAPTER_PER_WINDOW`, off on Windows until it is
checked there with Narrator, at the maintainer's word to leave Windows to its own session. Turned on
for Windows the same day in its own session: UI Automation, what Narrator reads, finds the
buttons of Add URL, Options, the Scheduler, the Speed limiter and About inside each one's
window (`scripts/windows-a11y-check.py`, 5 of 5; with the copy as published, none of Add URL's).
Ruled out: dialogs drawn inside the main window again (B42 is the maintainer's choice); an
AccessKit tree of the dialogs grafted onto the root's (AccessKit has one tree per window, and
a screen reader finds controls by window). On an eframe upgrade the change is carried over,
or dropped once upstream gives each viewport an adapter.

## 2026-11-08: The Scheduler hangs up the connection Options sets, when its downloads are done

Reference A's Scheduler (read on 2026-11-08, `docs/reference-behavior.md`) has "Hang up modem
when done" (off) between "Open the following file when done" and "Exit … when done", each a
tick of its own. dm's B49 said this was not offered, since dm had no dial-up connection to hang
up; with G84 built it has, and the maintainer's words of 2026-11-04 (dm works as reference A,
with its labels) and 2026-11-07 ("If reference A has then implement it") cover it. Chosen: a switch of
the schedule (`Schedule::hang_up`, a queue's own hours have their own), in reference A's words,
below the program to run, on Windows alone; at the moment that program runs, the connection
named in Options › Advanced › Dial Up / VPN (with the switch on) is hung up if Windows has it
open, whoever connected it, as the person asked for that connection to go. A dial of dm's under
way is let finish first (the shared dialling lock), and Windows' end of the connection is waited
for off the window's thread. The status line says it was hung up, or that no connection is set
in Options; a refusal goes to the problems. Ruled out: hanging up every open connection (one the
person keeps for something else would go too), and greying the switch while Options sets no
connection (reference A leaves it on offer, and what dm says at the end names the setting).

The end of the scheduled downloads moved with it: it came the first time the queue ran dry,
before the rounds of Try a failed download again, so a hang-up there would have been followed by
the tries again dialling once more and the connection left up at the true end; the program to run
had the same fault on a batch not yet done. Now a round owed comes first and the end after the
last (`app/scheduled.rs` `Watch::at_end`), as reference A's "when done" comes after each file's
retries; contract row B62 says so.

## 2026-11-08: Another site's file takes only the cookies the browser itself sent it from the tab

The gate of the same day ("A panel choice's session goes only where the tab's own requests
went") let a file on another site carry all of that site's cookies once one of the tab's
requests had gone there (`RequestHeaders::reached`). A page can send its tab anywhere: a
`fetch`, a hidden frame, a preloaded `<video>`, then name that site's file in its player; and the
browser, sending a request there from another site's page, keeps back every `SameSite` cookie
(Lax, Strict, and by default one with no attribute), a sign-in above all. So "reached" said
nothing of the sign-in, and dm sent what the browser had withheld. Chosen, at the maintainer's
standing word of 2026-11-07 ("if found bug or improvement just implement it") and within the
answer of 2026-11-08 to keep every site working: a file on the page's own site, or the page
itself, goes with the session as before (`decide::session_goes`); one on another site takes only
the cookies the browser itself sent that host from this tab, by name
(`RequestHeaders::sent_cookies`, read from each request's own `Cookie` header, which the
extension sees with `extraHeaders`; their values from the browser's store when the download
starts, `decide::cookies_named`). TikTok's video servers, on a domain of their own, get the
cookies its player was sent; a site whose cookies the browser blocked from the page gets none,
as from the browser. Ruled out: counting a host only once a request to it carried any cookie (a
host with one cookie it allows across sites and a Lax sign-in would still have given the
sign-in), and taking the store's cookies marked `SameSite=None` (the browser may block third-party
cookies altogether: what it sent is the answer). The plain panel's shadow root stays open, as the full
panel's does: closing it would have ended the site checks, which read the lines inside both
(`sites-check.py` `LITE_STATE`); a page could still restyle a button inside it, which `asDrawn`,
looking at the holder, does not see.

## 2026-11-08: Headless keeps its window-free DmApp; the application's work is the Session's (architecture step 13)

Step 13 of the architecture audit asked whether `--headless` should loop on the
application alone, with no window toolkit. Its answers to `command.run` (the
window's `Command`s: dm's every menu item) and to `ui.*` come from `DmApp`, the
window's state, ticked against a stand-in `egui::Context` that is never drawn.
Chosen (option (a) of the audit, the maintainer's word of 2026-11-08 that the
existing behaviour must not break): headless keeps that `DmApp`, and everything
it does that is not the window's moved into `dm_shell::app` instead. That covers
the read and what follows it, the schedules' tick, When done and its countdown,
the hand-offs (`handoff::Hands`), and who may use the control port
(`control_api::refused`, `person_only`, `batch`, `answer`). So headless and the
window follow one path, and a GPUI front end drives the same `Session`
(`session.rs` `no_window_tests` drives a download to When done with no window).
Ruled out: a headless loop on `Session` alone. It would have answered
`command.run` and `ui.*` differently (no commands, "there is no window"), a
change to what scripts and AI clients get. The audit counted it about 30
lines, but that was before `command.run` was counted. `Command` moving out of
the window crate is left for when GPUI needs it.


## 2026-11-09: The command registry is the application's; dm-core keeps `pub use reqwest` (architecture steps 15, 16 and 14)

The maintainer, 2026-11-09: "do not rebase for now. just implement the
UI-independent architecture fully then we will fix the merge conflict". The
decision of 2026-11-08 had left `Command` in the window crate until GPUI
needed it; finishing the architecture means doing it now. `Command` (names,
English labels, keys), which commands apply to the rows chosen and the
commands that are the application's alone are in `dm_shell::app::commands`
(step 15). Keys are a neutral `Chord` (the key, and whether Cmd/Ctrl and Shift
are held), not egui's type, so a second toolkit binds the same shortcuts; the
window keeps only `Shown` (a title in the language in force, the egui
shortcut) and `pressed`. What only a presentation sees of a row (retrying, the
file gone, whether it plays) is asked through `commands::Looks`, not copied
into the application. Ruled out: a `Command` enum per presentation (the
control port's `commands.list` and `command.run` would have differed by
toolkit), and moving egui's key types into dm-shell (`xtask layers` forbids a
toolkit there).

Step 16 moved the rest of the audit's section 3 rows: what a row is called
and where its file will go (`names`), imports (`import`), extraction
(`files`), the schedule's hold (`schedule::holds_waiting`) and the clipboard
watch (`links::Clipboard`). Each old path re-exports.

Step 14: `Entry::redacted()` (no cookies, request headers or form body, the
address's password masked, with `shown_url` moved into dm-core beside it) is
what `queue.list` sends, so a new reader cannot forget a secret. `pub use
reqwest` stays: the audit allowed either it or a client factory, a
presentation never names it, and the two places the window does (the update
check and a ZIP's listing) are network work a client factory would only move.
Ruled out for now: `Queue::new` taking a client factory, a change to every
caller of the queue for no behaviour.

## 2026-11-08: YouTube's checks are answered as reference A answers them, with nothing fetched

Asked by the maintainer on 2026-11-08 ("It should work out of the box for every users. make it rock
solid. speically youtube part"; "better way to fix the issue? without shipping any third party
package like quickjs?"; "how to [reference A] is doing it. also refer [reference A] and work as it"), and gaps row 162's
own ask (the weekly check fixing itself with no external video tool and no other third party).
That afternoon YouTube downloads failed on every fresh install: the two challenge scripts and
QuickJS were fetched from GitHub's release pages at first use, and GitHub answered 504 for hours.

How reference A does it, read from its own files (`docs/reference-behavior.md`, "How reference A
answers YouTube's checks"): its extension finds the player script on the page, fetches it and cuts
out the signature and `n` pieces with patterns its app sends it; its network library's YouTube parser
puts the pieces in a small program of its own and runs it in Windows' own JScript9 engine
(`IActiveScript`), with polyfills for that old engine. It ships no engine and fetches nothing.

dm now does the same, with nothing fetched and no one else's script:

- `youtube/player.rs` cuts the player itself. The player is one function whose statements define
  its parts and then start it; dm keeps every statement but the calls made while it loads (an
  expression statement that is not one assignment), so what is left defines and reaches for
  nothing. Its function that builds a stream's address (the one whose own statements include
  `x.set("alr","yes")`) is called as the player calls it, which deciphers a signature, and the
  method its address class adds to `set`, `get` and `clone` turns `n`. These are the player's shape,
  not its names, which change with every version; today's player (`a5c2aafe`) hides every name
  behind a string table and its signature steps inside numbered dispatchers, and the shape holds.
  A tokenizer of dm's own (strings, templates, regular expressions, comments) splits the
  statements; nothing is parsed further.
- `javascript.rs` runs the program in the computer's own engine. On Windows that is Windows' own
  Chakra (`chakra.dll`, in System32 on Windows 10 and 11), through its hosting functions, loaded
  from System32 only, with no compiling to machine code, no `eval`, 256 MB at most and 30
  seconds; a program there has JavaScript's own objects and no files, programs or network. In
  the app it runs in a process of its own: dm's own program started as the engine's host
  (`dm-desktop --javascript-host`, one request on its input and the answer on its output), as the
  warm page reader already is. Testing found that Chakra ends its whole process when a program
  passes its memory limit, whatever the program does to get there; in the app's own process that
  would have ended every download with it. A test or a tool with no host runs it in its own
  process. It reads the ES5 build of the player (`player_ias`, the same version
  as the page's `player_es6`, whose syntax Chakra predates). Where Chakra is missing, and on macOS
  and Linux until their own engines are used (gaps row 164), an installed Deno (no permissions) or
  QuickJS runs the same program.
- Checked against the scripts it replaces, on the live player: the same `n` and signature answers,
  in Node and in Chakra (about 2 s, 27 MB); the cut is 0.5 s where the scripts' parser took 3.7 s.

Ruled out: shipping the two scripts in dm (third party, and the maintainer's later words); running
the whole player against a stand-in browser (it starts at once and reaches for the page);
JScript9 as reference A uses it (no `Symbol`, which today's player names while loading);
`cscript.exe` (a separate host that policy may switch off and that security tools watch); WebView2
(a window and a loader for a few functions); a JavaScript engine built into dm (third party, and a
megabyte or more). Chakra holds pointers into the program's text and frees an unheld value at its
next collection, so dm keeps both texts alive for the runtime's life, holds every value it reads
(`JsAddRef`), and never reads the long program's own value (reading it, a function, crashed the
engine in testing).

The weekly job that moved the scripts' pin (`solver-update.yml`, `scripts/update-solver.py` and its
test) is gone. `.github/workflows/youtube-check.yml` takes its place: every Monday, on Windows, real
videos through dm's own reader, the first kilobyte of each chosen stream asked for as a download
asks (`youtube_read`, `DM_FETCH_FIRST`), and an issue opened, or added to, when one fails.


## 2026-11-09: A small slow file ends with its last range; the seams are still asked one at a time (gaps row 165)

Context: Edge's check of a 3 MB file from a server that holds each connection to 1 MB/s took
2.07 to 3.07 s on this Windows laptop (debug build) against a 2.8 s limit. Timed in parts, beside
the browser and in the engine alone (a server paced as the check's): the check's own tab took a
third of a second before the press, Edge and dm's host 0.4 to 0.9 s from the press to dm's first
request, the host's question to the app's 0.11 s, and the download itself 0.8 s in the engine and
1.0 s in the app. Of that 0.8 s, 80 ms or more came after the file was in: the last helping
connection to finish its range watched a whole window (`HELP_WATCH`) for more work before it
ended, and the download waited for it.

Decision:
- A helper's watching ends when the connections with ranges of their own are done (they end only
  when no range is left to halve, and ranges only shrink), and it is not begun once every byte is
  in. The engine alone: 0.82 to 0.86 s before, 0.72 to 0.80 s after; the pause between the last
  range's byte and the checks after it, 65 ms at best of three before and 17 ms after
  (`a_download_from_a_slow_server_ends_with_its_last_range`, which fails without it).
- `chrome-check.py` times that file from the press, as its words say ("a person clicks, and
  watches"), not from before it opens and loads the page. The limit stays 2.8 s: one connection
  alone takes over 3 s, so it still says the helpers came, with at least four of dm's connections.
  The check now says where its time went. In the whole QA after both: 1.40 and 1.59 s from the
  press (1.6 and 1.8 s counted the old way), all 64 rows holding.

Ruled out: asking the seams again all at once (about 35 ms of the 0.8 s): a server that limits
connections refuses some of them, and a refused seam proves nothing and is let pass, so the check
would look at fewer seams on exactly the servers that cut connections. Shorter `HELP_AFTER` and
`HELP_WATCH`: they decide whether a fast server ever sees a helper, and were set by measurement
(2026-10-26); nothing here says they are wrong. The sync of the finished file (90 ms on this
laptop's D: drive, 11 ms on C:) is what makes a completed row safe from a power cut.

## 2026-11-09: Typed events beside the change mark; the window asks no server itself (architecture step 17)

The maintainer asked for the spec's last items to be built ("implement it.
migrate it properly"). Two were coupling: the window made its own requests (the
update check, a ZIP's files, a file's name and size, a page's links, a video's
qualities, a playlist, a site crawl), and it held the engine's queue in six
places. Each is now the application's (`dm_shell::app::{update, files, lookups,
search}`, `Backend::{client, allowance_spent, remove_later}`), taking the
`Backend` and answering a callback; dm-desktop names `reqwest` nowhere. The
clients the window built differed (some without the read timeout); every one
is now `Backend::client`, the settings' proxy and timeout, inside each
lookup's own overall limit.

The third was events. `changes` stays as it is: a mark and a wake-up, never
data, which is what lets a burst of a thousand changes cost one read. Typed
`DownloadEvent`s are made beside it, from the same reads (`events::Events`,
published by `Session::read`), not sent by the engine: the engine has no
event of its own for progress, and adding a channel there would put the
subscribers' speed on its threads. Policy: lifecycle events kept in order and
never dropped, progress coalesced to the newest per row, each subscription
independent, a dropped one gone at once, nobody listening costing nothing.
The first version rebuilt the map of rows at each read (4.2 ms at 10,000
rows); it is updated in place and skipped with no subscriber (2.9 ms with two
subscribers, and the tick back near its 2.4 ms). Ruled out: a broadcast
channel with a fixed buffer (a slow subscriber would lose lifecycle events or
make the publisher wait), and an event per byte count from the runner (the
reads at four a second already carry progress).

## 2026-11-09: macOS runs dm's JavaScript in its own JavaScriptCore (gaps row 164)

On macOS the program that answers YouTube's checks, and a proxy script, ran in
an installed Deno or in QuickJS fetched from GitHub at first use. The first
YouTube download of a fresh install therefore needed GitHub, and failed the
afternoon GitHub answered 504 (the reason Windows moved to its own Chakra,
decisions 2026-11-08). macOS's JavaScriptCore is part of every macOS: dm loads
the framework from its system path at first use (`dlopen`, so a macOS without
it, or without the time-limit function, falls back as before rather than
failing to start) and runs the program in a context of JavaScript's own objects,
with the time it is given (`JSContextGroupSetExecutionTimeLimit`, exported by
the framework from its private header), in dm's own host process as Chakra
runs. Two differences from Chakra, both recorded in `javascript.rs`: macOS has
no limit on one program's memory to set (the host process keeps a program
that runs away from the app, and the time limit ends it), and `eval` is left
on (it reaches nothing a program does not already have; there is nothing of
the computer in the context, which a test checks). A person or a test that
names an engine (`DM_QJS`, `DM_DENO`, set empty to name none) gets that one,
on Windows too: the stand-in engines of the YouTube and proxy tests are used as
before. Ruled out: linking the framework at build time (a missing symbol would
stop dm from starting), and JavaScriptCore's Objective-C `JSContext` (the C
functions are enough and need no Objective-C runtime glue).

## 2026-11-09: Linux runs it in WebKitGTK's JavaScriptCore, and the JIT is off on both (gaps row 164)

WebKitGTK's `libjavascriptcoregtk` exports the same C functions as macOS's
framework, the time limit among them (checked with `nm -D` on Debian 12's
4.1 build), so the `jsc` module serves both, loading on Linux the first of
`libjavascriptcoregtk-6.0.so.1`, `-4.1.so.0`, `-4.0.so.18` the system's loader
finds. Where none is installed, dm runs Deno or QuickJS as before. It is not a
dependency of the packages: a GNOME desktop already has it and a KDE one may
not, and requiring it would pull WebKitGTK's libraries onto every system for a
fallback that works; the rpm recommends it (nfpm's `recommends`), the Arch
package lists it as optional, and the .deb cannot say so (cargo-packager 0.11.8
has no `Recommends`), Debian's and Ubuntu's GNOME bring it anyway.

The host process starts JavaScriptCore with `JSC_useJIT=false` (read from the
environment as the engine starts), its interpreter only, as Chakra runs
without native code on Windows: the JIT is where engine exploits usually live.
A hot loop takes nine times as long without it, but a YouTube reading took
2.0 to 2.9 s either way (the program is mostly parsed once, and the network
is most of it). The engine run in the app's own process (tests, the example
tools) keeps its JIT.

Row 164 asked for `human-qa.py` B35 and B36 on Linux; what was run in their
place is the reader on live YouTube in the Linux container (`youtube_read`
with `DM_FETCH_FIRST`) with github.com unreachable and no Deno or QuickJS:
four videos, both live players, every chosen stream accepted. B35/B36 drive a
real browser to YouTube, which the macOS QA run covers.

## 2026-11-09: The reference specs' last engine parts (EDM-E1 parts 2, 4, 6 and 7)

An audit of `docs/gaps.md`'s reference sections against the code (torrent T1 to
T8, EDM-V1, EDM-E1, reference A's extension R120 to R131) found everything
built or ruled out but four engine parts, at the maintainer's word of the day
("look into reference in gaps.md and implement here"):

- **Part 7, one file out of a ZIP before it is downloaded: built.** Download
  details › Contents › Take out (`zipinfo::zip_take_one`): the central
  directory already read for the list now keeps each file's header offset,
  method and CRC; the file's local header is read for its data's start, its
  packed bytes fetched by one range, unpacked (stored, deflated; any other
  method is refused with words) and checked against the CRC before it is
  written beside where the archive goes, under a free name, staged and moved
  into place. At most 256 MB, held in memory; larger is the archive's to
  download. Only in Download details: a finished archive's Properties has the
  file on disk, and Extract. Contract row B44 says so.
- **Part 2, a connection behind its peers: built, narrowly.** The 5 s window's
  rule was under a sixty-fourth of the whole download's bytes, which with two
  or three connections let one at a twentieth of its peer's rate go on. It is
  now also under 0.15 of the other connections' average (`helping::behind`).
  The spec's 3 s window and per-connection EMA are not taken: a fresh
  connection is still climbing at 3 s, and halving the largest range already
  leaves a slow connection a small piece (`tests/speed.rs` is no faster with
  the rule; it is kept for the connection that trickles beside a fast one).
- **Part 4, a 10 s cap and a 2 s probe for a server's 5xx: not taken.** A
  server that answered keeps `retry::backoff` (to 15 s, growing past a
  `Retry-After`): Wikimedia's limiter let a request through only after about
  twenty quiet seconds, and asking every two seconds kept it shut.
- **Part 6, a 256 KiB write buffer: not taken.** Measured before (`ranged.rs`,
  qa/after.md): no faster over loopback, 20 to 30 MB more memory over ten
  downloads of eight connections.

## 2026-11-09: dm in Default apps (spec T5); the MediaSource hook removed (EDM-V1 stage 1)

At the maintainer's word of the day ("go to reference in gaps.md and implement the improvement
... make it ready for production"), the two reference items left that are code:

- **Listed in Windows' Settings › Default apps.** `register.rs` `default_apps_keys` writes dm's
  own ProgIds `dm.magnet` (with `URL Protocol`) and `dm.torrentfile`, each with dm's command,
  `HKCU\Software\dm\Capabilities` naming them under `URLAssociations` and `FileAssociations`,
  and dm's line in `HKCU\Software\RegisteredApplications`. Separate ProgIds rather than the
  `magnet` key and `dm.torrent`: being listed must not take a link or a file type from another
  program, nor mark dm's own registration as present where it is not (`torrent_is_ours` reads
  `dm.torrent`'s marker). Written at registration and once at a start where missing
  (`refresh_windows_handlers`), removed on Uninstall (`unlist_from_default_apps`). Tested with a
  stand-in `reg`; not yet seen in Windows' Settings (a person on Windows).
- **The MediaSource hook removed.** `document.js` replaced `URL.createObjectURL` and
  `MediaSource.prototype.addSourceBuffer` on every hooked page to pass on a player's codecs,
  which `content.js` kept and nothing read; a player makes its blob address before it declares
  its codecs, so what was passed on was nearly always empty. Patching a page's own functions for
  nothing is a risk (a site that checks them, a player that breaks) with no gain. Not taken:
  T6's registry test (Windows code that cannot be compiled here; for the Windows session).

## 2026-11-09: The work list keeps only what can be done

At the maintainer's word ("remove the things which are done or cannot be done due to lack of
sign-in or an open dialog; clean up gaps.md"), `docs/gaps.md` drops every row that waits on a
site sign-in (Facebook, Instagram, Reddit, YouTube signed in: rows 63, 71, 75, 110, 122, 142,
and the 68-site run beside reference A, row 152), on reference A's dialogs set by hand (row
65's proxies), on a person's hands or eyes on the window (rows 60, 66, 72f), on a reporter
(row 30), on the proof-of-origin token the maintainer declined (row 37), and on live checks of
site rules that only signed-in sites still lack (rows 68 and 69: Vimeo, TikTok and X were run
signed out with the shipped rules on 2026-11-06). Its notes (rows 77, 78, 99, 102, 109, 131,
132, 135, 143) were findings, not work, and are kept in `docs/reference-behavior.md` and git
history. The copy search (row 151) was run (no copy found) and is now a periodic duty at the
foot of the list. What is left needs Windows, a phone, a release, native readers or the
maintainer's GitGuardian dashboard. Not taken: keeping the signed-in rows as BLOCKED, which
left a list that read as unfinished when nothing in it could be done.

## 2026-11-09: A dubbed YouTube video's own language, not the track marked to play (issue #32)

The player API marks one sound track `audioIsDefault`, and which one follows the language of
the request: dm asks in English (`hl=en`), so for a video made in Spanish with an English dub
the English dub is marked and the Spanish original is not (seen on 2026-11-09 on every Spanish
and Portuguese video tried with an English or automatic dub; an English video's original is
marked, which is why it went unseen). dm had set aside every unmarked track as a dub, so it
took the dub. The track's part is named by YouTube itself in the stream's address (`xtags`:
`acont=original`, `dubbed`, `dubbed-auto`, `descriptive`) and in the track id (`.4` the
original, `.3` a dub, `.10` an automatic dub). dm keeps the track named original, and the
marked one only where no original is named; a video none of whose tracks would be kept keeps
them all rather than have no sound. `vb=1` (a voice-boosted copy, at the same bitrate as the
plain track for itag 140) is set aside with the loudness-evened `drc=1` copy. Not taken: asking
YouTube in the video's own language (the language is not known before the answer, and asking
twice costs a request and a bot check), or a setting for the preferred language (no one has
asked; the person's own language is a later choice for the quality list, not this fix).

## 2026-11-09: The window moves to GPUI, in phases, after an audit (branch `gpui-migration`)

At the maintainer's word of 2026-11-09 (the "egui → GPUI Migration" specification), the
egui presentation is to be replaced by GPUI, keeping `dm_shell::app` and dm-core and every row
of the behaviour contract. Phase 0, the audit, is in `docs/migration/` (no source changed).
Taken from it: the seam is finished first under the egui window (Phase A: about 7,100 lines
of dm-desktop that never touch egui moved to a toolkit-free crate, about 2,500 lines of
application rules still in `DmApp` moved into `dm_shell::app`, the control port's `ui.*`
answers made a toolkit-free contract, egui's performance measured), then a GPUI shell is
built as a go/no-go spike judged first on screen readers in every dialog window (Phase B).
Proposed pin `gpui-pre =0.3.12` with `gpui-component =0.9.1` (one maintainer publishes both;
`gpui-ce` 0.3.0 is the fallback), confirmed only by that shell. Not taken: starting with
GPUI views over `DmApp` as it is, which would copy its application rules into a second
window, and replacing the control port's `ui.*` answers, which `human-qa.py` and MCP read.
New B25 pictures, any contract row GPUI cannot hold as written, the go/no-go and the switch
of the default window are the maintainer's to approve (`GPUI_MIGRATION_PLAN.md`, D1 to D5).

## 2026-11-10: The bottom line: faster than reference A in every benchmark, and easier to use

The maintainer set the standard every change is judged by: dm's engine must be faster than
reference A in every benchmark, and dm must be easier to use than reference A. It is at the
top of `docs/gaps.md` and never closes. A speed row closes only when dm is faster on the same
run, so row 166's old target, within 10% of reference A, is replaced by "faster". "Every
benchmark" means every case of the side-by-side harness (`bench.mjs`) and any added to it later,
with `idle` judged by CPU and memory at rest. Being easier to use does not override
`docs/behaviour-contract.md`: a row proposes the change, and the maintainer approves it in
words before it is made.

## 2026-11-11: Every GPUI window that saves a program goes through the guard

Security review of 2026-11-11: the GPUI Scheduler's OK saved with `backend.set_settings`, past
`Backend::set_settings_from` and its script rule (`Settings::keep_programs_of`). A control-port
or MCP client could open the Scheduler (`command.run scheduler`), type a program into "Run this
program when they are done" and click OK, and dm ran it as the person when the hours ended; the
egui Scheduler and GPUI Options refused the same. The Scheduler now saves through
`Downloads::apply_settings`, as Options does, so a script's program is kept as it was and the
window says so. The other direct `set_settings` callers in `dm-gpui` each start from the
settings in force and change no program. A test in `dm-gpui` (`main.rs`) fails where a source
file both names a program field (`.run_when_done`, `.on_complete`, `.proxy.script`,
`.queue_schedules`) and calls `.set_settings(`.

## 2026-11-11: The GPUI window on Windows

At the maintainer's word of 2026-11-11 ("complete the windows part"; macOS was done and Linux is
carried on the Mac), the GPUI window was built, run and checked on a Windows computer. Taken on
the way, each the recommended option of `docs/migration/GPUI_MIGRATION_PLAN.md`:

- **The program's own description.** `dm-gpui/build.rs` gives `dm-gpui.exe` dm's icon and its
  version, as the egui crate's does, and no manifest: GPUI links its own into every program built
  on it (per-monitor DPI v2, as `dm.manifest` asks, plus asInvoker, the system's common controls
  and the segment heap), and a second `RT_MANIFEST` is a duplicate resource the linker refuses. A
  release opens no console (`windows_subsystem`), as the egui binary's does.
- **What GPUI's Windows windows do not say.** Their title (`get_title` is the platform default,
  empty): `screen::open_window` keeps each window's opening title and `screen::set_title` its
  later ones, which the control port reads (`ui.windows`, `ui.move_window`; a progress window's
  title starts with how far it is, B46). Their frame: GPUI's bounds are the client area, so the
  port reports and places the frame as it is seen (`DWMWA_EXTENDED_FRAME_BOUNDS`: the window
  without the borders Windows keeps invisible around it for resizing, 7 pixels a side at 100 %),
  where a person sees the window. `GetWindowRect`, which `human-qa.py` reads of Windows itself
  (B42) and the egui window reported, is those 7 pixels wider, within the checks' 16. A dialog
  pictured from the screen at the reported frame then holds nothing of what lies behind it, and
  a picture (`ui.screenshot`) is cut to the same frame.
- **Windows' own look.** The window's buttons in the system's caption glyphs (Segoe Fluent
  Icons, Segoe MDL2 Assets on Windows 10), restore while maximized; Windows' High Contrast
  palette over any look, as the egui window draws it (B45). Options gains what the egui window
  showed only on Windows: who opens magnet links and `.torrent` files (Browser) and Dial Up /
  VPN (Advanced, B43). The jump list and the taskbar button's progress, as the egui window's.
- **A greyed control lets the page scroll** (`native::enabled`: `block_mouse_except_scroll`, not
  `occlude`). Occluded whole, a greyed field under the pointer swallowed the wheel and stopped
  Options' Advanced page short of its end, on every system.
- **Screen readers (Phase B check 1) on Windows: pass.** UI Automation reaches every dialog's
  buttons in its own window (`windows-a11y-check.py`, 5 of 5), and still does after 50 dialog
  windows opened and closed. Options' pages are tabs to a screen reader in the GPUI window, so
  the check takes a page's tab as it takes a button.
- **No GPU.** GPUI draws through Direct3D 11 on the first adapter that makes a device, and DXGI
  lists Windows' own software adapter (WARP, "Microsoft Basic Render Driver") last, so a
  computer or a virtual machine with no GPU still draws; read from `gpui-pre-windows`'s
  `directx_devices.rs`, not run here (this computer has a GPU). CI's Windows runner has none,
  so its installer check is the run.
- **The installer replaces the egui window's program.** Installed over a dm whose window was
  egui's, `dm-desktop.exe` in the same folder is ended and deleted first, so the login item and
  the browsers' registrations, which start a program that is then gone, follow `dm-gpui.exe` at
  its first start (`autostart::takes_over`, `register::ensure_this_copy`).
- **A hand-off is acted on at once.** The GPUI window coalesces the queue's changes over a
  quarter of a second; a wake to act on now (a browser's hand-off) that came in that quarter
  waited it out, so behind a running download the click's first request came 160 to 650 ms
  later than in the egui window, whose `Now` cuts the wait short. It does here too. Download
  details also begins its early row and asks the server about the file before its window is
  made, not from inside the window's building (a new Direct3D or Metal surface first).
- **Found by a review of these changes (five reviewers, each finding put to a skeptic):** the GPUI
  window put the chosen language in force only when Options was saved, so it started in English
  on every system (now at startup, before the window, menus, tray and jump list are made, held by
  a test); a greyed control took clicks, because GPUI lets an element block only what was made
  before it, and the block was on the control's own wrapper (now a layer over the control, which
  lets the wheel through); a High Contrast theme changed while dm runs was drawn only when a window
  next drew for another reason (now every window is drawn again when the palette changes, at the
  next wake); and the docs and scripts that still named `dm-desktop.exe` for Windows.
- **`cargo packager -p`.** Run without it in a crate's folder, cargo-packager makes every
  workspace package that has its metadata, so the egui window's installer was made too and,
  renamed to `dm_…`, the two took one name. CI and the Makefile name the package now.
- **Not taken:** a release `dm-gpui.exe` from Docker (`scripts/windows-build.sh`, `cargo xwin`):
  GPUI compiles its Direct3D shaders for a release with the Windows SDK's `fxc.exe`, and only
  when the build runs on Windows, so the cross-build stays the egui window's portable zip; CI
  and a Windows computer build the GPUI window's packages.

## 2026-11-11: Reference A's benchmark is frozen in the repository as the floor of every parameter

The maintainer asked on 2026-11-11 for reference A's benchmark to be added to the repository. They
wanted it there so that work on other systems can make dm as fast. They said it never changes,
because it is the bottom line and a standard, and that every parameter goes in. What was decided:

- **Where.** `bench/` holds the harness (`bench.mjs`), the judge (`compare.mjs`), the frozen
  `reference-a-baseline.json` and a README written for whoever picks the work up, a person or an
  AI agent. The harness is `D:\compare\release-0.9.0\bench.mjs` with its server, data and
  scenarios unchanged. Reference A is named only in the git-ignored `bench/local.json`, and its
  results say `refa`.
- **What is frozen.** All 84 of reference A's runs in the 46 result files of 2026-11-09 and
  2026-11-10 are in the file, each with every parameter, along with the machine, both apps'
  settings and the harness's parameters. The runs fall into four groups: `standard` (8
  connections; dm's home on the SSD, both apps saving to the hard disk), `connections16`,
  `allSsd` and `allSsdConnections16`. For each group and scenario, a floor is reference A's
  **best** value of each parameter in any whole run. That is the strictest bar it has set, and a
  run slowed by the harness (the `spawnSync` of 2026-11-09, the window helper left running) can
  only have made a floor easier. The fastest run's request pattern and timeline are kept for each
  standard scenario, as a record of how reference A goes about it.
- **Every parameter is judged**, which widens the bottom line of 2026-11-10 ("faster in every
  case; CPU and memory at rest"). dm's median must be lower than the floor in time from the
  address and from the first request, the time to the first request, the finish after the last
  byte, bytes fetched twice, peak working set, private bytes, CPU and recovery after the outage.
  A tie fails, except at a floor of 0. Counts (requests, connections, bytes served) are recorded
  and shown, not judged: more connections is neither better nor worse in itself.
- **Other systems.** The capped scenarios are bound by the server's caps, not the computer, so
  their floors apply as they are, as do memory and CPU. `fast` (and, on the baseline laptop,
  `big40`) is bound by the computer, so its time floors are scaled by the `ceiling` scenario:
  floor × min(1, ceiling there / ceiling on the laptop). On a faster computer the floor gets
  stricter, and it is never looser than here. The ceiling is a reference client of the harness's
  own (`ceiling-client.mjs`: a process of its own, 8 ranges, 4 MiB buffered writes). It was
  measured on the same laptop on 2026-11-11, 6 runs, best 2.601 s for 512 MiB.
- **Frozen by its digest.** `desktop/xtask/src/baseline.rs` (`the_reference_a_baseline_never_changes`,
  in `cargo test --workspace`) and `compare.mjs` both pin the file's SHA-256. A new measurement
  of reference A goes into a new file beside it.
- **Not taken.** Re-measuring reference A now: the maintainer asked for the benchmark that exists
  to be the fixed standard, and a new measurement would be a file of its own. A median floor:
  dm could pass it while reference A has done better. curl as the ceiling: Windows' own wrote
  512 MiB in 21.6 s and Git's is a debug build, where reference A took 3.7 s, so it measured the
  tool. Scaling the capped scenarios too: their floors are near the ideal, and the outage's fixed
  15 s would have made one impossible.
- **Changed from the copy in `D:\compare`.** File names carry the run's key, so a second run no
  longer saves `x (1).bin` and reads as not whole. dm is sampled by its process id, so an
  installed dm left running is not counted. The sampler runs on macOS (`ps`) and Linux (`/proc`)
  as well as Windows. `big40` is a scenario again (its parameters taken from its result file),
  `ceiling` is new, and every result file carries the system, the binary and the folders.
- **Node, as it was.** `AGENTS.md` keeps out a JavaScript test suite. The benchmark stays plain
  Node with no packages and no `npm` because its server, write sizes and timers are part of the
  standard reference A was measured against: a server in another language would pace bytes
  differently and make the floors meaningless.

`compare.mjs` on the laptop's runs of 2026-11-10, dm's release build of that day: dm is better
end to end in `fast`, `per_conn`, `flaky` and `outage`, and worse in 22 parameters, which are
gaps rows 171 and 175 to 177. `batch` has no floor (row 172), and `inet` is never one.

## 2026-11-11: Windows' GPUI window opens smaller

At the maintainer's words of 2026-11-11 ("on windows UI looks super big", "fonts size is good. i
mean default windows size is big", "make sure it did not break on mac os because on mac it is
perfect"): on Windows the main window's first size is 840 by 520 points (as wide as its toolbar's buttons need) (900 by 600 before, two
thirds of a 1366 by 768 laptop's screen and most of its height), and the tallest dialogs open
smaller: Options 620 by 520 (680 by 600, nearly the screen's height), the Grabber 600 by 480,
Download links 680 by 480; their pages scroll as they did. Each is `native::pick(macos, windows,
gnome)` with only the Windows value changed, so macOS and Linux open as before, and the text and
every control keep their sizes. A window put somewhere else or resized still comes back as it
was left (`frame.rs`). On Windows a dialog has no minimize button (`screen::open_window`: not
the main window, whose title bar dm draws, nor the drop target), as Windows' own dialogs have
none. B25's Windows picture of the empty window (`docs/behaviour-baseline/gpui/windows/`) was
drawn again at the new size with `human-qa.py --save-baseline` and looked at, at those words.

## 2026-11-11: Linux ships the GPUI window, under the commands it had

At the maintainer's word of 2026-11-11 ("only focus on linux part"; Windows is done on a Windows
computer) and the recommended option at the font question ("remove after Windows"): Linux's
packages carry the GPUI window, as macOS's have since 2026-11-10, and the egui window, its fonts
(Inter, Selawik, the Fluent icon font) and its vendored patches are removed in a change of their
own once Windows holds the contract in GPUI (G9). The GPUI window uses the system's fonts only.

- **The installed command stays.** The .deb installs `dm-gpui` and a two-line `/usr/bin/dm-desktop`
  that runs it (`desktop/packaging/dm-desktop`, through the deb's `files`): the README's commands,
  an autostart entry and a browser's native-host manifest written by an earlier dm name
  `/usr/bin/dm-desktop`, and an upgrade that removed it would have stopped a login launch until
  dm was opened by hand. cargo-packager can neither rename a binary nor make a link; a copy of the
  binary would have doubled the package. The .rpm and Arch package install it as `/usr/bin/dm`,
  as before. dm writes its registrations again for the binary it runs as at each start.
- **Dependencies follow the binary:** it links xcb, xkbcommon(-x11) and FreeType, and opens
  Vulkan, else GL through EGL (Mesa's llvmpipe draws with no GPU), and Wayland's client
  libraries; X11's Xcursor, Xi and libGL, winit's and glow's, are no longer needed.
- **A question is a floating window on Linux, not GPUI's `Dialog`.** GPUI's X11 and Wayland
  windows make a `Dialog` the child of whichever window has the keyboard and close it with that
  window; a question asked as Add URL closed went with it, unanswered. `Floating` keeps it over
  that window without closing with it; macOS and Windows keep `Dialog`. A question whose window
  goes unanswered is answered as Escape answers it, so the questions after it are still asked.
  And a waiting question opens at once on Linux, as the egui window asks there, not once the
  main window is in front: a window manager hands the keys to whichever window it likes as a
  dialog closes, and "Already in the list" waited unseen. macOS and Windows keep their rule
  (the system gives the main window the keys back, and there a question is a `Dialog` owned by
  the window in front). On Linux an open question holds the main window's clicks and keys as
  Options does (`holder`), not what the tray, `--when-done` or the control port asks.
- **The main window is in gpui-component's `Root`,** as every dialog's is: on Linux its drawn
  menus (the top bar's) live in the overlay `Root` holds; without it a click on a menu's title
  opened nothing. The list keeps the keyboard inside it, so commands reach it as before.
- **A patched `gpui-pre-linux` 0.3.12** (`desktop/vendor/README.md`): GPUI tells AccessKit where
  an X11 window stands as it starts and as it moves. Upstream defines the call and makes it
  nowhere, so a screen reader placed every control as if the window stood at (0, 0).
- **Dialogs on X11 are placed where they asked to be** (`syswindow::place`: the position and
  static gravity in `WM_NORMAL_HINTS`, then a move), since GPUI shows a window before dm can
  place it and gives no position; the main window stays where the desktop places it.
- Tried and dropped: setting the X11 app id from dm's build closure (GPUI already sets it from
  `WindowOptions::app_id` before it shows the window); a per-window scale for View › Text size
  (deferred: macOS's slice, not Linux's).

## 2026-11-11: The window tests' unguarded settings writers, found by tracing every save

Gaps row 174: `window::options::site_limits_attempts_and_the_timeout_are_set_in_options_and_reach_the_queue`
sometimes read back `read_timeout` 30 from the shared settings file just after saving 10. To find
every writer rather than guess, `Settings::save_to` was instrumented for one run (not committed):
each save logged its test thread's name, whether that thread held `support::settings_file()`, and
its backtrace. One whole run of the window binary showed 38 tests saving without the guard:

- **The Details hint (33 saves).** `DmApp::details_hint` marks the hint given and saves the whole
  of `applied` the first time a download runs while the progress window is off, which the tests'
  window is (`World::settings`). Every such test wrote the defaults (`read_timeout` 30 among them)
  over whatever a guarded test had just saved. `World::settings` now has the hint given, as it
  has the progress and complete windows off: most tests are about the list, not the hint.
  `a_started_download_says_once_where_its_progress_is`, the hint's own test, turns it back on and
  holds the guard; three tests building their own settings with downloads that run
  (`a_fresh_link_is_taken_from_the_browser`, `a_cut_connection_is_said_plainly`,
  `the_series_error_waits_for_both_ends_and_speaks_german`) have it given too.
- **The rest take the guard:** Add URL's remembered addresses where a test drives the dialog itself
  rather than through `add_url`, Download details' remembered folder, a sort clicked in a heading,
  Options' OK, a script's settings applied, and three tests building their own settings whose
  window gives the hint.

- **The hint's save had been hiding a gap in the test window.** `World::app_with` built the queue
  without the settings' schedules and limits, which a launch puts on it (`shell::start`), and
  `control_cover::a_download_the_schedule_holds_is_not_resumed_over_the_port` passed only because
  the hint's save applied its closed schedule. `app_with` now sets them as a launch does, and
  `app_cover::a_hint_whose_mark_cannot_be_saved_says_why` turns the hint back on for its own check.

After: three traced runs showed no save without the guard (1,193 saves, all held), and the window
binary passed ten whole runs in a row. Taking the guard for the whole of every test was dropped:
it would serialise the binary's 291 tests behind one lock.

## 2026-11-11: The optimization spec, audited: what dm takes from it and what not

The maintainer's spec of 2026-11-11 ("GPUI + Rust Optimization for Cross-Platform Desktop
Applications") is audited requirement by requirement in `docs/optimization-audit.md`. What is not
met became gaps rows 180 to 192, and the agent rules that AGENTS.md lacked were added there.
Decided while auditing:

- **The crates stay as they are.** The spec's layout (`ui`, `app-core`, `download-engine`,
  `network`, `storage`, and so on) is an illustration. Every rule it serves already holds: the
  engine and the application layer use no window toolkit (`cargo xtask layers`), and no
  dependency cycle exists. Splitting `dm-core` into several crates would be churn with no
  boundary gained.
- **The window keeps its cadence.** The spec suggests publishing progress 5 to 10 times a second
  to start with. dm reads a queue change after 250 ms of coalescing, and redraws progress once a
  second while anything runs, as the egui window did. A faster cadence would change what people
  see, and would cost the CPU that row 176 is trying to win back. What the spec does buy is
  drawing only what changed (row 181), not a higher rate.
- **Not applicable to dm:**
  - Intel Macs, which are not built (README).
  - Notarization, which has no free route ("Released unsigned, with the free signing there is", 2026-10-16).
  - A GPU fallback on macOS, since every Mac on macOS 11 or later has Metal.
  - Reduced motion, since dm has no animation.
  - A narrower `dm-core` root. It exports 56 modules and items, each documented (`missing_docs`
    is on). Cutting the list down is a refactor that no rule asks for.
- **`panic = "abort"` is open again for the GPUI window.** It was turned down on 2026-11-06 for
  the egui tray's `catch_unwind`, and `dm-gpui` has none. What is left to weigh is tokio's
  isolation of a task's panic, against the size it saves. That weighing is row 189's, measured.
- **The measurements in the audit's §9** were taken on this Mac while the other track built and
  benchmarked the engine on the same computer, so they hold only as upper bounds. They come from
  `scripts/ui-bench.py` and `stress-check.py window`, and the commands are in the audit's §9.
  ui-bench's Find step waits for a Find next button that only the egui window has, so it
  measures nothing on the GPUI window (row 187).

## 2026-11-11: The GPUI window drags files out through GPUI's own platform drag, not the `drag` crate

Gaps row G10 proposed the egui window's route for the GPUI window: the `drag` crate given the
window's handle, started from a foreground task once GPUI's own drag of a row began, then GPUI's
drag stopped. The pinned GPUI (gpui-pre 0.3.12) turned out to have the route itself: an element's
drag can carry an `external_drag_payload`, which GPUI hands to the system as the pointer leaves
the window (`PlatformWindow::start_external_drag`). On macOS that is AppKit's dragging session,
which is not modal and offers a copy outside the program; on Wayland the data device. So:

- **No `drag` crate in `dm-gpui`.** It would be a feature crate (the clean-room rule), and on
  macOS a second drag beside GPUI's. GPUI's drag also draws the system's own icon for the file's
  kind, and a drag that comes back into the window is GPUI's again.
- **X11 is added to the vendored GPUI** (`desktop/vendor/gpui-pre-linux`, `x11/drag_out.rs`),
  where upstream declines: the egui window's XDND source (`dm-desktop` `xdnd.rs`), moved behind
  `start_external_drag`, on a connection and thread of its own, ending GPUI's drag with a client
  message to the window when it is over. Winit's display is not involved, so the Xlib calls of
  the egui version are gone.
- **Wayland offers a copy only** (a `dm:` line in the vendored GPUI), as the egui window's winit
  does and as macOS does outside the program: a file manager that takes a move would carry the
  download away and its row would say the file is gone.
- **The files are read as the drag leaves the window**, not as it begins, so a file deleted in
  between is not offered; a row is draggable when it, or a chosen row it carries, is finished,
  decided without asking the disk, since rows are drawn many times a second.
- **Windows waits.** GPUI's Windows platform has no `start_external_drag`, so there a row's drag
  stays inside the window. The choice (an OLE drag source of dm's own on `windows-sys`, as
  `drops.rs` builds a drop target, or the `drag` crate the egui window already uses) is left to
  the Windows laptop, where it can be run.

## 2026-11-11: The window's scale is a slider of four stops (issue #32)

A person on a 4K monitor said the window got smaller in 0.9.0 and asked for a way to enlarge
it (GitHub issue #32). Why it is smaller: 0.9.0 still shipped the egui window on every system,
and its commit 066657ed ("reference A's labels, pop-ups and compactness", the maintainer's word of
2026-11-04) took its body text from 14 to 12 points, its controls and rows to 22 and its first
window to 820 by 500. Display scaling is not the cause: the egui window followed the system's
scale, and the GPUI window does too (GPUI links a PerMonitorV2 manifest into the Windows program,
reads `GetDpiForWindow` and handles `WM_DPICHANGED`; its text is 13 points on macOS and 14 on
Windows and Linux, rows 24, so its 100% is already larger than 0.9.0's egui window). The egui
window kept a Text size (Small 0.85, Medium 1.0, Large 1.2 in Options; View › Text size and
Ctrl/Cmd with + and − in steps of a tenth, kept in egui's memory); the GPUI window had none.

The maintainer's words: "previously we had an option to make the UI large, small and medium; we
keep it medium by default", then, the same day, "option of slider ... for scale ui to 60 80 100 or
120 percent. i think slider will be better option". So:

- **Four stops, 60, 80, 100 and 120 percent, 100 the default,** kept in `settings.json` as
  `text_size` (a factor), so either window can read it. Any other number is read as the nearest
  stop; the egui window's Small, Medium and Large as 80, 100 and 120; a percentage (80, "80%") as
  itself. View › Text size › Larger and Smaller (and Ctrl/⌘ with + and −) step a stop, held at
  60 and 120; Normal size (Ctrl/⌘ 0) is 100. The egui window keeps its own control.
- **One scale for everything dm draws** (`dm-gpui` `zoom.rs`): every measure dm gives GPUI goes
  through `zoom::px`, and gpui-component's rem follows through its theme's font size, so words,
  rows, controls, paddings and pictures grow together, as egui's zoom scaled them. What the
  system reports (a window's frame, the screen, the pointer) stays as it is. Tried and dropped:
  sizes in rems with `Window::set_rem_size` (every one of ~375 measures rewritten, and
  gpui-component's `Root` sets the rem itself each frame).
- **Live, without a restart:** each dialog's window is resized by as much as the scale changed;
  the main window keeps its size and its least size, the same on screen at every scale, as the
  egui window's. The top bar is never shorter than the system's traffic lights need; they are
  centred in it for the scale in force when the window opens (the system places them once).
- **Options' slider is not a draft,** as the egui window's Text size was not: a stop is in force
  when the slider is let go (resized under a drag, the window would move the slider from under
  the pointer) and kept whatever OK or Cancel. Each stop's percentage under the track picks it
  too, for the keyboard's and a script's click.
- **The toolbar wraps** to a second row where its buttons do not fit, as the egui window's does,
  rather than running past the right edge at 120%. Still open: the list has no sideways scroll,
  so at 120% in a narrow window the columns after Status are out of sight until it is widened.


## 2026-11-11: Links on the clipboard saves where Download links does (issue #32)

The person who asked for the Text size asked next: "when we choose Add links from clipboard, is
there an option to choose or create a new folder?" Links on the clipboard sent every link to its
category's folder, with no choice. The maintainer pasted the issue as the request.

- **One Save to for both dialogs**, not a second copy: what the choice means moved out of
  `links_import.rs` into `dm-desktop-base` `save_to.rs` (`SaveTo`, `SaveChoice`, which
  `LinksImport` now holds as `save`), and how a GPUI dialog draws it and takes its clicks into
  `dm-gpui` `savefolder.rs` (the heading, the three choices, the categories' pop-up, the field,
  Browse…). Same words as Download links, so no new translation was needed; a folder that cannot
  be made is said with the existing "{} could not be written: {}" and the system's reason in
  dm's words (`format::file_error`).
- **The folder is made when the links are added**, Add for later's too (the engine made it only
  as a download started, so a folder typed for later did not appear until then). A file in the
  way, as the folder or above it, is said as a name taken on every system (macOS says "not a
  directory" for one above it). The dialog stays open with the reason; nothing is added.
- **A name typed alone is a folder in the download folder** (`full_folder(typed, Some(root))`),
  where the queue refused it as not a full path: a person typing "Holiday" there means that.
- **Typing in the field chooses Every file to this folder**, and choosing it puts the keyboard in
  the field, in both dialogs: a typed folder left under "each to its category" was ignored.
- **The system's panel can already make a folder**, so the vendored GPUI is not patched:
  gpui-pre-macos 0.3.12 sets `NSOpenPanel` `canCreateDirectories` (platform.rs:888); on Linux
  the XDG portal's `OpenFile` with `directory` is GTK's or KDE's folder chooser, both with a
  new-folder button (the portal has no option for it); on Windows `IFileOpenDialog` with
  `FOS_PICKFOLDERS` has New folder in its command bar. Read from the sources; no panel was opened
  here (a panel waits for a person, and this terminal cannot move the pointer).
- The egui window (dm-desktop) is not shipped on any system since 6ab92583, so its clipboard
  dialog is left as it was; its Download links follows the moved fields only.

## 2026-11-11: YouTube's panel waits for a list rather than hand the page over unasked (gaps row 178)

`human-qa.py` on macOS failed B33 to B35 and B59 in two of four runs: once the first click on
"Download video" handed the page to dm instead of listing the qualities, and once the player did
not start when the check pressed play.

- **Why the click handed over.** Reproduced on macOS by making dm's reading of the page slow (its
  proxy set to a port that accepts and never answers; a scratch diagnostic, not committed): the
  player's own list was on the panel 0.1 s after the click, and 10 s later the panel said "Sent
  to dm" and "The dm app is reading this page for its video", the list gone, nothing chosen. The
  click took any answer from the background with fewer than two lines as final, even with the
  player's list on screen: none handed the page over for dm to read (and fetch whole), one handed
  that one over. Two things made such answers: the background cut the app's reading short after
  8 s whenever the page had shown a stream, and YouTube's player has fetched one by then (a single
  itag 18 stream was in the tab's kept streams that day), so a slow reading (a debug build, a busy
  machine, a cold player script) came back as that one stream; and a reading that ends with
  nothing. The player's list was asked only once, so a click while the player was still loading,
  or playing an advertisement, had no list to fall back on either.
- **The reading is cut short only for a choice** (`decide::cut_reading_short`: two stream lines
  or more). One stream is no choice, so the app's reading (at most 40 s, the panel saying
  "Reading qualities…" and that it can take half a minute) is waited for.
- **The click asks YouTube's player again** every half second, for up to 15 s, until it lists two
  heights or more of the video on the page (`player_list`), and an app answer with fewer than two
  lines, or a refusal, waits for that before handing over or giving the click back. Past the 15 s
  what the app gave is used as before. A list the player gave stays when the app's answer has
  less to offer. An older ask of the player still waiting is answered with
  nothing when a newer one replaces it, rather than left unanswered for good.
- **The check presses play as a person does** (`youtube-check.py` `press_play`): it waits a
  moment for the page's own autoplay, then clicks the picture with the pointer, then the play
  button, looking again after each, three times at most. One script click could land as autoplay
  began, pausing it again.
- Considered and not built: re-asking the app after a cut-short answer. The cut ask's host may be
  gone with its port, so the second ask would start a new reading and could be cut again.
- **Measured on macOS, 2026-11-11** (debug dm-gpui, Chrome for Testing in a visible window, other
  work running on the machine): `youtube-check.py` 10 of 10 runs passed, 68 of 68 checks each, no
  page refused by YouTube; two runs listed dm's reading with sizes, eight the player's own list
  first. The slow-reading diagnostic, old extension against new on the same build: the old
  handed the page over 10.1 s after the click; the new kept the player's two lines until the
  19 s video ended, 12 s after the click, nothing handed over. `press_play` saw a player paused
  by script play again in three of three cases: twice its first pointer click on the picture
  started it (2.3 and 2.4 s), once a play the page started itself came first (1.8 s); in the ten
  runs YouTube started the player itself every time, so it was not needed.

## 2026-11-11: The download pages' check clicks a link where it rests, not where a scroll left it (gaps row 173)

- **What was wrong.** `handover-check.py libreoffice` said "nothing started" because the click
  never reached the link. With DevTools in the check's own browser: the click went to
  `p.version_heading` (the heading above the buttons), not to `a.os_macos`. LibreOffice's menu bar
  (`#nav-search-wrapper`, `position: sticky`) is 128 pixels tall once the page is scrolled and
  191 near the top. The check scrolled the link to the middle of the window, which took the page
  from 81 pixels down to 11, the bar grew back within a frame, and the link moved 63 pixels down
  (its middle from 433 to 496) after it was measured. The check measured again half a second
  later, but with another scroll, which moved it the same way again. So dm and the extension
  were never asked: no download began in the browser at all.
- **What the page does with a real click.** Its `download.js` cancels the link's own navigation,
  opens the link in a new tab (`window.open(url, '_blank')`), where the browser downloads it, and
  sends the page to `/donate/`. The extension caught the new tab's download and dm fetched it
  from the mirror the 302 named.
- **The fix is the check's.** `link()` scrolls once; then the click waits (up to 10 s) for the
  link to be in the same place two measurements running, measured without a scroll, and to be
  the topmost thing at its middle (`elementFromPoint`), and clicks there. Nothing changed in dm
  or the extension: a person clicks what is on the screen, which is where the link rests.
- The earlier tries in the row (the first link a click reaches, an instant scroll, waiting for
  the link to hold still) are not in the history; if each still measured through the scroll
  in `link()`, as the check did, the link would read the same wrong place every time.
- **Measured on macOS, 2026-11-11** (debug dm-gpui, Chrome for Testing, other work running on the
  machine): `handover-check.py libreoffice` passed twice (the 299 MB `.dmg` whole in dm, nothing
  kept by the browser); the whole `handover-check.py` 68 of 68, all twelve pages OK.

## 2026-11-11: The release profile measured for `dm-gpui`, and kept (gaps row 189)

- **Measured** on this Mac (Apple M1, macOS 27.0.1, rustc 1.98.1, `aarch64-apple-darwin`, commit
  `78ce2636`), each variant a full `cargo build --release -p dm-gpui` with 4 jobs, **with the other
  track building and benchmarking on the same computer**, so times are upper bounds. Peak memory
  is the largest process of the build (the LTO link). Runtime figures are `ui-bench.py --rows 100
  --only start,idle,transfer`, three runs taken in turn across the variants (medians):

  | Variant | Binary | Build | Peak memory | Warm start | Idle CPU | Resident | 512 MB, CPU s (window / `--headless`) |
  |---|---|---|---|---|---|---|---|
  | Current: `"z"`, fat LTO, 1 unit, strip | 19,952,080 | 353 s | 4.35 GB | 0.44 s | 0.15% | 112 MB | 0.63 / 0.56 |
  | Thin LTO | 28,012,032 (+40%) | 287 s | 2.84 GB | 0.42 s | 0.10% | 113 MB | 0.66 / 0.53 |
  | `panic = "abort"` | 15,827,120 (−21%) | 327 s | 4.17 GB | 0.56 s | 0.15% | 112 MB | 0.68 / 0.58 |
  | GPUI's crates at 3 (`gpui-pre`, `-macos`, `-sum-tree`, `gpui-component`) | 22,141,776 (+11%) | 435 s | 5.88 GB | 0.42 s | 0.15% | 113 MB | 0.67 / 0.63 |
  | Symbols kept apart (line tables, `split-debuginfo = "packed"`) | 19,953,296 once stripped | 423 s | 5.69 GB | — | — | — | — |

  The 512 MB file came from the Python server at about 400 MB/s, the same for every variant
  (1.2 to 1.3 s). The start with dm's binary evicted from the file cache varied from 0.4 to 1.8 s
  within each variant, so it ranks none of them. At 10,000 rows (`--only idle,list`, two runs
  each), GPUI's crates at 3 changed nothing measurable: one scroll 0.17 s against 0.16 to 0.17 s,
  a sort by a heading 0.27 s against 0.26 to 0.28 s, the search box's filter 0.36 s against 0.30
  to 0.34 s. Those times are dm's own work on each wake (rows 180 and 181), not GPUI's drawing.
- **Kept as it is.** Thin LTO links faster and in less memory, but its binary is 40% larger, with
  no gain at run time. GPUI at 3 costs 11% and a slower build, for nothing measurable. The
  overrides for `egui` and `epaint` stay while the egui window is still built (G9).
- **`panic = "abort"` not taken**, though it is the largest saving there is (4.1 MB, 21%) and
  changes nothing measured at run time. dm's own code has no `unwrap`, `expect` or `panic`
  (clippy denies them), but what it depends on decodes what servers send: images, audio, torrents,
  HTTP. With unwinding, a panic there ends one task, and the other downloads and the window go on.
  With abort, a server that sends something malformed would close dm and every download with it.
  dm has no crash reporting (AGENTS.md rule 6), so nobody would learn why. Taking it needs a way
  to say what happened first, and is left to the maintainer.
- **Symbols kept apart cost nothing in the binary** (1.2 KB more once stripped: line tables do not
  change the code), but 70 s more build and 1.3 GB more peak memory in the link. Fat LTO's link
  already ran out of Docker's 7.6 GB on Linux (G8). So the release profile does not keep them yet:
  a release with its dSYM, PDB and `.debug` needs the Linux link measured with them first (row
  189, owed to the final Linux pass), and the release job must attach them apart from the
  installers, which the update manifest lists.
- **`[profile.profiling]`** is the release with line tables, symbols and a packed dSYM, for
  `sample`, Instruments, samply, perf and WPA (docs/testing.md, Profiling).
- **What the binary is made of** (`cargo bloat --release -p dm-gpui --crates`, symbols kept; `.text`
  11.6 MiB of a 34.5 MiB unstripped file): `dm_core` 1.4 MiB (11.7%), `std` 1.3 MiB, `gpui` 1.3
  MiB, unknown 887 KiB, `gpui_base` 646 KiB, `dm_shell` 520 KiB, `dm_gpui` 491 KiB, `librqbit` 390
  KiB, `gpui_component` 364 KiB, `aws_lc_sys` 298 KiB, `image` 290 KiB, `tokio` 290 KiB, `rustls`
  228 KiB, `dm_desktop_base` 162 KiB, `usvg` 152 KiB, `librqbit_dht` 147 KiB, `rustybuzz` 136 KiB,
  `retina` 131 KiB, `reqwest` 129 KiB, `ring` 128 KiB; 283 more crates 1.3 MiB.
- **Owed.** The same table on Linux (the final Linux pass) and on Windows (the Windows laptop).
  The judged benchmark (`bench/bench.mjs fast per_conn`) was not run here: it is the other track's,
  and it must be run again by that track after the merge. Nothing in the profile changed, so no
  judged number can move because of this entry.

## 2026-11-11: `cargo deny`, and what it refuses (gaps row 190)

- `desktop/deny.toml` serves both workspaces (`desktop/`, and `browser/` through `--config`). It
  allows the licences dm's 1,100-odd crates carry that are compatible with GPL-3.0-or-later, and
  allows OFL-1.1 and the Ubuntu font licence for `epaint_default_fonts` only, the egui window's
  fonts, until that window goes (G9). crates.io is the only source. Duplicates are warned about,
  not refused (about 100, from upstream crates moving at different speeds). The advisories ignored
  are `audit.toml`'s, each with its reason there.
- **"Unmaintained" is refused only for crates dm depends on itself** (`unmaintained =
  "workspace"`). The four found (`instant`, `rustls-pemfile`, `rustybuzz`, `ttf-parser`) come in
  under the pinned GPUI (`gpui-pre` 0.3.12, `gpui-component` 0.9.1), where dm cannot replace them.
  Refusing them would keep the check red until upstream moves. cargo audit still lists them weekly.
- dm's own crates were marked `publish = false`, which they always were in fact: cargo-deny
  counts a path dependency of a publishable crate as a wildcard version.
- Checked with cargo-deny 0.20.2 on this Mac: `advisories ok, bans ok, licenses ok, sources ok` for
  both workspaces.
- **Not done**: `criterion` benches (the spec asks for them, run by hand, never gating CI). They
  are a dev-dependency with a large tree, and were left for a session of their own (row 190).

## 2026-11-11: The end of a download: a range halved only where it is worth a request, each seam asked for once, the staged file held open (gaps rows 171 and 177)

The macOS benchmark (Apple M1, GPUI release build) had `fast` worse than reference A's floor in
time from the first request (0.327 s against 0.268 scaled), and in bytes fetched twice (4 to 7%
against 2.6), and `per_conn` tied it at 0.1%. The engine was traced for these runs (each request,
first byte, cut, split, check and the finish, with the time the server saw them; not committed),
which showed three causes, each changed:

- **A 150 ms stall in every fast download.** macOS writes out what a file still holds in memory
  when its last open handle is closed, before the close returns (a Python check: 512 MiB written by
  eight threads, every close a few microseconds, the last 143 ms). Each range attempt opened the
  staged file and closed it at its end, on a runtime thread, and whichever closed last in the end
  game held its connection, and every range opening the file meanwhile, 140 to 160 ms. The job
  now holds the staged file open from `preallocate` until the file is placed (`Job::holding`) and
  lets it go on a blocking thread: waited for where the parts are removed or made again next
  (Windows keeps a file being deleted while a handle is open, and a new one of its name cannot be
  made), not waited for once the file is placed. The mark of where it came from and Completed come
  within 5 ms of the last byte; the write-out happens after.
- **Halving however soon a range would end.** A connection with nothing left halved the range with
  the most bytes left, so the end of a fast file was a cascade (nine halvings of its last range in
  5 ms; 38 to 60 requests for 8 ranges), and each halving stopped the range's own connection short
  of the end it had asked for: what the server had sent past the new end, 2 to 8 MiB a time over
  loopback's buffers, came again. A range is now halved only where its own connection would take
  longer than `SPLIT_WORTH` (0.1 s), or twice what this download's requests waited for their first
  byte, to fetch what it has left at its pace over its last 20 ms window (`Going`, kept under the
  per-range lock the chunk loop already takes); and only once 40 ms have passed since a range last
  came whole, so the ranges still going have taken up its share (`SPLIT_SETTLE`). A range whose
  answer is still on its way is left be, until it has waited a second (`STUCK_AFTER`), when it
  counts as stuck however little it has left, as does one that has gone a second without bytes. A
  connection that finds nothing worth halving waits rather than ending (woken when a range ends or
  is given up, otherwise every 25 ms), since a range can stall or slow later; it ends once no range
  under way is long enough to halve.
- **Checks that overlapped.** Each range's last 4 KiB was asked for again when it ended, and 2 KiB
  either side of up to eight seams again one after another after the last byte: 66 KB of a 64 MiB
  file, all of `per_conn`'s 0.1%, and eight round trips in every finish. Now a seam is asked for
  once, 512 bytes a side, by whichever of its two ranges is whole second, as the download goes on
  (`assemble::check_seams`), and the file's last 512 bytes once; nothing is asked after assembly
  begins. A 200 under the validator is still another version of the file, and a difference restarts
  the file on one connection with the row saying why, as before. The 64 bytes before each range
  (`LEAD_BYTES`), compared at assembly, stay.

And one tuned figure moved with them: `HELP_WORTH_SECS`, the share of a file left at which the
helpers of a small file look for a range to halve, from 0.3 s to 0.2 s. With the range's own test
deciding whether a connection is opened, it no longer guards against a useless one; at 0.3 s,
`batch`'s 2 MiB files at 2 MiB/s a connection were seen with 0.29 to 0.34 s left and were helped
or not as their bytes fell into the 80 ms watched, 0.32 s with help and 0.52 s without.

Measured on the same machine, one build after the other (`bench.mjs`, three runs where the
harness allows): `fast` 0.211 to 0.230 s from the address against 0.329 to 0.448 before, 0.6 to
1.0% fetched twice against 3.3 to 6.0, 16 requests against 36 to 60; `per_conn` 0.0% against
0.1; `flaky` 0.0 to 0.1% against 0.3 to 0.5; `batch` 2.9 to 3.2 s against 3.3 s, 2.2% against
4.4; `big40` 0.05% against 0.15; `outage` 0.3% against 0.6, its time unchanged. `fast`'s
`lastByteToDoneMs` (77 ms against a scaled floor of 68) is the harness's: dm says Completed 5 ms
after the last byte, and the harness asks `queue.list` every 100 ms plus a round trip from the
moment it hands the address over, so a download that ends about 150 ms in is seen at the third
look, 80 to 95 ms later, every time.

Tried and dropped:
- A fixed half-second (the audit of 2026-11-11 suggested about that): `batch`'s helpers met ranges with
  0.3 s left and would have halved none, and its files took the 0.52 s of two connections.
- The pace since the attempt's first byte, and judging at once: a range that had shared the line
  with seven others looked three to four times slower than it was once they were done, and was
  halved for nothing; counting the first chunk in its window put a capped range above its cap.
- Settling only after the connection's own range: the ranges beside it were still ending, and one
  fast run in two still halved one, at 4 to 8 MiB fetched twice.
- A kibibyte a side for the seams: the probe's answer runs past the first range's end by what the
  server had in flight (4 to 18 KB at 1 MiB/s), and with 15 KB of checks one `per_conn` run in
  four came to 0.05%, printed 0.1.
- Not changed: the probe's `bytes=0-`, whose answer is the first range's and is dropped at that
  range's end. What the server had in flight then, 2 to 8 MiB over macOS's loopback, is now nearly
  all that `fast` fetches twice. A bounded first request would cost every download a round trip on
  its first range to save it.

## 2026-11-11 (evening): What is still fetched twice, the finish measured to the millisecond, a range's checks asked at once (gaps rows 171 and 177)

The work of the afternoon (926b703c, the entry above) was cut off before its numbers were taken a
second time. Measured again in the evening on the same Apple M1, now much busier (`auditd` at 63%
of a core and the antivirus's two daemons at 15% each, beside another session's builds), six runs
each of 926b703c and of the change below, the two builds alternated (`bench.mjs ceiling fast
per_conn flaky`, three runs a build at a time, four times): nothing of the afternoon's change hurt,
and it was kept whole. Medians, 926b703c then the change: `fast` 0.329 and 0.329 s from the
address, 1.8 and 1.6% fetched twice; `per_conn` 8.084 and 8.088 s, 0.0% in all twelve; `flaky`
8.075 and 8.066 s, 0.05 and 0.1%.

**What is fetched twice now**, request by request (each run's `perRequest`: the bytes of every
request against those of every other): in `fast`, the probe's answer, cut at the end of the first
range with what the server had already sent past it, 4 MiB when dm reads quickly and 9 MiB when the
computer is busy and the socket buffers fill; then 7 seams of 1 KiB, the file's last 512 bytes and
64 bytes before each range, 8 KB in all. In `per_conn`, 10 to 16 KB of the probe's answer past the
first range and the 8 KB of checks: 0.03%. In `flaky`, 25 to 41 KB each time a range is halved
under its connection, about twice a run (26 times in twelve runs), and 10 KB of checks. No reconnect
asked from a stale place (in those twelve runs each of the 426 requests after a cut asked from
exactly where the cut left its range), no range was asked for twice at the end, and no halving
overlapped its range beyond the 64 bytes asked on purpose. What remains is what was in flight when a connection was let go, which no
request made after it can take back.

**The finish, measured.** dm was traced (each range whole, each check asked and answered, assembly,
the runner, Completed; with the server's own times; not committed) under a copy of the harness that
looks every 5 ms rather than 100. From the last byte of the file to Completed: `fast` 5.7 to 7.3 ms,
`per_conn` 14.2 to 17.6, `flaky` 6.6 to 9.5. Of `per_conn`'s, 11 to 14 ms were the checks of the range
whole last, asked one after the other: it nearly always has two (a seam either side, or a seam and
the file's end), and that server answers a new request 4 ms after it comes, so the second check
waited out a round trip the first had already spent. They are now asked at once (`check_seams`,
`join_all`), on whichever connections are free; the finished ranges leave theirs open, so no new
one is made in the usual case. Assembly read the 64 bytes before each seam through an open, a seek
and a read apiece, each a hop to the blocking pool; one blocking task now reads them all through
one open, and the part of assembly before the file is placed went from 0.8 to 2.6 ms to 0.4 to 1.0.
After: `fast` 4.4 to 5.9 ms, `per_conn` 8.9 to 12.1, `flaky` 6.2 to 7.8. On a real link the checks
asked at once are a round trip less at the end of nearly every download.

**Why the harness still sees 9 to 107 ms.** `bench.mjs` asks `queue.list` every 100 ms (each answer
takes 1.2 ms, median of 40) from the moment it hands the address over, so what it reports is dm's
finish plus the wait for its next look. Against `flaky`'s floor of 71 ms a median of three runs comes
out over it about one time in three whatever dm does in those few milliseconds: two of the five
medians taken this day did (73 and 76.5 ms). `fast`'s floor scales with the ceiling measured in the
same run, 68.8 ms with a 0.225 s ceiling and 80.7 with 0.264, and its verdict follows where the look
falls.

**What holds `fast` here.** The same builds took 0.15 to 0.27 s in the computer's quieter minutes and
0.86 to 1.7 s in its busier ones, with the reference client's own ceiling between 0.225 and 0.38 s;
judged against a floor scaled by that ceiling, `fast`'s time was worse with one run's ceiling and
better with another's though the medians were the same, 0.329 s. The bench's server is one thread:
eight native readers that write nothing take 512 MiB from it in 128 to 172 ms, but dm's seven range
requests besides the probe, sent within half a millisecond of each other, were taken up 2 to 37 ms
after the probe while the server streamed its answer, and the range taken up last ended last.

Tried and dropped:
- Leaving the staged file to grow on macOS rather than making it full length first. Eight threads
  writing a region of 64 MiB each front to back, as the ranges do, took 293 to 416 ms into a file set
  to 512 MiB first and 155 to 200 ms into one left to grow; but dm's `fast` over eight pairs of runs,
  the two builds alternated run by run, was 0.285 s with the file made full length and 0.272 s
  without (medians), five pairs of eight faster with it. Not a gain anyone could measure, against a
  branch for one system and a resume that would have had to make the file full length before its
  check reads a range's end. Not kept.
- Leaving the probe's answer unread until the other ranges' requests are answered, so a server of
  one thread takes them up first: it tunes dm to the bench's server. A server with a thread or a
  task per connection has no such queue, and on a real link the first range's connection would stop
  for a round trip.
- Asking the probe for a closed range (`bytes=0-N`) so nothing is cut: it would save `fast`'s 4 to 9
  MiB, but every download's first range would then need a second request and a round trip of its
  connection, and the probe is where the fallbacks for servers that refuse an agent or a referrer
  live. Not done; `fast` is at 1.4 to 1.8% against the floor's 2.6.
- Halving a range only where the download's rate fell when the last range ended (a limit shared by
  every connection is not helped by one more): `flaky` would save its 25 to 41 KB a halving, but on a
  shared link a connection slow for its own reasons would be left to end alone. Not done.
- Writing each range through a duplicate of the handle the job holds on the staged file, to save an
  open per attempt (0.2 to 1.6 ms between a range's answer and its first byte written): a duplicate
  shares the file position, so two ranges seeking and writing at once would put bytes in each
  other's places. Positional writes would not, but change the whole write path for that.
- Removing the ledgers and the plan after Completed rather than before (0.8 to 2.8 ms of the
  finish): a download of the same row started at once would have its new ledgers removed under it.

## 2026-11-11 (night): CPU while a held-down download runs (gaps row 176)

On macOS (Apple M1, GPUI release build) `per_conn` had used 1,860 ms against the floor's 1,281 and
`flaky` 1,885 against 1,406, and a `sample` of the process showed no hot spot. Measured again by
thread (`ps -M`), with the instructions and cycles the kernel counts for the process
(`proc_pid_rusage`) beside its CPU time, and a symbolized release build under `sample`, with a
copy of the bench's server driving one download at a time (not committed):

- **The window drew for the harness.** `bench.mjs` asks `queue.list` every 100 ms, and each request
  woke the window for a whole look: a read, the tick and a frame, ten frames a second while the
  window was on screen, and after the first download "Download complete" was drawn with each of
  them too (it was drawn again whenever the list was). GPUI draws nothing in a window it cannot
  see, which is why the same build's main thread took 30 to 80 ms of a run in some runs and 260 to
  990 in others: whether the window was covered.
- **A running download was read once a second.** Progress raises no signal (`EVENT_FLOW.md`: it is
  polled), and the GPUI window's clock was a second while a download ran, where the egui window and
  that document say a quarter: a person saw the figures move in jumps.
- **The engine wrote each piece a held-down server sends as a write of its own.** At 1 MiB/s a
  connection the bench's server sends 16 to 20 KiB at a time, over the 16 KiB buffer, so about 450
  pieces a second each went to a blocking thread and a `write`. With the window covered, the
  engine took 1.85 to 2.03 G instructions for `per_conn`'s 64 MiB.

What changed:

1. `queue.list` is answered on the control port's own thread (`control::listen_reading`, the
   reader in `dm-gpui/src/port.rs`), from a snapshot of its own with each finished row by its
   file's size as the window's read gives it, and who may ask as the window says
   (`control_api::refused`): the window is neither woken nor drawn for it, and the answer does not
   wait behind a frame. Anything else a script asks is the window's, as before. First tried as an
   answer on the window's thread that skipped the look and the frame (`Wake::Asked`); the port's
   thread is simpler and cheaper again (3 % fewer instructions with the window on screen), and an
   answer no longer waits while the window draws.
2. The window reads and draws every quarter of a second while a download runs (`RUNNING`), a second
   for When done's countdown and the clipboard, 30 seconds otherwise; its clock runs from its last
   look, and a queue change's quarter-second wait ends no later than that.
3. "Download complete" is drawn again only when what it lists changes.
4. A range gathers 64 KiB before it writes (`WRITE_BUFFER`): the most a held-down server sends at a
   time, which tokio's own copy of each write already grows to, so a download of eight connections
   holds 384 KiB more. A fast server's pieces are larger than the buffer and pass as before. And
   what came is written down within the ledger's quarter second even while the server says nothing
   more (`attempt_segment`, `tests/quiet.rs`): held in the buffer, less than a buffer's worth sent
   and then nothing stayed off the disk a preview reads, and past the ledger a crash goes back to,
   for as long as the server was quiet (as up to 16 KiB had before). The window test of a preview
   from what is on disk (`tests/window/downloads.rs`) now has its server send 128 KiB, two buffers,
   where 64 KiB was four of the old.
5. A piece of words the window draws records where it was laid out from the box around it
   (`Screen::mark`, `on_children_prepainted`), not from a canvas of its own beside it: one element
   fewer to lay out and paint for each, about a quarter of the main window's; 2.5 % fewer
   instructions with the window on screen.

Measured, `per_conn` with the window brought to the front (`ui.window shown`), the server polled
every 100 ms, the two builds alternated: the old 2.46 to 2.88 G instructions and 600 to 1,060 ms,
the new 1.18 to 1.39 G and 390 to 610 ms. The engine alone (`--headless`, interleaved): `per_conn`
1.86 G instructions to 0.87, `flaky` 1.13 G to 0.99. The bench, the old and the new build
alternated: in a quiet hour `per_conn` 1,550 ms to 1,000 and `flaky` 1,630 to 1,130 (medians of
three, before changes 1's port thread and 5; the old build over its floors, the new under), and in a
busier one 760 to 540 and 725 to 560 (of six). The whole bench on the new build
(`bench/results/darwin-2026-11-11`): `cpuMs` better than the floor in every scenario (`per_conn`
730, `flaky` 840, `fast` 440, `outage` 1,810, `big40` 2,695, `idle` 30), the time and what was
fetched twice as before. The peak working set was 111 MiB against the old build's 118 to 120 in the
alternated runs, and 133 to 142 in the whole run, as far from its floor as it was (row 175).

**Why one run's CPU is not the next one's.** Counted per run, cycles over CPU time came to 1.1 to
3.0 GHz and dm's time on the fast cores to 10 to 39 % of it, while the instructions moved by a few
percent: the same work took up to two and a half times as long, by where the system ran dm's
threads and how fast, and in the computer's quiet hours most. That is how the old build measured
1,860 ms earlier in the day, 1,550 in the quiet hour and 760 in the busier one, and why builds were
compared by instructions, alternated, never by one run's `cpuMs`.

**The finish in that run.** `flaky`'s `lastByteToDoneMs` came out at 84 ms there (29, 84 and 85),
over its floor of 71, and 83 to 87 in the run of `per_conn` and `flaky` before it. It is where the
harness's look every 100 ms falls: with a copy of its server and its looks, both builds came out
either 5 to 20 ms or 90 to 110 (the old: 98, 5, 99, 96, 109, 19; the new: 92, 94, 100, 10, 8, 106),
the run finishing just before a look or just after it, and a `flaky` run is nearly the same length
each time. Looked at every 5 ms, dm said Completed 6 to 12 ms after the server's last byte with the
new build and 4 to 18 with the old.

Tried and dropped:
- Fewer of tokio's workers (`TOKIO_WORKER_THREADS` 2 and 4 against the 8 the M1 gets), alternated
  over three rounds: 531, 568 and 572 M instructions, 535, 600 and 530 ms. Not a setting worth having.
- GPUI and taffy built at opt-level 3, as egui and epaint are: the binary 0.99 MB larger
  (19,901,696 bytes to 20,888,432) for 5 to 8 % fewer instructions with the window on screen. A
  frame is GPUI's layout and prepaint of the whole tree (`sample`: 77 of 90 samples on the main
  thread were `Window::draw`, half of it taffy's layout, dm's own `render` 3), which the level
  barely moves. What would: drawing again only the parts that change (GPUI's cached views), which
  the window's one view and its record of the words on screen do not allow as they are.
- Not tried, written down: reading a slow connection's bytes in fewer, larger reads by pausing
  between them. The bench's server sending 64 KiB pieces in place of 16 KiB took dm's engine from
  0.83 to 0.88 G instructions to 0.56 to 0.66, so the share per piece is real; but every pause
  would add to the finish after the last byte (row 177) and to the time a cut connection waits to
  be asked again, and the kernel still wakes dm for each piece that arrives.

## 2026-11-12: Memory on macOS: a download's large blocks go back to the system, and what any GPU window costs (gaps row 175)

What the benchmark reads on macOS is the resident size `ps` gives (`rss`), every 400 ms;
`peakWorkingSetMB` is its highest in a run. It counts the pages in dm's own page tables: its heap
and stacks, its code, the system's data files it has read, the pages of the system libraries' data
it has touched, and of GPU memory only what the CPU has written; not the libraries' code, which
every program shares. `vmmap` lists more as resident than that: the window's drawables (two of its
three 1800 by 1200, 8.4 MiB each, at rest) are drawn by the GPU and composited by WindowServer, and
a bare window measured below is 83.1 MB with none drawn and 84.5 with three. A Metal buffer or
texture counts only as the CPU writes it: in a test program a 32 MiB shared buffer counted nothing
when made, 4 MiB once 4 MiB were written, and a texture as its rows were uploaded. It is not the physical
footprint (`footprint`), which counts only what dm alone holds and was 49 MB at rest against
107 MiB resident. dm-gpui (release, Apple M1, macOS 27, a home of its own, the window on screen) was
taken apart with `vmmap`, `footprint` and `heap`, at rest and around downloads driven through the
control port by a copy of the bench's server (not committed).

**At rest**, 106 to 108 MiB, steady over two minutes; of what `vmmap` lists, beside the drawables:
- the heap, 17.5 MiB: 11.5 MB in use (4.2 MB of it dm's and GPUI's own: GPUI's two arena chunks of
  1 MiB, its scene, taffy's nodes, the keyboard layouts; the rest AppKit's menus and their SF
  Symbols, CoreText, the Objective-C runtime) and 5.7 MB freed during the start and kept;
- the system's data files dm has read, 14.3 MiB, clean pages: the SF Symbols catalogues (5 MiB,
  pulled in by the menus' separators), LaunchServices' store (2.2), the system font (1.7), the
  keyboard layouts (1.6), the appearance catalogues and ICU;
- dm's own code, 8.2 MiB of its 17.3, clean;
- GPUI's Metal buffers, 3.9 MiB listed: the frame's instance buffer (2 MiB) and the glyph atlas
  (1 MiB), of which only what each frame writes and the glyphs uploaded count;
- and the rest, the system frameworks' own data pages dm has touched: AppKit loads the same 1,140
  images whatever a program links.

**What any GPU window costs.** A 60-line Objective-C program that opens an `NSWindow` of the same
size with a `CAMetalLayer`, with no menus, text or Rust, is 81 to 83 MiB resident: 81.1 with
nothing drawn, 82.3 to 83.3 with one to three drawables drawn (four runs). Linked with every
framework dm-gpui links it is the same (the same 1,140 images). So no GPU window on this computer
comes near reference A's floor, measured on Windows (39.2 MiB at rest, 20.9 to 21.6 while
downloading); dm's own share at rest is the 24 to 26 MiB above that program: its code (8.2 MiB),
its heap beyond the program's (14 to 16 MiB of small-block regions against 8.7, the start's
leftovers among them), the data files its menus and keyboard read (about 3), what GPUI writes of its
Metal buffers, and the frameworks' pages its own calls touch.

**While downloading it rose, and stayed.** 107.5 MiB at rest, 129.5 after one `fast` download
(512 MiB), 134.7 after two `fast` and two `per_conn`, 134.4 ten seconds later; with no window
(`--headless`) 21.7, 39.6 and 42.5. The heap then held 14.2 MB in use in 37.1 MiB resident,
22.9 MiB freed and kept; headless 0.6 MB in use in 20.3 MiB, 17.4 MiB of it regions with nothing in
them. macOS 27's allocator gives freed memory back only when the computer runs short: in a test
program, 64 MiB freed in blocks of 64 KiB, 416 KiB, 1 MiB, 4 MiB or a mix stayed resident for as
long as it was watched, and `malloc_zone_pressure_relief` released none of it (which is why the
timer of 2026-10-27 "did not move" RSS). It came back once, when the computer was short of memory,
as it was in one bench run where `per_conn` measured 98 MiB after three `fast` downloads, and in one
`outage` run below. The blocks were a download's largest, each freed as its connection or range
ends: halfway through a 3 GiB download from the local server, 7.7 MiB of them were alive, eight
read buffers of 416 KiB (hyper grows each a step at a time from 8 KiB to 408), eight of tokio's
copies of what is being written (up to 640 KiB) and eight write buffers of 64 KiB.

**What changed.** `dm_core::memory::Allocator`, dm-gpui's global allocator on macOS, maps each block
of 64 KiB or more on its own (`mmap`, tagged "App-Specific Tag 1" for `vmmap`) and unmaps it the
moment it is freed. Smaller blocks stay with the system's allocator, so the many small ones cost
no call to the system each. Windows and Linux keep the system's allocator: not measured there.

**Measured**, the old build and the new alternated (`peakWorkingSetMB`, medians):
- `node bench/bench.mjs idle fast per_conn`, three rounds of three: `idle` 107.0 and 107.1 (no
  change); `fast` 131.7 MiB (123.9 to 134.9) to 119.2 (109.8 to 126.4); `per_conn`, after the
  `fast` runs, 134.6 (132.2 to 136.0) to 120.8 (119.8 to 121.9).
- `flaky` (three) and `outage` (one), three rounds, each from a fresh dm: `flaky` 120.8 (105.1 to
  121.9) to 117.2 (110.3 to 119.7); `outage` 121.7 and 122.0 to 114.0 to 120.4 (the old build's
  third, 62.6, came while the system was short of memory and took back pages of its own).
- The driver: after two `fast` and two `per_conn` and ten seconds, 134.4 MiB to 116.3 (the heap
  17.5 MiB resident, 0.9 of it empty regions, and 1.7 MiB of mapped blocks; footprint 79 MB to
  59); headless, after the same four, 42.5 to 42.9 MiB to 25.4 to 28.0, and 38 to 40 at the peak of
  a `fast` download to 34 to 35.
- CPU: over three `flaky` downloads the whole process ran 7.12 G instructions with the old build
  and 7.19 G with the new (medians of three, +1 %), with 8,970 page faults to 11,100 and 71,200
  system calls to 73,000, the new buffers' fresh pages and their maps; `cpuMs` per run 620 to 600
  (medians of nine). Headless, per download: `per_conn` 893 to 912 M instructions, `fast` 3.0 to
  3.2 G (medians of six, in the runs' spread). Seconds as before.

The whole bench on the new build afterwards, in one process as it runs (`bench/results`, quieter
hour): `idle` 106.6 MiB, `fast` 114 to 117, `per_conn` 120, `flaky` 120, `outage` 120, `batch` 125
to 127, `big40` 127 to 129, every other parameter as before; the build of the night before had
133 to 142 in its whole run. What is left at rest is the window itself, which this does not touch;
in a download, the Download complete window from the first one on (about 5 MiB beyond what the same
download costs with no window: its own GPUI state and the code and data its first drawing reads),
and dm's code and data files read for the first time.

Tried and dropped:
- `malloc_zone_pressure_relief`: releases nothing on macOS 27.
- Linking fewer frameworks (`-dead_strip_dylibs`): AppKit loads them all anyway.
- Two drawables in place of three: drawables are not counted in the resident size (the bare
  window measures the same with one drawn or three), and GPUI's display link hands a drawable out a
  frame ahead, so frames would wait for one.
- Not listing the installed fonts at the start (gpui-component does, once, to check the theme's
  font names; 3 MiB resident in a test program): skipping it means naming the theme's fonts
  another way, which could change the window's text.

- Starting GPUI's Metal buffers smaller (its 2 MiB instance buffer, each window's 1 MiB glyph
  atlas) in a patched copy of `gpui-pre-apple`: they count only as far as the CPU writes them, and
  a frame writes what it needs whatever the buffer's size, so it would change next to nothing.

Not done, written down: tokio's file copies each write a second time (about 3.5 MiB at full speed
on eight connections), which only writing without tokio's file would avoid; and the 5.7 MB the
start frees and the heap keeps are small blocks, below what is mapped on its own.

## 2026-11-12: After an outage, a download's connections take turns asking, and all come back with the first byte (gaps row 177)

**What was measured.** `bench` outage (64 MiB at 8 MiB/s shared; at 30% every connection is cut
and every request dropped unanswered for 15 s), the release build of a3a28292 on macOS (Apple M1):
the first byte after the server returned came 195 to 216 ms late in seven runs. The eight
connections are cut in the same instant, ask again at once, then 279, 665 and 1,052 ms later all
together (each lane's `quick_nap` is the same for the same attempt), and from then on each on its
lane, a quarter-second apart, about every two seconds. The attempts' own few milliseconds move
that pattern a little, and in the bench they put the first request after the fifteen seconds
about 0.2 s late every time. The other seven connections then came back on their lanes, the last
1.75 s after the first: a copy of the bench's outage server that writes down every request, run
beside it, saw them back at 210 to 1,961 ms, 200 to 1,954 and 34 to 1,790. 80 requests dropped.
For an outage of any length the lanes leave seven gaps of 250 ms between asks and one of 250 to
500, a wait of about 135 ms on average after a server returns.

**What a request can and cannot do.** During the outage the bench's server reads each request
and drops it; only one that reaches it after the outage is answered. So the wait after the server
returns is the time to dm's next request: nothing can be held open at the server, and nothing
tells dm when to send. With a request every 100 ms the wait is between 0 and 100 ms, 50 on
average, and 0 (the floor) about one time in a hundred. Reference A's 0 is about six hundred
requests a second (8,054 to 10,200 dropped in 15 s), and even so its four runs read 0, 1, 6 and
16 ms.

**What changed.** `retry::Together`, one for each download (`Job::together`, which also holds the
count of requests bringing bytes in that `grow` reads):
- A connection that failed for want of a connection, was asked again at once (as before) and
  failed again, while no request of the download is bringing bytes in, waits for its turn rather
  than its lane's time. Each connection's turns are a round apart, 0.8 s, and the download's are
  the round shared among its connections, at least 100 ms apart: eight connections ask in turn
  every 100 ms, ten requests a second for the download, evenly.
- The first turn of an outage falls at a random point of its share (`getrandom`), so computers
  or downloads that lost a server in the same instant do not ask it in the same instants, and the
  wait after an outage does not hang on how long the outage lasted. Turns counted from the
  outage's first instant would read a few milliseconds in the bench, fifteen seconds being a whole
  number of tenths, while an outage a few milliseconds longer waits a tenth more: a reading of the
  bench, not of dm, so not built.
- From twenty seconds in, the round is a twenty-fifth of how long the failing has lasted, up to
  2 s from fifty seconds on: a long outage is asked about four times a second, as the lanes asked.
- The first byte any request of the download gets (`Together::bringing`, where `grow`'s count was
  kept) sends every connection waiting its turn at once, and one whose attempt failed after that
  byte came asks again at once.
- Unchanged: a connection failing while others bring bytes in (a server turning one connection
  away is not asked ten times a second), a wait the server or the person named, and FTP, HLS,
  DASH and the probe, which have no `Together`.

**Measured.** Alternated with a3a28292, four runs each: the first byte after the server returned
216, 205, 199 and 204 ms against 58, 82, 37 and 15; the whole download medians 23.260 and 23.099 s,
from the first request 23.240 and 23.083 (floor 23.312); requests dropped 80 against 157 and 158,
112 to 122 in all against 183 to 189; `cpuMs` medians 1,315 and 1,290; peak working set 115.7 to
116.6 MiB against 113.0 to 116.8. Three runs of each before that, not alternated: 196, 196 and 195
against 28, 73 and 48 (`cpuMs` 690 to 740 against 1,360 to 1,400, the first three in a quieter
minute; alternated, the same). Medians of the seven runs of each: 199 ms against 48. The
committed build, three runs into `bench/results`: 92, 65 and 43 ms, 23.073 to 23.093 s, and
`compare.mjs` better than the floor in every outage parameter but `peakWorkingSetMB` (row 175) and
`recoveryAfterOutageMs` (65 against 0). In the copy of the server that writes down every request,
the other seven connections came back within 2 or 3 ms of the first in three runs (35 to 37 ms,
34 to 37, 90 to 92), where they had taken 1.75 s. `tests/errors.rs`
`connections_that_lose_the_server_take_turns_and_come_back_together` fails with the turns switched
off: the server went unasked for 1.05 s of its 2.5 s outage.

Not done, written down:
- One connection asking for all while the others wait: the same number of requests finds the
  server no sooner, and one held in a twenty-second connect would hold every connection; with
  turns each connection asks for itself.
- Turns 50 ms apart: about 300 requests in the bench's fifteen seconds, for an average wait of
  25 ms instead of 50.
- Turns shared among the downloads from one server: each download takes its own, so two at once
  from one server ask it twenty times a second.

## 2026-11-12: The main window reads and draws what changed; quitting stops the downloads (gaps rows 180, 181, 182, 186)

- **A read keeps what only figures changed.** The queue hands out a row's own `Arc` until it
  changes it, so `Downloads::read` finds what changed by comparing pointers. Where only figures
  moved (bytes, rate, time left; `placed_alike` compares the rest that names and places follow),
  the names, places and choice are kept; they are worked out again where a row was added,
  removed or changed otherwise, after Options, and every 2 s while anything changes, so a
  waiting row's "name (1)" still follows its folder. Working everything out at every read was
  most of the cost of a scroll at 10,000 rows (0.17 s, now 0.03 s). `SavedSizes` gives a
  finished row the same copy while the queue's is the same, or a joined MP4's row (its size on
  disk not the engine's count) looked changed at every read.
- **The names' folder cache drops what expired, swept when it has doubled.** Emptied past 2,000
  entries, it asked the disk for every one of 10,000 waiting rows at every read; swept at every
  miss it would have been quadratic.
- **A wake draws only where something shown may have changed** (`needs_drawing`): a change found
  by the read, a request answered, something that moves with the clock (a running row, a retry,
  When done, a sentence about to go, a file's days before the Trash), or a change beside the
  rows (problems, the status bar's sentence, sheets, the menu, the choice). The tick's helpers
  that change the window otherwise notify it themselves. Splitting the window into views of
  their own, so a running row does not build the toolbar and sidebar again, is not done: it is
  row 181's remaining part.
- **Frames are counted on the port's own thread** (`ui.frames`): asked through the window, the
  question would have drawn a frame itself.
- **A file's "missing" is looked for off the window's thread**, for the rows on screen and those
  chosen only, at most once a second and at once where the view moved or the window comes to the
  front. Looking at every finished row was dropped: 10,000 `stat` calls a second while downloads
  run, for rows nobody sees. Until a row has been looked at it reads as present, as a file just
  finished is.
- **The row at the top of the view is held** where rows come and go above it (`holding_top`), not
  at the very top, where new rows are meant to be seen. Rows are keyed and handled by download id.
- **Quitting stops and awaits the downloads** (`Queue::shut_down`, 2 s at most, then the rest
  aborted). The rows go back to Queued, as a restart made of Running ones before. A restart still
  fetches each part's last 64 KiB again (`RESUME_CHECK`), as after any pause: skipping it needs a
  stop that syncs the part and marks its ledger, an engine change left to the engine's track.
- **`ui-bench.py`'s paging under load is a dm of its own with 60 downloads** (10 at once, dm's
  most), so the CPU figure with 20 stays comparable with the stored baseline.

## 2026-11-12: Slow work off the window's thread; panels that do not hold it (gaps rows 183, 180)

- **One errand thread, in the order asked** (`dm-shell` `app/errands.rs`, `Backend::later`). A
  handler's slow work (a command on the chosen rows, Start and Stop Queue, Move/Rename, Update
  download link's and Properties' OK, Delete's Trash, Export, the settings file after a drag) is
  handed to it, and its answer comes back as a future the window awaits and applies on its own
  thread (`list/later.rs`). One thread, not a pool or the runtime: two commands given one after
  the other (Stop on 10,000 rows, then Move up) still happen in that order, as when the window
  waited for each. The errand calls the same blocking `Backend` methods as before, so their
  logic and words are unchanged; it is not the runtime's thread, so their `block_on` is allowed.
- **A `command.run` answer waits for the errands it started**, held on the window's side and
  sent when they are done, with the status bar's sentence then. A script that runs Stop and
  reads the list finds every row stopped, as before; `ui.texts` on another connection is
  answered meanwhile. Clicks (`ui.click`) are not held: they are a person's, who sees the result
  come.
- **Import still adds on the window's thread.** The rows it adds are marked the window's own
  (`Session::own`) once their ids are known; added from an errand, a read in between would take
  them for rows started elsewhere. Reading the file is quick; adding 10,000 rows is not, and is
  what is left of row 183.
- **The panels.** `rfd`'s sync panels run a modal loop on the window's thread for as long as
  they are open. `rfd`'s async ones are, on macOS, a sheet on the window in front (and a sheet on
  "Download complete", which closes as Open with… acts, would go with it); GPUI's prompt has no
  filter and no starting folder. So on macOS dm begins the NSOpenPanel and NSSavePanel itself with
  a completion handler (`services.rs`, `objc2-app-kit`, no `unsafe` needed): the same free
  panel, filters (Download lists, dm settings, WAV, cookies.txt, Applications) and starting
  folder as before. Elsewhere `rfd`'s async dialogs; checked on Linux and Windows in their passes.
- **A drag of a column or toolbar button is the window's look alone** (`Backend::keep_look`): in
  force at once, nothing applied to the queue, the file written by an errand that writes the
  settings in force when it runs, so a change made in between is never written over by an older
  copy.
- **A row's provisional name is kept by address** (`names::provisional`, per thread, let go past
  50,000). After Stop on 10,000 rows, a read named every row twice, each parsing its address:
  most of the 90 to 140 ms the window's thread still spent per read once the errand had taken the
  rest. It costs about 2 MB at 10,000 rows.
- **Draw-time reads moved off the thread** (row 180's leftovers): the progress window's pieces and
  resumability (on the runtime, beside the downloads, not behind the errands, every half second),
  Download details' clash (a background task, again where the folder or name typed changed or a
  second passed; Start now still asks it at the press), the add-to-browser bar's two files.

## 2026-11-12: No lock of the queue's held across slow work, and the order they are taken in (gaps row 184)

- **`store`, `options` and `client` are plain mutexes, read by copying out.** `store` was a
  tokio mutex read as `if let Some(path) = store.lock().await.clone() { … }`: in edition 2021
  the guard made in an `if let` lives to the end of its block, so `flush` held `store` across
  the queue file's whole write, and every save meanwhile (a pause, a resume, a finished
  download's) waited for it, as did the download limit's own write from the progress. A plain
  mutex's guard cannot be held across an `.await` at all (clippy's `await_holding_lock`, which CI
  refuses), so this cannot come back unseen. `options.limiter.rate().await` no longer waits
  under the options' lock either.
- **A save due while a write is under way goes into the next write** (`saving.try_lock()` in
  `save`), made `SAVE_EVERY` after it as a save within `SAVE_EVERY` of a write always was. Before,
  each such change waited out the write, then wrote the whole list itself, one after another on
  a slow disk. `flush` still waits for the write under way, as a caller that asks for the list
  on disk (quitting) needs. Measured by `dm-core/tests/locks.rs`, every write of the queue file
  made to take 2 s (`Queue::slow_writes_by`, for tests): a change made during the write answered
  in 4.05 s before this (the old code, given only that hook), and in under 100 ms after; a read of
  the list under 10 ms, and two paced downloads further on in every half second of the write,
  before and after (those two held already: the write reads the rows, then lets them go).
- **The disk is not asked anything under `slots`.** A download that failed on a disk error has
  its folders looked for (`out_of_room`, whose `create_dir_all` can take seconds on a drive just
  taken out) before the lock is taken, off the runtime's threads; a cancelled run's finished file
  is removed after the lock is let go; the queue restored at start looks for each old row's parts
  folder before it locks the rows. The quota file and `recheck-at` beside the queue file are
  read and written off the runtime's threads (the quota with each write of the queue file, in the
  same blocking task).
- **`note` is never called under a lock of the queue's**: `write_held` returns what went wrong,
  said once `saving` is let go, and the cancelled run's note comes after `slots`.
- **Kept, each with a comment saying why**: `saving` across the write (one write at a time, so an
  older list is never written after a newer one); a download's `segments` across a halving's plan
  write (`split_largest`: a range reads its end under that lock, and a split whose plan could not
  be written gives the end back before letting go); QuickJS's fetch lock across the fetch (what
  waits needs the same program and would fetch it again). **Reworked**: the proxy script's lock is
  one per script and site (`pac.rs` `ASKING`), where one for all held every other site's first
  request behind one slow script or server; assembly and the adoption of staged parts read the
  disk from a copy of the plan, not under its lock.
- **The order**, written above `Inner`: `saving`, then `slots`, then `order`; every other lock is a
  leaf, taken last and let go before anything else is locked or awaited. A scan of every tokio
  guard in `dm-core` finds none held across an `.await` but these.
- **Not done: `slots` and `order` as plain mutexes**, which would make the compiler refuse a guard
  held across an await there too. About sixty sites in the engine's busiest code, for code that
  already lets them go before each await (the scan above); worth it only if a guard across an
  await is ever found there again.

## 2026-11-12: A busy wake of the window at 10,000 rows names only the rows in sight (gaps row 181)

- **Found by profiling, not by reading.** macOS's `sample` on a `profiling` build (symbols kept),
  10,000 waiting rows and 20 downloads queued at 1 MB/s (3 run at once), 20 s: about half the
  window thread's busy samples were its reads (580 of 1,170). Of those, 180 were the names (every
  2 s while anything changed, every waiting row was named again, each name a `lstat` of its
  folder: 5,000 a second at 10,000 rows) and 157 were the queue names, asked of the engine at each
  read and again at each tick's menu refresh (each a pass over every row under the queue's lock).
  The change detector's hashing (about 56) and the list's pointer copies (about 70) were the rest.
- **The queues are worked out from the rows where the list changed** (`queue_list`, the same
  `queue_names` the engine uses, over the same rows in the same order), for Start and Stop Queue's
  arrows, the menu bar and the side panel; the menu bar's queues after a command now follow at the
  next read (a quarter of a second at most while anything runs), where they were asked at once.
- **While only figures change, only the rows on screen and those chosen are named again**, every
  2 s, and at the next read after the view moves. Every row is still named where the list itself
  changed (a row added, removed, or changed in more than its figures), as before. What a person
  sees is the same: a name is shown only for a row in sight, the filter and the sort read the
  rows themselves, not the names shown. A row scrolled into sight shows the name of the last whole
  read until the next read names it (at most a quarter of a second while downloads run; a name
  waits only on a file of that name appearing in its folder meanwhile).
- **Measured**: CPU with 20 downloads at 10,000 rows 17.8% (16.5 to 19.7) to 13.9% (13.5 to 13.9),
  three runs each (`ui-bench.py --only busy`), against 12.8% at 100 rows: what a wake costs no
  longer grows much with the list. Nothing else in `ui-bench.py --only idle,list` moved beyond its
  spread.
- **Left, and why**: the detector, `SavedSizes::apply` and When done's dry watch each still hash
  every row at each read; making them follow only the rows whose pointer changed means a change to
  `Session`'s API that the egui window shares (`before()` hands out the whole map), worth it once
  a measurement of a 10,000-row history while downloading says so.

## 2026-11-12: The computer's heavy work takes turns, and a Cancel stops a digest or a join (gaps row 185)

- **Turns, not a pool of threads of their own** (`dm_core::heavy`): a digest of a finished file,
  the join of its parts, an MP3 or a GIF made from a video, and a ZIP's extraction each take a
  turn first: as many at once as half the cores, at least one, set apart from how many downloads
  and connections run. The work still runs on tokio's blocking pool (an extraction on its thread
  of its own, waiting there for its turn); work on the runtime waits for its turn on the runtime
  (`Notify`), holding no thread, so queued digests never hold the threads every file write
  needs. A pool of dm's own would have been a second set of threads doing what the blocking pool
  does; a tokio `Semaphore` alone could not be waited on by the extraction's own thread.
- **A digest is no longer worked on the runtime's threads between reads**: it reads and hashes
  on the blocking pool, and looks at the download's control between pieces of 256 KiB. A join
  copies a part 32 MiB at a time (`io::copy` of a `take`, so Linux keeps the kernel's own copy),
  looking between. **Only a Cancel stops them**: by then every byte is here, and a join or digest
  cut short by a pause would lose the joined file a resume needs; a pause lets them finish, as it
  always did. Tests: a 2 GiB (sparse) file's digest ends within a second of a Cancel, a pause
  before it did not end it (`checksum.rs`); a join of 8 parts of 256 MiB ends within a second of a
  Cancel and leaves no file under the name (`download/mod.rs`).
- **Not stopped part-way, on purpose**: one part copied to another drive by the system's own copy
  (`std::fs::copy`: `fcopyfile`, `copy_file_range`, `CopyFileEx`) is one call; its speed across
  drives was measured worth having (row 166), and a loop of dm's own in its place is unmeasured on
  Windows. JavaScript's evaluation (Windows' engine, a thread of its own) takes no turn: it is
  short, held to 2 to 3 s, and a video's start waits on it, so it must not queue behind a digest.
- **The blocking pool keeps tokio's size** (512 threads, reaped after 10 s unused), said where the
  runtime is made (`dm-desktop-base` `boot.rs`): every connection's writes go to it, each awaited
  before its connection reads on, so it never holds more than there are connections, and the heavy
  work no longer waits in it. A smaller pool would queue writes behind one another at the most
  connections dm runs; nothing measured asks for one.
- **A slow disk slows the download and does not grow memory**, as designed and now tested
  (`dm-core/tests/slow_disk.rs`, with `slow_part_writes_to`, a test hook on every write of a part):
  writes held to 1 MB/s, eight connections to a server that would send 50 MB/s; over 6 s the
  download wrote 6.3 MB (1.03 MB/s) and the process grew 4.7 MB (most of it the test server's own
  buffers), within the 10 MB allowed. The hook is one atomic load a write when no test sets it.

## 2026-11-12: A frame's costs counted, in a debug build (gaps row 180)

- **Counted where they are made, while a thread draws** (`dm-shell` `app/costs.rs`): the main
  window's `render` and the rows' builder (`table::rows`, run as the list is laid out, after
  `render`) mark the window's thread as drawing; `Backend::block_on` and `Backend::settings`
  count themselves when the thread they run on is. Only in a debug build: in a release build the
  marks and counts are empty functions, and `ui.frames` says `"drawing": null`.
- **The disk is counted by a library preloaded into dm, not by dm** (`scripts/fs-count.c`, built by
  `stress-check.py frame` with the system's `cc` and loaded with `DYLD_INSERT_LIBRARIES`, or
  `LD_PRELOAD` on Linux). dm reaches the disk through hundreds of calls in its own code, the
  standard library's and the system's (AppKit, fonts); wrapping its own would have missed the
  rest. The library counts each thread's `stat`, `open`, `access`, `opendir`, `realpath`,
  `getattrlist` and the like, and says the calling thread's count as `dm_fs_calls`, which the
  debug build reads (`dlsym`) as a draw begins and ends. Without the library the disk is reported
  as not counted, not as zero.
- **What it found:** one call. When a finished row finished ("Last try") was written in the frame
  through chrono's `Local`, which looks at `/etc/localtime` to see whether the zone changed, at
  most once a second. The words are now kept as the queue is read (`format::Whens`), worked out
  where a time is new or the day, the zone's offset or the language changed, and the frame looks
  them up. Not a fixed offset worked out once: a time on the other side of a change to summer
  time would have been an hour out.
- **The bound:** no `block_on`, no file-system call, and the settings copied once a frame at most
  (a frame drawn after no read copies them once, `Downloads::drawn`; one after a read uses the
  read's). Measured on macOS (debug build, the other track running): 58 and 59 frames at 1,000
  rows (500 finished, 10 running), and 23 at 10,000 (5,000 finished), each with 0 `block_on`, 0
  settings copied and 0 file-system calls; before the change, 1 and 2 file-system calls over 50
  and 61 frames.

## 2026-11-12: Import adds off the window's thread; its rows still the window's own (gaps row 183)

- **The rows an import adds are held, not judged, while it adds them.** The decision of the morning
  kept Import on the window's thread because its rows are marked the window's own
  (`Session::own`) once their ids are known, and a read in between would have taken them for rows
  started elsewhere (a browser's hand-off). The ids cannot be known before the queue gives them
  (`dm-core` is not changed here), so instead the read does not judge new rows while an import
  runs: `Session::adding` (on the window's thread) counts one, the errand imports and says the ids
  it was given (`Adding::added`, under the same lock as the count going down), and each read takes
  those ids and the count before it reads the queue (`Detector::read_holding`). An adding begins
  only on the window's thread, where the read is made, so none begins between the two: a row of an
  import in the read is either held or among the ids said. Held rows are judged at the first read
  with no import running, so a browser's hand-off added meanwhile is still told, late by the
  import's length (0.5 s for 10,000 links), rather than an imported row taken for one. An errand
  that panics drops its `Adding` and its rows are then judged as any new row.
- **Reserving ids or marking before adding was not done:** both need the queue to hand out ids
  before it adds, an engine change on the other track's ground.
- **The rows imported are the ids the queue gave**, not the rows new since the import began, which
  took a hand-off added meanwhile for the list's (and read the whole queue twice).
- **The list is read, and what of it is listed already counted, on the errand too**, and counted
  against a set of the listed addresses made once: going through every row for each link was
  10,000 times 10,000 comparisons, and imported a second time a list of 10,000 held the window
  more than a minute before it asked.
- **`ui.choose {path}`**: the control port answers the next of dm's panels with a path (within 30 s,
  then forgotten, so a stray one does not answer a person's panel later; refused to an assistant
  over MCP). A script cannot reach the system's panel; Import is measured through it.
- **Measured** (macOS, Apple M1, release, the other track benchmarking; `stress-check.py import`,
  `ui.texts` asked every 20 ms on its own connection; the base build with `ui.choose` added before,
  this one after): 10,000 links imported, worst answer 323 ms to 90 ms (71 in a second run), the
  rows listed in 0.6 to 0.5 s and said with them (the sentence came 60 s late before, the window
  busy); the same list again, asked in more than 60 s (the probe's wait ran out; the window
  answered nothing meanwhile, worst 60,039 ms) to 356 to 378 ms, skipped in 0.9 to 1.0 s. The worst
  answer through the second case is 278 to 312 ms, the question's window opening and closing
  (265 ms with a list of 10 links): row 194.

## 2026-11-12: A question's sheet slides with dm working on

Gaps row 194: a question held the GPUI window's thread about 0.27 s as it opened and again as it
closed. Measured before anything was changed (macOS, Apple M1, the release code with symbols): the
window's own wake-up log showed GPUI's `open_window` for the question taking 288 ms, and the time
after `remove_window` 270 ms, with the main run loop in `_NSMoveTimerRunLoopMode` for 263 ms of
each; `sample` put that under `-[NSWindow _beginWindowBlockingModalSessionForSheet:]` →
`-[NSSheetMoveHelper openSheet]` → `-[NSMoveHelper _doAnimation]`, the thread waiting, not
working. AppKit slides a sheet in a run loop mode of its own, which is not one of the common
modes, so the main dispatch queue (GPUI's foreground executor) is not served until the slide is
over; GPUI begins the sheet inside its window's making and ends it in a task of the main queue.
Nothing of dm's (settings, translations, the list, fonts, Metal set-up: 9 to 18 ms of the 288)
was the cost.

- **dm begins and ends the sheet** (`dm-gpui/src/sheets.rs`): the question is made as GPUI's
  `Floating` window, unshown, given a `Dialog`'s level back, drawn once, and then
  `beginSheet:completionHandler:` is called from a run loop timer of dm's own, where nothing of
  GPUI's is under way; `endSheet:` the same way, and the GPUI window is removed once the sheet is
  off. While a sheet slides, a repeating 4 ms timer in AppKit's mode runs one pass of the default
  mode (`CFRunLoopRunInMode`), which serves the main queue: GPUI's tasks, frames and the control
  port's answers run as between any two frames. One sheet moves at a time, in order. A slide
  begun by that work (from inside GPUI) runs none of it, so GPUI is never entered while its `App`
  is in use. Quitting while a sheet slides waits for it (AppKit dropped a `terminate:` asked
  inside the slide, and dm ran on); quitting otherwise takes every sheet off at once, as before.
  The mode's name is private: were it renamed, the timer would never run and a slide would hold
  the window as it did, nothing worse.
- **Measured** (other work running): "Already in the list" opened and closed ten times, `ui.texts`
  every 20 ms worst 305 ms to 24 ms; `stress-check.py import` (10,000 rows) the same list again
  299 ms to 34 ms, asked in 373 to 134 ms. The question now slides in drawn; before, its first
  frame came after the slide.
- **Dropped: the main dispatch queue served in the slide's mode for good**
  (`CFRunLoopAddCommonMode`). It cannot be taken back, and every other AppKit slide (a window
  zoomed from a menu item, inside GPUI's handling of it) would then run GPUI's tasks while its
  `App` is borrowed, a panic. Nor does a run loop inside a task of the main queue serve that
  queue, so the slide cannot simply be started from a GPUI task.
- **Dropped: a shorter slide** (the parent window's `animationResizeTime:`, or no animation): it
  changes how the sheet looks. **Dropped: a question window kept and reused**: the slide is the
  cost, and a reused window slides as long.
- Options, File properties, a progress window, Add URL and the Scheduler are windows of their own,
  not sheets: measured, they held `ui.texts` 13 to 28 ms opening and closing, before and after.

## 2026-11-12: A pause writes what has come, syncs, and marks it, so a resume does not check it again (gaps row 186)

- **What a stop cost twice** (`dm-core/tests/shutdown.rs`, 20 downloads on 2 connections, stopped
  part-way): 2,981,120 bytes, about 73 KiB a connection, of which 64 KiB was each part's end asked
  for again at the resume and compared (`RESUME_CHECK`), a check that exists because a ledger can
  outlive its bytes across a power cut; the rest what had come and was dropped as the range let go.
- **A pause syncs and marks** (`stopped_synced`): flushed, then `sync_data` on the part's file,
  then `done-NNNN.synced` holding how many bytes the sync covered. The next run trusts the range up
  to it (`Live::trusted`, the same flag a range written by this run has). Only after the sync is it
  written, so a crash or a power cut at any point leaves either no mark or a mark the disk honours;
  a mark cut short reads as less or nothing. It is forgotten wherever a ledger is cut back (a
  repair, a server that stopped serving ranges, a new range at that index) and goes with the parts
  (it is a `done-` file to every sweep of them). A Cancel does not sync: its parts go.
- **A pause first takes what has already come** (`draining`): what hyper and the system's buffers
  hold is read without waiting (`now_or_never`), written and counted, up to 512 KiB a connection,
  then the range stops. The limiter is not waited for on those bytes: they are here.
- **Cost**: one `sync_data` per range at each pause, inside the grace a stop gives a connection
  (1 s, then it is stopped and resumes as after a crash, with the check). The shutdown test still
  ends in under 2 s with 40 ranges.
- **Measured**: 0.78 to 1.47 MB fetched twice in five runs (19 to 36 KiB a connection), the
  restart's own share 30 to 460 KB. What is left is counted by the test's server as it yields
  pieces to the connection, including what never reached dm (its send buffer, and pieces yielded
  before it sees the connection closed); the row's 16 KiB is not met as the test measures it.
- **The sync waited for past the grace** (2026-11-12, after): a range still syncing when the grace
  ran out was stopped, but its `sync_data` and its mark went on, on their blocking thread, after the
  download had returned. A Delete at once then raced the mark into the folder it was removing, and
  the folder stayed (`queue.rs` `deleting_a_row_removes_the_parts_where_it_began_after_the_folder_changed`,
  failed once in a whole `cargo test --workspace` with the disk nearly full). The sync and the mark
  now run together on one blocking thread, counted on the job (`files::Stopping`), and the run
  waits for the count to be zero before it returns. **Dropped: retrying the folder's removal** in
  Delete: it hides a write after the end rather than ending it.

## 2026-11-12: A server that limits connections and requests is kept to (gaps row 197)

- **Found**: on a local copy of a mirror run by nginx with `limit_conn` and `limit_req` (two at
  once, a request a second, as Hetzner's mirrors answer) dm took 1.6 times as long as one
  connection, and four times on Hetzner's own mirror. Each turned-away connection ended for good;
  each range's next request was refused and waited out with backoff while nothing came; the
  seams' checks took the request the server would have given a range; and a halving whose request
  was refused left its back half to no one.
- **Chosen** (`download/ranged/limits.rs`): once a request is turned away (429, 503, or left
  unanswered) while others are served, no more connections work than the most the server has
  served at once, the rest waiting for a place; requests are paced after a refusal (0.5 s, half
  again each refusal of one that waited, at most 4 s; a `Retry-After` is kept to, at least
  0.1 s); an answer that brings its range whole reads on into ranges nobody has, as far as it asked,
  and asks that far; a halving's back half stays with the answer that would read into it; the
  seams are checked one at a time once nothing else is asked for; and the next file from the same
  server opens that many connections. Ranges after the first are asked for from the file's end
  backwards, so the probe's answer can run on through those refused.
- **Measured** (`tests/limits.rs`): 32 MiB at 4 MiB/s a connection, two at once, a request a
  second with a burst of two, each answer 100 ms late: about 0.6 of one connection's 8 s alone,
  up to 0.65 beside a whole workspace's tests; the test holds it under 0.7 (2026-11-12).

## 2026-11-12: The shown speed's first whole interval is its figure (gaps row 196)

- **Found**: `speed::the_shown_speed_keeps_to_the_limit_from_the_first_second` failed once under
  load at 408 KB/s against a 500 KB/s limit, then 437, 458, 472: each gap 0.7 of the one before,
  the meter's smoothing (`progress::Meter`, three tenths of each 250 ms sample) coming up from a
  first interval whose start went on the request. The limiter held; what a person saw lagged.
- **Chosen**: the first interval that brings bytes is shown, and the next whole interval replaces
  it rather than being averaged with it; smoothing from then on, as before. Passes 20 runs of 20
  beside four copies of the queue tests.
- **Dropped: a wider margin in the test**: it would pass a figure a person reads as wrong.

## 2026-11-12: One gate command, a pinned Rust, the lints in one place (gaps row A1)

- **`cargo xtask check`** (`desktop/xtask/src/check.rs`) runs what needs no window, for both
  workspaces: formatting, clippy (desktop, browser on the host and as WebAssembly), cargo-deny's
  licences, bans and sources (advisories stay with `audit.yml`: they need the database fetched),
  the tests (nextest where installed, then doctests; else `cargo test`, doctests included). Every
  step runs and the failures are listed at the end. **Dropped: a Python or shell script**: dm
  already has three languages in its gate; xtask is Rust and runs anywhere cargo does.
- **`rust-toolchain.toml` at the repository root** pins 1.98.1 with clippy, rustfmt, llvm-tools
  and the wasm32 target, so `browser/` is pinned as well; the workflows name the same version.
  Raised by hand in a change of its own.
- **`[workspace.lints]`** holds `missing_docs` and the three clippy lints that were pasted into
  six crate roots. They now reach each crate's tests too, which found two gaps (an undocumented
  `unsafe` in the window tests, no crate docs on dm-desktop's binary). The `not(test)` denials of
  `unwrap`, `expect` and `panic` stay in each crate's root: `[lints]` cannot exempt tests, and
  clippy's `allow-*-in-tests` reach `#[test]` functions only, not an integration test's helpers.
- **`clippy.toml`**: the version clippy may suggest for, and `dbg!` refused.
- **Retries**: none, except the egui window's tests that wait on real downloads and row 195's
  test, each named in `.config/nextest.toml`; `ci-failed-tests.py` now runs on a passing run too,
  so a retried test is named as flaky where everyone can read it.
- **CI builds the shipped window**: `ci.yml`'s hand-off check and `store-check.yml` build and
  start `dm-gpui`, with GPUI's Linux libraries installed.

## 2026-11-12: Nothing fetched and run: the computer's own JavaScript engine alone (gaps rows P1, D1)

- **Removed**: `dm-core/src/tools.rs` (QuickJS fetched from a pinned GitHub release where a
  computer had no engine), the lookups of an installed `deno` or `qjs`, the command-line engine
  path in `javascript.rs` and `pac.rs` (with the hardening prelude only it needed), the setting
  `fetch_tools`, Options › Downloads' "Get the video tools when needed" with its hint, the row's
  "Getting ready for videos (first time only)…", and those words in the 12 catalogs. A settings
  file that still names `fetch_tools` reads as before: the field is ignored.
- **In its place**: the computer's own engine only, as on Windows and macOS already. On Linux the
  .deb depends on `libjavascriptcoregtk-4.1-0 | libjavascriptcoregtk-6.0-1`, the .rpm on
  `libjavascriptcoregtk-4.1.so.0`, the Arch package on `webkit2gtk-4.1`, and the AppImage
  carries the library (cargo-packager's `appimage.libs`). With no engine the reading says which
  package to install (`javascript::NO_ENGINE`).
- **Tests**: what a test needs of an engine it now gets from a stand-in host
  (`javascript::set_host` naming a script that speaks the host's protocol), not a stand-in
  QuickJS: `youtube_cover.rs`, and `proxy.rs` where a Linux has no engine.
- **Owed on Linux** (row P1 stays until then): the AppImage started on a system without WebKitGTK
  reads a YouTube video, and `out-of-box-check.py` on a minimal Debian with github.com blocked.
- **D1**: PRIVACY.md and README.md say what dm does now: nothing fetched to run.

## 2026-11-12: The window's small dead ends (gaps rows U1, U3, U7, U9)

- **U1**: Unfinished is every row not Completed (`places.rs`), so it and Finished add up to All.
  Before, it held running, queued and schedule-held rows only, and said 0 beside 29 stopped or
  failed ones.
- **U7**: the shared row words (`rows::status_in`) say until when: the limit's wait and the
  schedule's next opening (`rows::set_hold_times`, set by the GPUI window and `--headless` at each
  read), and a retry's next try (`rows::next_try`, which only the egui window used, now in the
  words for every window and the control port). Two new phrases, put into the 12 catalogs by
  hand from each language's words for the waiting states (row 5's native readers to look at them).
- **U9**: the list answers Edit's own Undo, Copy and Paste actions (`gpui_base::input`), bound to
  ⌘ or Ctrl with Z, C and V for the window: a text field binds those in its own context, which
  GPUI puts first, so its text is cut and pasted as ever (as ⌘A and the arrows already were).
  **Dropped: a Command each** (menus, the control port's names, every list of commands): Edit's
  items already were the place a person looks for them.
- **U3**: Status grows with the window (a share of the room, up to twice its 150 points), shows
  its whole words (and a failure's raw words) on hover where they run long, and a name is drawn
  as its stem cut with an ellipsis and its extension whole, still read as one label by the
  control port and screen readers (`Screen::mark`). **Dropped: columns a person can widen**
  (row U4): a larger change, left to its row.
- **U5** (after): the menus grey what does not apply (`Ticks::off`, from `Downloads::commands_off`,
  the rows chosen read once for every command; the menu bar is built again only when that set
  changes), and a greyed toolbar button says why (`Downloads::why_off`: none chosen where one would
  do, the ones chosen not ones it acts on, or nothing in the list for it).
- **U8**: "Delete file after" in the GPUI row menu (the four times, the shared one ticked) and in
  Properties (a pop-up OK keeps), for rows that have or will have a file; Never stops the countdown.
- **U6** (part): the egui window's stepping (`selection::stepped`, moved to dm-desktop-base for
  both windows) drives the GPUI list's Home, End, page keys and Shift runs, from a keyboard place
  of its own (`Downloads::cursor`) beside the click's anchor; the keys are window-wide bindings,
  which a text field's own (in its context) come before, as ↑ and ↓ already were.
- **U4**: a heading's right edge (6 points) is dragged to set its column's width, kept in
  `Settings::column_widths` (points at 100%, by key) a few times a second during the drag and at
  its end; a double-click fits it to the longest words the listed rows show there (letters times
  an average letter's width: GPUI is not asked to measure each). The name keeps taking the rest;
  a Status a person sized no longer grows.
- **S1** (part): the port's own thread answers `downloads.pause`, `.resume` and `.cancel` by id
  (`control_api::answer`, as `--headless` does); the window learns of the change as of any change
  to the queue.

## 2026-11-12: YouTube's captions in the video (gaps row Y6, part)

- The reading names `requested_subtitles` from the player's caption tracks (the fetch step and the
  muxers already took them): the written ones, else one speech-recognised track, at most 8, each
  as WebVTT (`fmt=vtt`) on `https://www.youtube.com` alone (`caption_address`, shared with the
  caption line's TTML). **Dropped: every track and every translation**: a video lists dozens, each
  a request, for languages nobody asked for.
- The fetch step fetches them only with `Options::subtitles` on: what it fetched was put inside
  and then removed whatever the switch said.

## 2026-11-12: Small engine costs and two small features (gaps rows S7, S8, S10, S17, F3, F5)

- **S7**: `or_stop` polls its work once before making a timer (`now_or_never`): a chunk the
  limiter lets through at once, or with no limit, costs no timer entry; work still to come is
  waited on in quarter seconds, the work first (`biased`).
- **S8**: closed as done by row 177 (`retry::Together`: the first byte any connection gets sends
  every waiting one at once; `errors.rs`'s turns test).
- **S10**: the scheduler's fallback look is once a second (`runner::LOOK_EVERY`), not every 50 ms;
  what changes the queue kicks it already (an add, a download's end, a resume, an edit). What
  nothing announces, a schedule's window opening or a retrying row lending its place, is seen
  within a second. **Dropped (for now): counters of running and queued rows**: the passes over
  the rows are each cheap and only run when woken.
- **S17**: a fresh link from the browser is given off the window's thread (`later`), as Update
  download link's OK already was.
- **F3**: `--quiet` before links asks nothing for them (`Request::quiet`, read by
  `handoff::next`). `docs/parity.md` names the switches the code reads.
- **F5** (part): SHA-1 and CRC-32 by length or by name, with crates the build already had.

## 2026-11-12: AI assistants off by default; embedded YouTube players; TLS made once (gaps rows F7, Y12, S14)

- **F7**, the recommended option taken (the maintainer's standing word to take it): `allow_mcp`
  is off in a new install, as dm's rule has it that nothing listens unless turned on
  (2026-11-01, above). A settings file that names the switch keeps its value; one written before
  the switch existed now reads it as off. `dm mcp` with no dm to talk to says where to turn it on.
  **Dropped: turned on by the first `dm mcp`**: a program an assistant starts would then turn on
  what the person never did.
- **Y12**: an embed, live, no-cookie or YouTube Music address is read as its video's watch page
  (`page_data::watch_address`); the extension's player list takes the same forms, so a YouTube
  player embedded in another site's page lists its qualities.
- **S14** (part): `rtmp::tls_config` is made once (`OnceLock`); every client clones it. Idle
  connections kept 10 s (`pool_idle_timeout`). **Not done: two idle a host**: eight connections a
  file reused for the next file of a batch would then be made again.

## 2026-11-12: Where `fast`'s 90 ms after the last byte goes (gaps row 177)

- **A trace of the finish** (`dm-core/src/finish_trace.rs`): `DM_FINISH_TRACE=<file>` appends
  each stage's name and wall-clock milliseconds (a range whole, its seams checked, the
  connections ended, assembled, named, placed). Off, a stage costs one look at a value read once.
  The benchmark's poll for the finished file is `BENCH_DONE_POLL_MS` (100 by default, as
  reference A was measured).
- **Found**: three `fast` runs, polled every 5 ms: 79 to 93 ms after the server's last write,
  of which dm's own finish, from its last read to the file renamed into place, is 1 to 2 ms. The
  ranges end over 46 to 63 ms, the last ones as what the server had already written drains from
  its 1 MiB writes and the sockets' buffers. **Not done**: a harness that times the last byte as
  dm received it (TCP does not say), or ranges cut to end closer together; both are row 177's.

## 2026-11-12: A security audit, as Cloudflare's skill runs one (quick profile)

- Four hunters, one a boundary (the browser hand-off, local IPC, a hostile server, programs run
  and files touched), then a verifier that had not found it for every candidate, re-reading every
  cited line; findings only with a boundary and a result. Report:
  `docs/security-audit-2026-11-12.md`. Seven candidates, all confirmed (three medium, three low,
  and the ZIP cap), all fixed with a test each; what is left is rows 199 to 201.
- **The `magnet:` fix is at the launch, not in the registry**: `one_link_launch` makes a launch
  whose first word is a `magnet:` link that link alone, for every reader of the arguments (the
  options, `--control`, `--headless`, the modes). **Dropped (for now): registering
  `"<dm>" --open "%1"`**: it changes the keys every installed dm wrote, for the same effect.
- **Inside addresses are judged where a document names them** (`redirects::named_inside`), with
  the judgement a redirect already had: from a public document to an inside address is refused;
  inside to inside, or public to public, is not.
- **A script's settings change keeps the folders** (`keep_places_of`), as it kept the programs;
  an imported settings file does not, as a person moving dm brings their folders.
- **Row 199**: one rule for every window and dialog, held where a folder reaches the queue
  (`Backend::add_with`, `add_each`, `relocate`, `move_to`) rather than in each dialog: a folder
  outside the download folder (or reaching out of it by `..`) is refused for `SCRIPT_ACTS_FOR`
  after a control client last acted (`control_api::folder_refused`, a process-wide mark set by
  `ScriptGuard::acted`).

## 2026-11-12: A stream's size, about (gaps row Y22)

- `hls.rs` said a bar that guessed a total would be a bar that lies. **Chosen instead**: a total
  marked as an estimate (`Progress::estimated`, shown "≈ 120 MB" in the Size column), from what
  the pieces so far came to times how many the playlist lists (from the second piece; surer with
  each), or for DASH from each track's bitrate and playing time; exact (not marked) where the
  manifest gives every piece's bytes. The meter takes a total that can change
  (`Meter::set_total`, `ranged::watch_estimating`), so the time left follows it. A live
  recording has no end, so no total.

## 2026-11-12: Small rows: S15, Y19's extension half, Y23's cap

- **S15**: `Entry::torrent_files` is an `Arc<Vec<_>>` (serde's `rc`), so a row copied on write
  (`Arc::make_mut` in `Slot::entry_mut`, four times a second while it runs and a list read
  earlier holds it) shares the file list; editing a file's choice or priority copies the list
  alone. The queue file is written as before.
- **Y19** (extension): `decide::protection` judges as `dm-core` does: any `ContentProtection`
  scheme but the bare `mp4protection:2011` marker, and any key (`#EXT-X-KEY` or
  `#EXT-X-SESSION-KEY`) with a format other than `identity` or an address that is not a web one.
- **Y23**: HLS takes Connections per file at once (the setting's bounds hold it), not eight.
- **Next for Y19 and Y23**: a note a running download puts on its row (`Update::Note`), for
  "higher qualities are protected" and for live segments the playlist dropped before dm got them.
- **The row's note** (`download::Update::Note`, `Entry::said`, `dm_core::NOTES`): a download
  sends one of a fixed list of English notes (a number where its template has `{}`); the queue
  keeps it on the row, finished or not, and the Status column says it after the state in the
  person's language (`rows::note_in`). Used for Y19 (`dash::Plan::protected_above`) and Y23 (a
  live playlist's sequence numbers that skip).
- **S19**: a resolver of dm's own (`dm-core/src/dns.rs`, set in `http_client::builder_with`)
  keeps each name's addresses for 60 s, at most 512 names, a failed lookup not kept; a miss
  asks tokio's `lookup_host`. It is also where row 200 can judge the address a name gives.

## 2026-11-12: Windows and Linux code written on the Mac (G10, G11, 191, U15, U16)

The maintainer: "Make sure to implement coding part of all linux mac and windows". Each piece is
compiled and linted for Windows by `scripts/windows-check.sh clippy` and for Linux by
`scripts/linux-check.sh`, both in Docker; seeing it work there is owed to those computers.

- **G10, the drag out on Windows**: not the `drag` crate (a third party's, and its own COM
  objects) but the shell's: the files as shell items (`ILCreateFromPathW`,
  `SHCreateShellItemArrayFromIDLists`), their data object as Explorer's (`BHID_DataObject`) and
  `SHDoDragDrop` with the shell's own drop source, so no COM object of dm's is written. Its
  modal loop runs from a message posted to the window (`WM_DM_DRAG_OUT`), outside GPUI's
  updates, in a vendored `gpui-pre-windows` as `gpui-pre-linux` is for X11's.
- **191, long paths**: `longPathAware` cannot be declared, because the manifest is GPUI's own
  and `gpui-pre-platform`, `gpui-component` and `gpui-base` each turn it on; a second manifest
  is a duplicate resource the linker refuses. So the shell is handed a path of 260 letters or
  more as its 8.3 short form (`shell::for_shell`). Dropped: vendoring `gpui-pre` (the whole
  core of GPUI) for one line of its manifest.
- **G11, Mica**: the check moved toolkit-free (`dm_desktop_base::mica`); the frame (title bar,
  toolbar, side panel, status bar) is drawn clear (`theme::frame`) and the list's column solid,
  as the egui window kept the list solid.
- **U16**: the Dock's bar is an `NSProgressIndicator` over the program's picture in the tile's
  content view, dropped (the Dock's own picture again) when nothing sized runs; Linux's is
  Unity's `LauncherEntry` signal, which the docks and panels that show a launcher's progress all
  read, sent from a thread of its own. Both in whole hundredths. The window's title stays "dm"
  for now: scripts and the control port find the main window by it.
- **U15**: on macOS a notification is waited on in step, as on Linux (`notify-rust`'s
  `wait_for_response`, which macOS answers while GPUI's main run loop runs), at most eight at
  once.
- **Row 202, closed (2026-11-12)**: not dm's. The test server let the probe's open-ended
  request through uncounted, so dm saw three connections served at once and kept a third ranged
  request out, which the server held. Counting every connection, as nginx's `limit_conn` does,
  every run held exactly the first wave's four ranges and none twice; the test now asserts both.

## 2026-11-12: The JavaScript host's sandbox (201) and names held to public addresses (200)

- **201**: the host enters the system's wall after a first call has loaded the engine. macOS:
  `sandbox_init` with a written profile (allow by default; deny network, process fork and exec,
  file writes). The system's named "pure computation" profile was tried first: JavaScriptCore was
  killed (SIGKILL) inside it. Linux: seccomp BPF written here (no crate), refusing with `EPERM`,
  `clone3` with `ENOSYS` so the C library falls back to `clone`, whose `CLONE_THREAD` flag is
  read; a kernel without seccomp is let be. Windows: low integrity and a one-process job;
  Windows' own AppContainer was dropped, since it needs a profile registered for the program.
- **200**: the resolver cannot know why a name is asked, so what judges an address
  (`redirects::inward`) tells it: a public-looking name a public address leads to is held to
  public addresses for ten minutes. The check and the connection read the same kept answer, so
  the name cannot change between them.

## 2026-11-12: The maintainer's word on every open decision

The maintainer (2026-11-12): "do all things like removable of egui code and everything. okay from
my side. only things take care of security and coding is finished and all the gaps of gaps.md
finsished", after "if all coding part finished then merge all the branches to main and we will test
in main itself", and that Windows and Linux are only tested, with no coding there.

- **G9**: egui is removed: the `dm-desktop` crate, its vendored `winit`, `egui-winit` and `eframe`,
  its fonts and everything rows A2 and A3 name; the tests that hold shared behaviour move to
  `dm-desktop-base`, `dm-shell` or `dm-gpui` first.
- **Every row marked "decide" or awaiting "the maintainer's word"** (G8's packaging choices, P2 to
  P5, F1, F4, F8, F9, F11 to F13, F15, U10, U11, U14, U17, U20, U22, U24, U26, U30 to U32, U34,
  Y7, Y27, A4, A16, R1, R4): the option the row recommends is taken, and a contract row it changes
  is edited in the same commit, naming this word.
- **Security** is checked again over the whole tree before the merge to `main`.
- **When the code is finished**, `gpui-migration` is merged to `main` and the testing (the full
  gate, `human-qa.py`, Windows and Linux) is done there.

## 2026-11-13: The GPUI window's tests drive it as a script does (egui removal)

The egui window's 570 tests drove it through `egui_kittest`. The GPUI window had no tests of its
own beyond pure functions: dm-gpui is a binary, with no library for a test crate to open.

- **Harness**: `dm-gpui/src/harness.rs`, a test module. It opens the real `Downloads` in GPUI's
  test platform (`gpui-pre`'s `test-support`, `TestAppContext`) over a real queue with a fake
  system (`Fake`, as the egui harness's). A test asks it through the control port's own methods
  (`ui.click`, `ui.texts`, `ui.windows`, `command.run`…), sent down the window's request
  channel, a wake rung, and the test platform run until it parks. The words and where they are
  drawn come from `screen.rs`'s record, as for a script. A test reads the view's state too
  (`Harness::view`). One window opens, draws and answers in about 0.1 s.
- **Why the port's methods**: they are what `human-qa.py` and the AI clients drive, so a test
  ported this way checks the same path a person's script takes, and the egui tests' kittest
  clicks map onto them one for one (`get_by_label(..).click()` to `click(..)`).
- **Kept off in tests**: the system tray (`start_tray` returns under `cfg(test)`). The test
  runner's threads are not the main thread, and muda refuses to make a tray there.
- **Dropped, not ported**: what tests egui itself (its fonts, kittest's pictures). The egui
  window's own font check goes (the GPUI window draws in the system's fonts).

## 2026-11-13: Three settings kept (gaps row F13)

Taken at the maintainer's word of 2026-11-12 (each decide row's recommendation): each setting that
looked droppable is kept, with its reason.

- **`remux_ts`** (Options › Downloads, "Put a .ts stream into an MP4"): a transport stream holds
  what an MP4 does not (teletext and broadcast subtitles, some sound tracks a recorder keeps), and a
  published digest can name the `.ts` bytes, which the remux would change (`download/hls.rs`,
  `remux_wanted`). On by default; off keeps the stream as it came.
- **`second_path`** (Options › Advanced › Network, a second network): a real use, a phone's
  connection over USB beside the Wi-Fi, every other connection of a download on it. Tested with a
  second local address only (gaps row 47); not removed for that.
- **`mica`** (Options › General, Windows 11): the GPUI window draws it since gaps row G11, opt-in
  because an old GPU driver can draw a see-through window black.

## 2026-11-13: Toolbar icon sets and a phone's links declined (gaps row F15)

Taken at the maintainer's word of 2026-11-12, the row's recommendation being each declined in words
or built:

- **Icon sets of a person's own**: declined. The toolbar has three looks (icons and words, icons,
  words) drawn from dm's own icons in the system's colours; a set of a person's own is a file format,
  a loader and a look per system to keep, for what the three looks already give.
- **Sending a link from a phone**: declined for now. It needs dm to listen on the network beyond
  the loopback, and a pairing to keep it to the person's own phone: a new way in from outside the
  computer, against the security stance (the control port listens on the loopback alone). The
  browser extension on the phone's own browser, where there is one, sends to a dm on the same
  device; a link sent another way (mail, a chat) is pasted or dropped.

## 2026-11-13: BitTorrent's library, a recorded exception (gaps row P3)

Taken at the maintainer's word of 2026-11-12, the row's recommendation: the vendored `librqbit`
(`desktop/vendor/librqbit`, with its SHA-1 wrapper since gaps row P7) is a recorded exception to
"no third-party package that is a feature": a library compiled into dm, never a program it runs or
fetches, and patched in dm's tree (`dm:` marks, `desktop/vendor/README.md`). Writing BEP 3, 5, 9,
10, 11, 29 and the stream encryption of dm's own is months of work for no gain a person sees. It is
to be kept behind one crate of its own (`dm-torrent`, with gaps row A6) so it can be replaced piece
by piece, and brought to upstream 9.1.0 (a bencode recursion limit, a bounds fix, a listener panic)
as upstream source plus one patch; both are the row's remaining work.

## 2026-11-13: HTTP/2 where a server refuses HTTP/1.1 (gaps row F9)

Taken at the maintainer's word of 2026-11-12, the row's recommendation. The client stays pinned to
HTTP/1.1 (each range a connection of its own: the 2026-11-09 entry's reason holds), and where the
first request is refused in a way only an HTTP/2-only server refuses (505, an HTTP/2 preface or
GOAWAY read as an answer, a TLS handshake that offers only `h2`; `http_client::refuses_http1`), the
download is asked again from the start over a client that speaks HTTP/2 from its first byte
(`http2_client`), its ranges sharing that one connection. reqwest's `http2` feature is the `h2`
crate, in Rust. That client goes out directly: a server that refuses HTTP/1.1 behind a proxy dm was
told to use is not reached. Test: `dm-core/tests/http2_only.rs`, a server that speaks HTTP/2 alone.
## 2026-11-13: The egui window removed (G9, A2, A3)

At the maintainer's word of 2026-11-12 (above): the `dm-desktop` crate, its fonts (Inter,
Selawik, the Fluent icon font), its vendored `winit`, `egui-winit` and `eframe`, kittest, the
egui baseline pictures, `make app-egui`, `DM_WINDOW=egui` and `scripts/tabular-digits.py` are
gone. `cargo tree` has no egui, eframe or winit; `desktop/Cargo.lock` went from 1,151 packages to
1,022.

- **What dm-gpui borrowed moved into it**: `icons/` (the installers', the tray's and the Windows
  program's icon) and `macos/Info.plist`. Its `Cargo.toml` is where the Makefile, the workflows
  and the Arch and rpm packages read dm's version; the test that held it to the egui crate's
  now holds it to dm-core's, dm-shell's and dm-desktop-base's, which `--version`, the native
  host's hello and the User-Agent print.
- **Names kept from the egui window's program**: the .deb's `/usr/bin/dm-desktop`, which runs
  dm-gpui; the NSIS installer taking away an earlier `dm-desktop.exe`; the login item and
  registration code that recognise an old `dm-desktop` entry; `--help`'s and the extension
  popup's `dm-desktop --register`. They are what an installed dm and its users already have;
  the program's own name is row A4's.
- **No Windows build in Docker**: `scripts/windows-build.sh` built the egui window's portable zip,
  as the GPUI window's release build needs the Windows SDK's `fxc.exe` (2026-11-11). It is gone,
  and `make build-windows` says where the Windows packages are built (CI, or a Windows computer).
  `scripts/windows-check.sh` (a `cargo xwin check`) stays.
- **Catalog words**: the catalog check read the egui crate's source too, which hid that the GPUI
  window hands many labels to `t` from its own lists (an Options field's `("proxy.port",
  "Port")`, the menus' titles, `check("hide-html", "Hide HTML files", …)`, the side panel's
  headings). Read alone, it called 94 words unsaid, 40 of them said. The check now counts a
  literal of the window's source that a catalog translates as a word it says; the 54 left,
  which nothing in dm-gpui, dm-desktop-base or dm-shell names (the egui window's key names,
  Find what, Small, Medium and Large, its column and row menus' words), are out of all 13
  catalogs.
- **Not rewritten**: the comments in dm-gpui that say a behaviour is "as the egui window's"; they
  say where it came from (row A20).
