# Update download link

Update download link gives a download a fresh address, for when its link
has expired. Open it with **File › Update download link…** or **Update
download link** on a download's right-click menu, with one download
chosen. It also opens from:

- the **Update download link** button in a failed download's
  [progress window](download-window.md), when the link no longer works;
- the **Update download link** button of the question **The file doesn't
  exist** (see [Questions EDM asks](questions.md));
- by itself, when a download's link has expired and **Options › General ›
  Offer a new link when a download’s link has expired** is on, unless
  another of EDM's windows is in front.

The menu item is greyed while the download runs and once it has finished,
except while it waits to try again after a failure.

## Fields and buttons

- The download's name, and its address now.
- "The download’s link has expired? Paste a fresh one:" and a field for the
  new address.
- **Or:** **Get it from the browser:** shown when the download came from a
  web page. It opens that page in your browser and closes this window. A
  bar above the list then says "Click the download again on its page in
  your browser, and EDM uses that as the new link for …". Click the download
  on the page and EDM takes the link the browser hands over as the new one.
  **Cancel** on the bar stops waiting.
- **Cancel:** closes the window and changes nothing.
- **OK:** gives the download the new address. A download that had failed
  is tried again at once. If EDM cannot take the address, the window stays
  open and says why.

## Keys

- **Return** in the field is **OK**.
- **Escape**, Ctrl+W (⌘W on macOS) and the close button are **Cancel**.
