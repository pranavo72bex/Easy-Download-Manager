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

As far as code and automated checks can say, yes, as of 2026-10-24; nothing
is open. What they cannot stand in for is a person at a real computer: Windows
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
| 18 | Queue persistence on a long list. Fixed: a change after a quiet moment is written at once and the rest are folded into one write every 150 ms (`queue/persist.rs`), the write is made off the runtime's threads, a batch add checks the list once, and `ui.inspect` names the rows in one pass; measured with 20 000 links: adding takes 3 s (was 97 s), cancelling 2 000 takes 0.1 s (was 60 s and a timeout), `ui.inspect` of 5 000 rows 0.0 s (was 7.6 s), 2 000 small files finish at 132/s (was 57/s). Not fixed: with 20 000 rows in the list small files finish at 37/s (was 13/s), because each write is still the whole 13 MB list; and a row holds about 14 KB (278 MB for 20 000) | PARTIAL | A journal of changes beside a snapshot, written every few seconds, and a smaller row (the cookies, paths and strings every row carries are mostly empty); `stress-check.py bigqueue` and `many_small` measure it. A kill within 150 ms of a change can lose that change |
| 19 | A download of no known length can no longer take the disk below 256 MB (looked at every 32 MB written; an endless body and a decompression bomb were both written until the disk was full). A gzip response of 300 KB on the wire still unpacks to 300 MB in the folder above that line: the engine decodes `Content-Encoding` as a browser does, and reqwest gives no count of the wire bytes once it has | PARTIAL | A ratio limit needs the compressed byte count, which takes reading the body below reqwest's decoder; or decode only what a download asked for and save the rest as sent (the `gzip` case in `tests/zoo.rs` says the first is wanted) |
| 20 | Not covered by the stress tests: the clipboard offer (it needs the clipboard written by another program), a full disk and the computer sleeping (both would harm this computer), the real DASH download of a playable stream (only hostile manifests were tried), HTTPS through a proxy (CONNECT), FTPS and every other use of TLS (no certificate could be made without another program), and Windows and Linux. Covered since: SOCKS5 proxies, and torrents on loopback inside a sandbox and once against webtorrent.io's public Creative Commons "The WIRED CD" (54 MB, 18 files, 21 s, paused and resumed, nothing outside the folder) | OPEN | Each needs its own phase in `scripts/stress-protocols.py`; a full disk and sleep need a disposable volume or machine |
