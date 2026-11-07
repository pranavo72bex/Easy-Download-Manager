# Contributing to dm

Thank you for helping. Bug reports, fixes and new features are all welcome.

## Reporting a bug

Open an issue with the **Bug report** form. The most useful reports say
what you downloaded (a link, if it is public), what you expected, what dm
did, and your operating system and dm version (About). For a security
problem, see [SECURITY.md](SECURITY.md) instead.

## Before writing code

Read [AGENTS.md](AGENTS.md): it maps the code and says where each kind of
change belongs. `docs/gaps.md` is the ranked list of open work, and
`docs/decisions.md` records why things are the way they are, including
approaches that were tried and dropped. For anything larger than a small
fix, open an issue first so the approach can be agreed before you build it.

## Building and testing

You need Rust (stable) from <https://rustup.rs>.

```sh
# the app
cd desktop
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo run -p dm-desktop

# the browser extension
cd browser
cargo test
cd .. && sh scripts/package.sh     # builds dist/chrome and dist/firefox
```

`docs/how-it-works.md` explains how the app and the extension fit together
and how to load the extension unpacked. `docs/testing.md` lists every check
and how to run it, including on Linux in Docker.

### Logins and secrets in tests

No password, token or cookie is written in the code, not even a made-up
one for a test. A test asks `desktop/test-support/secrets.rs` for one by
name; it is `DM_TEST_<NAME>` from the environment or from a `.env` file at
the repository's root, and otherwise made up for that run. `.env` is in
`.gitignore`; `.env.example` lists the names. `cargo xtask secrets`, run by
CI and by `scripts/pre-commit`, fails on a login, token or key written into
a tracked file. Release signing reads its secrets from the repository's
Actions secrets only (docs/RELEASING.md).

## What a pull request needs

- **Tests.** A behaviour change comes with a test that fails without it.
  dm's tests run against real servers on loopback and a real queue, not
  mocks; follow the tests next to the code you change.
- **No panics on outside input.** Clippy denies `unwrap`, `expect` and
  `panic!` in shipped code. Anything that reads a server's reply, a file or
  a message returns an error instead.
- **The docs it touches.** If you change what dm does, update the line in
  `docs/parity.md` (and the checklist in `CLAUDE.md`) that describes it.
- **Your own work.** Do not copy code, text, artwork or layouts from other
  download managers or other products. dm is written from scratch.
- **Privacy.** dm sends nothing anywhere except the downloads a person asks
  for. `cargo xtask privacy` checks that no analytics or stray addresses
  creep in.
- **The licence header.** Every source file of dm's own starts with its
  copyright and licence notice, in its language's comment style:

  ```rust
  // Copyright (C) 2026 pranavo72bex and the dm contributors.
  // SPDX-License-Identifier: GPL-3.0-or-later
  // Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
  // with the attribution term in NOTICE. See the LICENSE and NOTICE files.
  ```

  `cargo xtask headers fix` (in `desktop/`) writes it into a new file;
  `cargo xtask headers`, which CI runs, fails a file without it. Third-party
  code in `desktop/vendor/` keeps its own licence and takes no header, and
  test data under `fixtures/` takes none either. The NOTICE's section 7(b)
  term asks everyone who copies dm's code to keep these notices unchanged.

By contributing, you agree that your contribution is licensed under the
[GNU GPL v3 or later](LICENSE), with the attribution terms in [NOTICE](NOTICE), like the rest of dm.
