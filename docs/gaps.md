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

Found by a security review (2026-10-24); each row says how it closes.

| #  | Gap | Status | How it closes |
|----|-----|--------|---------------|
| 4  | `progress_is_reported_and_the_speed_limit_holds` takes ~35 s instead of ~8 s when `cargo test` runs `torrent_features`' tests as threads of one process (the other torrents share the transfer); it passes alone and under CI's one-process-per-test runner | OPEN | Give the test a limit of its own that the siblings do not share, or run `torrent_features` single-threaded in the check that uses `cargo test`; shown by `cargo test --workspace` passing 5 times in a row |
| 5  | The 71 reworded strings were translated into the other twelve languages by a machine pass (consistent with each catalog's own vocabulary, checked for placeholders and length) and have not been read by a native speaker | OPEN | One native reader per language goes through the 71 strings listed by `git log -p -- desktop/crates/dm-desktop/i18n` for the 0.7.0 commit and corrects them |
| 6  | A video site's watch page still needs yt-dlp: dm reads manifests, but the address of the media is minted by the site's own player | OPEN | The extension takes the real addresses (a manifest, or a picture and a sound stream) from the page's own network traffic, as it does for a plain media request, and hands those to dm; a page that comes with no browser stays yt-dlp's. Shown by a page served with a player that fetches a `.mpd`, downloaded with `DM_YTDLP` empty |
| 7  | An HLS stream whose sound is a playlist of its own, or whose segments are cut from one file by byte ranges, still goes to yt-dlp (`stream::beyond_dm`) | OPEN | Fetch the audio rendition's playlist as a second track and join the two with `mp4mux` (fMP4 segments first); honour `#EXT-X-BYTERANGE` with the ranged piece fetch `download/dash.rs` has. Shown by tests with `DM_YTDLP` empty |
