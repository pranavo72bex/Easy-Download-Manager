# Releasing EDM

What has to be true before anyone can install this, and what only a person
holding an account or a certificate can do.

## The two halves

Everything about *building* a release is automated and has been run. Everything
about *being trusted by an operating system* needs a certificate that has to be
bought, and no amount of code substitutes for one.

| | State |
|---|---|
| Installers build | Verified — a `.dmg` and a `.app` were produced on macOS, and the Arch package was built in Arch's own container |
| Update check | `latest.json` is written by the release job, with each installer's SHA-256; the release job signs it with the project's Ed25519 key (`latest.json.sig` beside it), and EDM believes it only with that signature (the public key is built into `edm-shell/src/app/update.rs`), says when a newer version is out, and checks the installer it downloads against the manifest's digest. Until the maintainer makes the key (step 3 below) EDM carries a placeholder and the release job's signing step fails. It does not install anything yet (gaps row F6) |
| macOS is notarised | Needs an Apple Developer ID — $99/year |
| What is in a release, and that it is the release | Each release carries `crates.txt` (every crate and version the app and the extension are built from) and `SHA256SUMS` with `SHA256SUMS.sig` (the digest of every file, signed by the same key as `latest.json`); `cargo xtask verify-release` checks a download (below, gaps row W21). Not yet run at a tag. A second build compared byte for byte has not been made: what is and is not reproducible is said under "Building the same bytes" |
| Windows is signed | Not yet. A paid Authenticode certificate would do it; the free route is SignPath Foundation's (the README's Code signing policy is what it asks release pages to carry), and until then SmartScreen warns on first launch |
| Firefox add-on is installable permanently | Signed by Mozilla (unlisted) in `release.yml` when `AMO_JWT_ISSUER` and `AMO_JWT_SECRET` are set; 3.0.0 was signed by hand |

## One-time setup

### 1. Apple, if you want macOS to open without a right-click

Without these, macOS refuses the app until someone right-clicks it and chooses
Open, and says it is "damaged" if they double-click. That is Gatekeeper working
as designed, not a bug in the build.

```sh
gh secret set APPLE_CERTIFICATE           # base64 of the .p12
gh secret set APPLE_CERTIFICATE_PASSWORD
gh secret set APPLE_ID                    # the Apple ID
gh secret set APPLE_PASSWORD              # an app-specific password, not the real one
gh secret set APPLE_TEAM_ID
```

The identity, `Developer ID Application: NAME (TEAMID)`, goes in
`crates/edm-gpui/Cargo.toml` (the window macOS ships) as `packager.macos.signingIdentity`.
Without these the build still succeeds — it just produces something
Gatekeeper distrusts.

### 2. Windows

Authenticode signing is configured under `packager.windows` in
`crates/edm-gpui/Cargo.toml` (the window Windows ships; a certificate
thumbprint or a sign command). Without it, SmartScreen warns until enough people install it
anyway.

### 3. The update-signing key (once, before the next `desktop-v` tag)

EDM believes a release's `latest.json` only when `latest.json.sig` beside it
is the signature of the project's Ed25519 key (gaps row F6). The key is free
and made here; nobody but the maintainer holds its private half.

```sh
cargo xtask update-key
```

It prints two lines:

- the **public key**: paste it as `SIGNING_KEY` in
  `crates/edm-shell/src/app/update.rs`, replacing the placeholder,
  and commit that;
- the **private key**: set it as the repository secret, and keep a copy
  offline (a password manager). Never commit it or paste it anywhere else.

```sh
gh secret set DM_UPDATE_SIGNING_KEY        # paste the private line
```

The `release` job runs `cargo xtask sign-manifest ../installers/latest.json`
with that secret; the signer refuses a key whose public half is not the one
EDM carries, so a missing secret, the placeholder or a mismatched pair fails
the job before anything is drafted. To sign a manifest by hand:

```sh
EDM_UPDATE_SIGNING_KEY='…' cargo xtask sign-manifest path/latest.json   # writes path/latest.json.sig
```

An EDM built with one key refuses every manifest signed by another. Losing
the private key, or replacing it, means the dms already installed can no
longer believe an update: they would have to be updated from the releases
page by hand. Keep the copy.

## Cutting a release

```sh
# The version appears in these places and they must agree.
$EDITOR crates/edm-gpui/Cargo.toml      # version (and edm-core's, edm-shell's,
                                               # edm-desktop-base's: a test holds them equal)
$EDITOR packaging/PKGBUILD             # pkgver

cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings
(cd crates/edm-catcher && cargo test) && sh scripts/package.sh && python3 scripts/chrome-check.py

git commit -am "EDM 0.2.0"
git tag desktop-v0.2.0
git push origin main --tags
```

