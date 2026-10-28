#!/bin/sh
# Why dm fetches yt-dlp's unpacked build, shown: on a computer whose /tmp does
# not let programs run (mounted noexec, as hardened and tuned systems have it)
# the single-file build cannot start at all, where the unpacked one does.
#
#   sh scripts/linux/noexec-tmp-check.sh        # needs docker and a network
#
# The single-file yt-dlp unpacks itself into /tmp each time it starts. Under a
# noexec /tmp it dies with "error while loading shared libraries: libz.so.1:
# failed to map segment from shared object", which dm 0.8.3 said as "dm could not
# get the video from this page" (every yt-dlp failure it had no words for).
# Run from the release's own files, for this machine's processor.
set -eu
case "$(uname -m)" in
  x86_64) one=yt-dlp_linux ;;
  aarch64 | arm64) one=yt-dlp_linux_aarch64 ;;
  *) echo "no yt-dlp Linux build is known here for $(uname -m)"; exit 2 ;;
esac
# -i hands the script below to the container; without it Python reads nothing,
# runs nothing and exits 0, which is why the answer is looked for in the output.
out=$(docker run --rm -i --tmpfs /tmp:rw,noexec,nosuid,size=1g -e ONE="$one" python:3-slim python3 -I - <<'PY'
import os, subprocess, tempfile, urllib.request, zipfile, pathlib

one = os.environ["ONE"]
base = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/"
work = pathlib.Path("/root/ytdlp")
work.mkdir(parents=True)
single = work / one
urllib.request.urlretrieve(base + one, single)
single.chmod(0o755)
urllib.request.urlretrieve(base + one + ".zip", work / "unpacked.zip")
with zipfile.ZipFile(work / "unpacked.zip") as archive:
    archive.extractall(work / "unpacked")
program = work / "unpacked" / one
program.chmod(0o755)
# Files extracted from a zip keep the modes it records; the loader's own files are
# read, not run, so only the program needs the bit.

tmp_options = next(line.split()[3] for line in open("/proc/mounts") if line.split()[1] == "/tmp")
print("/tmp is mounted:", tmp_options)
assert "noexec" in tmp_options.split(","), "this check needs a noexec /tmp"


def starts(path):
    done = subprocess.run([str(path), "--version"], capture_output=True, text=True)
    return done.returncode == 0, (done.stdout or done.stderr).strip().splitlines()[-1:]


for label, path in (("single file", single), ("unpacked", program)):
    ok, said = starts(path)
    print(f"{label}: {'starts' if ok else 'does not start'} {said}")
    if label == "single file":
        single_ok = ok
    else:
        unpacked_ok = ok
assert unpacked_ok, "the unpacked build must start under a noexec /tmp"
assert not single_ok, "the single-file build started: this check no longer shows what it was written to show"
print("on a noexec /tmp the single file cannot start and the unpacked build can")
PY
) || { printf '%s\n' "$out"; exit 1; }
printf '%s\n' "$out"
case "$out" in
  *"the single file cannot start and the unpacked build can"*) ;;
  *) echo "the check printed no answer"; exit 1 ;;
esac
