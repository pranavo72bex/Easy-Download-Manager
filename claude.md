# dm - Development Tasks

Checked against the code on 2026-09-05, not against memory. `[x]` means done
and tested; an unticked box says what is missing rather than nothing.

`desktop/` is the product and does the
downloading; the browser extension catches downloads and hands them to it,
and downloads nothing itself — with no app, the browser keeps its downloads.
Everything is Rust: the extension is `browser/` compiled to WebAssembly,
loaded by `extension/background.js`, a 20-line loader that exists because a
Manifest V3 background must register its listeners before any WebAssembly
can load. The JavaScript extension (popup, options, in-browser downloader)
was removed on 2026-10-12 and is in git history.

## Core downloader
- [x] HTTP/HTTPS downloading
- [x] Multi-connection downloading — and a limit of a site's own (Options ›
      Network) for a server that refuses a second connection, never above
      the setting (`limits.rs`)
- [x] Resume interrupted downloads — the plan a download's parts were fetched
      under is written beside them and is the plan a resume finishes, whatever
      the connection setting now says. Parts are matched to ranges by index,
      and re-planning from the setting of the moment put an eight-way plan's
      part-0001 at a sixteen-way plan's offset: a file the right length and
      wrong in the middle, reported Completed. Parts with no plan beside them
      are discarded rather than trusted at a guessed offset. A power cut can
      leave a part's length on disk without its last bytes (zeros), which the
      length-as-ledger resume used to keep; each part's last 64 KiB is now
      fetched again on resume and compared, and the part is cut back to the
      last byte that agrees (`download.rs`, whole and half parts). The queue
      and settings are synced to disk before they replace the old copy. A
      real `kill -9` mid-download, restarted, completes byte-exact
- [x] Range request support
- [x] Automatic retry
- [x] Connection failure recovery — with a connect and a read timeout on the
      one client, because a server that sent its headers and then nothing
      held a task forever with the pause flag never read; the backoff
      between attempts is cut short by a pause or a cancel rather than sat out.
      The attempts (0 to keep trying) and the read timeout are settings
      (Options › Network, `limits.rs`)
- [x] Speed calculation
- [x] Download queue
- [x] Pause/resume
- [x] Cancel downloads — including one cancelled after its last byte
      arrived: the join used to skip the parts the cancel had already removed
      and report the shorter file Completed. Nothing is joined for a cancelled
      row, a part that goes missing under the join is an error, and the queue
      does not let a task's late outcome overwrite a row already Cancelled.
      A row is also not started again while its previous run is still
      winding down, or a resume put a second writer on the same part files;
      and that run's late word on how it ended — Paused, seen at the read
      timeout — is not written over the row that has since been put back in
      line, which a queue test against a stalled server found it was
- [x] Large file support — a ranged download is written in place: every
      range at its own offset in one staged file, renamed into place when
      whole, so nothing is copied and a 4 GB download needs 4 GB (it used to
      copy every part into the finished file at the end). A test watches the
      directory and fails at anything approaching 2x

## Browser integration
State of the Rust extension, 2026-10-12. What the JavaScript one had
verified (Firefox 145 install, its own Chrome suite) was of different code
and is not carried over.
- [x] Chrome/Chromium extension — `scripts/chrome-check.py` loads the built
      `dist/chrome` into Chrome for Testing 155 (macOS arm64): the worker
      starts, the WebAssembly loads, all seven listeners are registered on the
      first turn, the id is the one the manifest key fixes, and the worker
      logs no errors. Also in CI under Xvfb (written, not yet run there)
- [x] Firefox extension — `dist/firefox` (Firefox MV3, `background.scripts`,
      `wasm-unsafe-eval`), driven in Firefox 157 by `scripts/firefox-check.py
      native`, which fetches Firefox into `~/.cache/dm-firefox` rather than
      installing it. It installs with no warnings, the WebAssembly loads, a
      download is caught and lands in dm's folder, "Download link with dm"
      works, and the "Download video" panel shows and hands its file.
      Its first run found that the panel's click failed in Firefox: a module
      a page script imports does not see `browser`, so the loader now hands
      it over (2026-10-12)
