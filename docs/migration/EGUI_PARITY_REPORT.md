# egui parity report

Every row of [docs/behaviour-contract.md](../behaviour-contract.md), with where it stands in the
egui window (today's) and in the GPUI window. A row is "holds" in a window only when
`scripts/human-qa.py` passes it there on macOS, Windows and Linux (`scripts/linux-human-qa.sh`);
anything less is said in words. The GPUI column is updated by each slice of
[GPUI_MIGRATION_PLAN.md](GPUI_MIGRATION_PLAN.md) Phase C (`docs/gaps.md` G7), and the report is
complete when every row holds in both, or a row's change is approved by the maintainer (D4).

State on 2026-11-09: egui holds 64 of 64 (macOS and Linux from the 0.9.0 release's own files;
Windows by the maintainer's run). No GPUI window exists yet (G6).

| Row | What a person does | Kind | Slice | egui | GPUI |
|---|---|---|---|---|---|
| B01 | Clicks Add URL and presses OK with nothing typed | window | 4 | holds | not started |
| B02 | Types an address in Add URL and presses OK | window | 4 | holds | not started |
| B03 | Presses Start now | window | 2 | holds | not started |
| B04 | Selects a running download and clicks Stop | window | 2 | holds | not started |
| B05 | Clicks Resume | window | 2 | holds | not started |
| B06 | Any file from a server that takes ranges | engine (shown, not drawn) | all | holds | not started |
| B07 | A fast server that keeps connections open | engine (shown, not drawn) | all | holds | not started |
| B08 | A server that holds each connection to a low speed | engine (shown, not drawn) | all | holds | not started |
| B09 | A server that allows two connections at once and answers 503 to more | engine (shown, not drawn) | all | holds | not started |
| B10 | A server that does not take ranges | engine (shown, not drawn) | all | holds | not started |
| B11 | A server that says the file is longer than it sends | engine (shown, not drawn) | all | holds | not started |
| B12 | An address that redirects | engine (shown, not drawn) | all | holds | not started |
| B13 | A server that names the file (Content-Disposition) | engine (shown, not drawn) | all | holds | not started |
| B39 | A server names the file differently from the address (`/dl?id=7` answered with "Report… | window | 4 | holds | not started |
| B40 | An address that ends like a file (`.zip`) answers with a web page | window | 4 | holds | not started |
| B14 | An address that answers 404 | engine (shown, not drawn) | all | holds | not started |
| B41 | An address that answers 404, the row having failed | window | 4 | holds | not started |
| B15 | Every download that completes | engine (shown, not drawn) | all | holds | not started |
| B16 | Sorting into folders by kind (on by default) | window | 2 | holds | not started |
| B17 | Adds an address already in the list | window | 3 | holds | not started |
| B18 | Adds five downloads at once | window | 3 | holds | not started |
| B19 | Presses Add for later, then Start Queue | window | 5 | holds | not started |
| B64 | Opens Downloads › Start Queue (or Stop Queue), or the arrow beside the toolbar's Start… | window | 5 | holds | not started |
| B20 | Chooses Low speed (256 KB/s) | window | 5 | holds | not started |
| B21 | dm is killed in the middle of a download, then opened | engine (shown, not drawn) | all | holds | not started |
| B22 | dm is quit and opened again | engine (shown, not drawn) | all | holds | not started |
| B23 | Selects a finished download and clicks Delete | window | 3 | holds | not started |
| B38 | Selects a stopped, unfinished download and clicks Delete | window | 3 | holds | not started |
| B24 | Clicks Delete Completed | window | 3 | holds | not started |
| B25 | The empty window, Add URL, and Download details | window | 8 | holds | not started |
| B42 | Opens a dialog (Add URL, Download details, Options, a question) and drags it by its tit… | window | 8 | holds | not started |
| B43 | Opens Options and looks at its tabs; opens Advanced | window | 7 | holds | not started |
| B44 | Opens Download details for a link that ends in `.zip` (from Add URL or the browser), or… | window | 4 | holds | not started |
| B45 | Looks at the list of downloads | window | 1 | holds | not started |
| B63 | A new install (nothing chosen yet): adds a download from Add URL and lets it finish | window | 1 | holds | not started |
| B46 | Double-clicks a download that is running, waiting or stopped; looks at its progress win… | window | 2 | holds | not started |
| B47 | Adds a download from Add URL whose address has a token, a signature, a key, a password… | window | 4 | holds | not started |
| B48 | Types text in Find, then presses Shift-F3, or chooses Downloads › Find previous | window | 2 | holds | not started |
| B49 | Opens the Scheduler, picks what dm does when the scheduled downloads are done (nothing,… | window | 5 | holds | not started |
| B50 | Adds an address that is already in the list; looks at the question | window | 4 | holds | not started |
| B51 | Opens Options › Advanced; reads how dm identifies itself; turns on Identify as dm only | window | 7 | holds | not started |
| B52 | Opens the tray icon's Speed menu; chooses Settings… | window | 8 | holds | not started |
| B53 | Opens a running download's progress window and its Connections section | window | 6 | holds | not started |
| B54 | Opens the window of a download that failed with the disk full, a link that no longer wo… | window | 6 | holds | not started |
| B55 | Chooses "Copy details" on a download that failed (its right-click menu, the File menu,… | window | 3 | holds | not started |
| B56 | Opens Options › Downloads and looks at "Connections per file" | window | 7 | holds | not started |
| B57 | Opens Options › Advanced and adds a site with its own connection limit | window | 7 | holds | not started |
| B26 | Clicks a link to a file | browser and hand-off | all | holds | not started |
| B37 | With dm set as installed, clicks a link to any kind of file: `.bin`, `.txt`, `.zip`, `.… | browser and hand-off | all | holds | not started |
| B27 | Clicks a link to a PDF the browser would show | browser and hand-off | all | holds | not started |
| B28 | Alt-clicks a link | browser and hand-off | all | holds | not started |
| B29 | Right-clicks a link, "Download link with dm" | browser and hand-off | all | holds | not started |
| B30 | Right-clicks a page, "Download all links with dm" | browser and hand-off | all | holds | not started |
| B31 | Switches "Catch downloads" off in the toolbar popup | browser and hand-off | all | holds | not started |
| B32 | Downloads with dm not installed | browser and hand-off | all | holds | not started |
| B33 | Rests the pointer on a playing video | browser and hand-off | all | holds | not started |
| B34 | Clicks it | browser and hand-off | all | holds | not started |
| B35 | Picks one | browser and hand-off | all | holds | not started |
| B36 | Everything else the browser check covers | browser and hand-off | all | holds | not started |
| B59 | Opens a video's quality list in the browser panel and looks at its first line | browser and hand-off | all | holds | not started |
| B60 | Opens Tasks › Grabber and looks under the page depth | window | 5 | holds | not started |
| B61 | Opens Tasks › Grabber and looks under "Also follow links to other sites" | window | 5 | holds | not started |
| B62 | Opens the Scheduler and looks under the "When the scheduled downloads are done" choices | window | 5 | holds | not started |
| B65 | Opens the Scheduler | window | 5 | holds | not started |

Rows with no slice: none.
