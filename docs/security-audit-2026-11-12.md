# Security audit, 2026-11-12

Run as [Cloudflare's security-audit skill](https://github.com/cloudflare/security-audit-skill)
describes its **quick** profile, on dm's source at `b7e1a3d1` (`gpui-migration`): four hunters, one
for each trust boundary below, read the source; every candidate went to a verifier that had not
found it and tried to refute it by re-reading every cited line; only confirmed records are listed
as findings. Nothing of dm's was run outside its own tests (no sandbox was set up, so no target
code was executed to reproduce a finding: each result is the source trace's).

A finding needs a boundary and a result: a lower-trust actor, what they control, the control
meant to stop them, and what they get. A missing best practice, a weakness another layer already
stops, or harm only to oneself is a hardening note, not a finding.

## Posture

No critical or high finding. dm keeps a server's names inside the download folder (sanitised
names, `create_new`, no overwriting), unpacks ZIPs safely, scopes cookies and logins to their
host, refuses a redirect from the internet to an inside address, gates its control port behind a
constant-time token, and runs JavaScript in an engine context that holds nothing of the computer.
Three medium findings and three low ones were confirmed, each where a stated control of dm's own
had a path around it. All six are fixed in the same change as this report, with a test each.

## Findings

| Severity | Finding | Boundary | Result |
|----------|---------|----------|--------|
| Medium | A `magnet:` launch on Windows read the words after the link as options | a program that hands Windows a `magnet:` link with an unescaped `"` | a file of its choosing saved into a folder of its choosing (a Startup folder), nothing asked; a shutdown could be set |
| Medium | "Download all links" sent requests to inside addresses a public page named | a public web page | dm asked routers and other hosts on the person's network for each link (with any login written into the link) as the dialog opened |
| Medium | Playlists, DASH manifests and Metalink mirrors could name inside addresses | a public server | dm fetched inside hosts the page named, with its Referer and any cookies held for them |
| Medium | A script or assistant driving the window could move the download folder | an AI assistant (when allowed) steered by a page | Options' download folder set to a startup folder, then a download added into it, which the control port's own rule was meant to stop |
| Low | Taking one file out of a remote ZIP unpacked to whatever size the archive claimed | a server | memory used up to hundreds of gigabytes, ending dm and every download in it |
| Low | After a crash, a client could read a new control token beside the old port | another account on the computer | the token sent to a port that account had taken, in a window of milliseconds |
| Low | Linux: the opener's error file sat in the shared temporary folder | another account on the computer | a file of the person's truncated through a planted link (where the system does not block such links) |

### Fixes

- **`magnet:` launch** (`dm-shell/src/shell.rs` `one_link_launch`, read by `args()`): a launch whose
  first word is a `magnet:` link is that one link; every word after it is part of it. Test
  `a_magnet_launch_is_one_link_whatever_follows_it`.
- **Inside addresses** (`dm-core/src/redirects.rs` `named_inside`, the same judgement as a
  redirect): a public playlist's or manifest's joined address (`stream::join`, `dash::join`) and a
  public Metalink's mirrors are refused when inside; a public page's links are dropped when inside,
  and any login written into a page's link is taken out (`dm-shell/src/host.rs` `page_link`). Tests
  `an_address_a_public_document_names_inside_is_judged_inside`,
  `a_public_pages_links_inside_are_dropped_and_logins_taken_out`.
- **Folders kept from a script** (`dm-shell/src/settings.rs` `keep_places_of`, used by
  `Backend::set_settings_from` for a script's change, as the program settings are): the download
  folder, the parts folder, sorting by kind, each kind's folder and each site's folder; the window
  says they were not changed. Test `a_scripts_settings_change_keeps_the_folders_a_persons_sets_them`.
- **ZIP take-out** (`dm-core/src/zipinfo.rs`): refused when the unpacked size is over the cap, and
  never unpacked past it. Test `a_file_said_to_unpack_past_the_cap_is_refused_and_never_unpacked_past_it`.
- **Token and port** (`dm-desktop-base/src/control.rs` `fresh_token`, `mcp.rs` `finder`): a stale
  port file is removed before a new token is written, and a client reads the token before the port.
  Test `a_new_token_takes_away_a_port_file_left_behind`.
- **Linux opener file** (`dm-shell/src/shell.rs` `OpenerSays`): made new (`create_new`, 0600) in
  dm's own home and handed to the opener as the same open file. Linux-only code; compiled by CI's
  Linux build, not on this Mac.

## Still owed

- A script typing a folder into Add URL's, Properties' or Save to's folder field is not yet held to
  the download folder, as Options now is (the folder verifier's note). The control port's own
  `downloads.add` is (`control_api::add_asked`).
- An inside address reached through a public name that resolves inside (DNS rebinding) is judged
  by the name as written, for redirects and the new checks alike.
- The JavaScript host has no OS sandbox of its own (seatbelt, seccomp, AppContainer): an exploit
  of the system's JavaScript engine itself would run as the person.
- Updates are checked against a SHA-256 published beside them, from the same release; a separate
  signature would hold if the release itself were taken over (row F6).

## Hardening notes

- `via_mcp` is said by the client itself: "assistants only" is a label for a client holding the
  token, not a control (the token is the control).
- A page can rewrite the words of dm's panels in its own page (open shadow roots); the clicks still
  need a person's real click and use dm's own choices.
- Response bodies read whole where a server sends no length (`zipinfo::range`, `assemble` seam
  checks, an MMS description) are bounded by their timeouts only.
- `videopage/fetch.rs` keeps a file under its folder with `Path::starts_with`, which does not
  refuse `..`; safe today as its only names come from `safe_title`.

## Coverage

Covered: the browser hand-off (extension messages, the native host, the inbox, the links dialog),
local IPC (control port, MCP, inbox and bell, the instance lock, the command line, the JavaScript
host), a hostile server against the engine (names, ZIP, Metalink, torrents' paths, HLS and DASH,
redirects, cookies and logins, PAC, the YouTube player script, sizes), and every program dm runs,
file it deletes or moves, secret it keeps, and its update. Not covered: librqbit's peer and tracker
protocols, the RTMP, RTSP and MMS parsers, the decompressors and the media muxers in depth, the
Windows Chakra path in depth, and Firefox-only paths.
