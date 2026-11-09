# GPUI dependency decision

**Status: DRAFT. Proposed, to be confirmed by building the Phase 2 shell.**
Researched 2026-11-09. Every outside fact below names its source and the date
it was read. A fact this research could not check is marked **UNVERIFIED**.
Nothing in this document changes a `Cargo.toml`. The first build of the GPUI
shell confirms or overturns it.

## 1. What dm needs from a GPUI dependency

These requirements come from dm's present window (`desktop/crates/dm-desktop`)
and its behaviour contract:

- **A screen reader reaches every dialog in that dialog's own window.**
  Contract row B42 makes each dialog a separate OS window.
  `scripts/human-qa.py` checks that VoiceOver gets OK and Cancel inside Add
  URL's own window. `docs/parity.md` ("Every control has a name a screen reader
  says") checks AT-SPI on Linux, and `scripts/windows-a11y-check.py` checks UI
  Automation on Windows. To get this from egui, dm had to patch eframe so that
  each window has its own AccessKit adapter (`docs/decisions.md`, 2026-11-08,
  gaps row 160).
- **Dialogs are real windows.** Each opens over the middle of the main window
  and can be moved anywhere. A modal dialog blocks the main window (B42).
- **A download list** that can be sorted, has resizable columns, and stays
  responsive with 5000 rows (CHANGELOG).
- **Text inputs** that support IME, plus menus, context menus, tooltips, check
  boxes, radio buttons, toggles and tabs.
- **System integration:** a tray icon, file choosers, the clipboard, files
  dropped in, and finished files dragged out to other apps. dm does part of
  drag-out itself (`xdnd.rs` on X11, `drops.rs`), using raw window handles.
- **Tests:** tests drive the window through its accessibility tree and compare
  offscreen snapshots (`egui_kittest` with `wgpu` and `snapshot`). Linux QA runs
  on Xvfb in Docker (`linux-human-qa.sh`).

### dm's toolchain and size today (repo, 2026-11-09)

| Fact | Value | Where |
|---|---|---|
| Window stack | eframe/egui 0.37, features `default_fonts, glow, persistence, wayland, x11, accesskit, clipboard`. winit 0.30, egui-winit and eframe are patched copies in `desktop/vendor`. | `desktop/crates/dm-desktop/Cargo.toml`, `desktop/Cargo.toml` |
| Other window crates | `tray-icon 0.26` (`ksni`), `rfd 0.17`, `arboard 3`, `raw-window-handle 0.6`, `unicode-bidi 0.3`, `egui_extras 0.37` (table) | same |
| AccessKit in the lock file | `accesskit 0.25.1`, `accesskit_macos 0.27.1` (through egui) | `desktop/Cargo.lock` |
| Toolchain | No `rust-toolchain` file. CI uses `dtolnay/rust-toolchain` on `stable`. This machine has rustc 1.98.1. Current stable is 1.99.1 (channel manifest dated 2026-10-15). | `.github/workflows/*.yml`, `rustc --version`, https://static.rust-lang.org/dist/channel-rust-stable.toml (2026-11-09) |
| Release profile | `opt-level = "z"`, `lto = true`, `codegen-units = 1`, `strip = true`. `egui` and `epaint` are built at `opt-level = 3`. No `panic = "abort"`. | `desktop/Cargo.toml` |
| Licence | GPL-3.0-or-later | `desktop/crates/dm-desktop/Cargo.toml` |
| 0.9.0 package sizes | .dmg (aarch64) 9.7 MB; .deb amd64 13.3 MB and arm64 12.4 MB; AppImage x86_64 13.2 MB; Windows setup .exe 9.9 MB; portable .zip 11.8 MB; .rpm x86_64 13.6 MB | `gh release view desktop-v0.9.0` (2026-11-09) |

## 2. Distributions available on 2026-11-09

### 2.1 Upstream GPUI in Zed's repository (git dependency)

