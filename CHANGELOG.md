# Changelog

What changed in each release, newest first. The app (`desktop-v*` tags) and
the browser extension (`v*` tags) are versioned separately.

## App 0.3.0

A Fluent look throughout, a faster and lighter engine, and new ways in:
sign-ins, streams and FTP folders.

- Looks: Fluent 2 throughout — WinUI's colours, dialogs laid out with
  their buttons on a footer band, the navigation pane's expandable places,
  Fluent UI System Icons, Segoe UI on Windows and Selawik elsewhere. Drop-
  downs as tall as the buttons beside them, a picked item marked with the
  accent, number boxes and fields underlined, Options' tabs fitting at 150%
  text, Properties opening in the middle.
- Faster: a download over several connections is written in place into
  one file and finished with a rename, not copied together at the end —
  2 GB in about 2 s where it took about 3.6 s, and half the writes.
- Lighter: started at login, dm waits in the tray without making a window
  until one is asked for (13 MB rather than 43, and no flash); threads end
  once downloads do; ten downloads at once hold 20–30 MB less; an idle
  window wakes only when something changes.
- Sign-ins: NTLM, under NTLM or Negotiate, for Windows servers and company
  intranets (`DOMAIN\user` in Options › Sign-ins or the address).
- Streams: `rtmp://`, `rtsp://` and `mms://` addresses, fetched through an
  installed ffmpeg and saved as playable files.
- Files on a site: reads an FTP server's folders too, and a saved search
  can stop reading pages after so many minutes.
- Clearer: a failed row says why in the list itself; a full disk says
  which folder's; "not enough room" names the download folder; Escape
  closes the window in front; Copy link in the row and File menus.
- Fixed: a file downloaded twice at once under a chosen name pushing the
  other to "(1)"; a deleted file's row turning Complete when the same
  address was downloaded again; Time left cut off at the first window
  size; two tooltips over a shortened error.

## App 0.2.0

Nine rounds of hands-on testing, each fixed (docs/gaps.md, gaps 169–243).

- Fits at large text: every dialog stays inside the window at 150%, 200%
  and 300% text, scrolling where it must; Options never draws over itself.
- Reads well: sizes, speeds and times in tabular digits; Inter for every
  script it covers, bold titles in Cyrillic and Greek; status sentences
  whole, read out to a screen reader, and translated in all ten languages.
- Keyboard: F10 and the arrow keys walk the menus, Shift+F10 opens a row's
  menu, a visible focus ring round every control; Cut, Copy and Paste on a
  right-click in any field.
- The list: drag a row to reorder its queue, columns that follow the window
  until you size them, a third heading click back to queue order, a
  re-added file shown under the name it will get.
- Downloads: the wait between retries can be set; a retrying download can
  be given a new address; sites whose links work only once are taken
  without a check that would spend them; a video asked for is never saved
  as its page's HTML; the progress window shows each connection's share.
- Linux packages suggest a font for Chinese, Japanese and Korean names, and
  a Firefox unpacked into /opt is registered.

## App 0.1.0 and extension 3.0.0 — first public release

### The app
- Downloads over HTTP, HTTPS and FTP with several connections at once,
  resumes after a dropped connection, a crash or a restart, and checks each
  part against the server before trusting it.
- A queue with pause, resume, retry, named queues with their own limits, a
  scheduler and a speed limiter.
- Video and audio: HLS streams (with conversion to MP4 where ffmpeg is
  installed), and video sites through an installed yt-dlp.
- BitTorrent (magnet links and `.torrent` files) and Metalink with mirrors.
- Sign-ins (Basic and Digest), a `cookies.txt` file, and proxies (HTTP,
  HTTPS, SOCKS4, SOCKS5, or the computer's own settings).
- Categories that sort downloads into folders, checksum verification, a
  download history, and an update check that never installs anything by
  itself.
- Lives in the tray, can start at login, and runs one copy at a time.
- Windows, macOS and Linux (.deb, .rpm, AppImage, Arch).

### The browser extension
- Chrome, Edge, Brave, Vivaldi, Opera and Firefox.
- Hands the browser's downloads to the app. With no app running, the
  browser keeps its downloads, so nothing is lost.
- "Download with dm" on links, media and selections, and a "Download video"
  panel over video players.
- Optional per-site switches: take every download from a site, or hand over
  a site's sign-in.

### Known limitations
- The builds are not code-signed. Windows SmartScreen and macOS Gatekeeper
  warn on first launch; the README says how to open dm anyway.
- dm has not yet been run on a real Windows computer.
