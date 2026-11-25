# Questions dm asks

Some things dm does first ask you. This page lists every question, when it
comes, and what each answer does.

## How a question works

- A question opens in front of dm's main window and holds it until you
  answer. On Windows it is a small window of its own; on macOS it slides
  down from the top of the main window. If the main window is in the tray,
  it comes back for the question.
- Questions come one at a time. One asked while another is open waits its
  turn.
- One button is highlighted: **Return** picks it. **Escape** picks the
  answer that changes the least; for each question below it is named.

## Already in the list

When an address you add in [Add URL](add-url.md), or one the browser hands
over, is already a download in the list. It shows the address and "This address is
already in the list (…)", with that download's status.

- **Show it:** chooses that download in the list. It reads **Resume it**
  when that download is stopped or failed, and then also resumes it or
  tries it again.
- **Download again:** opens [Download details](download-details.md) to add
  it as a new download.
- **Replace the old file:** downloads it again into the same folder under
  the same name, and replaces the old file once the new one is whole. Only
  offered when that download has finished and its file is still there.
- **Cancel:** does nothing.
- **Remember my choice:** gives the same answer from now on without asking.
  Change it back in **Options › General › When an address is already in
  the list** (**Ask me**).

Return: **Show it** (or **Resume it**). Escape: **Cancel**.

## Cancel this download?

From **Cancel download** in a download's [progress
window](download-window.md). It names the file, and says "Cancelling
discards what was downloaded." when something has come.

- **Keep downloading:** changes nothing.
- **Yes, cancel:** cancels the download and closes its progress window.

Return and Escape: **Keep downloading**.

## Removing downloads

From **File › Delete**, the toolbar's **Delete**, **Delete** on the
right-click menu or the Delete key; and from **File › Delete file…** or
**Delete file** on the right-click menu. Delete is greyed while a chosen
download runs or waits in line.

With **Delete**, finished downloads are taken off the list without a
question (their files are kept); the status bar says "Removed … downloads
from the list." with **Undo** for a moment. Anything else asks first. The title says what it is about:

- **Remove an unfinished download** or **Remove unfinished downloads:**
  one, or several, not finished.
- **Remove from the list:** finished and unfinished together.
- **Delete file:** from **Delete file**.

The question names the file, or says "Downloads: …" for several, and says
what will happen: for example "This download isn't finished. Removing it
discards what was downloaded.", "Nothing was downloaded yet. Remove it from
the list?", or "Its file is kept." / "Its file is deleted too.".

- **Also delete the file** (or **Also delete the files**): shown when
  finished files are still on disk. Ticked, their files go to the Recycle
  Bin (the Trash on macOS) with them. It starts ticked for **Delete file**.
- **Keep it** (or **Keep them**): changes nothing.
- **Remove download** or **Remove downloads:** takes the downloads off the
  list. It reads **Delete file** or **Delete files** while files are to be
  deleted too.

Return and Escape: **Keep it** (or **Keep them**). A file that cannot be
deleted is said in the status bar: "Couldn't delete …".

## Start outside the schedule?

When you start downloads that the [Scheduler](scheduler.md) or the
download limit (see [Speed limiter](speed-limiter.md)) is holding: with
**Start now** in Download details, **Resume** on the list, or **Start now**
in a progress window. It says "… waits for the schedule or the download
limit." and what is holding it ("Held by the schedule; it starts by itself
at …", or "Held by the download limit; it goes on by itself in …"), then
"Start it now? Only your selection starts."

- **Start now:** starts these downloads now, this once.
- **Keep waiting:** leaves them to start by themselves.

Return: **Start now**. Escape: **Keep waiting**.

## Name sent by the server

When a server sends a different name for the file than its address gives:
"The name to save the file was …, but the server sent …. Save the file
with the name received from the server?"

- **Use …:** keeps the server's name.
- **Keep …:** gives the file the address's name, at once if it has
  finished, else as it finishes.

