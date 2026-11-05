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
| B02 | Types an address in Add URL and presses OK | The Download details dialog opens: the address, Category, Save to folder, Save as, Size, and Delete file after (Never, 10 minutes, 1 hour, 1 day; added 2026-10-31 at the maintainer's word to take what is needed from the other macOS manager's save dialog), and Start now / Add for later / Cancel. For a video page it also lists Quality (each of the video's qualities with its size, the setting's one picked, sound alone among them) and, for a YouTube video, Part ("Only a part of the video", from and to). A YouTube playlist address opens the Playlist dialog instead: its videos listed with a tick each, Select all, Video quality, Start now / Add for later / Cancel. A download the browser caught also says which browser it came from ("From Microsoft Edge") and offers "Always save here without asking", which stops the asking for caught downloads (Options › Browser). (Added 2026-10-30 at the maintainer's request to take the other macOS manager's good things, as a clean-room implementation in Rust and egui.) | same (its file information dialog); the other macOS manager has the quality list and the part |
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
| B13 | A server that names the file (Content-Disposition) | Saved under that name ("Report 2026.pdf") unless the person says No to the question of B39 | same |
| B39 | A server names the file differently from the address (`/dl?id=7` answered with "Report 2026.pdf") | A question, "Name sent by the server": "The name to save the file was dl, but the server sent Report 2026.pdf. Save the file with the name received from the server?", Yes / No. Yes saves it as "Report 2026.pdf"; No saves it under the address's name. Not asked when the names agree, for a name the person typed in Download details, or for links added together by "Download all links". Added 2026-11-01 at the maintainer's word | same |
| B40 | An address that ends like a file (`.zip`) answers with a web page | A question, "Web page instead of the file": "The requested file looks like an HTML web page. Would you like to continue downloading it and save it as an HTML page?", Yes / No. Yes saves the page as `<name>.html`; No removes the row and keeps nothing. Where nothing can ask (the command line, an AI client) the row fails with "web page rather than the file", as before. Added 2026-11-01 at the maintainer's word. Since 2026-11-06 an address typed in that is a web page and not a file (nor a video site's page dm reads itself) is asked about the same way, where it was refused; its row's words still say a video comes from the browser's Download video button (gaps row 136, the maintainer's word of 2026-11-06 to do what is better for the person) | same |
| B14 | An address that answers 404 | The row fails and says "This link no longer works", with the server's answer, and a window tells it once (B41) | same |
| B41 | An address that answers 404, the row having failed | Once, a window "The file doesn't exist": "The requested file doesn't exist.", Details: "HTTP/1.1 404 Not Found", OK. It does not come back for the same row. Added 2026-11-01 at the maintainer's word | same |
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
| B19 | Presses Add for later, then Start Queue | The download waits, not running, until Start Queue; then it completes | same |
| B64 | Opens Downloads › Start Queue (or Stop Queue), or the arrow beside the toolbar's Start Queue (or Stop Queue) | Each lists the queues; the queue clicked is started (or stopped), whatever the list shows. The toolbar button itself still acts on the queue the list shows (the Main queue where it shows no queue). Added 2026-11-05 at the maintainer's word that dm "should work exactly like" reference A (`docs/decisions.md`, 2026-11-04) | same |
| B20 | Chooses Low speed (256 KB/s) | Downloads together keep to about 256 KB/s; Unlimited lifts it | same |

## When things go wrong

| ID | Situation | What must happen | Reference A |
|---|---|---|---|
| B21 | dm is killed in the middle of a download, then opened | The row is there and continues from where it was; the file whole | same |
| B22 | dm is quit and opened again | Every row comes back in the state it had | same |

## Tidying the list

| ID | A person does | What must happen | Reference A |
|---|---|---|---|
| B23 | Selects a finished download and clicks Delete | It leaves the list at once, without a question; its file stays; the window says "Removed 1 download from the list." with Undo, and Undo puts the row back | **differs**: reference A asks every time. dm asks only when something would be lost (B38); kept until the maintainer decides |
| B38 | Selects a stopped, unfinished download and clicks Delete | dm asks first, "Remove an unfinished download": "This download isn't finished. Removing it discards what was downloaded.", Yes, remove / Keep it. Keep it leaves it as it was; Yes, remove takes it off the list | same |
| B24 | Clicks Delete Completed | Finished rows leave the list, the window says "Deleted N …", every file stays | not documented |

## How the window looks

| ID | Situation | What must happen | Reference A |
|---|---|---|---|
| B25 | The empty window, Add URL, and Download details | Each looks as in `docs/behaviour-baseline/` (on the machine those were taken on; no more than 1% of pixels differ, which the clock and an address account for). Taken again on 2026-10-30 with the side panel's switch at the toolbar's left end, at the maintainer's request for that icon; Download details again on 2026-10-31 with its Delete file after row, at the maintainer's word; since 2026-11-01 each dialog is pictured as the window of its own it is (B42): the macOS set in `docs/behaviour-baseline/` was taken before and stays as it is, the Windows set is in `docs/behaviour-baseline/windows/` and waits for the maintainer's approval. Taken again on 2026-11-04 with reference A's labels and columns (Tasks menu; Delete, Delete Completed, Stop All, Start Queue, Stop Queue, Grabber; All Downloads, Unfinished, Finished; File Name, Q, Size, Status, Time left, Transfer rate, Last Try Date, Description), at the maintainer's word that dm "should work exactly like" reference A (`docs/decisions.md`, 2026-11-04); and as compact as reference A's window (a new window 820 by 500, 12-point text, rows 20 apart, every toolbar button named), at the maintainer's word the same day; and the Windows set again on 2026-11-05 with check boxes, radio buttons and toggle switches one size (16 points, the switch 32 by 16, its row 28 tall) and the theme picker at a control's height, at the maintainer's word that the controls "should be consistent"; and the macOS empty window again on 2026-11-06, of the 820 by 500 window (the maintainer's word of 2026-11-06 to do what is better for the person); the macOS Add URL and Download details pictures are taken again by the first run of `scripts/human-qa.py` in a program allowed to record the screen | n/a |
| B42 | Opens a dialog (Add URL, Download details, Options, a question) and drags it by its title bar to another place on the screen, outside the dm window too; then closes dm | Each dialog is a window of its own: the system's title bar with its title and its close button, first over the middle of the dm window, movable anywhere; the dm window stays behind it. Its close button does what Cancel and Escape do. A dialog that keeps the clicks to itself (a question, Options) holds the dm window, which takes no click or key until it is answered, and a click on the held window brings the dialog forward. Closing dm closes them. Added 2026-11-01 at the maintainer's word ("make the UI improvements in gaps.md"): reference A's dialogs are windows of their own | same |
| B43 | Opens Options and looks at its tabs; opens Advanced | Options has six tabs: General, Browser, Folders, Downloads, Advanced and Sign-ins; there are no tabs called Network or Proxy. Advanced holds, one under another on one page, what those two tabs held: the connection's settings (speed limit, attempts, wait between attempts, time out of a silent connection, a second network, a site's limits and rules, the cookies file) under "Network", and the proxy's (use of one, its type, host, port, login, the sites not sent through it; since 2026-11-05 a proxy of its own for HTTP and for HTTPS downloads, each with its host and port, and an automatic configuration script's address, as reference A's, at the maintainer's word that dm "should work exactly like" reference A, `docs/decisions.md` 2026-11-04) under "Proxy", each with the words and the explanation line it always had; OK and Restore defaults keep and put back all of them. A proxy that OK refuses on another tab opens Advanced at the proxy section, with the reason shown. Added 2026-11-01 at the maintainer's word ("make the UI improvements in gaps.md"; gaps row 16). Since 2026-11-06, at the maintainer's word to do what is better for the person: a new dm has "Connect through a proxy" on with the computer's proxy settings (a saved choice is kept), and under "BitTorrent" "Keep sharing a finished torrent" (off) with its times its size, minutes and upload limit (gaps rows 80 and 147) | not documented |
| B44 | Opens Download details for a link that ends in `.zip` (from Add URL or the browser), or Properties of a finished ZIP | A "Contents" line lists the archive's files, each with its name and, beside it, its size once unpacked (a folder has none), in a list that scrolls: the first 500 files, then "and N more". It is read from the end of the archive alone, with a few range requests (from the file on disk for a finished one), so the archive is not fetched to list it: "Reading the contents…" shows while it is read, and nothing shows where the server serves no ranges or the file is no ZIP. Nothing is opened or unpacked. Added 2026-11-01 at the maintainer's word | same |
| B45 | Looks at the list of downloads | Each row has a small picture of its file's kind before its name, told apart by shape and not by colour alone: a play mark for a video, a note for sound, lines for a document, a window for a program, a zip for an archive, a hill and a sun for a picture, a plain sheet for any other file; each in its kind's colour, on every look (light, dark, high contrast), without changing the row's height. Added 2026-11-01 at the maintainer's word ("look for UI improvements which we can see and implement") | same |
| B63 | A new install (nothing chosen yet): adds a download from Add URL and lets it finish | Its progress window opens as it starts, and a Download complete window says so when it ends, with Open and Open folder. A download caught from the browser, or a video from its panel, opens Download details first. (Since 2026-11-04, at the maintainer's word that dm "should work exactly like" reference A, `docs/decisions.md`; until then all four were off. A settings file from before takes these once; a choice made since stands. The browser checks of B26 to B37 run with the two Download details off, as the window tests drive that dialog.) | same |
| B46 | Double-clicks a download that is running, waiting or stopped; looks at its progress window's title bar | Its progress window opens (a finished download does what Options › General says a double-click does; a failed or cancelled one shows its Properties). The window's title starts with how far the download is, `37%`, and the number goes on counting as the download runs; while the size is unknown the title has no percentage. Added 2026-11-01 at the maintainer's word | same |
| B47 | Adds a download from Add URL whose address has a token, a signature, a key, a password or a session in its query (`?token=…`), then opens Add URL again | The download is added as ever, but its address is not in the Recent list, and such an address already in the settings is not offered: the list is kept in the clear and a signed address has expired by the next time. An address without one is remembered and offered as before, without any login. Added 2026-11-01 at the maintainer's word | same |
| B48 | Types text in Find, then presses Shift-F3, or chooses Downloads › Find previous | The previous row whose name has the text is selected and brought into view, wrapping from the first row to the last; with no row selected it starts at the bottom. F3 and Find next go forward as before, also from Find's own field; the menu shows both keys. Added 2026-11-01 at the maintainer's word | same |
| B49 | Opens the Scheduler, picks what dm does when the scheduled downloads are done (nothing, exit dm, sleep, hibernate, shut down) and how many times a failed download is tried again (0 to 5); then the schedule's hours begin with downloads waiting | The choice is kept with the schedule. When the hours begin with something waiting to run, dm arms it as Downloads › When done does, with the same 30-second countdown that can be cancelled, and says so in the status line; a choice made in the Downloads menu this session is never overridden; sleep, hibernate and shut down are greyed where the computer cannot do them. If the hours end with it unused it is put away. While the hours are open, each time the queue has run dry the failed downloads are started again, up to the number set. 0 leaves them failed, as before. Added 2026-11-01 at the maintainer's word | same |
| B50 | Adds an address that is already in the list; looks at the question | It offers Show it (Resume it for a stopped or failed one), Download again, Replace the old file (only when the finished file is on disk), and Cancel, with a tick "Remember my choice". Replace downloads to a temporary name and takes the old file's place when whole. With the tick, the choice is kept in Options › General › When an address is already in the list and the question is no longer asked; setting that row to Ask me brings the question back. Added 2026-11-01 at the maintainer's word | same |
| B51 | Opens Options › Advanced; reads how dm identifies itself; turns on Identify as dm only | The page shows the name dm sends to servers (a browser's by default). With the switch on dm sends its own name first and, for a site that refuses it, asks again with another name. The change applies at once and is kept. Added 2026-11-01 at the maintainer's word | same |
| B52 | Opens the tray icon's Speed menu; chooses Settings… | The Speed limiter dialog opens with the window shown, as Downloads › Speed limiter › Settings… does; the tray's Speed limiter tick and the presets are unchanged. Added 2026-11-01 at the maintainer's word | same |
| B53 | Opens a running download's progress window and its Connections section | Each piece under way shows its own speed beside its bar, worked out from how much it took since the last look; a piece that is done or waiting shows none. Added 2026-11-01 at the maintainer's word | same |
| B54 | Opens the window of a download that failed with the disk full, a link that no longer works, a refused connection, a redirect loop, a name already taken or a certificate that could not be checked | Under the reason it says one plain next step (free space or choose another folder, open the page in the browser, allow dm in a firewall, another name with Move/Rename, check the computer's clock); a failure whose reason already says what to do has none. Added 2026-11-01 at the maintainer's word | same |
| B55 | Chooses "Copy details" on a download that failed (its right-click menu, the File menu, or the button in its window) | The clipboard holds one block to paste to whoever is helping: dm's version, the system, the time, the state and the reason, the host (never the path, a query, a login or a token), the status the server answered, the names of the headers sent (never a cookie's or login's value), the connections and the free space of the download folder's disk; with several failed rows selected, one block each. Added 2026-11-01 at the maintainer's word | same |
| B56 | Opens Options › Downloads and looks at "Connections per file" | Under the box and its hint a line "Presets:" offers Slow (2), Normal (8), Fast (16) and Maximum (32); choosing one sets the box (the draft, kept with OK as any other change); the one matching the box is shown chosen. Each retry, wait and silent-connection row of Advanced has its own hint under it. Added 2026-11-01 at the maintainer's word | same |
| B57 | Opens Options › Advanced and adds a site with its own connection limit | Under "Sites with their own connection limit:" a row appears with a Site field and a Connections number; typing a host name and pressing OK keeps the limit; a download from that host uses at most that many connections; Delete site limit removes it. Added 2026-11-01 | same |

## The browser (Microsoft Edge with dm's extension)

| ID | A person does | What must happen | Reference A |
|---|---|---|---|
| B26 | Clicks a link to a file | dm fetches it; the browser keeps no copy | same |
| B37 | With dm set as installed, clicks a link to any kind of file: `.bin`, `.txt`, `.zip`, `.exe`, `.dmg`, `.iso`, `.json`, `.csv`, `.mp3`, `.mp4`, `.jpg`, `.xlsx`, an unknown type, no extension | Every one is downloaded by dm, whole; the browser keeps none. The browser keeps a download only when the person asks it to: Alt-click (B28), Catch downloads off (B31), a site paused in the popup, a private window, a type list they set themselves in Options, or (since 2026-11-05, reference A's list of the browsers it captures from, at the maintainer's word that dm "should work exactly like" reference A, `docs/decisions.md` 2026-11-04) a browser they untick in Options › Browser | same, and more: reference A takes the types on its list; dm takes every type |
| B27 | Clicks a link to a PDF the browser would show | dm fetches it and the tab stays where it was | not documented |
| B28 | Alt-clicks a link | The browser downloads it itself | same |
| B29 | Right-clicks a link, "Download link with dm" | dm fetches it | same |
| B30 | Right-clicks a page, "Download all links with dm" | Every link goes to dm's queue | same |
| B31 | Switches "Catch downloads" off in the toolbar popup | Downloads stay with the browser | same |
| B32 | Downloads with dm not installed | The browser downloads it itself | same |

## Videos (a real YouTube page in Edge)

| ID | A person does | What must happen | Reference A |
|---|---|---|---|
| B33 | Rests the pointer on a playing video | "Download video" appears in the player's top corner and stays. Since 2026-11-05, as reference A's bar, at the maintainer's word that dm "should work exactly like" reference A (`docs/decisions.md`, 2026-11-04): in the corner of the part of the video in view, below a page's fixed header over its top; and on a video under 400 px wide as the download mark and the close mark alone, its button still named "Download video" | same |
| B34 | Clicks it | The video's qualities are listed with their sizes, as reference A words them since 2026-11-04 at the maintainer's word (`docs/decisions.md`, "dm works exactly like reference A"): a "Download all" line first, which hands every quality over (the app asks about each in turn), then each line numbered with the video's title, "1. <title>… MP4 file, quality 1080p HD" (on YouTube the page's own player lists them in a moment, and the sizes follow with dm's reading of the page, which takes the list's place; added 2026-10-30 at the maintainer's request to take the other macOS manager's extension's way) | same |
| B35 | Picks one | It says "Sent to dm"; the video lands in dm's folder whole, picture and sound | same |
| B36 | Everything else the browser check covers | Every one of its checks passes | n/a |
| B59 | Opens a video's quality list in the browser panel and looks at its first line | A "Part" line sits above the qualities: a start and an end box (m:ss or seconds, either may be left empty). Picking a quality with times typed opens Download details with "Only a part of the video" on and the times filled in (an end alone starts from 0:00); with nothing typed it is as before. Added 2026-11-01 at the maintainer's word | same |

## Grabber

| ID | A person does | What must happen | Reference A |
|---|---|---|---|
| B60 | Opens Tasks › Grabber and looks under the page depth | A box "Also follow links to other sites", off by default, sits there. Left off, only pages on the start page's own site are read, as before; ticked, pages on other sites that the start page links to are read too, to the same depth. A saved search keeps the setting, and one saved before the box existed runs with it off (added 2026-11-02 at the maintainer's word, "yes", to a list of four proposed options) | same |
| B61 | Opens Tasks › Grabber and looks under "Also follow links to other sites" | A box "Keep the site's folders", off by default. Off, every file found is added to the one download folder, as before; ticked, each is saved under the download folder in folders that mirror its address (`site/folder/subfolder`). A saved search keeps the setting, and one saved before the box existed runs with it off (added 2026-11-02 at the maintainer's word, "yes", to a list of four proposed options) | same |

## Scheduler

| ID | A person does | What must happen | Reference A |
|---|---|---|---|
| B62 | Opens the Scheduler and looks under the "When the scheduled downloads are done" choices | A box "Run this program when they are done (optional)", empty by default. With a program typed, it is run once, with no shell and no file, when the queue runs dry in a window in which something ran. A settings file imported or a script driving the window never sets it, as they never set the command run after each download (added 2026-11-02 at the maintainer's word, "yes", to a list of four proposed options) | same |
| B65 | Opens the Scheduler | The queues are listed at its left, the list's own queue chosen; the Main queue's hours are the ones every queue without hours of its own follows, and any other queue can be given its own ("Hours of its own for this queue"; off, it says it follows the Main queue's). The hours start every week on the days ticked or once on a date (YYYY-MM-DD; a date that is not one is refused where it was typed). Start now and Stop start or stop the queue chosen there. OK keeps every queue's hours; Cancel none. Added 2026-11-05 at the maintainer's word that dm "should work exactly like" reference A (`docs/decisions.md`, 2026-11-04) | same |
