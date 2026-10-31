# R&D: a minimal Rust extraction layer beside yt-dlp

Checked 2026-10-31 against dm 0.8.6 (`main` at d6f3b2e) and yt-dlp
2026.10.21. Design and process are in [architecture.md](architecture.md),
risks in [risks.md](risks.md).

## Summary

dm is already "partially independent". A YouTube watch page, a YouTube
playlist and YouTube's quality list are read by dm's own Rust
(`dm-core/src/youtube.rs`, `ytdlp/page_data.rs`). yt-dlp runs only
behind them, for what they cannot read, and for every other site. dm also
does the downloading and the audio+video merge itself, so no ffmpeg is
needed. What this R&D adds is mostly *process*:

- a written list of the yt-dlp files we mirror,
- a watcher on those files,
- a scheduled check against real sites,
- port notes on each ported function.

A new crate is not needed (see [architecture.md](architecture.md),
"Decisions").

Measured today on three real videos (method at the end):

| | dm's own reader | `yt-dlp -J` |
|---|---|---|
| Time per video, warm | 1.45-1.8 s | 2.7-4.1 s |
| First read (fetches the solver scripts once) | 4.1 s | n/a |
| Memory, dm's process | 26 MB | n/a |
| Memory, with the JS engine run (Deno) | ~280 MB peak | ~340 MB peak |
| Title and duration | the same as yt-dlp's on 3 of 3 | |
| Plain-HTTPS formats | every one yt-dlp lists except itags 18 and 139 | |
| Code added to the binary | ~98 KB (`youtube` symbols, unstripped build) | none, but a separate 30+ MB tool |

A real download (`SITES=youtube scripts/real-sites.py`) came out whole:
"Me at the zoo", 240p H.264 + AAC, the native reader's own pick. yt-dlp
would have picked AV1 + Opus, so the file shows the native path did the
work.

## Phase 1: Audit, how dm uses yt-dlp today

### Where yt-dlp is called

The Rust module is `dm-core/src/ytdlp.rs` and its sub-files:

