# Gaps

The work list: what is known to be missing from a production-ready download
manager and is both wanted and possible for free. A row says what is missing
and how it closes; when it closes, its row is deleted (what was done is in
`CHANGELOG.md`, `docs/parity.md` and git history).

| Status       | Meaning                                                      |
|--------------|--------------------------------------------------------------|
| OPEN         | Nothing done, or done but never run                          |
| PARTIAL      | Works and is tested, but not everywhere it must              |
| BLOCKED      | Needs something this project cannot get for free, or a person |

## Is dm production ready?

As far as code and automated checks can say, yes, as of 2026-10-24, apart from
the open rows below. What they cannot stand in for is a person at a real computer: Windows
has run only on CI's machines ([windows-checklist.md](windows-checklist.md)),
nothing is signed with a paid certificate, and no release has been cut. That
is not a gap in the program; it is what has not yet been watched.

## Open work

Found by a security review (2026-10-24) and a stress test (2026-10-27,
`scripts/stress-check.py` and `scripts/stress-protocols.py`: local servers, a
dm of its own and Microsoft Edge; torrents on loopback inside a sandbox, and
one public Creative Commons torrent); each row says how it closes.

| #  | Gap | Status | How it closes |
|----|-----|--------|---------------|
| 5  | The 71 reworded strings were translated into the other twelve languages by a machine pass (consistent with each catalog's own vocabulary, checked for placeholders and length) and have not been read by a native speaker | BLOCKED | One native reader per language goes through the 71 strings listed by `git log -p -- desktop/crates/dm-desktop/i18n` for the 0.7.0 commit and corrects them |
| 6  | A video site's watch page still needs yt-dlp to be *read*: YouTube's web player streams by SABR (a POST to `videoplayback?…sabr=1` whose format is chosen in a protobuf body and answered in UMP frames), so its requests carry no `itag` or `mime`, the panel's own catch (`track_of`) keeps none of them, and none could be fetched by a plain range if it did. Since 2026-10-26 yt-dlp only reads the page and dm fetches what it found on every connection (`ytdlp::download_page`), and a caught pair refused to dm is fetched from its page the same way | PARTIAL | Decided (2026-10-26): yt-dlp stays the reader for YouTube, as a SABR client of dm's own would be YouTube's protocol rewritten and kept up by this project alone, and it would need the page's proof-of-origin token. Closes for other sites with a recorded run on a page that serves the two files in plain |
| 11 | High Contrast (`contrast.rs`, `Theme::HighContrast`) and the taskbar jump list (`jumplist.rs`, `--new-download`) are written and compile for Windows (`scripts/windows-modules-check.sh`), and the High Contrast look is checked here as pictures (`DM_HIGH_CONTRAST=black` or `white`, a debug build), but neither has run on Windows (`docs/fluent-audit.md` §3, §10, §12) | PARTIAL | On a Windows computer: switch on Aquatic, Desert, Dusk and Night sky in turn (Settings › Accessibility › Contrast themes) and check each screen reads, the focus shows and selection is clear, while dm runs; right-click dm's taskbar icon: Add URL opens its dialog in the running dm, Open downloads folder opens Explorer there |
| 14 | Held notifications while a fullscreen app, a presentation or Focus Assist is on (`quiet.rs`, `Platform::tick`; standards §9) are written and compile for Windows (`scripts/windows-modules-check.sh`), and the holding and releasing are tested here, but `SHQueryUserNotificationState` has not been asked on a Windows computer | PARTIAL | On a Windows computer: with a video or game fullscreen, or Focus Assist on, finish a download: no toast and no sound; leave it: the toast comes; check the Windows codes 2, 3, 4 and 6 are the states seen |
| 16 | Options has a line under 13 of the settings whose names are the program's own, and a Restore defaults per tab; the rest have none, and there is no separate Advanced page for connections, proxies and the user agent (§8) | PARTIAL | A line under each remaining setting (each is a string in all thirteen catalogs, which `tests/i18n.rs` enforces); the expert tabs (Network, Proxy) behind one Advanced page |
| 18 | A very long list costs memory and speed: with 20 000 rows the program holds about 277 MB (14 KB a row, though the live data is about 26 MB: freed blocks of the list's copies stay resident; the allocator was asked to give them back and did not), and small files finish at 37/s (about 57/s with a short list) because each write is still the whole list (9 MB, written compact above 500 rows without building it in memory first). Adding 20 000 links takes 4 s, cancelling 2 000 0.1 s | PARTIAL | A journal of changes beside a snapshot, and a smaller row (the cookies, paths and strings every row carries are mostly empty); `stress-check.py bigqueue` measures it. A kill within 150 ms of a change can lose that change |
| 20 | Not covered by the stress tests: the clipboard offer (it needs the clipboard written by another program), a full disk and the computer sleeping (both would harm this computer), the real DASH download of a playable stream (only hostile manifests were tried), HTTPS through a proxy (CONNECT), FTPS and every other use of TLS (no certificate could be made without another program), and Windows and Linux. Covered since: SOCKS5 proxies, and torrents on loopback inside a sandbox and once against webtorrent.io's public Creative Commons "The WIRED CD" (54 MB, 18 files, 21 s, paused and resumed, nothing outside the folder) | OPEN | Each needs its own phase in `scripts/stress-protocols.py`; a full disk and sleep need a disposable volume or machine |
