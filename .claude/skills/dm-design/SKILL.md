---
name: dm-design
description: Use before designing, changing or reviewing any part of dm's window, on desktop or phone (crates/dm-gpui, assets/icons, assets/i18n). Maps the better-* interface skills, written for web pages, onto dm's GPUI window, its tokens and its checks, and says which rules win when they disagree.
---

# dm's interface: one design on every system

dm has one window, the GPUI one in `crates/dm-gpui`, drawn by the same code on macOS,
Windows, Linux and Android (under 600 points wide it is the phone's layout). Consistency
means every surface reads its colours, sizes and words from the same few places, and is
checked the same way.

## Which rules win

1. `docs/behaviour-contract.md`: what a person does and what then happens. A change to a
   row, the window's layout, a dialog or its words edits that row (and
   `docs/behaviour-baseline/` when the window changes) in the same commit, naming the
   maintainer's approval.
2. `docs/ui-design.md`: dm's own rules (one filled button, three uses of the accent, three
   ranks of text, a state is a mark and a word, labels in one column, the word rules).
3. The better-* skills in this folder: everything those two do not settle.

Where a better-* skill disagrees with dm's docs, follow dm's docs and say so in the report.
Known disagreements: dm allows "OK" where no verb fits (better-writing says never); dm ships no
font (better-typography's font choices do not apply: the font is the system's); better-ui's
CSS shadow values are a starting point only, and a shadow follows the ones dm already draws
(menus, the switch's knob) and what the system itself draws.

## Which skill for which job

| Job | Skill |
|---|---|
| Review a screen, dialog or flow | `better-interface` (it calls the others in its order) |
| Review a branch or the uncommitted change | `interface-review` (the person types `/interface-review`) |
| Corners, edges, pictures (`assets/icons/*.svg`), hover and pressed faces | `better-ui` |
| Type sizes, cutting with "…", wrapping, figures in a column | `better-typography` |
| Colours, contrast, a new role | `better-colors` |
| Grouping, alignment, narrow widths, right-to-left | `better-layout` |
| Keys, focus, screen readers, hit areas, text size | `better-accessibility` |
| Labels, failures, empty lists, confirmations | `better-writing` |
| What moves and how | `better-motion` |

## The web words in dm's terms

The skills speak CSS and the DOM. In dm:

| The skills say | In dm |
|---|---|
| Design tokens, CSS variables | Colour roles of `Theme` (`crates/dm-gpui/src/window/theme.rs`); sizes in `widgets/native.rs` (`TEXT`, `SMALL_TEXT`, `CONTROL`, `MARGIN`, `ROW_GAP`, `RADIUS`, …) and `widgets/space.rs` (`XXS` 2 … `XXXL` 24, `SECTION`, `TOUCH` 48, `CORNER_S/M/L`) |
| `rem`, the reader's font size | `zoom::px` (`window/zoom.rs`): every size goes through it so Text size (60–120%) reaches it |
| `prefers-color-scheme`, two theme blocks | `Theme::of(appearance)`: a palette per system (AppKit, WinUI, libadwaita) light and dark, and High Contrast's system colours |
| `forced-colors` | Windows High Contrast: every colour is the system's, so nothing may rely on colour alone |
| Contrast checked from tokens | `theme/tests.rs`: 4.5 to 1 for words on every surface in every palette; add a case for a new pair |
| `oklch()`, hex in components | `Rgba` from a `Theme` role; a widget never holds a colour of its own |
| `:hover`, `@media (hover: hover)` | GPUI's `.hover(…)` with `theme::HOVERED` / `theme::PRESSED`; a phone has no hover, so nothing may be found only by hovering (long press opens the menu) |
| `:focus-visible` ring | `Theme::ring` |
| ARIA, accessible names, live regions | `window/a11y.rs` (off state, a bar's progress, a spoken status sentence); every control has a name |
| `dir="rtl"`, logical properties | `window/mirror.rs` (`rtl()`): Arabic and Hebrew mirror the side panel, toolbar, columns and status bar; only direction pictures flip |
| `prefers-reduced-motion` | `dm_desktop_base::motion::reduced()` → `cx.set_reduce_motion` (`lib.rs`); every animation checks it |
| Viewports 320, 768, 1280 | dm's widths: 360 (least, `narrow::LEAST_WIDTH`), 700 and 1280 points; under 600 the phone layout |
| Touch target 44/48 px | `space::TOUCH` (48) on a phone, `native::CONTROL` on desktop |
| An icon library, `currentColor` | dm's own SVGs in `assets/icons/`, outline, one stroke weight, coloured by the role they sit in |
| Copy in components | Every word goes through `i18n::t("…")` (`dm-desktop-base/src/view/i18n.rs`) and is in `assets/i18n/template/catalog.json` and each language; a translation fits the same room (1.4× its English at most) |
| A throwaway page with every state | `python3 scripts/ui-shot.py out-dir [command …]`: the real binary on a throwaway home with rows in every state; a PNG of the window and each dialog |
| A browser to verify in | `ui-shot.py` pictures, looked at, in light and dark, at 360, 700 and 1280 points; `scripts/android-emulator-check.sh` for the phone |

## Consistency checks for any change

- Same detail, one way: if two dialogs draw the same thing (a foot of buttons, a hint, a
  switch's line), they use the same helper in `widgets/native.rs` (`buttons`, `row`, `note`,
  `dialog`, …). A second way of drawing it is a finding; the fix is the shared helper.
- No literal number for a size or colour in a widget: a size from `native`/`space`, a colour
  from `Theme`. A new value needs a new named token and a line in `docs/ui-design.md`.
- Desktop and phone differ only through the tokens' `on_a_phone(…)` values and the narrow
  layout, never by a second copy of a dialog.
- Every state drawn: empty, loading, running, paused, waiting, done, failed, a long name, the
  longest language, right to left, Text size 120%.

## Finishing

- `cargo test --workspace` (the privacy check reads comments: name no other download manager;
  they are "reference A/B/C").
- Pictures from `ui-shot.py` looked at before the commit; `python3 scripts/human-qa.py` in a
  visible window after any change to `crates/`, `assets/` or `extension/`, reporting every row.
- `docs/ui-design.md` updated when a rule or a token is added.
