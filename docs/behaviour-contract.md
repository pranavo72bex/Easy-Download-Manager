# Behaviour contract

What a person does in dm and what must then happen. This list is fixed:
**no change by an AI or anyone else alters a behaviour here unless the
maintainer has approved that change in words.** An approved change edits its
row here, and the baseline pictures if the window changes, in the same
commit, which names the approval.

Every row is checked by `scripts/human-qa.py`, which drives the real window
and Microsoft Edge the way a person does, slowly and in sight, and fails if
any row below was not checked. Run it whole, not a part of it: before a merge,
after any change to `desktop/`, `browser/` or `extension/`, and whenever
someone asks for QA.

    python3 scripts/human-qa.py

The target is reference A, the best-known commercial download manager for
Windows. The "Reference A" column says whether dm already does what it does;
a row that differs stays as dm has it until the maintainer decides.
Recorded 2026-10-29 from the merged `main`, where every row passed.

## Adding a download

| ID | A person does | What must happen | Reference A |
|---|---|---|---|
| B01 | Clicks Add URL and presses OK with nothing typed | The Add URL dialog stays, says "Type or paste the address of what to download." and nothing is added | same |
| B02 | Types an address in Add URL and presses OK | The Download details dialog opens: the address, Category, Save to folder, Save as, Size, and Start now / Add for later / Cancel. For a video page it also lists Quality (each of the video's qualities with its size, the setting's one picked, sound alone among them) and, for a YouTube video, Part ("Only a part of the video", from and to). A YouTube playlist address opens the Playlist dialog instead: its videos listed with a tick each, Select all, Video quality, Start now / Add for later / Cancel. A download the browser caught also says which browser it came from ("From Microsoft Edge") and offers "Always save here without asking", which stops the asking for caught downloads (Options › Browser). (Added 2026-10-30 at the maintainer's request to take the other macOS manager's good things, as a clean-room implementation in Rust and egui.) | same (its file information dialog); the other macOS manager has the quality list and the part |
| B03 | Presses Start now | The download starts at once and its row shows its progress | same |

## Stopping and resuming

| ID | A person does | What must happen | Reference A |
|---|---|---|---|
| B04 | Selects a running download and clicks Stop | It stops; the row says "Stopped · N%" and keeps what it fetched | same |
| B05 | Clicks Resume | It goes on from what it had: no byte fetched twice, the file whole | same |

## How a file is fetched

| ID | Situation | What must happen | Reference A |
|---|---|---|---|
| B06 | Any file from a server that takes ranges | Several connections (8 by default); a connection with nothing left halves the largest piece still being fetched | same |
| B07 | A fast server that keeps connections open | Finished connections take the next piece: more requests than connections | same |
| B08 | A server that holds each connection to a low speed | Every connection is used; 32 MiB at 2 MiB/s a connection finishes in a few seconds, not sixteen | not documented |
| B09 | A server that allows two connections at once and answers 503 to more | The refusals are absorbed and the file completes | same (fewer connections for such servers) |
| B10 | A server that does not take ranges | Fetched on one connection, whole | same |
| B11 | A server that says the file is longer than it sends | The file completes; it does not stay "running" | not documented |
| B12 | An address that redirects | Followed; saved under the name of what it reached | same |
| B13 | A server that names the file (Content-Disposition) | Saved under that name ("Report 2026.pdf") | same |
| B14 | An address that answers 404 | The row fails and says "This link no longer works", with the server's answer | same |
| B15 | Every download that completes | Its bytes are exactly the server's | same |

## Where files go

| ID | Situation | What must happen | Reference A |
|---|---|---|---|
| B16 | Sorting into folders by kind (on by default) | A PDF lands in `Documents`, a `.bin` in `Other`, under the download folder | same (categories) |
| B17 | Adds an address already in the list | dm asks first: "Already in the list", "This address is already in the list (Complete).", with Download again / Show it / Cancel. Cancel adds nothing; Download again, then Start now, saves a second copy beside the first as "name (1).ext" | same (it asks what to do with a repeated link) |

