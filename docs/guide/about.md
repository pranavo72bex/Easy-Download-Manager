# About dm

About dm says which version of dm you have, links to the project's pages,
and checks for a newer version. Open it with **Help › About dm** (on macOS
also **dm › About dm**). **Help › Check for updates** opens it and checks at
once. Asked for again while it is open, it comes to the front.

## What it shows

- **dm**, "Version …", and "A free download manager, written in Rust."
- Links to the project's pages: **Source code**, **Licence (GPL v3)**,
  **Privacy** and **Code signing policy**. Hold the mouse over one to see
  its address; click it to open it in your browser.

## Checking for updates

- **Check for updates:** asks the project whether a newer version is out.
  Only an answer signed with the project's key is believed. The line under
  the button says where it stands:
  - "Not checked yet."
  - "Checking…"
  - "This is the newest version (…)."
  - "dm … is available (you have …). Download its installer here."
  - "dm … is out (this is …). It is on the project's releases page."
  - "No newer release yet (you have …)."
  - "Couldn't reach the update server: …"
- **What’s new:** opens the notes of the newer version, where there are
  some.
- **Download the update:** shown when a newer version has an installer for
  this computer. dm downloads it like any other download and checks it
  against the digest the release gives; the status bar says "Downloading
  the update. dm will offer to install it when it's verified."

Once the update is downloaded and checked, a bar above the list says "The
dm update is downloaded and checked." with **Later**, which puts the bar
away, and **Install now**, which opens the installer and then exits dm
(asking **Exit dm?** first while downloads run, see [Questions dm
asks](questions.md)). On Linux the
second button is **Open folder**, which shows the downloaded file.

With **Options › General › Check for updates once a week** on, dm checks by
itself once a week and opens About dm when a newer version is out.

**Escape** closes the window.

## Tip of the day

**Help › Tip of the day** opens a small window with one tip about dm.

- **Next tip:** shows the next one. **Return** does the same.
- **Close:** closes it. **Escape** does the same.
