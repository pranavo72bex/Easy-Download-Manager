# Feature matrix: dm's engine against two other download managers

Reference A is a commercial download manager for Windows; reference B a free
one for Windows, macOS and Linux. What each does, and how it is known, is in
[reference-behavior.md](reference-behavior.md). Values: verified (observed or
stated by its maker), reported (third parties only), untested, unknown.
"Match" for dm is claimed only where a named test checks it; "different, kept"
is a difference found and left, with the reason in its row. Reference B was run
in Docker (Linux build, 2026-10-29), reference A on Windows (from 2026-10-31).

| Feature | Reference A | Reference B | Existing dm | Target behavior | Status |
|---|---|---|---|---|---|
| Split while downloading; a free connection halves the largest range left | verified | verified (Docker: offsets at 1/2, 1/4, 3/4 …) | yes: `ranged.rs` `split_largest`, `tests/speed.rs` | as now | match |
| Finished connections reused, no new connect | verified | untested | yes: `tests/reuse.rs`, 11 requests on 4 connections | as now | match |
| Positions saved often; resume after a kill | verified (several times a minute) | untested | yes, every second or 4 MB: QA `resume_after_kill9` | as now | match |
| First request | unknown | verified: a plain `GET`, kept streaming as the first piece | `bytes=0-`, its answer kept streaming as the first piece (`probe.rs` `probe_get`), so the server also says whether it takes ranges | as now | match |
| Range form | unknown | verified: open-ended `bytes=N-`, connection closed at the next piece | closed `bytes=a-b` | as now: a closed range never reads past its end, which resume's tail check relies on | different, kept |
| Connections on a server that holds each to a low rate | verified: 8 or fewer at once, more while they raise the rate (measured, `docs/decisions.md`, row 168) | verified: grows to 29 (24 at once), 32 MiB in 1.2 s | 8 at once, then more, up to the setting, only while the last ones added raised the total rate (`ranged::grow`; `tests/grow.rs`); never past the setting: at the default 8, 2.0 s | candidate, see "Worth taking" | open |
| A 503 on an extra connection | unknown | verified: adds no more, finishes on the first | moves the refused ranges to its other connections | as now | dm faster |
| Default connections | reported: 8 (it advises 4 or fewer) | unknown | 8 (`dm-shell/src/settings.rs`) | as now | match |
| Smallest piece split off | measured: a 100 KB file from a slow server cut into pieces down to about 14 KB (`reference-behavior.md`, 2026-10-31) | verified: halves of what is left | halved down to 16 KB a half (`MIN_SPLIT`, `download/mod.rs`); the first plan's ranges at least 1 MiB (`MIN_SEGMENT`, `plan.rs`): lowering that opens every connection at once on a small file from a fast server, which contract row B36 refuses | as now | different, kept |
| Fewer connections for some servers | verified (an exceptions list) | unknown | yes: site limits, and fewer after a refusal | as now | match |
| Error recovery and retries | verified | untested | yes: `retry.rs`; QA `stalls_and_cuts`, `network_loss_60s` | as now | match |
| Queues, scheduler, shut down when done | verified | untested | yes: `parity.md` | as now | match |
| Re-download a file that changed | verified | unknown | yes: `queue/recheck.rs` | as now | match |
| Download allowance per hour | verified | unknown | yes: `quota.rs` | as now | match |
| Categories | verified | untested | yes: `category.rs` | as now | match |
| Basic, NTLM, Negotiate, Kerberos | verified | unknown | Basic, NTLM, Negotiate carrying NTLM; no Kerberos | Kerberos needs a realm to test against | partial |
| A program run on each finished file (virus checker) | verified | unknown | yes: `dm-shell/src/settings.rs` | as now | match |
| A player's picture and sound, fetched apart, taken from its requests and joined, no helper | verified (its documented method) | n/a | yes, first, on every site but YouTube: the extension's `track_of` (`browser/src/decide/streams.rs`), the app's `download_media_pair` (`download/dash.rs`); `tests/caught_pair.rs` with no helper program on the computer | as now | match |
| YouTube | verified: the pieces of the player's script that answer its checks, cut out and run in Windows' own JScript9; a new build whenever YouTube changes | only through a third-party add-on that runs a video tool | read by dm itself (`youtube.rs`): the watch page, the player API as a TV client, the player script, its checks answered in the computer's own JavaScript engine (Windows' Chakra, macOS's JavaScriptCore, WebKitGTK's on Linux) in a process of its own; nothing fetched to run, no helper; `tests/youtube_own.rs`. The client's name and version are dm's to keep current | as now | match |
| The page is read; the manager fetches the files | n/a | verified: its add-on reads, its engine fetches | yes: `youtube.rs` reads a watch page into plain `https` addresses and `videopage/fetch.rs` fetches them on dm's connections, with resume, splitting and the speed ceiling | as now | match |
| Picture and sound joined | n/a | verified: ffmpeg, a required package | dm's own muxer, nothing to install | as now | dm needs less |
| A site's own script kept from the computer | n/a | verified: its add-on in bubblewrap on Linux, run plainly where none can be made | no helper program at all; the only site script dm runs is the part of YouTube's player that answers its checks, in the system's engine with no files, programs or network in reach, in a process of its own (`javascript.rs`) | as now | dm needs less |
| A page's video named by the page | reported | verified: "Lab clip (1).mp4", its add-on's number kept | a caught video by the page's own title, else its `og:title`, without a site's unread count (`browser/src/decide/sites.rs` `page_title`, `clean_title`) | as now | match |
| Reading the browser's whole cookie store | n/a | verified, when the page's cookies are not given | never; only the page's own session, from the extension | as now, by decision | different, kept |

## Worth taking

Found by the comparison, not yet in dm, and each a change to existing
behaviour, so each waits for the maintainer:

1. **More connections than the person set while each is slow** (reference B).
   dm already grows past 8 while the connections it adds raise the rate,
   up to the number set (row 168, above). Going past that number is faster on
   servers that hold each connection to a rate (1.2 s against 2.0 s above),
   but some servers block clients that open many, and reference A advises
   four or fewer. Whether to, and how far, is the maintainer's to choose.