- [x] Detect downloadable files — the app's catch rules (`catch` in its
      settings: common download file types by default, a size floor, sites to skip),
      fetched with `settings.get`; matched on the browser's name, the URL and
      the server's type. `dm-protocol` tests; `chrome-check.py` shows a
      listed type caught and an unlisted one, and one the app's settings
      leave out, finished in the browser
- [x] Intercept download links — a clicked link's download is paused, handed
      over, and cancelled and erased in the browser once the app has it:
      `chrome-check.py`, file byte-exact in dm's folder, the browser's list
      and folder empty
- [x] Send downloads to desktop application — over native messaging with the
      one protocol crate (`dm-protocol`) both sides use. Never loses one: no
      host, no answer or a refusal resumes the browser's download
      (`chrome-check.py`, host unregistered). A download the extension
      restarted itself is not caught again (unit-tested; the restart path is
      not seen in a browser, since Chrome resumed every paused download).
      Context menus on links, media and selections, and the toolbar button,
      are unit-tested for what they hand over; the link handler is driven
      from the worker in `chrome-check.py`. The toolbar popup's "Take every
      download from this site" takes a site's downloads whatever the rules
      say about type or size — matched on the asked-for address, the served
      one and the referrer, so a site's redirect to its file host is covered
      — but not past the app's master switch or its sites to skip, which are
      matched the same way. A hand-off carries when the extension stops
      waiting, and the host refuses rather than files one it would answer
      late, so a slow host does not have the file fetched twice. Nothing
      from a private window is handed over, by download, menu or panel
      (unit-tested, 2026-10-17); two cancelled hand-offs from one site have
      the app offer to leave that site to the browser. In Firefox the menu is used for
      real: `scripts/linux-firefox-check.sh` right-clicks a link with X11
      input and picks "Download link with dm" with real keys, and dm fetches
      it (2026-10-13). Chrome's menu was clicked for real in UI round 10 (2026-10-19). `dm-desktop --register` replaces
      `engine/install.mjs` and refuses a binary macOS keeps from browsers
- [x] Handle authentication/cookies — Basic from the address or Options ›
      Sign-ins, Digest where the server asks for it (`auth.rs`,
      `tests/auth.rs`), and NTLMv2 where it asks for NTLM or for Negotiate
      (a `DOMAIN\user` login; `ntlm.rs` checked against MS-NLMP's worked
      example, `tests/ntlm.rs` against a server that signs each connection
      in, the file fetched over several). Kerberos is not offered; a `cookies.txt` chosen in Options › Network is sent
      and handed to yt-dlp. The browser's own cookies stay with the browser
      unless the popup's "Hand over this site's sign-in" is on for the site,
      when they travel with the hand-off and the row keeps them
      (`tests/cookies.rs`, `host.rs`); a hand-off
      carries the page's referrer (`chrome-check.py` sees dm's requests carry
      it). The host asks the server once before taking a caught download, so a link
      only the signed-in browser can fetch is refused and finishes in the
      browser (`chrome-check.py`, a cookie-gated file) — and so is one the
      site answers dm with a sign-in page for: the browser's type goes with
      the hand-off, and HTML served to dm for a download the browser was not
      getting as HTML is refused (`host.rs`, unit-tested, 2026-10-17)
- [x] Handle redirects — the browser's `finalUrl` is what is handed over

