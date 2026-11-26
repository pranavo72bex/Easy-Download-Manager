# Grabber

The Grabber reads a website and lists the files it links to, so you can
pick which to download. Open it with **Tasks › Grabber…** or the toolbar's
**Grabber** button. A saved search under **Grabber projects** in the side
panel opens it with that search filled in. Asked for again while it is
open, it comes to the front as it stands.

## The search

- **Start page:** the page to begin from. If **Options › Sign-ins** keeps a
  login for the site, the Grabber uses it and says "Signed in with the
  login Options › Sign-ins keeps for this site."
- **Template:** **Choose…** fills **File types** for **Pictures**,
  **Documents**, **Videos and music**, **Archives and programs**, or
  **Every file** (which empties it).
- **File types:** the kinds of file to find, separated by spaces or
  commas, such as `zip pdf mp4`. A new search starts with `zip rar 7z exe
  msi dmg iso mp4 mkv mp3 pdf jpg png`. Empty finds every file.
- **Page depth:** 0 reads the start page alone; 1 also reads the pages the
  start page links to, 2 the pages those link to, and so on, up to 5. At
  most 100 pages are read in one search.
- **Also follow links to other sites:** off, only pages on the start
  page's own site are read; on, pages elsewhere are read too.
- **Keep the site's folders:** saves each file in folders named after its
  site and the folders of its address, instead of straight into the
  download folder.
- **Time limit (minutes):** a search stops after this long
  with what it has found. 0 is no limit beyond the 100 pages; the most is a
  day.
- **Smallest file (KB)** and **Largest file (KB):**
  leave files out by size; 0 is any size. dm asks each file's server for its size. A file
  whose size the server does not say is kept.
- **Only paths containing:** words a file's address must have, separated
  by spaces or commas; empty is any.
- **Skip paths containing:** files and pages whose address has one of
  these words are left out.
- **Find files:** starts the search.

## While it reads

The window says "Reading the page…", then "Reading… … pages, … files
found". **Stop** ends the search early and lists what it found so far.

## What it found

- "Files found: … · pages read: …". If the time ran out first: "It ran out
  of time before reading every page. These are the files it found."
- "No files of those types are linked from that page." when there are none.
- **Check all** and **Uncheck all:** tick or untick every file.
- The list: each file's address with a tick box. Every file is ticked
  except one already in dm's list, which says "(already in the list)".
- A search that could not be made says why in the warning colour.

## Saving a search

- **Save this search as:** a name for the search.
- **Run every (hours):** how often dm runs it again by itself,
  up to a month (744 hours). 0 runs it only when you ask.
- **Save search:** keeps the search under its name; saving again under the
  same name replaces it and keeps what it found before. A search needs a
  name ("A saved search needs a name.") and a start page ("A saved search
  needs a start page.").
- **Saved searches:** each saved search, by name and start page, with:
  - **Run:** runs it now, in the background.
  - **Delete search:** forgets it.

Saved searches are listed under **Grabber projects** in the side panel too.
A saved search that runs by itself, or with **Run**, adds only the files it
has not found before and that are not already in the list, to the Main
queue for later, and the status bar says "Added … downloads to Main
queue." or "The search … found nothing new."

## Buttons

- **Close:** closes the Grabber.
- **Add for later:** adds the ticked files to the Main queue without
  starting them. Shown once files are found.
- **Start now:** adds the ticked files and starts them. Shown once files
  are found.

## Keys

- **Return** is **Find files**.
- **Escape** closes the Grabber.
