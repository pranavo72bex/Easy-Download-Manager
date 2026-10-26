# WinUI 3 / Fluent 2 standards — audit of dm (2026-10-26)

The standards were given as "mandatory"; dm draws with egui, so the rule is to
reproduce the values in the token file (`theme.rs`, `theme/apply.rs`). Checked
by reading the tokens and code, a test run, and pictures of the window (dark
theme, macOS). **Not checked by eye on Windows**, which is where most of
§4, §8, §10 and §12 can only be judged.

| § | Standard | State | Evidence |
|---|---|---|---|
| 1 | Multiples of 4, 8 between controls, 24 dialog padding | Met | `item_spacing` 8, `window_margin` 24, `menu_margin` 4 |
| 1 | Standard control height 32 | **Met** | `interact_size.y` is 32 everywhere in the modern look; menu items are 32 with no gap between them. Making it so found a real bug (the list's vertical bar clamped the list's offset in a one-row list: `a_list_with_room_for_one_row_stays_where_it_was_scrolled`) |
| 2 | Type ramp, Segoe UI Variable, no bold/italic | Mostly met | 12 / 14 / 20 semibold are the ramp's Caption, Body, Subtitle; Segoe UI Variable in `theme/fonts.rs`. Line heights and the 28+ sizes are not set (nothing uses them). Monospace 13 for hashes is outside the ramp |
| 3 | Theme tokens only, Light/Dark/High Contrast, system accent | Met, High Contrast unseen on Windows | Colours come from `Tokens`; Light, Dark, System and Classic exist, accent follows the system; High Contrast (`Theme::HighContrast`) takes the system's colours whatever look was chosen, and its readability is tested (4.5:1 and up in both built-in schemes). Not yet run on Windows (gaps row 11) |
| 4 | Mica, Acrylic, shadows 8/16/32/128 | Met as far as egui allows | Mica where `chrome::mica()`, solid in High Contrast and elsewhere. A dialog's shadow is the deepest of 128, 64 and 28 its window has room for (egui counts a shadow in a window's size; `dialog_shadow_blur`, tested at four window sizes); menus 32, and tooltips share egui's one popup shadow, so 32 too |
| 5 | Radius 4 and 8, 1 px strokes | Met | `radius` 4, `card_radius` 8 in the modern look |
| 6 | One icon set, tooltips, accessible names | Not verified | One font of icons in `icons.rs`; tooltips on every icon-only button not walked |
| 7 | One accent button per view, verbs, ellipsis, InfoBar, TeachingTip | Mostly | One primary button per dialog in every picture; the "Add dm to your browser" banner is the first-run hint. Dialog button order is OK / Open / Cancel |
| 8 | Title bar, Snap Layouts, window size and columns remembered | Not verified | `window.rs` remembers the position; columns' widths are kept (`table.rs`) |
| 9 | Motion 83/167/250 ms, off when animation is off | Met for off | egui's fade is 1/12 s = 83 ms; `motion.rs` sets it to 0 with the system's setting (tested) |
| 10 | Keyboard, double-stroke focus, Narrator, High Contrast | Partly | Focus ring is two strokes (`focus_outer`/`focus_inner`); `accesskit` is wired; keyboard tests exist. Narrator and High Contrast not run |
| 11 | Sentence case, plain words | Met for case, partly for words | 0 of 666 strings in Title Case. Jargon rows remain (gaps row 9) |
| 12 | Taskbar progress, toasts, tray, jump list | Met, the new parts unseen on Windows | `taskbar.rs` (progress), notifications, a tray, `jumplist.rs` (Add URL, Open downloads folder) and held toasts during a fullscreen app or Focus Assist (`quiet.rs`). A Retry action on a failure toast not found. Gaps rows 11 and 14 |
| 13 | Forbidden list | Not walked | — |

## Written without being run

`contrast.rs`, `jumplist.rs` and `quiet.rs` are for Windows and have not run
there. `scripts/windows-modules-check.sh` compiles them for
`x86_64-pc-windows-msvc` from any computer and tests their logic; what is left
for a Windows computer is in gaps rows 11 and 14.