## Media
- [x] Video detection — the extension extracts no video. "Download media with
      dm" on a `blob:` (Media Source) player hands the page to the client,
      which runs yt-dlp. The toolbar button only brings dm's window up
      (2026-10-14: it used to hand the page over too, downloading a video
      nobody asked for)
      (driven in `app-check.py`). With no app the notification names the fix;
      an app that said it has no yt-dlp gets the page and a notification
      saying a video page needs it. "No app" is believed for ten seconds, so
      an app registered since is found without a browser restart. The
      "Download video" panel shows over players, specified line by line
      with its tests in `docs/video-panel.md`. It
      is a compact bar (dm's mark, "Download video", ▾ on a player page,
      ? and ×) in the player's top corner. A click on a player page lists the
      qualities the app's yt-dlp finds (`media.formats`, `yt-dlp -J`), one
      line per height with its size, then the sound alone. The line picked
      reaches the app with its quality and container, so an "MP4" line
      arrives as .mp4. The app then opens "Download details"
      for it (Options › General can turn that off). Real YouTube: the list
      showed three lines and the picked MP4 landed as .mp4. A page script, Rust like the rest,
      draws it in a shadow root. A file is handed over as a file; a `blob:`
      player is handed over as its page. `chrome-check.py` drives it on local
      pages (an overlay over the video, a MediaSource player, a 96px clip
      that gets none). `scripts/youtube-check.py` drives it by hand on a real
      YouTube watch page: hover, rest five seconds, click, and the video is
      in dm's folder through yt-dlp. That run found the panel vanishing
      under a resting pointer, because YouTube puts `pointer-events: none`
      on its player as the controls fade. A site whose CSP forbids
      WebAssembly gets no panel; its right-click menu still works
- [x] Generic media downloads — a direct media file still downloads directly
- [x] Playlist handling where technically supported — HLS in the client;
      a player page's stream is resolved by the client's yt-dlp
- [x] Filename detection — the client's, held to the cases in
      `shared/naming-cases.json`. The extension names nothing any more

## UI
- [x] Download manager — the client's. Its window is drawn in Rust with
      egui (`desktop/crates/dm-desktop`), no web view; it replaced a Tauri web-view window (in git history)
      on 2026-10-11. It calls the queue directly, and `tests/window.rs` drives
      every control through its accessibility tree against a real queue on
      loopback
- [x] The window — menu bar (Program, File, Downloads, View, Help),
      toolbar, category tree with Everything, In progress, Done and Queues,
      a sortable table (Name, Queue, Size, Status, Time left, Speed, Date,
      Note, Referrer, Folder), the row menu, and dm's dialogs: Add URL with
      "Needs a sign-in" (an address already listed is asked about first),
      Download details (folder, Save as, Note, Start now / Add for later),
      a progress window per download with each connection, Download
      complete, Properties with Update download link, Add numbered series,
      Add links from clipboard, Site files (pages deep on one site,
      or an FTP server's folders read by MLSD or LIST; path filters, saved
      searches that run on a timer and can stop reading after so many minutes), Import/Export as dm's
      `.dmlist` (plain lists of links import too), Find, Scheduler, Speed
      limiter, Options (General, Folders, Downloads, Browser, Network,
      Proxy, Sign-ins). Every action is a named command (`commands.rs`),
      and window tests click every menu item, toolbar button and row-menu
      item by its name and check what it does
      (`every_menu_item_clicked_does_what_it_says`,
      `every_toolbar_button_clicked_does_what_it_says`,
      `every_row_menu_item_clicked_does_what_it_says`). A
      control protocol (`--control <port>`) lets a test or agent run
      commands, read the window back and screenshot it; `app-check.py`
      drives it against the real binary
- [x] Progress — drawn and driven
- [x] Speed — drawn and driven
- [x] ETA — drawn and driven
- [x] Pause/resume — driven; each button checked to send its own command for
      its own row
- [x] Queue — driven, with a named queue's own files-at-once (Options ›
      Downloads) and a chosen folder for unfinished parts that each row
      records, so changing it strands nothing. The window is a rail of places over one list: filter by
      state or by kind, each with a live count, search across names, and
      pause or start everything with one command rather than one per row
