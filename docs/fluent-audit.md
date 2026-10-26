# WinUI 3 / Fluent 2 standards — audit of dm (2026-10-26)

The standards were given as "mandatory"; dm draws with egui, so the rule is to
reproduce the values in the token file (`theme.rs`, `theme/apply.rs`). Checked
by reading the tokens and code, a test run, and pictures of the window (dark
theme, macOS). **Not checked by eye on Windows**, which is where most of
§4, §8, §10 and §12 can only be judged.

| § | Standard | State | Evidence |
|---|---|---|---|
| 1 | Multiples of 4, 8 between controls, 24 dialog padding | Met | `item_spacing` 8, `window_margin` 24, `menu_margin` 4 |
| 1 | Standard control height 32 | **Not met: 24** | `interact_size.y` is 24 ("compact density", chosen earlier). Set to 32 as an experiment: 4 window tests fail (Properties and menu clicks fall below the fold; a list row ends under the sideways bar at large text). Reverted. Gaps row 10 |
| 2 | Type ramp, Segoe UI Variable, no bold/italic | Mostly met | 12 / 14 / 20 semibold are the ramp's Caption, Body, Subtitle; Segoe UI Variable in `theme/fonts.rs`. Line heights and the 28+ sizes are not set (nothing uses them). Monospace 13 for hashes is outside the ramp |
| 3 | Theme tokens only, Light/Dark/High Contrast, system accent | Partly | Colours come from `Tokens`; Light, Dark, System and Classic exist, accent follows the system. **No High Contrast handling** in code (gaps row 11) |
| 4 | Mica, Acrylic, shadows 8/16/32/128 | Partly | Mica where `chrome::mica()`; solid fallback otherwise. Shadows are 28 (dialog) and 16 (menu, tooltip): egui counts a shadow in a window's size and 128 no longer fitted at 200% (comment in `apply.rs`). Gaps row 12 |
| 5 | Radius 4 and 8, 1 px strokes | Met | `radius` 4, `card_radius` 8 in the modern look |
| 6 | One icon set, tooltips, accessible names | Not verified | One font of icons in `icons.rs`; tooltips on every icon-only button not walked |
| 7 | One accent button per view, verbs, ellipsis, InfoBar, TeachingTip | Mostly | One primary button per dialog in every picture; the "Add dm to your browser" banner is the first-run hint. Dialog button order is OK / Open / Cancel |
| 8 | Title bar, Snap Layouts, window size and columns remembered | Not verified | `window.rs` remembers the position; columns' widths are kept (`table.rs`) |
| 9 | Motion 83/167/250 ms, off when animation is off | Met for off | egui's fade is 1/12 s = 83 ms; `motion.rs` sets it to 0 with the system's setting (tested) |
| 10 | Keyboard, double-stroke focus, Narrator, High Contrast | Partly | Focus ring is two strokes (`focus_outer`/`focus_inner`); `accesskit` is wired; keyboard tests exist. Narrator and High Contrast not run |
| 11 | Sentence case, plain words | Met for case, partly for words | 0 of 666 strings in Title Case. Jargon rows remain (gaps row 9) |
| 12 | Taskbar progress, toasts, tray, jump list | Partly | `taskbar.rs` (progress), notifications and a tray exist. **No jump list.** A Retry action on a failure toast not found |
| 13 | Forbidden list | Not walked | — |

## Not done on purpose
The 32 px control height: a broad change that moves every dialog and needs a
look on a real Windows screen at 100–225 % before it is kept.
