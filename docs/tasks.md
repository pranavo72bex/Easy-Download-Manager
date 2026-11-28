# Twenty everyday tasks, counted

"Easier to use than reference A" is measured here: twenty things people do
with a download manager every day, each with the path it takes in EDM and what
that path costs in clicks, keys, typed fields and windows. A person counts the
same twenty once in reference A and fills in its columns; any task where EDM
takes more steps or opens more windows becomes a row in `docs/gaps.md`
proposing the change (and, since the window's behaviour is fixed by
`docs/behaviour-contract.md`, the maintainer approves it first).

`scripts/task-steps.py` does every task marked **driven** in EDM's real window,
through the control port, the way a person would (clicking the words, typing
into the field that has the keyboard, pressing keys), counts what it did, and
checks that the task was done (the file whole, the row gone, the setting kept).
The tasks marked **by hand** need the browser, a real site, or would run a file
or turn the computer off; their EDM counts below are from the path, to be
confirmed by the person who counts reference A. The script prints all twenty,
with reference A's numbers beside EDM's once they are filled in, and lists each
task where EDM takes more.

    python -X utf8 scripts/task-steps.py            # every driven task
    python -X utf8 scripts/task-steps.py T05 T09    # some of them

On Windows, start it from PowerShell or Windows Terminal, not Git Bash (as
`scripts/human-qa.py`; `docs/testing.md`). It needs a debug build (`EDM_BINARY`
names another) and writes its record to `task-steps.json` in its work folder.

## How steps are counted

- **Click**: each press of the mouse on a button, a tab, a row, a menu title, a
  menu item or a pop-up's choice. A double-click counts one; a right-click one.
- **Key**: each key or chord pressed (Ctrl+F is one; Tab is one).
- **Typed**: each field typed into counts one, however long the text.
- **Window**: each window that opens of itself or from a step: a dialog in a
  window of its own, a progress window, the system's file window. Menus,
  pop-ups and the browser's own panel are not windows.
- **Steps** = clicks + keys + typed. A task where EDM has more steps, or the same
  steps and more windows, than reference A is one where EDM takes more.
- **Not counted**, being the same in both: what is done outside the download
  manager (copying a link in the browser, going to the page), waiting, and
  closing windows after the task is done.
- **Where each task starts**: a new install (nothing chosen in Options), the
  window open and showing every download, and what the task needs already
  there (a finished download, a failed one). On a new install EDM opens a
  download's progress window as it starts (B63), so those are counted.

## The tasks

The path uses the words the window shows. "›" goes from a menu to its item.
"Expected" numbers are the path's; for a driven task the script's count is the
measure and it says where the two differ.

| ID | Task | How | Path in EDM | Clicks | Keys | Typed | Windows | Reference A: steps | Reference A: windows | Reference A: path as taken |
|---|---|---|---|---|---|---|---|---|---|---|
| T01 | Catch a file from the browser | by hand | In the browser, click the link to the file; EDM's Download details opens; Start now | 2 | 0 | 0 | 2 |  |  |  |
| T02 | Download a video at 1080p | by hand | On the playing video, click "Download video"; click the line "… MP4 file, quality 1080p HD"; Download details opens; Start now | 3 | 0 | 0 | 2 |  |  |  |
| T03 | Download a YouTube playlist | by hand | Copy the playlist's address; Add URL (the address is put in from the clipboard); OK; the Playlist window lists the videos, each ticked; Start now | 3 | 0 | 0 | 2 + 1 a video |  |  |  |
| T04 | Download every file a page links to | by hand | In the browser, right-click the page; "Download all links with EDM"; the Download links window lists them, each ticked; Start now | 3 | 0 | 0 | 1 + 1 a file |  |  |  |
| T05 | Add a copied link | driven | Add URL (the copied link is put in); OK; Download details; Start now | 3 | 0 | 0 | 3 |  |  |  |
| T06 | Pause a download and resume it | driven | Click its row; Stop; later, Resume | 3 | 0 | 0 | 1 |  |  |  |
| T07 | Download a batch of copied links | driven | Copy the links; Tasks › Add links from clipboard; the Links on the clipboard window lists them, each ticked; Start now | 3 | 0 | 0 | 4 |  |  |  |
| T08 | Add a download now and start it later | driven | Add URL (the copied link is put in); OK; Add for later; later, Start Queue | 4 | 0 | 0 | 3 |  |  |  |
| T09 | Download only at night, 02:00 to 06:00 every day | driven | Scheduler; tick "Only download between set hours"; Start at: click 01, choose 02; Stop at: click 07, choose 06; OK | 7 | 0 | 0 | 1 |  |  |  |
| T10 | Limit the speed | driven | Downloads › Speed limiter › Low speed (256 KB/s) | 3 | 0 | 0 | 0 |  |  |  |
| T11 | Find a finished file and show it in its folder | driven | Ctrl+F; type part of its name; Find next; Close; File › Open folder (counted, not clicked by the script: it opens the system's file window) | 4 | 1 | 1 | 2 |  |  |  |
| T12 | Open a finished file | by hand | Double-click its row (Options › General: a double-click opens the file, as a new install has it) | 1 | 0 | 0 | 0 |  |  |  |
| T13 | Give a failed download a fresh link | driven | Right-click its row; Update download link; paste the fresh link; OK (it is tried again at once) | 3 | 0 | 1 | 2 |  |  |  |
| T14 | Try a failed download again | driven | Click its row; Resume | 2 | 0 | 0 | 1 |  |  |  |
| T15 | Move a finished file to another folder | driven | Right-click its row; Move/Rename; click the Folder field; Ctrl+A; type the folder; OK | 4 | 1 | 1 | 1 |  |  |  |
| T16 | Take a finished download off the list | driven | Click its row; Delete (no question: the file stays, Undo is offered) | 2 | 0 | 0 | 0 |  |  |  |
| T17 | Clear every finished download off the list | driven | Delete Completed | 1 | 0 | 0 | 0 |  |  |  |
| T18 | Download a numbered series (photo1 to photo3) | driven | Tasks › Add numbered series; type the address with *; Tab; type 1; Tab; type 3; Start now | 3 | 2 | 3 | 4 |  |  |  |
| T19 | Change where downloads are saved | driven | Options; Folders; click the Download folder field; Ctrl+A; type the folder; OK | 4 | 1 | 1 | 1 |  |  |  |
| T20 | Turn the computer off when the downloads are done | by hand | Downloads › When done › Turn off computer | 3 | 0 | 0 | 0 |  |  |  |

Notes on the paths:

- T03 and T04 open a progress window for each download as it starts, three at
  most at once (B18); T07 and T18 count those three.
- T05 and T08: Add URL puts in a link it finds on the clipboard. Typed in by
  hand instead, each is one typed field more.
- T11: a person who sees the row needs only the last two clicks; the count is
  for a list long enough to search.
- T13: when the link has just failed with "The file doesn't exist" (B41), that
  window's own Update download link button is the shorter way: one click, the
  fresh link, OK.
- T15: the script clicks the Folder field where its words are drawn, and
  presses Tab from the name field where they are not; either is one step.
- T20 is not driven: the script would turn the computer off.