- [x] Categories — the client's, configurable. A person can add,
      edit and delete categories of their own in Options › Folders. Each has
      a name (also its folder), file types and, optionally, the only sites
      it takes them from. These are looked at before the built-in kinds and
      listed in the tree. `category.rs` tests the rule; a window test makes
      one, downloads into its folder and deletes it
- [x] Settings — 17 controls in the client, covering every feature it has,
      and `scripts/app-check.py` reads four of them off the settings file
      into the real binary: the download folder and sorting, a proxy that
      is not gone around, a speed limit held to, a schedule that holds. The
      extension has none: what it catches is the app's `catch` setting,
      edited in Options › Browser
- [x] Retry — a failed row is not a dead end: Retry puts it back in line
      with its staged parts kept, so a failure while fetching resumes from
      where it got to; one that failed after the fetch (a wrong digest, a
      disk that filled during assembly) fetches again from a clean row, and
      an assembly that fails no longer leaves a truncated file holding the
      name. A queue test fails a download, retries it and completes it;
      driven in the window test too
- [ ] Drag and drop — winit delivers dropped files only, so a `.webloc`,
      `.url` or text file dropped on the window is read for links and they
      are queued: a `.url` file dropped from a real GTK file chooser was
      watched queuing and downloading (round 9, 2026-10-18). A drop with no
      link in it says how to add one, and so does the empty list. Links
      dragged from a browser are still not received at all (the web window
      took those)
- [x] Start on login — an opt-in setting, off by default, registers dm as a
      login item so a browser hand-off always has an app to land in.
      Launched by the login item it stays in the tray (`--hidden`) rather
      than opening a window over the desktop. On Linux the entry is dm's own
      XDG autostart file (`autostart.rs`): quoted per the desktop-entry spec,
      aimed at the AppImage itself rather than its vanishing mount, and one
      an older dm wrote is replaced. `scripts/linux-gui-check.sh` turns it on,
      has `dex` start the autostart folder as a session does at login, and
      sees dm in the tray with no window, its Show dm working, and the entry
      gone when turned off. macOS and Windows use `auto-launch`, not yet
      watched at a real login. Window size and position are remembered
      across launches
- [x] Lives in the tray — closing the window hides it and the queue keeps
      downloading; a tray icon with Show and Quit brings it
      back or ends it, a left click on the icon shows it, and on
      macOS the Dock icon reopens it. Cmd+Q still quits. One dm at a time: a
      lock under dm's home says it is running, so a browser hand-off no
      longer starts a second copy beside the first, and a second launch by
      hand — or a `.desktop` entry opening a link — hands its links to the
      running one and asks it to show itself. `scripts/app-check.py` drives
      the real binary through all of it: the lock, the second launch, the
      hand-off landing in the running dm and starting one when none runs.
      On Linux the tray is a StatusNotifierItem over D-Bus (`ksni`, no GTK),
      what GNOME's extension, KDE and waybar show: made whether or not a
      panel is up yet, kept registered across a panel restart, and closing
      the window hides only when the panel lists dm's item, otherwise quits.
      `scripts/linux-gui-check.sh` drives it as a panel does, 70/70
- [x] Problems that no row can carry are shown — the queue file that could
      not be written, parts that could not be removed, a saved proxy that
      could not be used, a login item that could not be registered. These
      went to stderr, which a Dock-launched app does not have, while the
      window showed the state the failure had just failed to record. A queue
      file that cannot be read is kept aside, not overwritten by the next
      save
- [x] Download history — the client keeps finished rows with the time they
      finished, persisted, filterable and clearable

## Reliability
- [x] Unit tests — all Rust: 155 in the desktop workspace and the
      extension's 24 host tests. There is no JavaScript suite
