# Releasing dm

What has to be true before anyone can install this, and what only a person
holding an account or a certificate can do.

## The two halves

Everything about *building* a release is automated and has been run. Everything
about *being trusted by an operating system* needs a certificate that has to be
bought, and no amount of code substitutes for one.

| | State |
|---|---|
| Installers build | Verified — a `.dmg` and a `.app` were produced on macOS, and the Arch package was built in Arch's own container |
| Update check | `latest.json` is written by the release job, with each installer's SHA-256; dm reads it and says when a newer version is out, and checks the installer it downloads against that digest. It does not install anything, so nothing is signed |
| macOS is notarised | Needs an Apple Developer ID — $99/year |
| Windows is signed | Needs an Authenticode certificate |
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
`desktop/crates/dm-desktop/Cargo.toml` as `packager.macos.signingIdentity`.
Without these the build still succeeds — it just produces something
Gatekeeper distrusts.

### 2. Windows

Authenticode signing is configured under `packager.windows` in
`desktop/crates/dm-desktop/Cargo.toml` (a certificate thumbprint or a sign
command). Without it, SmartScreen warns until enough people install it
anyway.

## Cutting a release

```sh
# The version appears in two places and they must agree.
$EDITOR desktop/crates/dm-desktop/Cargo.toml   # version
$EDITOR desktop/packaging/PKGBUILD             # pkgver

cd desktop && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings
cd ../browser && cargo test && cd .. && sh scripts/package.sh && python3 scripts/chrome-check.py

git commit -am "dm 0.2.0"
git tag desktop-v0.2.0
git push origin main --tags
```

The tag is what runs the `bundle` job. It builds a `.dmg`, a Windows
installer, a `.deb` and an `.AppImage`, writes `latest.json`, and opens a
**draft** release —
deliberately draft, so a broken build is not something anybody downloads before
you have looked at it.

To build installers without releasing anything, run the workflow by hand
(`workflow_dispatch`); the artefacts are attached to the run instead.

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

## The browser extension

The zips are built by `sh scripts/package.sh`, which compiles `browser/` to
WebAssembly with the wasm-bindgen CLI matching `browser/Cargo.lock`. The
Chrome zip carries the manifest `key` that fixes the unpacked id; remove it
before a Web Store upload (the store assigns its own id, which is then
registered with `dm-desktop --register <id>`). Chrome's needs uploading to the
Web Store.

Microsoft Edge Add-ons: Actions › "Edge Add-ons, upload and submit" ›
Run workflow builds the Chromium zip, takes out the manifest's `key`,
uploads it to the existing listing as a draft and submits it for review
(untick "Submit" to upload the draft only). It needs the repository
secrets `EDGE_CLIENT_ID` and `EDGE_API_KEY` (Partner Center › Microsoft
Edge › Publish API) and `EDGE_PRODUCT_ID` (the listing's Product ID, a GUID
on its Overview page). The version must be above the listing's. A copy
installed from the store has the store's own id (the listing's CRX ID):
until dm knows it, people run `dm-desktop --register <that id>`.

Firefox installs only add-ons Mozilla has signed. A `v*` tag submits the
Firefox build to AMO as a listed add-on and includes the source archive for
review, given the AMO API key as the secrets `AMO_JWT_ISSUER` and
`AMO_JWT_SECRET` (Developer Hub › Manage API Keys). The add-on must first
exist as a listed submission in the Developer Hub. Mozilla accepts each
version once, so bump `extension/manifest.json`,
`extension/manifest.firefox.json` and `browser/Cargo.toml` together (the
release checks all three agree with the tag). 3.0.0 went to the deleted
unlisted add-on and 3.0.1 to the listed one; 3.0.2 adds the `scripting`
permission (to read links on a tab open before the extension was); 3.0.3
sends a site's sign-in with "Download all/selected links" where the popup
hands it over (no new permission); 3.0.4 reads the click keys from the
app's rules: Alt leaves a download with the browser and Shift hands it to
dm whatever its type or size, each changeable in Options › Browser.

## Verifying a release actually works

The parts that are easy to get wrong, and how they were checked here:

```sh
# The window's controls do what they claim, against a real queue.
cd desktop && cargo test -p dm-desktop --test window

# The packaged app, driven: lock, hand-off, proxy, schedule, --hidden.
DM_BINARY=desktop/target/release/bundle/dm.app/Contents/MacOS/dm-desktop python3 scripts/app-check.py
sh scripts/linux-package.sh     # builds the .deb and AppImage, installs the .deb, drives it

# The extension installs into a real Firefox and downloads a file.
python3 scripts/firefox-check.py granted

# The Arch package builds in Arch, with a container standing in for the runner.
docker run --rm --platform linux/amd64 --security-opt seccomp=unconfined \
  -v /tmp/dm-src:/src:ro archlinux:base-devel bash /arch-job.sh
```

On an Apple Silicon machine the Arch container runs under emulation, and
pacman's download sandbox uses seccomp filters emulation cannot honour — hence
`--security-opt seccomp=unconfined` and `DisableSandbox`. Neither is needed on
the CI runner, which is native amd64.

### While the listed add-on is in review

Mozilla's listed review can take days. Meanwhile, **Actions › Firefox, signed
for self-distribution › Run workflow** (`.github/workflows/firefox-signed.yml`)
signs the Firefox build as unlisted — automatic, about two minutes — and
attaches `dm-firefox-<version>.xpi` to the release named. It installs and stays
installed in ordinary Firefox (the README's Firefox step). Each version signs
once, listed or not: bump both manifests and `browser/Cargo.toml` first.
3.0.5 was signed this way on 2026-10-19 and attached to `desktop-v0.2.0`.

