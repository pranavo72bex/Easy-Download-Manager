# R&D: extraction architecture and upkeep

Findings and measurements are in
[extractor-research.md](extractor-research.md); risks in
[risks.md](risks.md). The first part of this page describes what is
built; the maintainer's choices are at the end. "Proposed" marks what
is not built.

## What is built

```
the browser's player requests (the extension)      every site
        │ none usable (YouTube: SABR, no plain URLs)
        ▼
dm's own reader, youtube.rs                         YouTube watch pages, playlists
  watch page → player API as tv_samsung → base.js
  → yt-dlp-ejs solver in QuickJS/Deno (sandboxed process)
        │ fails: sign-in, live, bot check, changed client …
        ▼
yt-dlp (shipped by dm, current within a week,       every other site, and YouTube's
  fetched again once at a failure, sandboxed)       hard cases
        │ fails
        ▼
dm's words for why (refusal.rs), never yt-dlp's advice
```

Every reader produces **one shape**: yt-dlp's `-J` JSON (`formats`,
`requested_formats`, `title`, …). `ytdlp::download_page` takes it and
fetches each plain stream on dm's engine, many connections, then merges
picture and sound in Rust (`mp4mux.rs`, `mkvmux.rs`). The rest of the app,
the window, the queue and the extension, never knows who read the page.

### Rust notes, for a Dart developer

- **A shared JSON shape, not a trait, ties the readers together.** In Dart
  you might write `abstract class Extractor { bool matches(Uri u);
  Future<MediaInfo> extract(Uri u); }`. Rust's equivalent is a `trait`.
  With two readers (dm's and yt-dlp's) and one caller (`fetch.rs`), the
  chain is a plain `if`/`match`, and that is easier to read than a trait
  object. A trait earns its place at about the third native reader.
- **`Result<String, Error>`** is Rust's `Future<String>` that can throw,
  made explicit: the caller has to handle `Err`. The fallback is just
  `match youtube::read_for(..).await { Ok(json) => json, Err(why) => /* run yt-dlp */ }`.
- **`async fn`** is Dart's `async`. `tokio` is the event loop, like Dart's,
  but with many threads.

## The fallback rule

| Native outcome | What happens |
|---|---|
| A reading with formats | Used. yt-dlp is never started |
| An error of any kind (network, unexpected JSON, no solver, solver error, `playabilityStatus` not OK, live) | yt-dlp reads the page; the native error is logged (`dm: could not read the YouTube page itself (…)`) |
| A reading whose streams are refused by the server (403 after the first MB) | yt-dlp downloads from its own reading (`--load-info-json`); the refusal is remembered for that download's resume |
| yt-dlp absent and fetching off | The native error, in dm's words |
| yt-dlp fails | yt-dlp is fetched again once (at most hourly) and retried; then dm's words for its error |

The rule: **fall back automatically on every native failure**, because
yt-dlp is always the safer second reader. **Show an error only when the
last reader has failed.** A person never sees "dm's reader failed" on its
own.

## Keeping up without rediscovering fixes

The maintainer decided on 2026-10-31: §1 is built; §2-§4 are not built
for now, and are kept here in case they are wanted later.

### 1. Mirrored upstream files ([upstream.toml](upstream.toml), built)

