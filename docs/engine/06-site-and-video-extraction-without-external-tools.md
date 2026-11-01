# 06. Site and video extraction without external tools

Goal: dm finds the media of a page with its own readers. No external video
extractor, no plugin, no program fetched to run. Where a site cannot be read
without running that site's own script, this file says so and sets out the
choices.

History, so nobody repeats it: dm once ran an external extractor to read
video pages. It was removed (`docs/decisions.md`, 2026-10-31, "The external
video tool removed; videos as reference A gets them"). Today dm reads YouTube
itself and gets every other site's video from the requests the page's player
makes, caught by the browser extension. That is also how reference A was
observed to work: handed a video page's address on its command line it saved
the page and read no video (same decisions entry).

## The five ways a page gives up its media

| Way | How it works | Who does it | Status |
|---|---|---|---|
| W1. A file link | The address is the file (`.zip`, `.mp4`). | Engine (files 02, 03) | BUILT |
| W2. Caught player requests | The extension sees the page's media requests (`webRequest`), classifies them (progressive file, HLS playlist, DASH manifest, split picture and sound), and offers them in the panel. | `browser/src/decide/streams.rs`, `browser/src/web/streams.rs` | BUILT; real-site coverage partial (gaps rows 62, 63, 64) |
| W3. Static page reading | The page's HTML or embedded data names the media: `<video src>`, `<source>`, `<track>`, `og:video`, JSON-LD `contentUrl`, an `m3u8` or `mpd` address in a script block. | A reader in dm | TO BUILD for general pages; exists for YouTube only (`videopage/page_data.rs`) |
| W4. The site's own JSON answers | The player asks the site for a JSON document that holds the stream list. The extension's page hook reads that answer, driven by site profiles in the rules document. | `extension/document.js`, `decide/answers.rs`, `dm-protocol::rules` | PARTIAL: mechanism built, no profile written (gaps rows 68, 69) |
| W5. A site's own player API | dm asks the site's public player endpoint as a client of that site does (YouTube: a TV client answers with plain addresses). | `youtube.rs` | BUILT for YouTube; every other site TO BUILD one at a time |

W2 is the main path. It needs no knowledge of any site. W3 to W5 add reach
and each is a per-site cost with upkeep.

## Requirements

| ID | Requirement | Status | Code | Test |
|---|---|---|---|---|
| X1 | The panel lists, for a page, every candidate: progressive MP4/WebM/audio files, HLS (named by its playlist's qualities), DASH (named by its manifest), and split picture-and-sound pairs; a quality is named by its class (144p, 720p, 4K), not raw numbers. | BUILT | `browser/src/decide/streams.rs`, `web/streams.rs`, `desktop/crates/dm-core/src/videopage/choices.rs` | Local page serving a player that loads each kind; the panel lists each (offline tests in `decide/tests.rs`; live per gaps row 64). |
| X2 | A caught pair (picture address, sound address, same page) is fetched in up to 32 pieces each and joined by dm (`tests/caught_pair.rs`, no helper on the computer). | BUILT | `download/dash.rs` (`download_media_pair`), `tests/caught_pair.rs` | Test exists; add a pair served in chunks that throttle (capped-request tests). |
| X3 | Pieces named by `bytestart`/`byteend` or `range=a-b` in the query (the transport several social sites use) are recognised, merged into one file request, and the real size is taken from the largest end. | BUILT; checked live on one site, offline on another | `decide/streams.rs`, `decide/tests.rs` | Offline fixtures; live per gaps row 62. |
| X4 | Per-request headers: the cookies, referer, user agent and any extra header the page's request carried are kept for every request that stream makes, and for no other host (file 02, N11). | BUILT (docs/decisions.md 2026-10-17) | `lib.rs`, `videopage/fetch.rs` | Server that checks each header on every range of a stream. |
| X5 | Renewed addresses: a stream whose address expires mid-download gets the page's new address through the hand-over matching of S21. | PARTIAL | `queue` | S21 test. |
| X6 | Static page reader (W3): fetch the page (HTML up to a size cap), parse with a tolerant HTML tokenizer, collect candidates in the order above, resolve relative addresses against the final address and `<base>`, return them to the same quality list as W2. Never run scripts. | TO BUILD | new `videopage/static_page.rs` | Fixtures: five pages, one per method (video tag, source list, og:video, JSON-LD, embedded `m3u8` string) with expected candidates; a page with none returns "no video found" in words. |
| X7 | Site rules document: a JSON file that ships with dm and may be replaced by a newer signed one (checksum in the update manifest). It lists per site: hosts, which request patterns are media, which JSON answers hold a stream list and where, which headers to keep, which addresses to skip. Data only, no code. | PARTIAL: mechanism built, shipped document empty | `dm-protocol/src/rules.rs`, `dm-shell/resources/rules.json` | Rule fixture for a local test site: the extension offers a stream it could not offer without the rule; a malformed rule is ignored whole with a log line; a rule cannot name an action outside the schema. |
| X8 | JSON-answer reader (W4): the page hook reads a response the page itself fetched and matches it to a site profile; it also handles answers fetched before the hook loaded by asking the hook to wrap earlier (run at document start). | PARTIAL (gaps row 69) | `extension/document.js`, `decide/answers.rs` | Local page that fetches `/api/video/1` and builds a player from it; the panel lists the stream. Page that fetched before the hook: documented miss, with a test that shows the earliest start the browser allows. |
| X9 | Playlists: a YouTube playlist is read by dm; other sites' playlists come as the extension's list of the videos the page links. | BUILT (YouTube) | `videopage/playlist.rs` | Playlist fixture served by the stand-in host (`DM_YOUTUBE_HOST`). |
| X10 | YouTube reader: watch page for visitor id, player script address and signature timestamp; player API as a TV client; the player script; transform of the stream-address challenge (next section); streams fetched in requests of at most 10 MiB because the server slows a response past it. | PARTIAL (works; the challenge step uses a fetched engine and fetched scripts, see below) | `youtube.rs`, `videopage/fetch.rs`, `tests/youtube_own.rs` | `scripts/real-sites.py` against real YouTube (needs a person); offline against the stand-in host. |
| X11 | Failure in words: "needs sign-in", "age check", "live stream not supported", "removed", "not available in your country", "the site changed and dm cannot read it yet". Never a raw parse error. | BUILT for YouTube; to extend per site | `youtube.rs`, `videopage/` | Fixtures for each reply. |
| X12 | Sign-in: dm never reads a browser's cookie store. The extension hands over the page's cookies for that one download. | BUILT | `dm-protocol`, `extension/` | Cookies appear only in that download's requests. |
| X13 | Proof-of-origin token for YouTube streams that demand one. | BLOCKED (gaps row 37) | none | See option analysis; revisit with the maintainer. |
| X14 | No bypass of copy protection. | BUILT (refusal) | `dash.rs`, `stream.rs` | File 05, H4 and D7. |

## The signature and challenge problem

Some sites do not put a usable stream address in their pages. The address a
player uses is made by running the site's own script on a value the server
gave. Two forms exist:

1. A **signature cipher**: the address arrives without its signature; a short
   function in the player script turns a scrambled string into the signature.
2. A **throttling parameter** (YouTube's `n`): the address works, but the
   server slows the response to a trickle (or ends it after a megabyte) unless
   the `n` value was transformed by another function in the player script.

The functions change with every player release, are written in a minified,
obfuscated style, and are sometimes rewritten wholesale. A reader that does
not run them has to guess their shape and breaks when it changes.

A third thing exists: **streaming over a protocol the page's player drives**
(YouTube's SABR: a POST with a binary body, answered in framed binary). There
is no plain address to take. See "SABR" below.

### The options

| Option | What it is | For | Against | Matches rule "no helper, no plugin"? |
|---|---|---|---|---|
| A. Built-in interpreter | dm contains its own small interpreter for the subset of JavaScript those transform functions use (numbers, strings, arrays, loops, array methods, string methods, simple closures). dm cuts the functions out of the player script by pattern and runs them in it. | One program, no fetch, deterministic, tested offline; no process boundary to manage; dm controls memory and time limits. | Large to write (a parser, a runtime, string and array built-ins). Cutting out the right functions is a pattern match that breaks when the script is restructured, so there is upkeep per release. Scripts are untrusted: the interpreter must have step, memory and recursion limits and no I/O. Risk of subtle differences from a real engine. | Yes |
| B. Pinned scripts and a fetched engine | What dm does today: it fetches a small JavaScript engine (a program) and solver scripts written by a third party, checks both by digest, and runs the scripts against the player script. | Least code in dm; the scripts are maintained by others; known to work today. | A downloaded program and third-party scripts run on the person's machine. Upkeep is someone else's schedule. A digest pin means every site change needs a new dm release or a new pinned set, which is itself third-party material. Fails the rule. | No |
| C. None; use only what the browser already computed | The extension takes the addresses the page's own player requests. The browser's engine has already run the transform, so the address is valid. dm never runs a site script. | Nothing to maintain; the browser is the interpreter; same as reference A. | Needs a browser with the extension and a playing page. Does not work for an address pasted into dm. Does not cover SABR, where the requests have no plain address. | Yes |
| D. Ask the browser to compute | The extension injects a small function into the page that calls the player's own decode on a given value and returns the result to dm. dm still needs the page open. | Uses the browser's engine; no interpreter in dm; works for pasted addresses if a hidden tab or the person's open page serves as the evaluator. | Calls an internal of the site's player that is not a public interface; breaks when the player renames it. Asks the person to keep a tab open; a hidden tab is intrusive. May be blocked by a page's security policy. | Yes |
| E. Run an existing browser engine as a library inside dm | Link a full JavaScript engine crate into dm. | Real engine semantics. | A large dependency; still the same cut-out upkeep as A; the engine is code run on untrusted input, so it needs the same limits. Its size and licence need review. | Yes (a library, not a program) |

### Risks common to A, D and E

* **Upkeep.** Whoever writes the cut-out pattern owns a task that never ends.
  Budget it: a weekly test against real pages, an alert when it fails, a
  rules-document hot fix path (X7), and a dm release path that can ship in a
  day. Without a person on this, the feature will fail in weeks.
* **Safety.** The script is the site's, run on the person's machine. Give it
  no file, network, clock or random access. Limit steps and memory. Run it
  off the main thread with a time limit.
* **Terms of service.** Some sites forbid automated access. dm makes
  requests the way a player does, one person's own download; this is the
  maintainer's decision to own.
* **Detection.** Sites use request shape (headers, client name, order) to
  separate players from tools. Keep requests browser-like (file 02).

### SABR and tokens

For a playing YouTube page the player's media requests are a binary POST with
no plain address (`docs/reference-behavior.md`, measured 2026-10-29). Two
ways exist and neither is cheap:

1. Use a client whose API still returns plain addresses (the TV client used
   today). This depends on that client staying available. It is the present
   route and the only one that works without a token.
2. Speak SABR: send the same protobuf body the player sends and read the
   framed answer. This is a protocol implementation (a parser and a
   builder for the messages, a state machine for the stream) and needs the
   proof-of-origin token the player holds. The token comes from running a
   site program in a browser-like environment: gaps row 37, BLOCKED, the
   maintainer declined the upkeep.

### Recommendation (for the maintainer to confirm: DECISION)

1. **Now:** keep W1 and W2 as the main path (C). They need no site knowledge
   and match reference A.
2. **Next:** build X6 (static page reader) and fill the rules document (X7,
   X8) site by site from real pages. All are data, no code.
3. **For YouTube's challenge:** pick between A and D. If dm must work for an
   address pasted into dm with no browser, choose A and budget its upkeep. If
   a browser is always there, choose D first (smaller) and add A later. Do
   not keep B beyond a transition: it fails the rule and carries two third-party
   dependencies. Until the choice is built, keep B behind a setting that is
   off by default and says in words what it fetches.
4. **SABR and tokens:** leave BLOCKED until the maintainer decides to own the
   upkeep (row 37). When the plain-address client stops working, say so in
   words: "this video needs the browser to play it; use the panel on the page".

### Tests for whichever option is built

| Test | How |
|---|---|
| Transform correctness | A corpus of player scripts saved from 10 or more dated releases, each with the expected output for fixed inputs. The test runs the option over each and compares. New releases are added when seen. |
| Failure in words | A player script whose function cannot be cut out: the row fails with "the site changed and dm cannot read it yet", and the message includes the script's date. |
| Safety | A script with an infinite loop, a huge allocation, deep recursion, and a call to a missing global: each stops within the limit with an error, no crash. |
| Throttle check (real) | With a transformed address, the first 5 MB arrives at full speed; with an untransformed one it is slowed. Needs a person and a real page; run by `scripts/real-sites.py`. |
| No network from the script | A script that tries to read a file or open a socket gets nothing. |

## Per-site readers: how to add one

1. Find out first whether W2 already works (open the page in a browser with
   the extension and read the panel). If yes, stop; write no reader.
2. If a site builds its player from a JSON answer, write a profile (X7, X8).
   Test it with a local page that imitates the answer's shape; never ship a
   fixture copied from a site.
3. If a site has a public player API a client of that site uses, write a
   small reader (W5) in `videopage/`, with a stand-in host variable for tests,
   as `youtube.rs` does.
4. Each reader ends in the same output: a list of tracks with qualities and
   addresses, with the headers to keep. Everything after is the engine.

## What is not done

No generic "every site" reader exists, and none is promised. A site whose
media is only reachable by running its script and has no plain-address client
is offered only when the browser extension has caught the player's own
requests.
