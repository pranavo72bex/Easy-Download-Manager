#!/usr/bin/env python3
"""yt-dlp's unpacked build for this computer's kind, fetched from its own
release and unpacked into a folder, for packaging inside dm's installers:

    python3 scripts/fetch-ytdlp.py desktop/crates/dm-desktop/resources/yt-dlp

dm copies it into its own tools folder on first use (`tools::adopt`), so
the first video on a fresh install needs no fetch; newer releases dm
fetches later take its place. The release and each build's SHA-256 are
pinned in packaging/yt-dlp.pin.json, bumped by a commit: a build that does
not match fails the packaging, rather than shipping whatever `latest` is.
Standard library only.
"""
import hashlib, io, json, pathlib, platform, shutil, stat, sys, urllib.request, zipfile

PIN = pathlib.Path(__file__).resolve().parent.parent / "packaging" / "yt-dlp.pin.json"


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
    pin = json.loads(PIN.read_text())
    tag, want = pin["tag"], pin["sha256"].get(name)
    if not want:
        sys.exit(f"{PIN} pins no digest for {name}")
    agent = {"User-Agent": "dm-packaging (+https://github.com/pranavo72bex/Easy-Download-Manager)"}
    url = f"https://github.com/yt-dlp/yt-dlp/releases/download/{tag}/{name}"
    print(f"fetching {name} from yt-dlp {tag}", flush=True)
    with urllib.request.urlopen(urllib.request.Request(url, headers=agent), timeout=600) as reply:
        data = reply.read()
    got = hashlib.sha256(data).hexdigest()
    if got != want:
        sys.exit(f"{name} from {tag} is {got}, not the pinned {want}")
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
    (dest / "VERSION").write_text(tag + "\n")
    print(f"unpacked into {dest} ({tag})")


if __name__ == "__main__":
    main()
