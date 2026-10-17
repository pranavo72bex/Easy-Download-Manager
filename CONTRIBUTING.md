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
and how to load the extension unpacked.

## What a pull request needs

- **Tests.** A behaviour change comes with a test that fails without it.
  dm's tests run against real servers on loopback and a real queue, not
  mocks; follow the tests next to the code you change.
- **No panics on outside input.** Clippy denies `unwrap`, `expect` and
  `panic!` in shipped code. Anything that reads a server's reply, a file or
  a message returns an error instead.
- **The docs it touches.** If you change what dm does, update the line in
  `docs/parity.md` (and `CLAUDE.md`'s checklist) that describes it.
- **Your own work.** Do not copy code, text, artwork or layouts from other
  download managers or other products. dm is written from scratch.
- **Privacy.** dm sends nothing anywhere except the downloads a person asks
  for. `cargo xtask privacy` checks that no analytics or stray addresses
  creep in.

By contributing, you agree that your contribution is licensed under the
[MIT licence](LICENSE), like the rest of dm.
