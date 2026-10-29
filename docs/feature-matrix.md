# Feature matrix: dm's engine against two other download managers

Reference A is a commercial download manager for Windows; reference B a free
one for Windows, macOS and Linux. What each does, and how it is known, is in
[reference-behavior.md](reference-behavior.md). Values: verified (observed or
stated by its maker), reported (third parties only), untested, unknown.
"Match" for dm is claimed only where a named test checks it.

| Feature | Reference A | Reference B | Existing dm | Target behavior | Status |
|---|---|---|---|---|---|
| Split while downloading; a free connection halves the largest range left | verified | verified (Docker: offsets at 1/2, 1/4, 3/4 …) | yes: `ranged.rs` `split_largest`, `tests/speed.rs` | as now | match |
| Finished connections reused, no new connect | verified | untested | yes: `tests/reuse.rs`, 11 requests on 4 connections | as now | match |
| Positions saved often; resume after a kill | verified (several times a minute) | untested | yes, every second or 4 MB: QA `resume_after_kill9` | as now | match |
| First request | unknown | verified: a plain `GET`, kept streaming as the first piece | `bytes=0-0`, then the first piece on the same connection | as now (one round trip more, on a connection already open) | different, kept |
| Range form | unknown | verified: open-ended `bytes=N-`, connection closed at the next piece | closed `bytes=a-b` | as now: a closed range never reads past its end, which resume's tail check relies on | different, kept |
| Connections on a server that holds each to a low rate | unknown | verified: grows to 29 (24 at once), 32 MiB in 1.2 s | keeps to the setting (8): 2.0 s | candidate, see "Worth taking" | open |
| A 503 on an extra connection | unknown | verified: adds no more, finishes on the first | moves the refused ranges to its other connections | as now | dm faster |
| Fewer connections for some servers | verified (an exceptions list) | unknown | yes: site limits, and fewer after a refusal | as now | match |
| Error recovery and retries | verified | untested | yes: `retry.rs`; QA `stalls_and_cuts`, `network_loss_60s` | as now | match |
| Queues, scheduler, shut down when done | verified | untested | yes: `parity.md` | as now | match |
| Re-download a file that changed | verified | unknown | yes: `queue/recheck.rs` | as now | match |
| Download allowance per hour | verified | unknown | yes: `quota.rs` | as now | match |
| Categories | verified | untested | yes: `category.rs` | as now | match |
| Basic, NTLM, Negotiate, Kerberos | verified | unknown | Basic, NTLM, Negotiate carrying NTLM; no Kerberos | Kerberos needs a realm to test against | partial |
| A program run on each finished file (virus checker) | verified | unknown | yes: `dm-shell/src/settings.rs` | as now | match |
| A player's picture and sound, fetched apart, taken from its requests and joined, no helper | verified (its documented method) | n/a | yes, first, on every site: `track_of`, `download_media_pair`; `tests/caught_pair.rs` with no yt-dlp present | as now | match |
| YouTube | reads the player's requests; a new build whenever YouTube changes | only through its yt-dlp add-on | the player's requests name no format today (SABR, measured 2026-10-29), so yt-dlp reads the page and dm fetches what it found; yt-dlp fetched by dm, current within a week | as now; a helper-free YouTube is a client of YouTube's protocol kept up by hand (gaps.md rows 6, 32) | differs, by decision |
| yt-dlp only reads; the manager fetches the files | n/a | verified | yes: `ytdlp/fetch.rs` | as now | match |
| Picture and sound joined | n/a | verified: ffmpeg, a required package | dm's own muxer, nothing to install | as now | dm needs less |
| yt-dlp in a sandbox (Linux) | n/a | verified: bubblewrap, run plainly where none can be made | yes since 2026-10-29: `ytdlp/sandbox.rs`, checked in Docker with and without a sandbox | as now | match |
| A page's only video named by the page | reported | verified: "Lab clip (1).mp4", yt-dlp's number kept | "Lab clip [watch].mp4" since 2026-10-29: `ytdlp/fetch.rs`; QA `video_page` | as now | dm better |
| Reading the browser's whole cookie store | n/a | verified, when the page's cookies are not given | never; only the page's own session, from the extension | as now, by decision | different, kept |

## Worth taking

Found by the comparison, not yet in dm, and each a change to existing
behaviour, so each waits for the maintainer:

1. **More connections while each is slow** (reference B). Faster on servers
   that hold each connection to a rate (1.2 s against 2.0 s above), but more
   connections than the person set, and some servers block clients that open
   many; reference A advises four or fewer. A setting, or a ceiling above it,
   would be the maintainer's to choose.