Return and Escape: **Use …**.

## Web page instead of the file

When a web page comes back where a file was asked for: "The requested file
looks like an HTML web page. Would you like to continue downloading it and
save it as an HTML page?"

- **Save as HTML:** downloads the page and keeps it.
- **Don't download:** removes the download.

Return and Escape: **Save as HTML**.

## The file doesn't exist

When a server says the file is not there: "The requested file doesn't
exist.", with "Details:" and the server's answer, "HTTP/1.1 404 Not Found".

- **OK:** closes the question.
- **Update download link:** opens [Update download link](update-link.md)
  for that download.

Return and Escape: **OK**.

## The same file is already here

When a download's server gives the same name and size (and, where both
have one, the same entity tag or digest) as a finished download in the
list, or as a file already in the folder it would be saved in: it is asked
before any of the file is downloaded. It shows the name and "A finished
download in the list has this name and size." or "A file of this name and
size is already in the folder." An address already in the list is asked
about as **Already in the list** instead.

- **Show it:** takes the new download off the list, nothing of it
  downloaded, and chooses the finished download in the list, or shows the
  file in its folder.
- **Download again:** downloads it, saved beside the other as a numbered
  copy.
- **Remember my choice:** gives the same answer from now on without asking.
  Change it back in **Options › General › When the same file is already
  here** (**Ask me**).

Return and Escape: **Show it**. Nothing is asked with no window open
(`--headless`), nor for a download added from the command line or an AI
assistant: the file is downloaded as before.

## Some are already in the list

From **Tasks › Import…**, when some addresses in the list you open are
already downloads in dm: "… of the … addresses in this list are already in
dm's list."

- **Skip them:** adds only the new ones.
- **Add them again:** adds every address.
- **Cancel:** adds nothing.

Return: **Skip them**. Escape: **Cancel**.

## Leave this site to the browser?

When you cancel [Download details](download-details.md) for a second
download the browser handed over from the same site: "You cancelled two
downloads from …. Leave its downloads to the browser?"

- **Leave them to the browser:** adds the site to **Options › Browser ›
  Never take downloads from these sites**, so its downloads stay with the
  browser.
- **Keep handing them over:** changes nothing.

Return: **Leave them to the browser**. Escape: **Keep handing them over**.

## Keep running in the tray?

The first time you close dm's main window while **Options › General ›
When I close the window** is **Ask me**: "Keep dm in the tray and your
downloads carry on, and your browser can still send it links. Or exit dm
now. You can change this later in Options."

- **Exit:** exits dm (asking **Exit dm?** first while downloads run).
- **Keep running:** puts the window away in the tray; downloads go on.

The answer is kept in **When I close the window**, so it is not asked
again. Return and Escape: **Keep running**. Where there is no tray to keep
dm in, closing the window exits dm, asking **Exit dm?** first while
downloads run.

## Exit dm?

When you exit dm (**Tasks › Exit**, Ctrl+Q or ⌘Q, the tray's **Exit dm**)
while downloads run or wait to start. With nothing running or waiting, dm
exits without asking. It says how many downloads are running and how many
are waiting, and "Exiting pauses them. They pick up where they left off
the next time you open dm."

- **Keep downloading:** dm stays open.
- **Exit:** pauses the downloads and exits.

Return and Escape: **Keep downloading**. If every download finishes while
the question is open, it says "Nothing is downloading any more. Exit dm?",
**Keep downloading** becomes **Stay**, and Return picks **Exit**.

## Downloads are done

When **Downloads › When done** (or the Scheduler's, or a download's own
**When this download is done**) is set to exit, sleep, hibernate or turn
off, and its time has come. It counts down: "dm will put the computer to
sleep in 30 seconds." (or "quit", "hibernate the computer", "shut the
computer down").

- **Cancel:** stops the countdown; nothing happens.
- **Now:** does it at once.

Return and Escape: **Cancel**. The question closes by itself when the
countdown ends.
