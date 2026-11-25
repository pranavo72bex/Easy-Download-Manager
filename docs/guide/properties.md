# File properties

File properties shows everything about one download and lets you change its
address, sign-in, request headers and note. Open it with **File ›
Properties…**, **Properties** on a download's right-click menu, or Ctrl+I
(⌘I on macOS). With several downloads chosen, each gets a window of its
own; asking again for one already open brings it to the front.

## What it shows

- **File name**, **Status** and **Size.** Hold the mouse over a failure's
  **Status** to read dm's own sentence for it. Under a failure, a line says
  the next step where dm knows one (see [What dm's messages
  mean](failures.md)).
- **Category:** the kind of file, or the category you made that it belongs
  to.
- **Will be saved to** (before it is saved) or **Saved to:** the file's
  full path.
- **Referrer:** the page the download came from.

An empty value shows "—".

## What you can change

- **Address**, **User name** and **Password:** the link, and the sign-in that
  goes with it.
- **User-Agent:** the browser name dm gives the server for this download.
- **Request headers:** other headers to send, written `Name: value | Name:
  value`.
- **Note:** your own words, kept with the download.
- **Delete file after:** **Never**, **10 minutes**, **1 hour** or **1 day**
  after it finishes, the file goes to the Recycle Bin (the Trash on macOS)
  and the row leaves the list. Not shown for a failed or cancelled
  download.

**Address**, **User name**, **Password**, **User-Agent** and **Request
headers** are greyed while the download runs and once it has finished,
except while it waits to try again after a failure. **Note** can always be
changed.

## A torrent's files

- **Files:** each file of the torrent with its size and a tick box; untick
  the ones you do not want. With several files, each ticked one has a
  priority: **High**, **Normal** or **Low**.
- The list is greyed while the torrent runs, waits in line or has
  finished; while it runs or waits it says "Stop the download to change
  which files are fetched."
- OK with no file ticked says "Tick at least one file."

## A finished file

- **SHA-256:** the file's SHA-256 digest. It says "Working it out…" while
  dm reads the file. You can select it and copy it.
- **Check against:** paste a digest you were given (CRC32, MD5, SHA-1,
  SHA-256 or SHA-512, told apart by its length; a name such as `sha256:`
  in front is allowed). dm works out the same kind and says "It matches."
  or "It does not match."
- **Contents:** for a ZIP file, the files in it with their sizes, and **and
  … more** past the first ones.

## Buttons

- **Open:** opens the finished file. Greyed until it has finished and while
  the file is not where dm saved it.
- **Copy details:** for a failed download only. Copies a short report to
  paste to whoever is helping you, and the status bar says so.
- **Cancel:** closes the window and keeps nothing you changed.
- **OK:** keeps what you changed and closes the window. If dm cannot keep
  it, the window stays open and says why.

## Good to know

- When a download failed because it needs a sign-in, the window opens with
  the keyboard in **User name**. Changing the address (a login and password
  count) and clicking **OK** tries the download again, and the status bar
  says "Trying … again with the sign-in."
- **Return** in a field is **OK**; **Escape**, Ctrl+W (⌘W on macOS) and
  the close button are **Cancel**.
- The window follows the download as it changes.