The tag is what runs the `bundle` job. It builds a `.dmg` (its EDM.app with the
Safari extension, `scripts/safari-extension.sh --dmg`), a Windows
installer, a `.deb` and an `.AppImage`, writes `latest.json`, signs it
(`latest.json.sig`, step 3 above), writes the crate list (`crates.txt`) and
the signed digests of every file (`SHA256SUMS`, `SHA256SUMS.sig`; "Checking
a release" below), checks them, and opens a
**draft** release —
deliberately draft, so a broken build is not something anybody downloads before
you have looked at it.

The draft's text starts with a link to the README's **Code signing policy**,
which SignPath Foundation asks every release page to carry. When writing the
release notes, keep that line (put the notes under it).

To build installers without releasing anything, run the workflow by hand
(`workflow_dispatch`); the artefacts are attached to the run instead.

## Checking a release

Every release carries, beside its installers (gaps row W21):

- `crates.txt`: every crate and version the app (`edm-gpui`) and the browser
  extension (`edm-catcher`) are built from, every system's together, each with
  where it comes from: crates.io and the SHA-256 `Cargo.lock` pins for
  it, a git repository, a copy vendored in `vendor/`, or EDM's own
  folder. Written by `cargo xtask crates-list <file>` from the lock file
  (`cargo tree` without dev-dependencies, for every target), so what a test
  alone uses is not in it.
- `SHA256SUMS`: the SHA-256 of every file of the release (the installers, the
  zips, `latest.json` and its signature, `crates.txt`), one `<digest>  <name>`
  a line, as `sha256sum` writes it.
- `SHA256SUMS.sig`: the Ed25519 signature of `SHA256SUMS`, base64, by the
  project's key, the one that signs `latest.json` (step 3 above). The release
  job makes both with `cargo xtask sign-release ../installers`, which refuses a
  key that is not the one EDM carries, then checks them with
  `verify-release` before anything is drafted.

To check what you downloaded, put the files in one folder (all of them, or
yours with `SHA256SUMS` and `SHA256SUMS.sig`) and, from a checkout of the
release's tag:

```sh
cargo xtask verify-release ~/Downloads/edm-0.10.0
```

It checks the signature against the public key EDM carries
(`SIGNING_KEY` in `crates/edm-shell/src/app/update.rs`), then each file
present against its line, and `latest.json.sig` against `latest.json` when
both are there. A changed file, a changed line or another key fails it; files
the list names that are not in the folder are said and left unchecked.

Without Rust, the digests alone (this does not check who wrote them):

```sh
sha256sum -c --ignore-missing SHA256SUMS                     # Linux
shasum -a 256 -c --ignore-missing SHA256SUMS                 # macOS
Get-FileHash edm_0.10.0_x64-setup.exe -Algorithm SHA256       # Windows PowerShell: compare by eye
```

and the signature with OpenSSL 3, `KEY` being `SIGNING_KEY` from that file:

```sh
KEY='…'
{ printf '302a300506032b6570032100' | xxd -r -p; printf '%s' "$KEY" | base64 -d; } > edm-key.der
base64 -d SHA256SUMS.sig > SHA256SUMS.sig.bin
openssl pkeyutl -verify -pubin -keyform DER -inkey edm-key.der -rawin   -in SHA256SUMS -sigfile SHA256SUMS.sig.bin           # "Signature Verified Successfully"
```

(the hex is the fixed DER prefix of an Ed25519 public key; the 32 bytes of
the key follow it).

### Which commit and workflow built a file

Each release job (`desktop.yml`'s installers, `release.yml`'s extension zips and signed
Firefox file, `android.yml`'s APKs) records a build attestation for every file it publishes
(gaps row H29): GitHub signs a statement of the file's SHA-256, the commit and the workflow
run that made it, and keeps it with the repository. To check one, with the GitHub CLI:

```sh
gh attestation verify edm-0.10.4-arm64-v8a.apk --repo pranavo72bex/Easy-Download-Manager
```

It fails for a file no release job of this repository built. Together with `crates.txt`
(what the build was made from) and `SHA256SUMS.sig` (that the files are the ones published),
this says where a download came from, without trusting the page it was downloaded from.

### Building the same bytes

What fixes a build, and how to repeat the release job's:

- **Rust**: `rust-toolchain.toml` at the repository's root pins the toolchain
  (1.98.1 when this was written; the release jobs pin the same). rustup
  installs it on the first `cargo` in the checkout.
- **Crates**: `Cargo.lock`, with `--locked` so nothing in it moves;
  `crates.txt` is what it resolved to for the release. The patched crates are
  in `vendor/`, in the checkout.
- **Profile**: `[profile.release]` in `Cargo.toml` (size first, LTO,
  one codegen unit, stripped, a few crates at `opt-level = 3`); the extension
  is the `extension` profile.
- **Flags**: `CARGO_BUILD_RUSTFLAGS="$(sh scripts/release-flags.sh)"`, which
  writes the checkout, Cargo's folders and the home folder under neutral names
  so a build elsewhere carries the same paths. Windows keeps its own target
  flags (`+crt-static`, `.cargo/config.toml`), which Cargo takes over
  these.
- **Target**: the release's are `x86_64-unknown-linux-gnu` and
  `aarch64-unknown-linux-gnu` (built on Ubuntu 22.04), `x86_64-pc-windows-msvc`,
  `aarch64-pc-windows-msvc`, `aarch64-apple-darwin` and `x86_64-apple-darwin`
  (`.github/workflows/desktop.yml`, the `bundle` job).

```sh
git checkout desktop-v0.10.0
CARGO_BUILD_RUSTFLAGS="$(sh ../scripts/release-flags.sh)"   cargo build --release --locked -p edm-gpui --target x86_64-unknown-linux-gnu
```

and for the extension, `sh scripts/package.sh` (the same profile, flags,
`--locked`, and the wasm-bindgen version the lock names).

What is and is not reproducible today, honestly: **no second build has yet
been compared byte for byte with a release**, so nothing here is claimed
bit-for-bit. What is known:

- The installers and zips will not match as files: `.deb`, AppImage, `.dmg`,
  NSIS and zip archives record the time they were made. Compare the program
  inside instead: `edm-gpui.exe` in the Windows portable zip, `usr/bin/edm-gpui`
  in the `.deb` (`dpkg-deb -x`), `EDM.app/Contents/MacOS/edm-gpui` in the
  `.dmg`, `edm_catcher_bg.wasm` in the extension's zips.
- Windows: the build machine's paths stay in the binary (the remapping flags
  do not reach it, above), and the MSVC linker may write the time into the
  program's header unless told not to (`/Brepro`); expect a Windows build to
  differ until both are looked at.
- Linux: C code in some crates (ring among them) is compiled with the
  system's compiler and linked against its glibc, so expect a build to
  compare only on the same Ubuntu 22.04 image.
- macOS: the bundle is signed ad hoc on the runner; the signature is part of
  the file compared.

Comparing a second build at a tag, and closing what differs, is what is left
of gaps row W21; signing the installers themselves is R1's, at the
maintainer's word.

## Arch

The PKGBUILD fetches the release tarball for its tag, so it only works once the
tag above exists. After pushing the tag, replace `sha256sums=('SKIP')` with the
real digest:

```sh
curl -sL https://github.com/pranavo72bex/Easy-Download-Manager/archive/refs/tags/desktop-v0.2.0.tar.gz | sha256sum
```

`SKIP` means the package will build from whatever is at that URL, which is not
what anyone wants in a package other people install.

The pinned digest is that release's alone. CI's Arch builds skip it for the
tarballs they make themselves (the checkout's, and a new tag's archive), so a
pin never fails them; set it back to `SKIP` when bumping `pkgver`, and pin the
new tag's digest once the tag is pushed.

## Package managers

The release job writes them for each `desktop-v*` tag and puts them on the
release page as `edm_<version>_package-managers.zip` (the `packaging/managers/`
folder, from that release's `latest.json`). To write them again by hand, once the
release is published (not a draft: the manifests point at its files):

```sh
python3 scripts/package-managers.py
```

It writes `packaging/managers/`: a Homebrew cask (both Mac dmgs), a
Scoop manifest (the Windows portable zip, registered with the browser from
Scoop's `current` folder), the three winget files (the setup.exe) and a Flatpak
manifest (the .deb repackaged, both arches). Each carries the digests
`latest.json` gives. Submitting them:

- Homebrew: a tap of your own (`pranavo72bex/homebrew-EDM`, `Casks/`). The main
  cask list refuses apps Gatekeeper refuses, and EDM's dmg is unsigned. Check
  first with `brew audit --cask --online` from that tap.
- Scoop: a pull request to `ScoopInstaller/Extras` (`bucket/`).
- winget: a pull request to `microsoft/winget-pkgs`
  (`manifests/p/pranavo72bex/EasyDownloadManager/<version>/`), checked first
  with `winget validate` on Windows.
- Flathub: a new-app pull request to `flathub/flathub`, built first with
  `flatpak-builder`. In the sandbox the browser hand-over is not set up for you
  (the browser's host file lives outside it), so say so in the listing.

## The browser extension

The zips are built by `sh scripts/package.sh`, which compiles `crates/edm-catcher/` to
WebAssembly with the wasm-bindgen CLI matching `Cargo.lock`. The
Chrome zip carries the manifest `key` that fixes the unpacked id; remove it
before a Web Store upload (the store assigns its own id, which is then
registered with `edm-desktop --register <id>`). Chrome's needs uploading to the
Web Store.

Microsoft Edge Add-ons: each `v*` tag's Release run ends with an `edge`
job that builds the Chromium zip, takes out the manifest's `key`, uploads
it to the existing listing as a draft and submits it for review, as the
Firefox build goes to addons.mozilla.org in the same run. Microsoft refuses
the upload while an earlier version is still in review; then, once that
review ends, rerun the failed `edge` job or start Actions › "Edge Add-ons,
upload and submit" › Run workflow by hand (untick "Submit" to upload the
draft only). It needs the repository
secrets `EDGE_CLIENT_ID` and `EDGE_API_KEY` (Partner Center › Microsoft
Edge › Publish API) and `EDGE_PRODUCT_ID` (the listing's Product ID, a GUID
on its Overview page). The version must be above the listing's. A copy
installed from the store has the store's own id (the listing's CRX ID):
until EDM knows it, people run `edm-desktop --register <that id>`.

A `v*` tag (`git tag v3.3.0 && git push origin v3.3.0`) builds both store zips, opens a
**draft** GitHub release with them, as the app's tag does (publish it once looked at), and
submits the extension to addons.mozilla.org and Edge Add-ons for review.

Firefox installs only add-ons Mozilla has signed. A `v*` tag submits the
Firefox build to AMO as a listed add-on and includes the source archive for
review, given the AMO API key as the secrets `AMO_JWT_ISSUER` and
`AMO_JWT_SECRET` (Developer Hub › Manage API Keys). The add-on must first
exist as a listed submission in the Developer Hub. Mozilla accepts each
version once, so bump `extension/manifest.json`,
`extension/manifest.firefox.json` and `crates/edm-catcher/Cargo.toml` together (the
release checks all three agree with the tag). 3.0.0 went to the deleted
unlisted add-on and 3.0.1 to the listed one; 3.0.2 adds the `scripting`
permission (to read links on a tab open before the extension was); 3.0.3
sends a site's sign-in with "Download all/selected links" where the popup
hands it over (no new permission); 3.0.4 reads the click keys from the
app's rules: Alt leaves a download with the browser and Shift hands it to
EDM whatever its type or size, each changeable in Options › Browser.

## Verifying a release actually works

The parts that are easy to get wrong, and how they were checked here:

```sh
# The window's controls do what they claim, against a real queue.
cargo test -p edm-gpui window_tests

# The packaged app, driven: lock, hand-off, proxy, schedule, --hidden.
EDM_BINARY=target/release/bundle/EDM.app/Contents/MacOS/edm-gpui python3 scripts/app-check.py
sh scripts/linux-package.sh     # builds the .deb and AppImage, installs the .deb, drives it

# The release's own Linux .deb and extension, downloaded from its draft and checked
# against latest.json: the whole of human-qa.py in a Debian container (an X display,
# Chromium in Edge's place); the macOS run is human-qa.py with EDM_BINARY naming the
# .dmg's app and dist/chrome holding the release's Chrome zip.
EDM_RELEASE=desktop-v0.2.0 sh scripts/linux-human-qa.sh

# The extension installs into a real Firefox and downloads a file.
python3 scripts/firefox-check.py granted

# The Arch package builds in Arch, with a container standing in for the runner.
docker run --rm --platform linux/amd64 --security-opt seccomp=unconfined \
  -v /tmp/edm-src:/src:ro archlinux:base-devel bash /arch-job.sh
```

On an Apple Silicon machine the Arch container runs under emulation, and
pacman's download sandbox uses seccomp filters emulation cannot honour — hence
`--security-opt seccomp=unconfined` and `DisableSandbox`. Neither is needed on
the CI runner, which is native amd64.

### While the listed add-on is in review

Mozilla's listed review can take days. Meanwhile, **Actions › Firefox, signed
for self-distribution › Run workflow** (`.github/workflows/firefox-signed.yml`)
signs the Firefox build as unlisted — automatic, about two minutes — and
attaches `edm-firefox-<version>.xpi` to the release named. It installs and stays
installed in ordinary Firefox (the README's Firefox step). Each version signs
once, listed or not: bump both manifests and `crates/edm-catcher/Cargo.toml` first.
3.0.5 was signed this way on 2026-10-19 and attached to `desktop-v0.2.0`.
