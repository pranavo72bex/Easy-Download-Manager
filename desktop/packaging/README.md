# Installing dm on Linux

Two routes. The Arch package is built, checked and installed with pacman in
Arch's own container (`scripts/arch-check.sh`, arm64 and emulated x86_64);
neither route has been run on a real Arch machine.

## Arch (native package)

```sh
cd desktop/packaging
makepkg -si
```

Builds from source against the libraries Arch actually ships. The `.deb`
and AppImage from CI are built on Ubuntu 22.04 for portability; a package
built here links against Arch's own.

Runtime dependencies are what the window opens at run time: X11
(`libx11`, `libxcursor`, `libxi`, `libxkbcommon-x11`), Wayland, OpenGL
(`libglvnd`) and `hicolor-icon-theme`. No GTK and no web view: the window
is drawn in Rust, which is why the binary is about 8 MB rather than the
150 MB an Electron build would be.

`yt-dlp` is an optional dependency. dm downloads files itself; yt-dlp is only
consulted for sites that serve video through their own player.

## Anything else (AppImage)

The tagged release carries an AppImage, which needs nothing installed:

```sh
chmod +x dm-*.AppImage && ./dm-*.AppImage
```

It is built on Ubuntu 22.04 deliberately — an AppImage built against a newer
glibc will not start on an older distribution, so the oldest available runner
produces the most portable binary.

## Building it yourself

```sh
cd desktop
cargo build --release -p dm-desktop      # ~8 MB binary in target/release
cargo test                               # the workspace's tests, no display needed
```

The engine (`crates/dm-core`) has no idea a window exists and is tested
without one, which is why the test suite runs headless in a chroot.
