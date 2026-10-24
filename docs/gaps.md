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
| 1  | macOS downloads carry no quarantine mark (`dm-core/src/motw.rs` marks only on Windows), so Gatekeeper never checks a downloaded app, and nor does extraction | OPEN | Set `com.apple.quarantine` on macOS when a download finishes and on extracted files; a test reads the attribute back |
| 2  | An update is verified by a digest from the same release it comes from (`update.rs`); whoever can change the release can change both | OPEN | Sign `latest.json` with an ed25519 key whose public half is built into dm; refuse an update whose signature does not verify |
| 3  | A saved site login is sent to subdomains of the site and over plain `http://` (`dm-shell/src/settings.rs`) | OPEN | Send saved logins only over https, and only to the exact host unless a rule says otherwise; a test for http and for a subdomain |
| 4  | Torrent tests (`magnet_link_downloads_from_the_swarm`, `progress_is_reported_and_the_speed_limit_holds`) fail now and then when the whole workspace runs at once on a busy machine, and pass alone | OPEN | Give each its own port range and a time allowance that scales with load, or run `torrent_features` in its own test group in CI; shown by 20 full-workspace runs in a row passing |