- **Where it lives.** GPUI is now several crates in Zed's monorepo: `gpui`,
  `gpui_platform`, `gpui_macos`, `gpui_windows`, `gpui_linux`, `gpui_wgpu`,
  `gpui_web`, `gpui_ios`, `gpui_apple`, `gpui_macros`, `gpui_util`,
  `gpui_shared_string` and `gpui_tokio`
  (https://api.github.com/repos/zed-industries/zed/contents/crates, 2026-11-09).
  The README tells standalone apps to depend on `gpui` and `gpui_platform`
  with `version = "*"`, and to start the app with
  `gpui_platform::application()`
  (https://github.com/zed-industries/zed/blob/main/crates/gpui/README.md, 2026-11-09).
  `gpui_platform` and `gpui_wgpu` are **not** on crates.io: crates.io answers
  "does not exist" for both (2026-11-09). The README's instructions therefore
  work only with a git dependency.
- **Version and activity.** `crates/gpui/Cargo.toml` still says
  `version = "0.2.2"` and `license = "Apache-2.0"`, and has no `rust-version`
  (https://github.com/zed-industries/zed/blob/main/crates/gpui/Cargo.toml,
  2026-11-09). Commits to `crates/gpui` land daily. The latest seen was
  `39467cf4b1b9db03d180312a0ae681f59f85fe08`, 2026-11-09
  (https://api.github.com/repos/zed-industries/zed/commits?path=crates/gpui,
  2026-11-09). The README says GPUI is pre-1.0 and that breaking changes
  between versions are common.
- **Toolchain.** Zed pins Rust 1.98.1
  (https://github.com/zed-industries/zed/blob/main/rust-toolchain.toml,
  2026-11-09) and uses `edition = "2024"`. Zed's workspace takes `wgpu` from
  a **git rev** (`cddb2f5d80dc0009b2ef6767635140cacf90a007`). The comment
  there says 30.0.1 lacks a Vulkan surface-drop fix (wgpu#10230)
  (https://github.com/zed-industries/zed/blob/main/Cargo.toml, 2026-11-09).
- **Renderers.**
  - macOS uses Metal.
  - Linux uses wgpu, which replaced blade in PR #46758, merged 2026-02-13
  (https://github.com/zed-industries/zed/pull/46758). It asks for the Vulkan
  and GL backends (`WgpuContext::instance`,
  https://github.com/zed-industries/zed/blob/main/crates/gpui_wgpu/src/wgpu_context.rs,
  2026-11-09).
  - Windows uses Direct3D 11 directly, ported in PR #34374 on 2025-07-30
  (https://github.com/zed-industries/zed/pull/34374).
- **Wayland and X11.** Both are features of `gpui_platform` and are both
  compiled in by default. Windows needs no feature. On macOS, the `font-kit`
  feature is required to draw glyphs (README above).
- **Release cadence.** Zed itself ships a stable and a preview build every
  week (v1.27.0 and v1.28.0-pre on 2026-11-04,
  https://api.github.com/repos/zed-industries/zed/releases, 2026-11-09). GPUI
  has no release of its own.

### 2.2 `gpui` on crates.io

- The newest version is **0.2.2**, published **2025-10-22**. Versions 0.2.0
  and 0.2.1 came out in October 2025, and nothing has been published since.
  Its licence is Apache-2.0. The owners are Zed staff and
  `github:zed-industries:crates-io`
  (https://crates.io/api/v1/crates/gpui and `/owners`, 2026-11-09).
- 0.2.2 renders through `blade-graphics 0.7` on Linux and has **no
  `accesskit` dependency**
  (https://crates.io/api/v1/crates/gpui/0.2.2/dependencies, 2026-11-09).
  AccessKit came to GPUI on 2026-05-27 (§4.1).
- A secondary source agrees that this crate is about 11 months behind Zed main
  (https://baeseokjae.github.io/posts/native-app-for-coding-agents-rust-gpui/,
  dated 2026-10-01, read 2026-11-09).
- **Verdict: rejected.** It has no screen-reader support, which contract
  row B42 requires, and it uses the old Linux renderer.

### 2.3 `gpui-pre` snapshots on crates.io

- `gpui-pre` and its siblings (`gpui-pre-platform`, `-macos`, `-windows`,
  `-linux`, `-wgpu`, `-macros`, `-sum-tree` and others) republish Zed's GPUI
  crates as snapshots. The crate's description says: "Zed's GPU-accelerated UI
  framework (gpui-pre snapshot of zed@d2a4fb6)". Zed commit
  `d2a4fb6ded7c470be25d1541281da3882495b65b` is dated 2026-11-08.
- **0.3.12** was published **2026-11-09**. There have been 13 releases since
  0.3.0 on 2026-09-03, about one a week. The licence is Apache-2.0, and no
  `rust-version` is declared
  (https://crates.io/api/v1/crates/gpui-pre, 2026-11-09).
- **One person publishes it:** the only owner is `huacnlee`, the maintainer of
  the component library in §2.5 (https://crates.io/api/v1/crates/gpui-pre/owners,
  2026-11-09).
- It differs from upstream in one way that was seen: `gpui-pre-wgpu` uses
  `wgpu ^30.0.1` from crates.io instead of upstream's git rev. On Linux it
  enables the features `std, metal, gles, vulkan, wgsl, webgpu`
  (https://crates.io/api/v1/crates/gpui-pre-wgpu/0.3.12/dependencies,
  2026-11-09). Upstream's comment says 30.0.1 lacks a Vulkan surface-drop fix.
- AccessKit adapters are present on all three systems:
  - `gpui-pre-macos` uses `accesskit_macos ^0.26.0`.
  - `gpui-pre-windows` uses `accesskit_windows ^0.34`.
  - `gpui-pre-linux` uses `accesskit_unix ^0.22`, turned on by either the
    `wayland` or the `x11` feature.
  - The core uses `accesskit ^0.24.0`.
  (crates.io dependency API for each crate, 2026-11-09)
- `gpui-pre-platform` has the features `font-kit`, `wayland`, `x11`,
  `runtime_shaders`, `test-support`, `screen-capture` and `inspector`
  (https://crates.io/api/v1/crates/gpui-pre-platform/0.3.12, 2026-11-09).

### 2.4 GPUI Community Edition (`gpui-ce`)

- **It exists and is active.** The repo was created 2025-12-12. It has 1,573
  stars and at least 100 contributors (the first page of the API is full). The
  last push was 2026-11-09 (https://api.github.com/repos/gpui-ce/gpui-ce,
  2026-11-09). Recent commits come from several authors (philocalyst,
  temportalflux, cameronpcampbell, domenkozar)
  (https://api.github.com/repos/gpui-ce/gpui-ce/commits, 2026-11-09).
- **Version.** `gpui-ce` **0.3.0** was published **2026-10-20** with
  `rust_version = 1.95`. Before that came 0.2.2 (2026-08-28) and 0.3.2/0.3.3,
  both yanked (https://crates.io/api/v1/crates/gpui-ce, 2026-11-09). The
  platform crates (`gpui_ce_platform`, `gpui_ce_macos`, `gpui_ce_windows`,
  `gpui_ce_linux`, `gpui_ce_wgpu`, `gpui_ce_parley` and others) are all 0.3.0
  and all on crates.io (https://crates.io/api/v1/crates?q=gpui_ce, 2026-11-09).
  **UNVERIFIED oddity:** `crates/gpui/Cargo.toml` on `main` still says
  `version = "0.2.2"`.
- **Upstream it tracks.** It is synced from Zed by a local script, a
  three-way merge on a vendor branch. The last sync was Zed
  `b1a7ef0cf66dfbf9d7661170c96d97c7df916c68`, recorded as 2026-09-03.
  That is about two months behind Zed main
  (https://github.com/gpui-ce/gpui-ce/blob/main/scripts/sync-upstream/state.json
  and its README, 2026-11-09).
- **What it changes** (https://github.com/gpui-ce/gpui-ce/blob/main/CHANGELOG.md,
  2026-11-09). The API is diverging, and the README says it is "mostly API
  compatible with upstream, but that is changing":
  - text is laid out with Parley (0.12 is unreleased);
  - `AppContext` is split into traits;
  - `Background` replaces `Hsla` in borders and shadows;
  - an `AssetRegistry`;
  - a reworked drag-and-drop API;
  - the ARIA properties `disabled`, `modal` and `live`;
  - an opt-in wgpu renderer on macOS;
  - window icons on Wayland;
  - an `EditableText` element.
- **Platforms.** Its site claims macOS, Windows and Linux, with Metal, Vulkan
  and Direct3D, and says "the core crate includes an accessibility tree"
  (https://gpui-ce.github.io/, 2026-11-09). `gpui_ce_linux` depends on
  `accesskit_unix ^0.22`, Wayland and X11 crates, `ashpd` (portals) and
  `zed-xim` (https://crates.io/api/v1/crates/gpui_ce_linux/0.3.0/dependencies,
  2026-11-09).
- **Licence.** `LICENSE.md` and crates.io both say Apache-2.0. The website says
  "MIT OR Apache-2.0". **UNVERIFIED** which is meant. Either is compatible
  with dm's licence.
- **Ecosystem.** `gpui_ce_components` 0.3.0 (2026-11-05) is a fork of the
  component library in §2.5 (https://github.com/gpui-ce/gpui-component,
  20 stars, last push 2026-10-27).

### 2.5 Component library: `gpui-component` (longbridge GPUI Kit)

- **Releases.** `gpui-component` **0.9.1** was published **2026-11-09**. It is
  released weekly (0.7.0 on 2026-09-28, 0.8.0 on 2026-10-19, 0.9.0 on
  2026-11-02). Its licence is Apache-2.0
  (https://crates.io/api/v1/crates/gpui-component, 2026-11-09).
- **Repository.** It lives at https://github.com/longbridge/gpui-kit:
  20,077 stars, last push 2026-11-09. The code is Apache-2.0 and the docs prose
  is CC BY 4.0 (2026-11-09).
- **The GPUI it pins.** It depends on `gpui-pre = "=0.3.12"`, along with
  `gpui-pre-macros` and `gpui-pre-sum-tree` at the same exact version
  (https://crates.io/api/v1/crates/gpui-component/0.9.1/dependencies,
  2026-11-09). In the workspace these are renamed:
  - `gpui = { package = "gpui-pre", version = "=0.3.12" }`
  - `gpui_platform = { package = "gpui-pre-platform", version = "=0.3.12", features = ["font-kit", "x11", "wayland", "runtime_shaders"] }`

  A comment there says every snapshot crate must be bumped together
  (https://github.com/longbridge/gpui-kit/blob/main/Cargo.toml, 2026-11-09).
  **It cannot be used with upstream git GPUI or with gpui-ce:** those are
  different crates, so their types do not match.
- **What it provides.** The source tree of `crates/component/src` lists:
  input (with number input, textarea, search, OTP), table (`data_table`,
  `delegate`, `column`), `virtual_list`, list, menu and `native_menu`, popover,
  tooltip, dialog, sheet, notification, select, combobox, checkbox, radio,
  switch, tab, tree, progress, `status_bar`, toolbar, `title_bar`, sidebar,
  resizable and scroll (2026-11-09).
- **Accessibility.** The library sets AccessKit roles in its base controls
  (button, radio, switch, toggle, table, tabs, progress, focus trap). It has an
  accessibility guide with a headless test that checks AccessKit properties
  (https://github.com/longbridge/gpui-kit/blob/main/website/docs/accessibility.md,
  2026-11-09).
- **Dialogs draw inside the window.** `open_dialog` and `open_sheet` draw an
  overlay inside the current window's `Root`
  (`crates/component/src/window_ext.rs`, 2026-11-09). dm's dialogs must still
  be GPUI windows of their own (B42), with the library's controls inside them.
- **Features.** No default features. The required dependencies include
  `lsp-types`, `markdown`, `notify`, `ropey`, `rust-i18n`, `schemars` and
  `similar`. Tree-sitter and syntect are optional (crates.io, 2026-11-09).
  These count against binary size.
- **Is it needed for dm?** Yes, for the text input and the table:
  - Upstream GPUI has no ready-made text input with IME. It has only the
    `InputHandler` platform trait that an input must implement
    (`crates/gpui/src/platform.rs`, 2026-11-09).
  - Upstream has no table with sortable, resizable columns. It has only
    `uniform_list` and `list`.

  Writing both on dm's own would be the largest single cost of the
  migration. Menus and tooltips are smaller jobs, but they come with the same
  dependency.

### 2.6 Comparison

| | crates.io `gpui` 0.2.2 | Upstream git rev | `gpui-pre` 0.3.12 + `gpui-component` 0.9.1 | `gpui-ce` 0.3.0 (+ `gpui_ce_components` 0.3.0) |
|---|---|---|---|---|
| Newest code | Oct 2025 | daily | Zed main of 2026-11-08 | Zed main of 2026-09-03, plus CE changes |
| Registry | crates.io | git only (monorepo, wgpu by git rev) | crates.io | crates.io |
| AccessKit | no | yes (since 2026-05-27) | yes (all 3 adapters) | yes |
| Linux renderer | blade | wgpu (git rev) | wgpu 30.0.1 | wgpu ^29.0.3 (`gpui_ce_wgpu`) |
| API stability | frozen | none, pre-1.0 | exact pins; any snapshot may break | own semver-ish line, diverging from upstream |
| Ready widgets (input, table, menus) | via old gpui-component 0.5.x | none (write our own) | yes | yes, through a less-used fork |
| MSRV | none declared | Zed pins 1.98.1 | none declared; build with ≥ 1.98.1 | 1.95 |
| Who publishes | Zed | Zed | one person (`huacnlee`) | community org |
| Licence | Apache-2.0 | Apache-2.0 | Apache-2.0 | Apache-2.0 (site says MIT OR Apache-2.0) |
| Fit for dm | **no** (no screen readers) | possible, but every widget is ours | **best** | fallback |

## 3. Recommendation (proposed)

Use the **`gpui-pre` snapshots with `gpui-component`**, pinned exactly to the
pair that GPUI Kit publishes together. Use Zed's own source (git rev
`d2a4fb6ded7c470be25d1541281da3882495b65b`) only to read code and diagnose
problems, not as a dependency.

### 3.1 Exact pins for the Phase 2 shell crate (a new crate; do not add yet)

```toml
# Pinned together. gpui-kit's Cargo.toml says which snapshot each gpui-component release is built for.
gpui = { package = "gpui-pre", version = "=0.3.12" }
gpui_platform = { package = "gpui-pre-platform", version = "=0.3.12" }
gpui-component = { version = "=0.9.1" }          # default-features = false is already the default

[target.'cfg(target_os = "macos")'.dependencies]
gpui_platform = { package = "gpui-pre-platform", version = "=0.3.12", features = ["font-kit", "runtime_shaders"] }
[target.'cfg(target_os = "linux")'.dependencies]
gpui_platform = { package = "gpui-pre-platform", version = "=0.3.12", features = ["wayland", "x11"] }
# Windows: no features (Win32 + Direct3D 11 + DirectWrite).

[dev-dependencies]
gpui = { package = "gpui-pre", version = "=0.3.12", features = ["test-support"] }
gpui_platform = { package = "gpui-pre-platform", version = "=0.3.12", features = ["test-support"] }
```

- **macOS:** `font-kit` is required to draw glyphs (README). `runtime_shaders`
  compiles the Metal shaders at run time, as GPUI Kit does, so CI does not need
  the Metal compiler from full Xcode. **UNVERIFIED** that it removes that
  requirement.
- **Linux:** `wayland` and `x11` match eframe's features today. Either one
  also brings in `accesskit_unix`.
- **Windows:** no features.
- **Tray:** keep `tray-icon 0.26` at first, because GPUI has no tray (§4.3). If
  it does not work with GPUI's event loop, the alternative is `gpui-tray 0.3.0`
  with its `gpui-kit` feature. That crate's features are `gpui` (default),
  `gpui-ce` and `gpui-kit`
  (https://crates.io/api/v1/crates/gpui-tray/0.3.0, 2026-11-09).
- **MSRV and toolchain:** at least Rust **1.98.1**, the version Zed pins and
  this machine has. CI's `stable` (1.99.1) also works. `gpui-pre` declares no
  MSRV, so the Phase 2 shell should decide whether dm adds a
  `rust-toolchain.toml` pinned to 1.98.1.

### 3.2 Why this choice and not the others

- Only `gpui-pre` and gpui-ce come from a registry and include AccessKit.
  Registry crates suit dm's `cargo packager` releases and the PKGBUILD, which
  builds from a tarball. Upstream git would bring the Zed monorepo and a git
  `wgpu` into every build.
- `gpui-pre` sits one day behind Zed main, while gpui-ce is about two months
  behind. Accessibility fixes land upstream first: per-window adapter release
  on macOS (#64143, 2026-09-20) and accessibility IDs (#61926, 2026-08-07).
- The maintained component library is built for `gpui-pre`. Its gpui-ce fork
  is small (20 stars).
- **Fallback if `gpui-pre` stops being published** (it has one publisher):
  1. Depend on upstream git at the matching Zed SHA. The SHA is in each
     snapshot's description.
  2. Then either vendor `gpui-component` or switch both crates to gpui-ce
     together with `gpui_ce_components`.

  Whether Cargo's `[patch]` can map `gpui-pre` onto upstream's `gpui` through
  `package =` is **UNVERIFIED**.

### 3.3 Licence check

- Every crate recommended here is **Apache-2.0**. That includes `gpui-pre*`,
  `gpui-component`, `gpui-tray` and `accesskit`. (Apache-2.0 is in crates.io's
  metadata for each, read 2026-11-09. accesskit's licence is
  "MIT OR Apache-2.0" (**UNVERIFIED** on that page). It is already in dm's
  tree.)
- The FSF lists Apache-2.0 as compatible with GPLv3: code under it may be
  combined into a GPLv3 work
  (https://www.gnu.org/licenses/license-list.html#apache2, from knowledge of
  that page; **UNVERIFIED** today).
- dm must ship Apache-2.0's licence text and any NOTICE files of these crates
  with its packages. dm already has a `NOTICE` file to extend.
- Zed's repository as a whole is mixed-licence (GitHub reports
  `NOASSERTION`), but `crates/gpui` carries `LICENSE-APACHE`
  (https://github.com/zed-industries/zed/tree/main/crates/gpui, 2026-11-09).
- GPUI Kit's CC BY 4.0 applies only to its docs, which dm does not ship.

### 3.4 Upgrade procedure

1. **Pick the next pair.** Read GPUI Kit's `Cargo.toml` and `release-notes.md`
   at the `gpui-component` release you want. That release names the
   `gpui-pre` version it was built for. Never move `gpui-pre` without
   `gpui-component`.
2. **Read what changed.** Take the Zed SHAs from the two snapshots'
   descriptions and read `git log <old>..<new> -- crates/gpui*` upstream. Note
   anything touching `a11y`, `platform`, windows, input or `gpui_wgpu`.
3. **Bump everything together.** Change every `gpui-pre*` crate and
   `gpui-component` at once, with exact `=` pins. Then run `cargo update -p`
   for each and commit `Cargo.lock`.
4. **Run the full gate.** That is `cargo test --workspace`, then
   `python3 scripts/human-qa.py` in a visible window (B25 and B42 included),
   then `linux-human-qa.sh` and `scripts/windows-a11y-check.py`. A screen
   reader must still reach every dialog.
5. **Measure size.** Record the release sizes of the .dmg, .deb and setup .exe
   against the table in §1.
6. **Record the result.** Update this document's pin table and add a line to
   `docs/decisions.md`. Upgrade on purpose, not weekly: only for a fix dm
   needs, or every few months.

## 4. Capability findings

### 4.1 Accessibility (deciding)

- **History.**
  - AccessKit support was added on 2026-05-27 (PR #56065,
    https://github.com/zed-industries/zed/pull/56065). In nightly builds it
    caused panics: AccessKit panics when given an invalid `TreeUpdate`. In
    response, Zed added `Application::inaccessible()` (PR #57954, 2026-05-28).
  - **Zed itself still starts with `Application::new_inaccessible(platform)`**
    (`crates/zed/src/main.rs` line 92, 2026-11-09). GPUI's main user does not
    ship accessibility to its users yet.
  - Zed's issue "Windows: Screen reader accessibility missing completely" is
    still open (https://github.com/zed-industries/zed/issues/41138,
    2026-11-09).
- **Work for dm.** Only elements with an `id` **and** a role are reported to
  assistive technology
  (https://github.com/zed-industries/zed/blob/main/crates/gpui/src/_accessibility.rs,
  2026-11-09). egui names its widgets by itself, but in GPUI every custom
  control needs `.id(..).role(..).aria_label(..)`. `gpui-component`'s base
  controls set roles.
- **Per window.** `gpui_macos` keeps one AccessKit adapter per window (PR
  #64143, 2026-09-20). **UNVERIFIED** that each window has its own adapter on
  Windows and Linux, which is what B42 needs.
- **Other gaps.** Accessibility IDs map to UIA `AutomationId`, `AXIdentifier`
  and AT-SPI `AccessibleId` (PR #61926). `aria_keyshortcuts()` is not yet
  passed on to screen readers (PR #60397).

### 4.2 Windows (OS windows)

- `open_window` takes the kinds `Normal`, `PopUp`, `AnchoredPopup`,
  `Floating`, `LayerShell` (Wayland only) and `Dialog`. `Dialog` is
  documented as "blocks interaction with [its parent] until the modal window
  is closed" (`crates/gpui/src/platform.rs`, 2026-11-09). This matches B42.
- **On X11 a window's requested position is ignored** (issue #64447, open since
  2026-09-18). A `PopUp` window there can never take focus (issue #65493, open
  since 2026-10-12). B42 requires a dialog to open "over the middle of the dm
  window".

### 4.3 System integration

- **Tray: not in GPUI.** Upstream PR #44047 "gpui: Add Tray support" was closed
  unmerged on 2025-12-12 (https://github.com/zed-industries/zed/pull/44047).
- **File dialogs:** `prompt_for_paths` and `prompt_for_new_path` exist. On
  Linux they go through the XDG portal (`ashpd` with `file_chooser`), so they
  fail where no portal runs, such as minimal Docker or Xvfb. dm can keep
  `rfd 0.17`.
- **Clipboard:** `read_from_clipboard` and `write_to_clipboard` exist. dm can
  keep `arboard`.
- **Other:** `reveal_path`, `open_with_system`, `on_open_urls` and
  `register_url_scheme` exist (dm uses magnet links), as do `set_menus`, which
  is the macOS menu bar, and `set_dock_menu` (`platform.rs`, 2026-11-09).
- **Drag in:** external file drops (`ExternalPaths`) are a GPUI feature.
  **UNVERIFIED** on each OS in this research.
- **Drag out:** `start_external_drag` defaults to `false`. Code search finds
  implementations only in `gpui_macos` and the Wayland backend (2026-11-09).
  dm's own X11 and Windows drag-out code would have to keep working through a
  raw window handle. **UNVERIFIED** that GPUI windows expose
  `HasWindowHandle`.

### 4.4 Text input and IME

- The platform `InputHandler` covers marked text and IME on macOS and Windows.
  On X11 it uses `zed-xim`, and on Wayland text-input.
- `gpui-component`'s `Input` implements the handler.

### 4.5 Lists and tables

- `uniform_list` and `list` are virtualized. A `list()` padding fix landed on
  2026-11-06 (#64056).
- `gpui-component` has `table/data_table.rs` and `virtual_list.rs`.
  **UNVERIFIED:** column resizing, sorting, and smooth behaviour at 5000 rows.

### 4.6 Testing

- `TestAppContext` mocks the platform.
- `HeadlessAppContext` runs on every platform with real text shaping. It can
  render on a real GPU and capture screenshots.
- `VisualTestAppContext` renders really on **macOS only**, using off-screen
  windows (`crates/gpui/src/app/*.rs`, 2026-11-09).
- Linux gained a headless wgpu renderer for tests in PR #64718 (2026-09-25),
  which rejects software adapters unless a software-allowed constructor is
  used.
- GPUI Kit has a test helper that asserts AccessKit properties after a painted
  frame. That is the nearest replacement for `egui_kittest`'s
  accessibility-tree driving.

### 4.7 Rendering without a GPU

- **Linux:** wgpu enumerates the Vulkan and GL backends. A window's first
  context accepts software adapters (`DeviceType::Cpu`, for example Mesa
  llvmpipe or lavapipe) but ranks them last. Software is rejected only when
  the context is recreated after a lost device (`new_rejecting_software`,
  `crates/gpui_wgpu/src/wgpu_renderer.rs`, 2026-11-09). Xvfb with Mesa should
  therefore draw. **UNVERIFIED** in practice.
- **Windows:** the Direct3D 11 device is created on an enumerated DXGI
  adapter. `D3D_DRIVER_TYPE_WARP` appears only in a test in
  `directx_atlas.rs` (2026-11-09). **UNVERIFIED** whether a VM without a GPU,
  which has only the Basic Render Driver, opens a window.
- **macOS:** Metal only, with no software fallback. **UNVERIFIED** in macOS
  VMs.

### 4.8 Binary size

**UNVERIFIED.** No authoritative figure for a minimal GPUI build was found.
Secondary sources give 2 to 15 MB, without the build settings (search on
2026-11-09). Three things will push dm's size up:

- `gpui-component`'s required dependencies (§2.5);
- `gpui-pre-http-client`;
- GPUI Kit's advice to build `gpui-pre` at `opt-level = 3`. dm found that a
  whole-program opt-level 3 gave a 27 MB binary (`desktop/Cargo.toml`
  comment).

### 4.9 Windows maturity

- Direct3D 11 since July 2025. AccessKit's Windows adapter is in `gpui-pre`.
- Upstream issue #41138 says screen readers get nothing from Zed on Windows.
  That is about Zed's own UI, which runs inaccessible, but no evidence was
  found that GPUI's UIA output has been tested end to end. PR #61926 says
  "has not run live platform UIA, AX, or AT-SPI end-to-end tests".
- A 2026-10-01 blog post calls Windows "the least-evidenced target" (secondary
  source, §2.2).

## 5. Blockers to verify in the Phase 2 shell

Each item is pass or fail on a build of the pins in §3.1:

1. **Screen readers in every window.** VoiceOver, NVDA (UIA) and Orca (AT-SPI)
   reach OK and Cancel in a second `WindowKind::Dialog` window, through
   `human-qa.py` B42, `windows-a11y-check.py` and `linux-gui-check.sh`. The
   tree must not panic when windows open and close repeatedly.
2. **Dialog placement on X11.** A dialog opens over the middle of the main
   window under Xvfb and on GNOME/X11 (issue #64447). It must be modal
   (`Dialog`) and movable outside the main window on all three OSes.
3. **Software rendering.** The shell draws under Xvfb in dm's Docker images
   with Mesa only (llvmpipe/lavapipe), and on a Windows VM without a GPU.
   Otherwise `linux-human-qa.sh` and the Windows checks cannot run.
4. **Size.** Release .dmg, .deb and setup .exe stay near today's 10 to 13 MB
   under `opt-level = "z"` + LTO. Measure again with `gpui-pre` at
   `opt-level = 3`.
5. **Tray and login launch.** `tray-icon 0.26` (`ksni` on Linux) works next to
   GPUI's run loop on all three OSes, or `gpui-tray`'s `gpui-kit` backend does.
   dm's hidden login launch must still create the window only when it is
   first shown.
6. **Drag and drop.** Dropping files and URLs in works. Dragging finished
   files out works on macOS, Windows, X11 and Wayland, using GPUI's
   `start_external_drag` where it exists and dm's own code elsewhere.
7. **Text input.** IME works in `gpui-component`'s `Input` (CJK on macOS,
   Windows, Fcitx/IBus on X11 and Wayland). Arabic and Hebrew text reads in
   reading order and reaches the screen reader as stored.
8. **The download table.** It sorts, resizes columns, and keeps its CPU share
   with 5000 rows and 20 active downloads (today 3.3 % busy, 128 MB).
9. **Async runtime.** dm's tokio runtime sits next to GPUI's executors
   (`gpui_tokio` exists upstream; check its `gpui-pre` equivalent).
10. **Tests.** A `HeadlessAppContext` or GPUI Kit UI test drives a dialog
    through its AccessKit tree in CI on all three OSes.
11. **Toolchain.** The shell builds on 1.98.1 and on current stable (1.99.1),
    and `cargo packager` makes the .app, .deb, AppImage and NSIS installer
    unchanged.
12. **Linux package dependencies.** Find what the .deb must list for wgpu
    (Vulkan loader, EGL/GL) and fonts, and update
    `[package.metadata.packager.deb] depends`.

## 6. Sources (all read 2026-11-09 unless a date is given)

- crates.io API:
  - `crates/gpui`, `gpui/0.2.2/dependencies`, `gpui/owners`
  - `crates/gpui-pre`, `gpui-pre/owners`, `gpui-pre-platform/0.3.12`,
    `gpui-pre-wgpu/0.3.12/dependencies`, `gpui-pre-linux/0.3.12`,
    `gpui-pre-macos/0.3.12/dependencies`,
    `gpui-pre-windows/0.3.12/dependencies`
  - `crates/gpui-ce`, `gpui-ce/0.3.0/dependencies`, `crates?q=gpui_ce`,
    `gpui_ce_platform/0.3.0/dependencies`, `gpui_ce_linux/0.3.0/dependencies`,
    `gpui_ce_wgpu/0.3.0/dependencies`
  - `crates/gpui-component`, `gpui-component/0.9.1/dependencies`,
    `crates/gpui-kit`, `crates/gpui-tray`, `gpui-tray/0.3.0`
  - all under https://crates.io/api/v1/
- https://github.com/zed-industries/zed: `crates/gpui/Cargo.toml`,
  `crates/gpui/README.md`, `crates/gpui/src/_accessibility.rs`,
  `crates/gpui/src/platform.rs`, `crates/gpui/src/app/visual_test_context.rs`,
  `crates/gpui/src/app/headless_app_context.rs`,
  `crates/gpui_wgpu/src/wgpu_context.rs`,
  `crates/gpui_wgpu/src/wgpu_renderer.rs`,
  `crates/gpui_windows/src/directx_devices.rs`,
  `crates/gpui_windows/src/directx_atlas.rs`, `crates/zed/src/main.rs`,
  `Cargo.toml`, `rust-toolchain.toml`
- Zed pull requests (https://github.com/zed-industries/zed/pull/N):
  34374, 44047, 46758, 56065, 57954, 60397, 61926, 64033, 64143, 64718
- Zed issues (https://github.com/zed-industries/zed/issues/N): 41138, 64447,
  65493
- https://github.com/gpui-ce/gpui-ce: README, `CHANGELOG.md`, `SETUP.md`,
  `LICENSE.md`, `crates/gpui/Cargo.toml`, `scripts/sync-upstream/`
- https://gpui-ce.github.io/
- https://github.com/longbridge/gpui-kit: README, `Cargo.toml`,
  `crates/component/src/`, `window_ext.rs`, `website/docs/accessibility.md`
- https://github.com/gpui-ce/gpui-component
- https://baeseokjae.github.io/posts/native-app-for-coding-agents-rust-gpui/
  (secondary source, dated 2026-10-01)
- https://static.rust-lang.org/dist/channel-rust-stable.toml
- https://www.gnu.org/licenses/license-list.html#apache2 (not fetched today;
  **UNVERIFIED**)