| File | What it asks yt-dlp | Flags |
|---|---|---|
| `ytdlp/choices.rs` | The quality list for a page that is not YouTube | `-J --no-playlist --flat-playlist`, `--extractor-args` (skip HLS, translated subs) |
| `ytdlp/playlist.rs` | The names in another site's playlist | `-J --flat-playlist --playlist-items 1:1000` |
| `ytdlp/fetch.rs` (`resolve`) | What it *would* download; dm then fetches it | `-O "%()j"` (print only), `-f`/`-S` (format choice), `-o` (name), `--load-info-json` (reuse a saved reading) |
| `ytdlp.rs` (`download_ytdlp`) | A download yt-dlp does itself (HLS/DASH fragments, live, a site's own protocol, a refused fetch) | `--newline --progress-template`, `--concurrent-fragments`, `--fragment-retries`, `--retries`, `--http-chunk-size`, `--limit-rate`, `--continue`, `--merge-output-format`, `--write-subs --sub-langs --sub-format`, `--print-to-file` |
| All runs | Sign-in, network, isolation | `--cookies` (the browser tab's own session, written for this account alone), `--proxy`, `--referer`, `--socket-timeout`, `--video-password`, `--js-runtimes deno:PATH` or `quickjs:PATH`, `--no-config`, `--no-warnings`, `--no-color`; on macOS and Linux it runs inside a sandbox (`ytdlp/sandbox.rs`) |

Around those runs, dm also:

- **Explains failures:** `ytdlp/refusal.rs` turns yt-dlp's error lines into dm's words.
- **Updates itself on failure:** after a failed run it fetches the newest yt-dlp once and tries again (`tests/ytdlp_heals.rs`).
- **Ships yt-dlp:** `tools.rs` fetches yt-dlp and QuickJS, pinned by digest and refreshed weekly.

### Fields dm reads from the `-J` JSON

- **Page:** `_type`, `id`, `title`, `webpage_url`, `duration`, `is_live`, `extractor_key`, `playlist_count`, `n_entries`, `entries[].url`.
- **Format:** `format_id`, `url`, `protocol`, `ext`, `vcodec`, `acodec`, `width`, `height`, `abr`, `bitrate`, `filesize`, `filesize_approx`, `http_chunk_size`, `downloader_options`.
- **What would be downloaded:** `requested_formats` (picture + sound), `filename`, `http_headers` (user agent).
- **Subtitles:** `subtitles` and their `url`s (fetched by dm, `subtitles.rs`).

dm's own reader writes **this same JSON shape** (plus `read_by`). So the
"one output struct" the brief asks for already exists: it is yt-dlp's
reading format. The rest of the app does not know who read the page.

### Minimum feature set: feature → yt-dlp code → size → needed natively

yt-dlp line counts are from `master` today; yt-dlp has 941 files under
`extractor/`.

| Feature | yt-dlp module(s) | Size there | Needed natively? | dm today |
|---|---|---|---|---|
| YouTube player response (InnerTube client, formats) | `extractor/youtube/_base.py` (client table), `_video.py` | 1,371 + 4,599 lines | **Y** | Done, `youtube.rs` (TV client `tv_samsung`, the same one yt-dlp made a default in 2026.10.21) |
| `n` / signature challenge | `extractor/youtube/jsc/` (`_director.py`, `_builtin/ejs.py`, `quickjs.py`, `deno.py`) + the `yt-dlp-ejs` scripts | 287 + 328 lines + 2 minified JS files | **Y** | Done: the same `yt-dlp-ejs` 0.8.0 scripts, pinned by digest, run in Deno or QuickJS |
| Quality list (fast) | `_video.py` format sorting | part of the above | **Y** | Done, `page_data.rs`: reads the page's `ytInitialPlayerResponse`, under 1 s |
| YouTube playlists | `extractor/youtube/_tab.py` | 2,556 lines | **Y** | Done, `youtube::read_playlist` (page data, then the browse API) |
| Title, duration | `_video.py` | small | **Y** | Done |
| Thumbnail | `_video.py` | small | N (the window shows none) | Not read |
| Progressive (video and sound in one file) vs separate picture and sound | format table | small | Y | Picture + sound taken separately and merged by dm (`mp4mux.rs`, `mkvmux.rs`); itag 18 (progressive 360p) is not offered by the TV client, see "Gaps found" |
| Merge audio + video | `postprocessor/ffmpeg.py` (calls ffmpeg) | ffmpeg itself | **Y** | Done by dm in Rust; **ffmpeg is not needed anywhere** |
| HLS / DASH | `downloader/hls.py`, `dash.py`, `extractor/common.py` (manifest parsers, 4,185 lines) | ~4,700 lines | Y for streams caught from the browser | Done, `hls.rs`, `dash.rs`; for pages yt-dlp read, yt-dlp keeps them (dm measured only 12 % faster, decisions.md 2026-10-30) |
| Subtitles | `_video.py` captions | small | Y | Read by yt-dlp, fetched by dm |
| Cookies / sign-in | `cookies.py`, `YoutubeDL.py` | large | Y (the browser's own session) | Done: the extension hands over the tab's cookies |
| Age-restricted / "confirm you're not a bot" (PO tokens) | `extractor/youtube/pot/` + an outside provider plugin | 478 lines + plugin + Node | N for now | Blocked, gaps row 37: needs Node and a third-party provider kept current |
| Live streams | `_video.py` + HLS live | large | N | yt-dlp |
| Other video sites (Vimeo, Dailymotion, X, Instagram, TikTok, SoundCloud, …) | ~940 extractor files | very large | **N**: this is what yt-dlp is for | yt-dlp, its files fetched by dm's engine |
| `jsinterp.py` (yt-dlp's own JS interpreter) | `jsinterp.py` | 971 lines | **N** | yt-dlp no longer uses it for YouTube (its EJS wiki says so) |

### Gaps found by the comparison

- **itag 18 and 139 are missing from the native list.** These are the
  progressive 360p MP4 and the 48 kbps AAC sound. The TV client does not
  return them; yt-dlp gets them from its `visionos` or `web` client. dm's
  choice never picks them (it merges a better pair), so nothing a person
  sees is affected today. A "single file, no merge" option would need a
  second client (`visionos`, which needs no player JS) asked beside the TV
  client.
- **The native pick differs from yt-dlp's on purpose.** dm prefers MP4
  (H.264/AAC) at a height and plays it everywhere; yt-dlp prefers AV1/Opus.
  This is dm's existing choice (`choices.rs`), not a bug.

## Phase 2: How yt-dlp does the parts we need

In plain terms:

1. **Ask the player API as a TV.** yt-dlp posts to
   `youtubei/v1/player` with a client name and version from `_base.py`
   (`INNERTUBE_CLIENTS`). Each client gets different formats and has
   different rules. Today's defaults are `tv_samsung`, `visionos` and
   `web`:
   - `tv`/`tv_samsung`: no proof-of-origin token needed.
   - `web`/`mweb`/`web_safari`: a GVS token needed for HTTPS formats; `web` serves only SABR.
   - `visionos`: needs no player JS, but cannot play made-for-kids videos.
   - `android_vr`: dropped on 2026-08-19 (403 on every format).

   dm asks as `tv_samsung` alone.
2. **Solve the challenges.** Each format URL carries an `n` value, and some
   carry a scrambled signature `s`. The server rejects or slows URLs whose
   `n` is not transformed by a function hidden in the player JS
   (`base.js`, ~2-3 MB, obfuscated, changed every few weeks). Since
   2025.11.12 yt-dlp does not interpret that JS itself. `jsc/_builtin/ejs.py`:
   1. builds one script: the solver's lib, `Object.assign(globalThis, lib)`,
      the solver's core, then `console.log(JSON.stringify(jsc(input)))`;
   2. runs it in an external engine (Deno by default; Node, QuickJS);
   3. reads back `responses[]`.

   The solver parses `base.js` with a real JS parser (meriyah), finds the
   functions and evaluates them. `_video.py` collects every challenge
   first and solves them in one batch (`bulk_solve`). Formats whose
   challenge fails are dropped with "Some formats may be missing". dm does
   the same thing in `youtube::solve`, with the same scripts.
3. **Proof-of-origin (PO) tokens.** These are tokens made by YouTube's
   BotGuard program, now bound to one video. yt-dlp has a provider
   framework (`pot/`) but does not make the tokens itself; plugins do
   (bgutil, which runs in Node). This is the main source of "Sign in to
   confirm you're not a bot", along with the IP address's reputation.
4. **SABR.** This is YouTube's newer streaming protocol: protobuf requests,
   UMP answers, no plain URLs. The `web` client serves only SABR since
   April 2025. yt-dlp's SABR downloader (PR #13515) is still unmerged after
   16 months. It prefers clients that still give plain HTTPS URLs, and so
   does dm.

### What breaks, and how often

yt-dlp shipped about 26 releases in 2025 and 12 in 2026 so far, nearly all
with YouTube fixes. By cause:

- **Player-JS changes that break `n`/sig:** a burst of 4 releases in 10
  days (March 2025), plus several single ones a year. Since EJS, most of
  these are fixed by a **new solver release** (0.3 → 0.8 between Dec 2025
  and Mar 2026) and not by Python changes.
- **Client churn:** a default client retired or added about every 2-3
  months (Jan 2025, Aug 2025, Sep 2025, Jan 2026, Aug 2026, Oct 2026).
  For dm this means the TV client's version or name in `youtube.rs`.
- **PO-token enforcement:** about 3 times in 18 months. For dm this is
  fallback territory (gaps row 37).

### License

- **yt-dlp** is the Unlicense (public domain). We may port any of it. Credit
  is a courtesy, which we give in port notes.
- **`yt-dlp-ejs`'s source** is also the Unlicense. Its **built** scripts
  (the `.min.js` dm fetches and runs) bundle **meriyah (ISC)** and
  **astring (MIT)**. dm fetches them at run time and does not ship them,
  so dm distributes nothing of theirs today. If dm ever *bundles* the
  scripts in its installer, the MIT and ISC notices must ship with them.
- dm itself is MIT, so a port of public-domain code is fine.

## Phase 3: Existing Rust crates

| Crate | Latest | License | JS approach | State |
|---|---|---|---|---|
| rustypipe | 0.11.4 (2025-04) | **GPL-3.0** | Own deobfuscator + rquickjs | No release in 18 months; open "invalid argument" breakage since Jan 2026. GPL rules it out for MIT dm |
| rusty_ytdl | 0.7.4 (2024-08) | MIT/Apache | boa | Stale; 403 issue open since Feb 2025 |
| ytextract | 0.11.2 (2023-01) | MIT/Apache | none | Dead |
| rustube | 0.6.0 (2022-10) | MIT/Apache | regex | Dead |
| yt-dlp (wrapper crate) | 2.9.1 (2026-10) | GPL-3.0 | runs yt-dlp | Active, but only what dm already does, under GPL |
| ytdlp-ejs | 0.1.1 (2026-01) | MIT | Rust port of the ejs solver (SWC + QuickJS/Boa) | New and small; a possible future in-process solver, not ready |
| tydle | 0.1.15 (2026-04) | MIT | Subset of yt-dlp, Deno for signatures | Tiny user base |

**Recommendation: keep dm's own module; use no crate, fork none.** Every
YouTube crate is stale, GPL, or both. None tracks YouTube as fast as
yt-dlp. dm's reader already mirrors yt-dlp's current approach (the same
client, the same solver scripts), and so inherits yt-dlp's fixes
directly.

### JS engine for the solver

| Option | Size | Speed on the solver | Note |
|---|---|---|---|
| External Deno (installed by the person) | 0 in dm | good | dm uses it when present |
| External QuickJS (dm fetches it, ~1-2 MB, pinned) | 0 in the binary | fastest in ytdlp-ejs's benchmark (316 cases: 82 s vs Node 105 s, Deno 205 s) | **current default**; yt-dlp warns QuickJS older than 2025-04-26 is very slow, so keep the fetched build current |
| rquickjs embedded (MIT) | ~1 MB more in the binary | as QuickJS | Removes the sidecar, but runs untrusted player JS **inside dm's process**, outside the sandbox the sidecar gets. Not recommended |
| boa_engine (MIT/Unlicense) | ~6 MB | ~6× slower than QuickJS | No |
| deno_core / V8 | 30+ MB | fast | No: larger than the app itself |

In Dart terms: running the solver in a separate process is like running it
in a separate `Isolate` with no file access, rather than with `eval` in
your main isolate. A bug or a hostile player script can then not reach
dm's memory or files.

## How the numbers were measured

- **dm:** a throwaway example called `dm_core::youtube::read` on each
  address with a scratch tools folder; `/usr/bin/time -l` and `getrusage`
  measured it, three rounds. The example is not kept in the repo (the
  scheduled health check that would use it was not taken); it is the call
  above, timed, with `tools::set_folder` pointed at an empty folder.
- **yt-dlp:** `yt-dlp -J --no-playlist`, two rounds each, using the
  Homebrew yt-dlp 2026.10.21 with Deno.
- **Formats:** the `format_id` sets of HTTPS formats were compared between
  the two JSON files.
- **Binary size:** the sizes of `dm_core::youtube` symbols in an
  unstripped release build.
- **Videos:** `jNQXAC9IVRw` (19 s), `dQw4w9WgXcQ` (3:33),
  `aqz-KE-bpKQ` (10:35, 4K 60 fps).
