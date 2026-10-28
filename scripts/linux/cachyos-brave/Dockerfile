# CachyOS with Brave and dm's released AppImage, to reproduce what a person
# on that system saw: "dm could not get the video from this page".
#
#   sh scripts/linux/cachyos-brave/run.sh
#
# CachyOS's own image (x86-64 only; on an Apple Silicon Mac it runs under
# emulation), Brave as Brave publishes it, and dm's AppImage unpacked (its
# runtime is a static executable that emulation cannot start; the program
# inside is the same). Versions are build arguments: the release under test.
ARG BRAVE=1.98.62
ARG DM=0.8.3
FROM --platform=linux/amd64 cachyos/cachyos:latest
ARG BRAVE
ARG DM

# pacman's own sandbox and free-space check do not work under this emulation.
RUN sed -i '/^CheckSpace/d; s/^\[options\]/[options]\nDisableSandbox/' /etc/pacman.conf \
 && pacman -Sy --noconfirm --needed python xorg-server-xvfb mesa gtk3 libxkbcommon-x11 libxcb \
      xcb-util-cursor fontconfig libx11 libxcursor libxrandr libxi libxinerama libglvnd wayland \
      curl unzip squashfs-tools nss alsa-lib at-spi2-core libcups libdrm pango cairo libxcomposite \
      libxdamage libxfixes libxext libxshmfence dbus \
 && pacman -Scc --noconfirm

# Brave, as Brave publishes it (the build the AUR's brave-bin repackages).
RUN curl -fsSL -o /tmp/brave.zip https://github.com/brave/brave-browser/releases/download/v${BRAVE}/brave-browser-${BRAVE}-linux-amd64.zip \
 && mkdir -p /opt/brave.com/brave && unzip -q /tmp/brave.zip -d /opt/brave.com/brave && rm /tmp/brave.zip \
 && chmod -R a+rX /opt/brave.com/brave \
 && chmod a+x /opt/brave.com/brave/brave /opt/brave.com/brave/chrome_crashpad_handler \
 && ln -s /opt/brave.com/brave/brave /usr/bin/brave-browser

# dm 0.8.3, the AppImage that was released, unpacked: its squashfs starts after the
# runtime's ELF (end of the section headers), and the runtime itself is a static
# executable this emulation cannot start.
# A build not yet released is handed in as local/dm.AppImage (see run.sh).
COPY local/ /opt/local/
RUN if [ -f /opt/local/dm.AppImage ]; then cp /opt/local/dm.AppImage /opt/dm.AppImage; \
    else curl -fsSL -o /opt/dm.AppImage https://github.com/pranavo72bex/Easy-Download-Manager/releases/download/desktop-v${DM}/dm_${DM}_x86_64.AppImage; fi \
 && python3 -c "import struct,subprocess; d=open('/opt/dm.AppImage','rb').read(64); shoff=struct.unpack_from('<Q',d,0x28)[0]; shentsize,shnum=struct.unpack_from('<HH',d,0x3A); subprocess.check_call(['unsquashfs','-f','-o',str(shoff+shentsize*shnum),'-d','/opt/dm','/opt/dm.AppImage'])" \
 && rm /opt/dm.AppImage && ls /opt/dm