- [x] Integration tests — Rust against a real HTTP server; the extension in
      Chrome for Testing against a real dm (`chrome-check.py`).
      `integrity.rs` holds the cases a 2026-10-11 integrity audit
      found: a file republished between runs, a server without
      ranges retried, unknown length, a dropped range, empty files.
      Per-behaviour status with evidence: `docs/parity.md`
- [x] Startup that says what it did — a `--hidden` launch makes no window
      until one is asked for (the tray's Show, a second launch, the Dock,
      no tray to wait in): dm runs its own event loop and hands it to
      eframe then, so a login launch waits in the tray in 13 MB rather than
      43 and never flashes a window (2026-10-20). `app-check.py` counts windows under Xvfb in the Linux
      container — one normally, none with `--hidden`; on this Mac the shell
      may not script System Events, so there it says "not checked" rather
      than "ok", which it used to
- [x] Window tests — `desktop/crates/dm-desktop/tests/window.rs`, in CI:
      the window driven through its accessibility tree against a real queue,
      checking downloads land byte-exact, each row's buttons act on that row,
      settings are applied and saved (and put back when they cannot be),
      the clipboard watcher, 500 rows, and that every control has a name a
      screen reader says. They found five bugs on the way in. There is no
      IPC any more for a test to hold to a handler list
- [x] Never crash on outside input — clippy denies `unwrap`, `expect` and
      `panic!` in shipped code of every crate, and unchecked indexing in the
      modules that read outside input; locks recover from poisoning. Every
      such reader is fuzzed with proptest inside `cargo test`, and weekly at
      depth in CI; the fuzzing found a filename that cleaned differently
      twice
- [x] Server zoo — 31 server behaviours (`docs/zoo.md`), floor only rises,
      curl as a second opinion; it found a 206 for another range written at
      the asked offset, a 206 with bytes from elsewhere saved as the file
      (now caught by a one-byte overlap per range), and a PDF at `/get`
      saved as "get"
- [x] Measured and kept honest — `cargo xtask scorecard` (generated,
      held current by a test), `cargo xtask layers`, `cargo xtask privacy`
      (no analytics, crash reporting or stray addresses — dm is free and
      stays free)
- [x] Headless and MCP — `--headless --control 0` runs dm with no window;
      `dm-desktop mcp` gives an AI client dm's commands as tools; both driven
      against the real binary in `app-check.py`
- [x] Network failure tests
- [x] Resume tests
- [x] Concurrent download tests
- [x] Memory leak checks — six downloads of a 32 MB file, with resident
      memory measured across them, in a test binary of its own because
      resident memory is a property of the process and the rest of the suite
      allocates too
- [x] CPU usage optimization — the speed limiter took a shared lock once per
      chunk to discover there was no limit, which is every connection queueing
      behind every other on the hottest path in the engine. Reading the rate
      outside the lock: 1.6M calls across 8 tasks went from 0.08s to 0.01s

## Packaging
- [ ] Linux — packages: .deb and AppImage (cargo-packager), .rpm (nfpm,
      `desktop/packaging/nfpm.yaml`) and Arch (PKGBUILD), for x86_64 and
      arm64. Each names what winit and glow open at run time (X11, Xcursor,
      X11-xcb, Xi, Wayland, xkbcommon, GL); the .deb named too few until
      2026-10-16, when GTK — which had brought the X11 ones along — left
      with the tray's move to D-Bus, and a system without them could not
      open the window. Checked here, 2026-10-16: the .deb installed through
      apt and driven through every `app-check.py` check on Debian 12 and
      Ubuntu 22.04 (arm64) and Debian 12 x86_64 (emulated); the AppImage
      answering as a native host; the .rpm installed through dnf on Fedora
      41 and zypper on openSUSE Leap 15.6 and run through `scripts/smoke.py`
      (`scripts/rpm-check.sh`); the Arch package built by makepkg (with its
      check()), installed by pacman and answering a hand-off
      (`scripts/arch-check.sh`, arm64). CI installs and runs every installer on its
      own system before a release is drafted (`desktop.yml`, job
      `installed`), which passed on 2026-10-20 for the .deb, AppImage and
      .rpm on x86_64 and arm64, the x86_64 AppImage included. Unchecked: a
      real desktop install of each
