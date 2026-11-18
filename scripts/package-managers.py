#!/usr/bin/env python3
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""The package managers' manifests for a release, written from its latest.json.

    python3 scripts/package-managers.py                    # the latest release, through gh
    python3 scripts/package-managers.py path/latest.json   # a latest.json at hand

Writes, under desktop/packaging/managers/:
  homebrew/easy-download-manager.rb   a Homebrew cask (both Mac dmgs)
  scoop/easy-download-manager.json    a Scoop manifest (the Windows portable zips, x64 and arm64)
  winget/<version>/*.yaml             the three winget files (the Windows setup.exes, x64 and arm64)
  flatpak/<app id>.yml                a Flatpak manifest (the Linux .deb, both arches)

Each points at the release's own files with the digests latest.json gives,
so nothing is built again. Submitting them is the maintainer's: a pull
request to Homebrew/homebrew-cask, ScoopInstaller/Extras,
microsoft/winget-pkgs and flathub/flathub (docs/RELEASING.md).
"""
import json
import pathlib
import subprocess
import sys

# UTF-8 for this script's own output, whatever the console's code page (gaps row X4).
for _stream in (sys.stdout, sys.stderr):
    try:
        _stream.reconfigure(encoding="utf-8", errors="replace")
    except (AttributeError, ValueError):
        pass

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT / "desktop" / "packaging" / "managers"
REPO = "pranavo72bex/Easy-Download-Manager"
HOME = f"https://github.com/{REPO}"
SUMMARY = "A download manager: many connections, resume, and a queue"
# Homebrew's style wants no article first.
CASK_DESC = "Download manager with many connections, resume and a queue"
LICENCE = "GPL-3.0-or-later"
WINGET_ID = "pranavo72bex.EasyDownloadManager"
FLATPAK_ID = "io.github.pranavo72bex.EasyDownloadManager"


def release():
    if len(sys.argv) > 1:
        return json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
    raw = subprocess.run(["gh", "release", "download", "-R", REPO, "-p", "latest.json", "-O", "-"],
                         check=True, capture_output=True, text=True, encoding="utf-8", errors="replace").stdout
    return json.loads(raw)


def maybe(rel, name):
    """The address and digest of one of the release's files, or None if it has none."""
    digest = rel["sha256"].get(name)
    url = next((u for u in rel["installers"] if u.rsplit("/", 1)[-1] == name), None)
    return (url, digest) if digest and url else None


def asset(rel, name):
    """The address and digest of one of the release's files; stops if it has none."""
    found = maybe(rel, name)
    if not found:
        sys.exit(f"latest.json names no {name}")
    return found


# Windows' processors as Scoop, winget and the release's file names call them. arm64 (Windows
# on Arm, gaps row R3) only where the release has its files: 0.10.0 and older have none.
WINDOWS_ARCHES = (("64bit", "x64", "x64"), ("arm64", "arm64", "arm64"))


def homebrew(rel, v):
    arm_url, arm_sum = asset(rel, f"dm_{v}_aarch64.dmg")
    _, intel_sum = asset(rel, f"dm_{v}_x64.dmg")
    base = arm_url.rsplit("/", 1)[0].replace(f"desktop-v{v}", "desktop-v#{version}")
    return f'''cask "easy-download-manager" do
  arch arm: "aarch64", intel: "x64"

  version "{v}"
  sha256 arm:   "{arm_sum}",
         intel: "{intel_sum}"

  url "{base}/dm_#{{version}}_#{{arch}}.dmg"
  name "dm"
  desc "{CASK_DESC}"
  homepage "{HOME}"

  livecheck do
    url :url
    regex(/^desktop[._-]v?(\\d+(?:\\.\\d+)+)$/i)
    strategy :github_latest
  end

  depends_on macos: ">= :big_sur"

  app "dm.app"

  zap trash: "~/Library/Application Support/dm"
end
'''


def scoop(rel, v):
    asset(rel, f"dm_{v}_x64-portable.zip")
    arches, updates = {}, {}
    for key, name, _ in WINDOWS_ARCHES:
        found = maybe(rel, f"dm_{v}_{name}-portable.zip")
        if not found:
            continue
        url, digest = found
        arches[key] = {"url": url, "hash": digest, "extract_dir": f"dm_{v}_{name}-portable"}
        updates[key] = {
            "url": f"{HOME}/releases/download/desktop-v$version/dm_$version_{name}-portable.zip",
            "extract_dir": f"dm_$version_{name}-portable",
        }
    return json.dumps({
        "version": v,
        "description": SUMMARY,
        "homepage": HOME,
        "license": LICENCE,
        "architecture": arches,
        "bin": [["dm-gpui.exe", "dm"]],
        "shortcuts": [["dm-gpui.exe", "dm"]],
        # The portable copy keeps its settings and list in dm-data beside it.
        "persist": "dm-data",
        # The browser's host names the program's path: Scoop's `current` folder
        # stays the same across updates, so it is registered from there.
        "post_install": [
            "& \"$($dir -replace '\\\\[^\\\\]+$', '\\current')\\dm-gpui.exe\" --register | Out-Null"
        ],
        "checkver": {"github": HOME, "regex": "desktop-v([\\d.]+)"},
        "autoupdate": {"architecture": updates},
    }, indent=4) + "\n"


def winget(rel, v):
    asset(rel, f"dm_{v}_x64-setup.exe")
    installers = ""
    for _, name, arch in WINDOWS_ARCHES:
        found = maybe(rel, f"dm_{v}_{name}-setup.exe")
        if found:
            installers += (f"- Architecture: {arch}\n"
                           f"  InstallerUrl: {found[0]}\n"
                           f"  InstallerSha256: {found[1].upper()}\n")
    head = (f"PackageIdentifier: {WINGET_ID}\nPackageVersion: {v}\n")
    version = head + ("DefaultLocale: en-US\nManifestType: version\nManifestVersion: 1.6.0\n")
    installer = head + (
        "InstallerType: nullsoft\n"
        "Scope: user\n"
        "UpgradeBehavior: install\n"
        "Protocols:\n- magnet\n"
        "FileExtensions:\n- torrent\n"
        "Installers:\n"
        f"{installers}"
        "ManifestType: installer\nManifestVersion: 1.6.0\n")
    locale = head + (
        "PackageLocale: en-US\n"
        "Publisher: pranavo72bex\n"
        f"PublisherUrl: https://github.com/pranavo72bex\n"
        "PackageName: dm\n"
        f"PackageUrl: {HOME}\n"
        f"License: {LICENCE}\n"
        f"LicenseUrl: {HOME}/blob/main/LICENSE\n"
        f"ShortDescription: \"{SUMMARY}\"\n"
        "Moniker: easy-download-manager\n"
        "Tags:\n- download-manager\n- downloader\n- torrent\n"
        f"ReleaseNotesUrl: {rel.get('notes', HOME + '/releases')}\n"
        "ManifestType: defaultLocale\nManifestVersion: 1.6.0\n")
    return {
        f"{WINGET_ID}.yaml": version,
        f"{WINGET_ID}.installer.yaml": installer,
        f"{WINGET_ID}.locale.en-US.yaml": locale,
    }


def flatpak(rel, v):
    x_url, x_sum = asset(rel, f"dm_{v}_amd64.deb")
    a_url, a_sum = asset(rel, f"dm_{v}_arm64.deb")
    i = FLATPAK_ID
    return f'''# Repackages the release's .deb: the same binary the .deb installs.
app-id: {i}
runtime: org.freedesktop.Platform
runtime-version: "24.08"
sdk: org.freedesktop.Sdk
command: dm-gpui
finish-args:
  - --share=network
  - --share=ipc
  - --socket=wayland
  - --socket=fallback-x11
  - --device=dri
  - --filesystem=xdg-download
  - --talk-name=org.kde.StatusNotifierWatcher
  - --talk-name=org.freedesktop.Notifications
  - --talk-name=org.freedesktop.secrets
modules:
  - name: dm
    buildsystem: simple
    build-commands:
      - ar x dm.deb
      - tar -xf data.tar.*
      - install -Dm755 usr/bin/dm-gpui /app/bin/dm-gpui
      - install -Dm644 usr/share/icons/hicolor/128x128/apps/dm-gpui.png /app/share/icons/hicolor/128x128/apps/{i}.png
      - install -Dm644 usr/share/icons/hicolor/32x32/apps/dm-gpui.png /app/share/icons/hicolor/32x32/apps/{i}.png
      - install -Dm644 usr/share/applications/dm-gpui.desktop /app/share/applications/{i}.desktop
      - desktop-file-edit --set-icon={i} --set-key=Exec --set-value="dm-gpui %u" /app/share/applications/{i}.desktop
    sources:
      - type: file
        dest-filename: dm.deb
        only-arches: [x86_64]
        url: {x_url}
        sha256: {x_sum}
      - type: file
        dest-filename: dm.deb
        only-arches: [aarch64]
        url: {a_url}
        sha256: {a_sum}
'''


def main():
    rel = release()
    v = rel["version"]
    files = {
        "homebrew/easy-download-manager.rb": homebrew(rel, v),
        "scoop/easy-download-manager.json": scoop(rel, v),
        f"flatpak/{FLATPAK_ID}.yml": flatpak(rel, v),
    }
    for name, text in winget(rel, v).items():
        files[f"winget/{v}/{name}"] = text
    for name, text in files.items():
        path = OUT / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
        print("wrote", path.relative_to(ROOT))


if __name__ == "__main__":
    main()
