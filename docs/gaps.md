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
| 18 | The queue is written whole, pretty-printed and synced to the disk, on every state change (`queue/persist.rs` `save`, `write_atomic`), so what a long list costs grows with its length. Measured (release build, macOS): with a short list small files finish at 57/s whether 8 or 32 run at once (a sample of the running app is mostly `fcntl` full syncs, `rename`, `unlink` and `open`); adding 500 links takes 0.04 s to an empty list, 2.5 s at 10 000 rows and 4.8 s at 19 500, so 20 000 links take 97 s to add; with 20 000 rows small files finish at 13/s; cancelling 2 000 of them in one call takes 60 s and the control call gives up (`the window did not answer in time`) though the cancels then complete; a row holds about 14 KB (278 MB for 20 000) | OPEN | Save at most once every few hundred milliseconds from one writer (changes in between are folded into it), write compact JSON, sync once per save, and make a bulk add or cancel save once for the call. Closes when `python3 scripts/stress-check.py bigqueue` adds 20 000 links in under 10 s, cancels 2 000 in under 2 s, and `many_small` reaches 150 files/s |
| 19 | A server's `Content-Encoding: gzip` (or brotli) is undone as the body arrives and the result saved: a 300 KB response became a 300 MB file (`stress-check.py hostile`), so one link can be made to write far more than it sends | OPEN | Ask for `Accept-Encoding: identity` and save the bytes as sent (a download is the file, not a decoded page), or stop at the announced size and at the room the disk has |
| 20 | A link that redirects to a `file://` address is listed as a completed download of 0 bytes named `to`; a redirect to `ftp://` fails with "URL scheme is not allowed". Nothing of the computer's file is saved (checked against `/etc/hosts`), but the row says it worked | OPEN | Treat a redirect to a scheme dm does not follow as the download's failure, as the `ftp://` one is |
| 21 | A response whose headers can never be read (1 MB of header) is retried with back-off for 153 s before the row fails, as a passing error would be | OPEN | Count an oversized or unparsable header as final, not as a network error to wait out |
| 22 | The control call `ui.inspect` takes 25 ms with 1 500 rows, 2.7 s with 3 000 and 7.6 s with 5 000 (it grows with the square of the list; the window itself answers other calls in about 10 ms at any of these sizes and idles at 0.3% of a core) | OPEN | Build its rows from one pass instead of a search per row |
| 23 | A master playlist (`.m3u8`) whose only variant is itself keeps its row `running` with no bytes, one request about every 3 s, for as long as it was watched (6.5 minutes), with memory flat at 21 MB: nothing limits how deep a playlist may nest | OPEN | Give playlist nesting a depth limit (a few levels) and fail the row with a reason; `python3 scripts/stress-protocols.py manifests` shows it (a million-segment playlist and a 100 000-hour DASH manifest are refused at once, which is right) |
| 24 | A Metalink whose first mirror serves different bytes (same size, other content) fails the whole download with "this server does not serve ranges honestly", though a later mirror is good and the file's SHA-256 is given; a dead first mirror is skipped correctly | OPEN | Drop a mirror that disagrees with the probe's validator, take the next, and fail only when none is left (`stress-protocols.py mirrors`, case `wrongfirst`) |
| 25 | Not covered by the stress tests: the clipboard offer (it needs the clipboard written by another program), a full disk and the computer sleeping (both would harm this computer), the real DASH download of a playable stream (only hostile manifests were tried), HTTPS through a proxy (CONNECT), SOCKS proxies, FTPS, and Windows and Linux. Torrents were covered on loopback only, inside a sandbox that refuses every other connection, and once against webtorrent.io's public Creative Commons "The WIRED CD" (54 MB, 18 files, 21 s, paused and resumed, nothing outside the folder) | OPEN | Each needs its own phase in `scripts/stress-protocols.py`; a full disk and sleep need a disposable volume or machine |