- [ ] Windows — run on CI's Windows machines, not yet on a person's. The
      workspace tests pass on windows-latest on every push, and on
      2026-10-20 the NSIS installer was installed silently there and
      `scripts/smoke.py` passed against the installed binary: native host,
      the window (on Mesa's software OpenGL), headless, a byte-exact
      download; the uninstaller removed it. And the whole of `app-check.py`
      passes against the real `dm-desktop.exe` there (`windows-check.yml`,
      2026-10-21), which found and fixed a native host that held the
      browser's pipes open through the dm it started. `scripts/windows-check.sh`
      compiles it from this Mac. Unchecked: a real Windows desktop (GPU
      driver, tray, SmartScreen, a login)
- [x] Installer — `cargo packager` (no Tauri) builds a 7.9 MB dmg and
      `dm.app` on macOS, and the binary inside the .app passes every
      `app-check.py` check. The dmg step runs a Finder AppleScript that is
      skipped when CI is set; `make app` sets it
- [x] When done — Downloads › When done: nothing, exit dm, sleep, hibernate
      (greyed where the computer has no such command) or turn
      off, for this session only, with a 30-second countdown that can be
      cancelled; an optional sound when a download finishes, and another
      when one fails. Driven in
      `window.rs` against a fake, so nothing real sleeps
- [ ] Auto-update — About › Check for updates reads the release's own
      latest.json, which now lists every installer. When a newer version is
      out, "Download the update" queues the installer for this OS and
      processor as an ordinary dm download, and the person opens it. Nothing
      installs itself. The picker is unit-tested and the button is driven in
      `window.rs`. Never run against a real release: none has been published
- [ ] Code signing / notarisation

## More features
Checked against the code. These live in the client; the extension has none
of them.
- [x] Categories (Videos, Audio, Documents, Apps, Archives, Pictures, Other
      folders) — client, configurable
- [x] Speed limiting — client. One ceiling shared by every connection of
      every download dm fetches itself, which is the only kind worth having.
      A torrent or yt-dlp run, which dm cannot meter chunk by chunk, holds a
      share while it runs: with n of them the ceiling is split n+1 ways, one
      part for dm's own downloads (`limit.rs` tests). A torrent re-reads its
      part every tick; a yt-dlp run keeps the part it started with
- [x] Scheduler (download between set hours) — client. Local time, wraps past
      midnight, and does not restart what a person paused on purpose
- [x] Checksum verification — client. A mismatch fails the download and
      removes the file rather than reporting success beside a warning
- [x] Proxy support — client. HTTP, HTTPS, SOCKS4 and SOCKS5, with credentials,
      or the computer's own settings; a list of sites fetched directly. FTP
      goes through SOCKS or HTTP CONNECT (`tests/ftp.rs`); an HTTPS proxy
      refuses FTP rather than being gone around.
      Every request of dm's own, yt-dlp's connections, and a torrent's
      trackers and peers over SOCKS5; a torrent under any other kind of proxy
      is refused with the reason rather than sent out around it. A saved
      proxy that no longer parses is turned off for every route at startup,
      not bypassed by dm's client while still handed to the tools
- [x] Download history — client. Finished rows are kept with the time they
      finished, sortable and clearable
- [x] Recheck queue — on a timer (Options › Downloads) or "Start queue" on it asks the server
      about each finished file again (ETag, Last-Modified, length, recorded
      when it was fetched) and fetches afresh, in place, each one that
      changed. A queue test republishes a file and sees only that one fetched
      again, under the same name
- [x] Metalink / mirrors — client. It reads Metalink 3 and 4, probes
      every mirror, drops any that disagree about the length, and gives each
      range its own mirror so all of them are in use at once
- [x] HLS / DASH — HLS is done in the client (and a stream that arrives
      as `.ts` is put into an MP4 where ffmpeg can, `hls.rs`): master and media playlists, rendition
      choice, AES-128 with per-segment vectors, fMP4 initialisation segments,
      and resume from the segments already fetched — of the same rendition:
      the one chosen is written beside the parts, and segments of another
      are discarded rather than spliced in after a quality change. The page's
      referrer goes on every request of the stream, not only the probe; a
      test serves a stream only to its page and gets it. DASH in the client goes to
      yt-dlp, which reads MPDs natively and joins picture and sound with
      ffmpeg — the muxer the client itself does not have. Without yt-dlp a
      DASH manifest is refused with "install yt-dlp" rather than the video
      half fetched alone and called a success. Decided once, from the URL
      and the content type together, so a manifest served as `dash+xml` from
      a URL without `.mpd` takes the same route. Each route is pinned by
      `DM_YTDLP` — empty for the refusal, a stand-in that records its
      arguments for the hand-off — so neither can be deleted unnoticed
- [x] RTMP, RTSP and MMS streams — client (`live.rs`), no ffmpeg since
      2026-10-21: RTMP and RTMPS by dm's own client (`rtmp.rs`), RTSP
      through `retina` (`rtsp.rs`), both recorded into a fragmented MP4 that
      plays if cut short (`fmp4.rs`) and rewritten plain at the end; MMS over
      HTTP (`mms.rs`), kept as the Windows Media it is. RTMPE, RTMPT and
      RTSPS still go to an ffmpeg (fetched). A proxy in force is a refusal,
      never a route around it. `tests/live.rs`: RTMP from ffmpeg's server
      decodes identical to the clip; RTSP from an in-test server, every
      picture identical; MMS from an in-test server, byte for byte; mediamtx
      checked by hand
- [x] BitTorrent — client only, which is the only place it can be: an
      extension has no sockets. A magnet or a `.torrent` (fetched, or one on
      this computer opened with dm) is routed to
      `librqbit` before the HTTP path, reports progress on the same row as
      any download, is given the speed ceiling, and pauses and resumes from
      the files on disk. Staged under the row's own parts directory like
      every other download and moved into the download folder only when
      whole, so a paused torrent's half-fetched files do not sit in Downloads
      looking finished and removing the row removes them. A torrent of one
      file is placed exactly as a file is — sorted into its category folder,
      checked against a digest it was given — and a digest given for a tree
      of files fails the row rather than going quietly unchecked. A `.torrent`
      fetched over HTTP carries the page's referrer. A test stands up a
      one-seeder swarm on loopback and fetches a file through it, so the
      bytes really cross a BitTorrent connection rather than being copied
- [x] Player-only sites via yt-dlp — the client hands a page it cannot fetch
      itself (a video site's watch URL, where the media address is minted by
      the site's own player) to an installed yt-dlp, the sanctioned way:
      dm runs the tool the user installed and places what it produces like
      any other download — no site logic, token minting or signature work of
      dm's own. Routed only when the probe found a web page rather than a
      file and a yt-dlp is on the machine. Progress, quality and the shared
      speed limit are passed through, and so are the proxy and the page's
      referrer, which every request of dm's own already carried and yt-dlp's
      own connections did not; a row's checksum is checked over what it
      produced like any other download; a test drives the real yt-dlp
      against a loopback server end to end. The packaging already listed yt-dlp as an
      optional dependency; now the client actually uses it. Found even when
      launched from the Dock or by a browser's native host, whose minimal
      `PATH` hides `/opt/homebrew/bin` and `~/.local/bin` — the usual homes
      are searched and the tool is spawned by its resolved path, and ffmpeg,
      which joins a separate picture and sound, is resolved and handed over
      the same way. With no ffmpeg dm needs none: yt-dlp (and Deno, its
      JavaScript runtime) are fetched by dm, an MP4 picture and M4A sound
      are joined by dm itself (`mp4mux.rs`) and an HLS transport stream put
      into an MP4 by dm (`tsmux.rs`); a real 720p YouTube video arrived that
      way, decoded identical (2026-10-21). Proven under `PATH=/usr/bin:/bin`: a real watch URL completes
      as one merged file. Before this, that same launch quietly saved the
      page's HTML as a "completed" download named `watch`. Since 2026-10-21 nothing has to be installed:
      with no yt-dlp or ffmpeg on the computer, dm fetches the official
      yt-dlp build and a pinned ffmpeg build from GitHub into its own folder
      the first time a video page needs them, checked against their
      checksums, and fetches yt-dlp afresh weekly (`tools.rs`); a page
      refused to dm's own request (a 429 bot check) is yt-dlp's to try

# Autonomous Development Rules

You are the primary developer of this project.

Your objective is to make this software a complete, production-quality download manager.

## NEVER WAIT FOR ME

Do not repeatedly ask me:

* "What should I implement next?"
* "Should I continue?"
* "Would you like me to fix this?"
* "What feature should I work on?"

Instead, determine the next task yourself from docs/gaps.md.

## DEVELOPMENT LOOP

Repeat this cycle continuously:

1. Read docs/gaps.md (the ranked work list).
2. Read docs/decisions.md.
3. Inspect the existing implementation.
4. Select the highest-priority unfinished task.
5. Implement it completely.
6. Run tests.
7. Build the application.
8. Fix all errors.
9. Test again.
10. Update docs/gaps.md.
11. Update docs/decisions.md if a decision was made.
12. Continue immediately with the next unfinished task.

Use this loop until the project is genuinely complete.

## IMPORTANT

Never mark a task complete merely because code was written.

A task is complete only after:

* Implementation exists.
* It integrates with the existing architecture.
* It compiles/builds.
* Relevant tests pass.
* Error handling exists.
* Existing functionality still works.

## TOKEN EFFICIENCY

Do not waste tokens explaining obvious work.

Prefer:

IMPLEMENT → TEST → FIX → CONTINUE

rather than:

EXPLAIN → ASK → WAIT

Keep responses extremely concise.

Spend the available context primarily on:

* Reading code
* Writing code
* Running tests
* Debugging
* Improving architecture

## WHEN SOMETHING FAILS

Do not stop and ask me.

Investigate the failure yourself.

Try:

1. Read the error.
2. Inspect relevant source.
3. Identify the root cause.
4. Fix it.
5. Run the test again.
6. Try another approach if necessary.

Only stop if the problem genuinely requires information or credentials that you cannot obtain.

## WHERE THE STATE IS KEPT

No separate progress log. Keep these current instead:

* Architecture: docs/how-it-works.md and AGENTS.md
* Completed work: the checklist above, and the Closed table in docs/gaps.md
* Current task, known bugs and next tasks: docs/gaps.md
* Important technical decisions, and approaches tried and dropped: docs/decisions.md

Before ending a session, make sure docs/gaps.md accurately describes the current state.

## GIT

Use git when available.

Make logical commits after stable milestones.

Do not create meaningless commits after every tiny change.

## DEFINITION OF DONE

Do not declare the project finished until:

* Core downloader works reliably.
* Downloads can resume.
* Multiple connections work correctly.
* Network failures recover correctly.
* Browser integration works.
* UI works end-to-end.
* Download queue works.
* Settings work.
* Tests cover critical functionality.
* Production builds succeed.
* Major known bugs are fixed.

If docs/gaps.md still contains important unfinished tasks, continue working.

## START

Immediately inspect the repository.

Create docs/gaps.md if it does not exist.

Then begin implementing the highest-priority task.

After completing it, automatically continue to the next task.

Do not wait for another user message.