#!/usr/bin/env python3
"""yt-dlp's unpacked build for this computer's kind, fetched from its own
release and unpacked into a folder, for packaging inside dm's installers:

    python3 scripts/fetch-ytdlp.py desktop/crates/dm-desktop/resources/yt-dlp

dm looks there (`ytdlp::bundled`) after an installed or fetched copy, so
the first video on a fresh install needs no fetch; a newer copy dm fetches
after a failure takes its place. The build is the one dm would fetch itself
(`tools::ytdlp_asset_for`), checked against the release's SHA2-256SUMS.
Standard library only.
"""
import hashlib, io, json, pathlib, platform, shutil, stat, sys, urllib.request, zipfile

RELEASE = "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest"


def asset_name():
    system, machine = platform.system(), platform.machine().lower()
    arm = machine in ("arm64", "aarch64")
    if system == "Darwin":
        return "yt-dlp_macos.zip"
    if system == "Windows":
        return "yt-dlp_win_arm64.zip" if arm else "yt-dlp_win.zip"
    if system == "Linux":
        return "yt-dlp_linux_aarch64.zip" if arm else "yt-dlp_linux.zip"
    sys.exit(f"no unpacked yt-dlp build is known for {system} {machine}")


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    dest = pathlib.Path(sys.argv[1])
    name = asset_name()
    agent = {"User-Agent": "dm-packaging (+https://github.com/pranavo72bex/Easy-Download-Manager)"}
    with urllib.request.urlopen(urllib.request.Request(RELEASE, headers=agent), timeout=60) as reply:
        release = json.load(reply)
    assets = {a["name"]: a["browser_download_url"] for a in release["assets"]}
    if name not in assets:
        sys.exit(f"release {release['tag_name']} has no {name}")
    with urllib.request.urlopen(urllib.request.Request(assets["SHA2-256SUMS"], headers=agent), timeout=60) as reply:
        sums = dict(line.split()[::-1] for line in reply.read().decode().splitlines() if len(line.split()) == 2)
    print(f"fetching {name} from yt-dlp {release['tag_name']}", flush=True)
    with urllib.request.urlopen(urllib.request.Request(assets[name], headers=agent), timeout=600) as reply:
        data = reply.read()
    if sums.get(name) != hashlib.sha256(data).hexdigest():
        sys.exit(f"{name} does not match the release's checksum")
    if dest.exists():
        shutil.rmtree(dest)
    dest.mkdir(parents=True)
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        archive.extractall(dest)
    # The archive holds one folder (yt-dlp_macos/…): its contents are what
    # dm wants, beside each other.
    inner = [p for p in dest.iterdir() if p.is_dir()]
    if len(inner) == 1 and not any(p.is_file() and p.name.startswith("yt-dlp") for p in dest.iterdir()):
        for child in list(inner[0].iterdir()):
            shutil.move(str(child), dest / child.name)
        inner[0].rmdir()
    for program in dest.iterdir():
        if program.is_file() and program.name.startswith("yt-dlp"):
            program.chmod(program.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
    (dest / "VERSION").write_text(release["tag_name"] + "\n")
    print(f"unpacked into {dest} ({release['tag_name']})")


if __name__ == "__main__":
    main()
