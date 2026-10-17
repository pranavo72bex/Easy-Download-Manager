#!/bin/sh
# The .rpm, built and then installed the way a person would on Fedora and
# openSUSE, from any machine with Docker:
#
#   sh scripts/rpm-check.sh
#
# Builds the release binary and the .rpm in the Debian image linux-check.sh
# uses, then for each distribution installs it with its own package manager
# (which must resolve the libraries the package names), runs
# scripts/smoke.py against the installed /usr/bin/dm under a virtual display
# (so the window must open with only what the package pulled in), and
# removes it again.
# Streamed in, not mounted, for the reason linux-check.sh gives.
set -e
cd "$(dirname "$0")/.."
version=$(sed -n 's/^version = "\(.*\)"/\1/p' desktop/crates/dm-desktop/Cargo.toml | head -1)
docker build -q -t dm-linux-check scripts/linux >/dev/null
stream() { git ls-files -z --cached --others --exclude-standard | tar --null -T - -cf -; }

echo "== building dm $version and its .rpm"
stream | docker run --rm -i -v dm-linux-target:/target \
  -v "$HOME/.cargo/registry":/usr/local/cargo/registry dm-linux-check sh -c "
    set -e; mkdir -p /work && cd /work && tar -xf -
    cd desktop && cargo build -q --release -p dm-desktop && cd ..
    rm -rf /target/rpm
    NFPM_DIR=/target sh scripts/linux/nfpm.sh /target/release/dm-desktop $version /target/rpm"

# DM_RPM_DISTROS picks which, e.g. to retry one after a mirror failed.
for distro in ${DM_RPM_DISTROS:-fedora:41 opensuse/leap:15.6}; do
  echo
  echo "== $distro: install, smoke, remove"
  case $distro in
    fedora*) first=true
             install="dnf install -y -q python3 xorg-x11-server-Xvfb /target/rpm/*.rpm"; remove="dnf remove -y -q dm" ;;
    # Leap's image has no tar, which the source arrives through.
    opensuse*) first="zypper -q --non-interactive install -y tar"
               install="zypper -q --non-interactive install -y --allow-unsigned-rpm python3 xvfb-run /target/rpm/*.rpm"
               remove="zypper -q --non-interactive remove -y dm" ;;
  esac
  stream | docker run --rm -i -v dm-linux-target:/target "$distro" sh -c "
    set -e; $first >/dev/null; mkdir -p /work && cd /work && tar -xf -
    $install >/dev/null
    rpm -q dm
    # 24-bit: openSUSE's xvfb-run defaults to an 8-bit screen, which has no
    # GL config a window can use.
    xvfb-run -a -s '-screen 0 1280x800x24' python3 scripts/smoke.py /usr/bin/dm
    $remove >/dev/null
    if [ -e /usr/bin/dm ]; then echo 'FAIL  removing the package left /usr/bin/dm'; exit 1; fi
    echo 'ok    removed cleanly'"
done