The file pins the yt-dlp commit last reviewed (2026.10.21, checked
2026-10-31: dm's client constants and solver scripts are the same as
yt-dlp's at that release), and the files whose changes we port:

| yt-dlp file | What dm takes from it | dm code |
|---|---|---|
| `yt_dlp/extractor/youtube/_base.py` | `INNERTUBE_CLIENTS["tv_samsung"]`: client name, version, user agent, `clientScreen` | `youtube.rs` consts `TV_CLIENT_*` |
| `yt_dlp/extractor/youtube/_video.py` | `_DEFAULT_CLIENTS`, the player request body, `_extract_formats` (which fields make a format), the playability statuses | `youtube::read_within`, format parsing |
| `yt_dlp/extractor/youtube/_tab.py` | Playlist browse continuation | `youtube::read_playlist` |
| `yt_dlp/extractor/youtube/jsc/_builtin/ejs.py` | How the solver script is assembled and its input/output | `youtube::solve` |
| `yt_dlp/extractor/youtube/jsc/_builtin/vendor/_info.py` | The solver version and allowed hashes | `SOLVER_VERSION`, `SOLVER_LIB`, `SOLVER_CORE` |
| `yt-dlp/ejs` releases | The solver scripts themselves | fetched and pinned by digest |

### 2. Upstream watcher (GitHub Action, weekly; not built)

`.github/workflows/upstream-watch.yml`, about 40 lines, run weekly and
by hand:

1. Read the pinned commit from `upstream.toml`.
2. Compare it with yt-dlp `master` through the GitHub compare API
   (`/repos/yt-dlp/yt-dlp/compare/PIN...master`), keeping only the
   mirrored files, and check for a newer `yt-dlp/ejs` release.
3. If anything changed, open one issue in our repo (or update the open
   one), titled "yt-dlp upstream: N mirrored files changed". The body lists
   each file's diff (shortened, with links) and the commit messages.
4. A person ports what matters, then moves the pin forward in the same
   commit.

It needs only the default `GITHUB_TOKEN` with `issues: write`. It reads a
public repo, and runs no code from it.

### 3. Port notes in the code (not built)

Each ported function gets one line, so the next reader can diff it against
upstream:

```rust
// Ported from yt-dlp extractor/youtube/_base.py (INNERTUBE_CLIENTS
// "tv_samsung") at <commit>, 2026-10-21.
```

A test in `xtask` checks that every `Ported from yt-dlp … at <commit>`
names a commit that exists in `upstream.toml`'s history, so stale notes
are found.

### 4. Health check against real sites (scheduled CI; not built)

`scripts/real-sites.py` already does the check by hand (14 sites, about
220 MB). Proposed `.github/workflows/real-sites.yml`, daily on macOS:

- **Native reader alone, yt-dlp hidden.** Three YouTube videos must read,
  and one must download whole. A failure here means *our* port is behind,
  even when yt-dlp would have saved the download.
- **The full chain.** YouTube plus 3-4 other sites must download.
- **On failure:** open or update an issue "health: <site> failing since
  <date>", with dm's log lines. Close it automatically when the check
  passes again.

The catch: CI runners' IPs are often answered with "confirm you're not a
bot" by YouTube, which would raise false alarms. So a failure counts only
after it happens twice in a row, and the job can run on a self-hosted
runner if GitHub's prove too noisy. This is the main unknown, and it
needs one week of trial.

### 5. When the native path is "proven"

The native path is already first. If a port falls behind, the cost is only
speed: about 1-2 s more per video while yt-dlp reads instead. No download
fails. That is why upkeep can be weekly and not urgent.

## Proposed: a single-file option (itag 18)

The TV client does not list the progressive 360p MP4. If a "one file, no
merge" pick is wanted, ask the `visionos` client too: it needs no player
JS, and its answer has itag 18. That costs one more request, ~0.3 s. Not
needed for dm's current choices, which always merge a better pair.

## Decisions (maintainer, 2026-10-31)

0. **Direction: yt-dlp is to be removed.** Only what dm needs of it is
   kept, ported into Rust; the whole package is not wanted. yt-dlp stays
   the fallback for each part until that part is ported, so nothing that
   downloads today stops working on the way. `upstream.toml` lists what
   has been ported so far and where it came from.
1. **No new `crates/extractor` crate and no feature flag.** The native
   YouTube reader stays in `dm-core`, where it has been on `main` since
   2026-10-30. It moves into its own crate, behind a trait, only when a
   second site gets a native reader.
2. **Upkeep:** only the list of tracked files (§1). Without the watcher,
   the review is by hand: before a release, and whenever the log shows
   "could not read the YouTube page itself".
3. **Not decided, and not proposed now:** a second native site, and
   embedding the JS engine. The recommendation stays: keep the engine as
   a separate, sandboxed process.
