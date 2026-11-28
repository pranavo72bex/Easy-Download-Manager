# Add URL

Add URL takes the address of something to download. Open it with **Tasks ›
Add URL…**, the toolbar's **Add URL** button, the tray icon's **Add URL…**,
or Ctrl+N (⌘N on macOS).

If the clipboard holds a link when it opens, the link is already in
**Address**, selected, so a link you paste or type replaces it. Asked for
again while it is open, Add URL comes to the front as it stands.

## Fields and buttons

- **Address:** the link to download. EDM takes addresses that start with
  `http://`, `https://`, `ftp://`, `magnet:?`, and the stream addresses
  `rtmp://`, `rtmps://`, `rtmpe://`, `rtmpt://`, `rtsp://`, `rtsps://`,
  `mms://`, `mmsh://` and `mmst://`. The path of a `.torrent` file on this
  computer is taken too. A plain `http://` address to a site on the internet is tried as
  `https://` first, so the file comes encrypted where the site offers it;
  EDM keeps `http://` only when the secure address does not answer within
  five seconds (an address on this computer or your local network is taken
  as typed).
- **Recent:** the addresses you added here before, newest first, to pick
  again. Shown only when there are some. An address whose query holds a
  password, token, key, signature or session is never kept here.
- **Clear history:** forgets the Recent list.
- **Needs a sign-in:** tick it to show **User name** and **Password**. The
  login and password go into the address EDM downloads.
- **Cancel:** closes Add URL and adds nothing.
- **OK:** sends the address on (see below).

## What OK does

- One address opens [Download details](download-details.md), where you
  choose where it goes.
- Several addresses separated by spaces, each of them a link, open one
  Download details for all of them.
- A YouTube playlist address opens [Playlist](playlist.md).
- A feed's address (RSS or Atom, such as a podcast's, ending in `.rss` or
  named `feed`) opens [Playlist](playlist.md) too, where it can be followed.
- An address already in the list asks **Already in the list** (see
  [Questions EDM asks](questions.md#already-in-the-list)), unless you ticked
  **Remember my choice** there before or set **Options › General › When an
  address is already in the list**.

When OK cannot go on, Add URL stays open, says why under the fields, and
selects the address so you can type a new one:

- "Type or paste the address of what to download." The field is empty.
- "That isn't a link EDM can download. Paste a web address like
  https://example.com/file.zip." It does not start with one of the
  beginnings above.
- "That looks like two links. Add one at a time, or use Add links from
  clipboard." Two addresses run together, or mixed with other words.

The warning goes as soon as you change the address.

An address whose site name is written with letters of another alphabet that look like Latin
ones (a Cyrillic "а" in "apple.com") is said under the field as you type it: "Look-alike
letters from another alphabet: the real name is xn--…". It may not be the site it seems. Such a
name is shown in its `xn--` form everywhere in EDM, and Download details says the same line.

## Keys

- **Return** in any field is **OK**.
- **Escape**, Ctrl+W (⌘W on macOS) and the window's close button are
  **Cancel**.
