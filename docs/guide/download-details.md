# Download details

Download details is where you choose where a file goes, what it is called,
and whether it starts now or waits in a queue. It opens:

- after **OK** in [Add URL](add-url.md);
- for a download your browser hands over, when **Options › General › Ask
  where to save browser downloads** is on, and for a video from a page
  when **Ask where to save videos** is on;
- for a link you copy, when **Options › General › Offer to download links
  I copy** is on and you click **Download** on the offer above the list
  (or at once, with **Take a copied link at once, without offering it
  first**);
- when you answer **Download again** to **Already in the list**.

The window's top line is the address. A download from the browser also
says **From** and the browser's name. With several addresses it says **and
… more**, and the name, size and video rows are left out.

## Fields

- **Category:** **As its type says** saves the file in the folder dm picks
  for its kind. Picking a kind (or a category you made in Options) puts
  that kind's folder in **Save to folder**, so a file can go to another
  kind's folder without typing its path.
- **Save to folder:** where the file is saved. Left empty, the hint shows
  where it goes: the download folder, with "(sorted by kind)" when
  **Options › Folders › Sort finished files into folders by kind** is on. An empty folder
  means, in this order: the folder of a site rule for the address
  (**Options › Advanced**); else, where files are sorted by kind, the
  kind's folder; else the download folder. `~` is your home folder.
- **Browse…:** chooses a folder in the system's folder panel. Where there
  is no panel, the field says "No folder picker here: type the path."
- **Always save here without asking:** shown only for a download the
  browser caught. Ticked, Start now or Add for later also turns off
  **Ask where to save browser downloads**, so the next one goes straight
  in.
- **Save as:** the file's name (one address only). While it is empty its
  hint shows the name the file will get: the name in the address, "The
  video's own title", or "The torrent's own name". When the server says
  its own name, an empty field is filled with it.
- **Quality:** for a video page or a YouTube video. It says "Reading the
  video's qualities…" and then shows each quality as a button, with its
  size where known ("1080p MP4" over "275.0 MB"; "Sound only" for the
  sound alone). The one picked at first follows **Options › Downloads ›
  Video quality**. For a download the browser's video panel handed over,
  where the window lists no qualities itself, this row shows the quality
  you picked in the panel. If the page turns out to hold
  several videos, Download details closes and [Playlist](playlist.md)
  opens instead.
- **Contents:** for a ZIP on the web. It says "Reading the contents…",
  then lists the files in it with their sizes, and **and … more** past the
  first ones. **Take out** beside a file downloads that one file alone into
  the folder above, without the rest of the archive; the line under the
  list says "Taking out …" and then "… saved to …".
- **Part:** shown for a YouTube video, for a download the browser handed
  over, or when a part is already asked for.
  - **Only a part of the video:** tick it and give **from** and **to** as
    minutes and seconds (`m:ss`, such as 4:10 and 4:45).
  - Where the link starts partway in, the row says "This link starts at
    …" with a **From …** button, which ticks the box and fills **from**.
  - **Shape** (with a part): **As it is**, **Vertical 9:16** or **Square
    1:1**.
  - **Part as** (where the part can be made a GIF): **Video** or **GIF**.
- **Size:** "Checking…" while dm asks the server, then the size and the
  free space where it is going ("12.4 MB · 80.1 GB free"). It turns to the
  warning colour when the file will not fit. A video page says "Known once
  the video starts"; a server that does not say gives "unknown".
- **Note:** your own words, kept with the download.
- **Delete file after:** **Never**, **10 minutes**, **1 hour** or **1
  day**: that long after the download finishes, its file goes to the
  Recycle Bin (the Trash on macOS) and the row leaves the list.
- **Keep no record:** ticked, dm forgets the download once its file is
  whole: its row leaves the list, and its address, the page it came from
  and any sign-in sent with it go too, so nothing of it is left in dm's
  own files. The file itself stays where it was saved. Until
  the file is whole the download is kept like any other, so it can go on
  after dm is closed.
- **Unpack when done:** shown for a ZIP, 7z or tar archive (by the name in
  **Save as** or in the address). Ticked, once the download finishes its
  files are unpacked into a new folder of its name beside it, even where
  its kind is not ticked under **Unpack when done** in
  [Options › Folders](options-folders.md#unpack-when-done), which also says
  whether the archive is then kept or sent to the Trash. Nothing on disk is
  replaced, and nothing is written outside that folder.

## Warnings

- "A download with this name exists. This one will be numbered." Another
  download in the list will save a file of this name in this folder.
- "A file of that name is in the folder already." Pick **Save this one as
  …** (a free name, numbered) or **Replace that file once this one is
  whole**. The old file stays until the new one has finished.
- "Not available: …" The server answered with a problem.
- "Enter the start and end as minutes and seconds, such as 4:10 and 4:45.
  The end comes after the start." The part's times cannot be read.

## Buttons

- **Queue:** the queue **Add for later** puts it in (the Main queue at
  first).
- **Add for later:** adds the download to that queue, without starting it.
- **Cancel:** adds nothing. A download the browser handed over is left
  with nothing; cancel two from the same site and dm asks **Leave this site
  to the browser?** (see [Questions dm asks](questions.md)).
- **Start now:** adds the download and starts it. If the Scheduler or the
  download limit is holding downloads, dm asks **Start outside the
  schedule?**.

## Good to know

- **Return** is **Start now**; **Escape**, Ctrl+W (⌘W on macOS) and the
  close button are **Cancel**.
- dm remembers the folder you last chose for each kind of file (a video, a
  document, and so on) and offers it the next time a file of that kind
  comes in with an empty **Save to folder**.
- With **Options › General › Start downloading while Download details is
  open** on, a single plain file (not a video, a torrent, a form's answer
  or several addresses) starts downloading as the window opens. **Start
  now** or **Add for later** puts it where you chose; **Cancel** removes
  it.
- With **Options › General › Show progress when a download starts** on,
  **Start now** opens the [download's window](download-window.md).
