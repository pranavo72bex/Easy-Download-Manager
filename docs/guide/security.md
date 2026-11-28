# What EDM protects you from

EDM downloads from servers, pages and peers it cannot trust. This page says what it
guards against, what it does not, and where the test that holds each guard is, so
you can check it for yourself. To report a hole: [SECURITY.md](../../SECURITY.md).

## What EDM guards against

### A hostile server

| The attack | What EDM does | The test that holds it |
|---|---|---|
| A redirect, or a name that resolves, into your own network (your router, a printer, `localhost`) | Refused, unless you typed that address yourself | `edm-net/src/redirects/tests.rs` (`a_redirect_inward_from_a_public_address_is_refused_and_no_other_is`), `edm-net/src/dns/tests.rs` (`a_name_that_answers_public_then_inside_is_refused_when_it_turns`) |
| A file name that climbs out of your download folder (`../../`), or a device name on Windows | Made safe before anything is written | `docs/security-report.md` S1; `edm-core/src/naming/tests.rs` |
| An HTTPS download sent on to plain HTTP | Refused | `docs/security-report.md` S4 |
| Your sign-in or cookies sent on to another site by a redirect | Not sent; only the page's origin crosses as `Referer` | `edm-net/src/redirects/tests.rs` (`a_referer_crosses_hosts_as_its_origin_alone`) |
| A server that lies about a file's length, or sends without end | Never a "complete" file that is not whole; a reply read only as far as asked | `docs/security-report.md` S10; `edm-net/src/body.rs` tests |
| A number that overflows while reading what a server sends | The download stops with an error, in the release build too | `Cargo.toml`'s release profile (`overflow-checks`) |
| A video page that names a file outside EDM's working folder | Not fetched | `edm-core/src/videopage/fetch/tests.rs` (`a_name_that_steps_out_of_the_staging_folder_is_not_a_plan`) |

### A hostile file

| The attack | What EDM does | The test that holds it |
|---|---|---|
| An archive whose entries leave its folder | The whole archive refused before anything is written | `edm-core/src/extract.rs` (`entries_that_leave_the_folder_fail_the_whole_archive_before_any_write`) |
| An archive that unpacks to gigabytes from a few bytes | Refused by its size ratio | `edm-core/src/extract.rs` (`a_bomb_is_refused_by_its_ratio`) |
| A program named as a picture, film or document (`invoice.pdf` that is a Windows program) | Its row says "not what its name says" | `edm-core/src/disguise.rs` tests; `edm-desktop-base/src/view/rows/tests.rs` (`a_program_in_disguise_is_said_on_its_row`) |
| A downloaded file opened without the system's warning | Marked as from the internet, as a browser marks it (Windows' Mark of the Web, macOS's quarantine) before the row says Complete | `edm-core/src/motw.rs` tests |
| A `.torrent` whose files leave its folder | Refused | `edm-core/tests/torrent_stress.rs` (`paths_outside_the_folder`) |

### A hostile web page

| The attack | What EDM does | The test that holds it |
|---|---|---|
| A page that makes the extension hand EDM something you did not ask for, or choose where it is saved | The hand-off carries the link alone; the folder and name are yours | `docs/security-report.md` S8 |
| Downloads from a private window recorded | Not recorded; nothing from a private window is handed over from a menu or the panel | `edm-catcher/src/decide/tests.rs` (`nothing_from_a_private_window_is_handed_over_from_a_menu_or_the_panel`) |

### Another program on your computer or phone

| The attack | What EDM does | The test that holds it |
|---|---|---|
| A program driving EDM through its control port | Nothing is answered without the token, which only your account can read | `edm-desktop-base/src/run/control/tests.rs` (`nothing_is_answered_before_the_token`, `a_token_is_made_once_kept_and_private`) |
| Your saved passwords and cookies read from EDM's files | Sealed with the system's own store (Windows' Credential Manager, the macOS Keychain, Linux's Secret Service, the phone's hardware key); with no store, kept in a file only your account can read | `edm-core/tests/queue.rs` (`a_sealed_file_of_secrets_with_no_store_to_open_it_is_said_and_kept_aside`) |
| Another Android app pointing EDM at EDM's own files | Only another app's `content://` address is read, never `file://` or EDM's own | `edm-android/src/shared.rs` tests |
| The Android app asking for more than it needs, or opening a part to other apps | Every permission and open component held to a kept list, checked on each release | `cargo xtask android-manifest`, `crates/edm-android/android/manifest-kept.txt` |
| A fake EDM APK | Its signing certificate's SHA-256 is in the README; the release fails if it changes | `.github/workflows/android.yml` |

### A hostile peer (torrents)

| The attack | What EDM does | The test that holds it |
|---|---|---|
| Peers on a block list | Not connected to | Options › BitTorrent's block list |
| A peer reading what you download on the way | Peer encryption | Options › BitTorrent |

## What EDM does not guard against

- **A computer or phone already taken over.** A program running as you can read what you
  can read, EDM's files included, and can watch the screen.
- **A file that is harmful and is what its name says.** EDM says when a file is not what its
  name says; it does not judge whether a program is safe. Options › Downloads can run a
  virus checker of your own on each finished file ("When a download finishes, run this command").
- **A site you trust that turns hostile.** EDM checks a file against a checksum or digest the
  site gives; if the site itself is changed, so is its checksum.
- **Who sees that you connected.** EDM's traffic goes where a browser's would, through any
  proxy you set. It does not hide that you visit a site.
- **A torrent's peers seeing your address.** In a swarm, every peer sees every other peer.

## Checking for yourself

- Every test named above runs on each change: `cargo test --workspace`.
- `cargo audit` and `cargo deny` check the libraries EDM uses (`.github/workflows/audit.yml`).
- The audits so far: [security report](../security-report.md) and
  [audit of 2026-11-12](../security-audit-2026-11-12.md).