## The queue and speed

| ID | A person does | What must happen | Reference A |
|---|---|---|---|
| B18 | Adds five downloads at once | No more than three run at once (the default); the rest wait their turn and complete | same |
| B19 | Presses Add for later, then Start queue | The download waits, not running, until Start queue; then it completes | same |
| B20 | Chooses Low speed (256 KB/s) | Downloads together keep to about 256 KB/s; Unlimited lifts it | same |

## When things go wrong

| ID | Situation | What must happen | Reference A |
|---|---|---|---|
| B21 | dm is killed in the middle of a download, then opened | The row is there and continues from where it was; the file whole | same |
| B22 | dm is quit and opened again | Every row comes back in the state it had | same |

## Tidying the list

| ID | A person does | What must happen | Reference A |
|---|---|---|---|
| B23 | Selects a finished download and clicks Remove | It leaves the list at once, without a question; its file stays; the window says "Removed 1 download from the list." with Undo, and Undo puts the row back | **differs**: reference A asks every time. dm asks only when something would be lost (B38); kept until the maintainer decides |
| B38 | Selects a stopped, unfinished download and clicks Remove | dm asks first, "Remove an unfinished download": "This download isn't finished. Removing it discards what was downloaded.", Yes, remove / Keep it. Keep it leaves it as it was; Yes, remove takes it off the list | same |
| B24 | Clicks Clear finished | Finished rows leave the list, the window says "Deleted N …", every file stays | not documented |

## How the window looks

| ID | Situation | What must happen | Reference A |
|---|---|---|---|
| B25 | The empty window, Add URL, and Download details | Each looks as in `docs/behaviour-baseline/` (on the machine those were taken on; no more than 1% of pixels differ, which the clock and an address account for). Taken again on 2026-10-30 with the side panel's switch at the toolbar's left end, at the maintainer's request for that icon | n/a |

## The browser (Microsoft Edge with dm's extension)

| ID | A person does | What must happen | Reference A |
|---|---|---|---|
| B26 | Clicks a link to a file | dm fetches it; the browser keeps no copy | same |
| B37 | With dm set as installed, clicks a link to any kind of file: `.bin`, `.txt`, `.zip`, `.exe`, `.dmg`, `.iso`, `.json`, `.csv`, `.mp3`, `.mp4`, `.jpg`, `.xlsx`, an unknown type, no extension | Every one is downloaded by dm, whole; the browser keeps none. The browser keeps a download only when the person asks it to: Alt-click (B28), Catch downloads off (B31), a site paused in the popup, a private window, or a type list they set themselves in Options | same, and more: reference A takes the types on its list; dm takes every type |
| B27 | Clicks a link to a PDF the browser would show | dm fetches it and the tab stays where it was | not documented |
| B28 | Alt-clicks a link | The browser downloads it itself | same |
| B29 | Right-clicks a link, "Download link with dm" | dm fetches it | same |
| B30 | Right-clicks a page, "Download all links with dm" | Every link goes to dm's queue | same |
| B31 | Switches "Catch downloads" off in the toolbar popup | Downloads stay with the browser | same |
| B32 | Downloads with dm not installed | The browser downloads it itself | same |

## Videos (a real YouTube page in Edge)

| ID | A person does | What must happen | Reference A |
|---|---|---|---|
| B33 | Rests the pointer on a playing video | "Download video" appears in the player's top corner and stays | same |
| B34 | Clicks it | The video's qualities are listed with their sizes (on YouTube the page's own player lists them in a moment, and the sizes follow with dm's reading of the page, which takes the list's place; added 2026-10-30 at the maintainer's request to take the other macOS manager's extension's way) | same |
| B35 | Picks one | It says "Sent to dm"; the video lands in dm's folder whole, picture and sound | same |
| B36 | Everything else the browser check covers | Every one of its checks passes | n/a |
