# Changelog

What changed in each release, newest first. The app (`desktop-v*` tags) and
the browser extension (`v*` tags) are versioned separately.

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
