# Noob UX test — report (macOS, Microsoft Edge, 2026-10-26)

Run against `noob-ux-test.md`, as far as one machine without a person at the
screen allows. **What was and was not done is stated per scenario; nothing
here was guessed.**

| Item | Value |
|---|---|
| App / extension | dm 0.7.1 (local build) / extension 3.1.0 (unpacked, `dist/chrome`) |
| OS, browser | macOS (Darwin 27), Microsoft Edge 156 |
| Theme | dark (the window pictures); light not looked at |
| Profile | the real Edge profile for downloads; a throwaway profile (headless) for the scripted browser check |
| How | real Edge opened on local test files; `scripts/chrome-check.py` in Edge (60 of 62 pass; the 2 failures need a machine **without** dm registered as the browser's host); the window's own pictures through `scripts/ui-shot.py` |

**Not testable here, not claimed:** installer, SmartScreen, signing, uninstall
(A, H), Mica/Segoe/Fluent icon checks and snap layouts (§11, Windows-only),
High Contrast, Narrator, keyboard-only pass (§12), reboot (E4), Wi-Fi off
(E2), full disk (E5). These have automated coverage in the repo for some
(`docs/parity.md`) but were not re-run by hand.

## What Sam gets, scenario by scenario

| Scn | Result | Evidence |
|---|---|---|
| B1–B3 first launch | Good. A banner says "Add dm to your browser: Microsoft Edge / Firefox / Chrome and others / Not now". Toolbar icons all have words. Disabled buttons are greyed. | window picture |
| C3–C5 extension | Connected; popup text checked by the browser check. | chrome-check |
| D1 click a link | Edge's own download is cancelled at once and dm fetches it; **no double download** (Edge's history: 0 downloads; the file is only dm's). | live, real Edge |
| D1 other types | zip, exe, 7z, msi, iso (after a redirect), dmg, docx: dm. pdf, mp4, mp3 links: **were** opened in the tab; **fixed** today (a plain click is taken by dm; Alt keeps it). | live + check |
| D5/D6 where it lands | `~/Downloads/dm/<Category>/file` (e.g. `Archives/`, `Apps/`, `Documents/`). | live |
| E6 dead link | Row turns red: "The server has no file at thi…" | window picture |
| E7 same file again | A second copy is named `a (1).zip`; never overwritten, no `(1) (1)`. | live |
| F6 scheduler | Plain words: "The hours when downloads may run, in every queue." | window picture |
| Text fields | Add URL, Properties, Options: focused field has the accent underline, no white corners. | window picture |

## Findings

### N1 Two stale saved settings kept downloads in the browser or waiting 🔴 (fixed)
- **Expected:** a download goes to dm and starts. **Happened:** a catch list
  saved by an older version had no documents (docx/xlsx stayed in Edge), and
  "ask before starting" saved as on made every caught download wait.
- **Fix:** saved lists are brought up to the defaults once (`catch.defaultsVersion`);
  the two "ask" settings were turned off in this machine's file. **Still open:**
  an upgrader with `ask_caught_downloads: true` saved keeps being asked. That
  is a choice the file cannot tell from a deliberate one; see N2.

### N2 Nothing tells Sam a download is waiting for a click 🟠 (fixed: the window comes forward)
- When dm asks first ("Download details"), the browser has already let go of
  the file. If that window is behind another one, the download seems lost.
- **Fix:** bring the window to the front (and say so in the notification), or
  default to starting at once everywhere. **IDM:** shows its dialog on top.

### N3 PDF/video/song links opened in the browser 🟠 (fixed today)
- Plain click on `.pdf`, `.mp4`, `.mp3` etc. now goes to dm by the app's
  rules; where it cannot, the link opens as before. Not covered: an address
  typed or bookmarked, which no extension can see before the page loads.

### N4 The last column is cut off 🟡
- At 1920 px wide the table has a horizontal scrollbar and the last column's
  header is cut to "C…". **Fix:** let the Name column give up width first.

### N5 The failed row says what, not what next 🟡 (reworded: "this link no longer works (404)")
- "The server has no file at thi…" is truncated and has no next step.
  **IDM:** "Refresh link". **Fix:** shorter wording ("This link no longer
  works") with the row's menu offering Update link.

### N6 Jargon Sam will hit 🟡
- Toolbar: **Site files**. Options › General: **Allow the command line to
  control dm**. Properties: **Referrer**, **SHA-256**, **Check against: A
  digest you were given**. Theme choices are four icons with no words (tool
  tips were not looked for). **Fix:** one plain sentence under each option;
  words under the theme icons.

### N7 Files land in sub-folders Sam did not choose 🟡
- `Downloads/dm/Apps`, `Archives`, `Documents`…  A first download is hard to
  find without knowing categories. **Fix:** the completion toast and the
  "Open folder" button name the folder; consider a first-run line saying so.

### N8 The status bar shows a raw folder path 🟡
- Bottom right shows the downloads folder as a path; for Sam a short "Saved to
  Downloads/dm" with the path in a tip is friendlier. (In the pictures it was a
  temp folder because the picture tool uses a throwaway home.)

### N9 "Speed limiter" on the status bar with no explanation ⚪
- It reads as a status, not a switch, and the limit is not named.

## What was good
- Banner pointing at the browser extension on first launch.
- Disabled buttons greyed; OK is the accent button; Escape and Enter work (tests).
- Scheduler wording; no-overwrite naming; red failure row.
- Focus: accent underline, no stray outline, in every dialog checked.

## Comfort ratings (Sam, from what was seen — not a human's)
| Question | Score | Why |
|---|---|---|
| Getting started felt easy | 4 | banner and labelled toolbar; installer not seen |
| Always knew what the app was doing | 3 | N2: a waiting dialog can be missed |
| Downloading as easy as IDM | 4 | caught and started at once; categories folders N7 |
| Errors told me what to do | 2 | N5 |
| Looks modern and native | n/a | Windows checks not possible here |
| I trust it | 3 | unsigned local build; not judged |

**Verdict:** *Needs work* — mostly wording and discoverability (N2, N4–N8);
the catching bugs (N1, N3) are fixed and covered by tests.
