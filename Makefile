# dm — clone it and install it.
#
#     ./edm --install        (or: make install)
#
# What this can and cannot do, said plainly, because the difference matters:
#
#   The desktop client is a program. `make install` builds it and puts it
#   where your system looks for programs. That part is genuinely one command.
#
#   The browser extension is not installable from a command line. Chrome and
#   Firefox both refuse to load an extension a program put somewhere — that
#   is a deliberate security boundary, and developer mode does not lift it.
#   It exists so that no installer, however well meant, can add an extension
#   that reads every page you visit. So `make install` stages the extension,
#   opens the page you load it from, and tells you the two clicks. It does
#   not pretend the clicks did not happen.
#
#   Between them is the native messaging host, which lets the extension hand
#   downloads to the client: `make register`, which runs the installed
#   client's own `--register`. The unpacked extension's id is fixed by the key
#   in its manifest, so no id has to be read off a page; a store build's is
#   named with EXT_ID=….

SHELL := /bin/sh
.DEFAULT_GOAL := help
.DELETE_ON_ERROR:

# rustup puts cargo here and adds it to the *shell profile*, which make does
# not read. Someone who has just installed Rust and not opened a new terminal
# would otherwise be told they have no cargo.
export PATH := $(HOME)/.cargo/bin:$(PATH)

UNAME := $(shell uname -s)
VERSION := $(shell sed -n 's/.*"version": "\([^"]*\)".*/\1/p' extension/manifest.json | head -1)

# Where the client goes. Override for a system-wide install:
#     sudo make install PREFIX=/usr/local
PREFIX ?= $(HOME)/.local
BINDIR := $(PREFIX)/bin

# On macOS the app bundle is the thing people expect; the bare binary works
# but has no icon and no place in the Applications folder.
APPDIR ?= /Applications

BINARY := desktop/target/release/dm-desktop
# The extension, built: dist/chrome and dist/firefox, loadable as they are,
# and the store zips beside them.
STAGED := dist

# Where each browser family keeps native messaging manifests, so `doctor` can
# say whether the bridge is in place. Missing directories are fine — `find`
# is told to be quiet about them.
ifeq ($(UNAME),Darwin)
HOST_DIRS := $(HOME)/Library/Application\ Support/Mozilla/NativeMessagingHosts \
             $(HOME)/Library/Application\ Support/Google/Chrome/NativeMessagingHosts \
             $(HOME)/Library/Application\ Support/BraveSoftware/Brave-Browser/NativeMessagingHosts
else
HOST_DIRS := $(HOME)/.mozilla/native-messaging-hosts \
             $(HOME)/.config/google-chrome/NativeMessagingHosts \
             $(HOME)/.config/chromium/NativeMessagingHosts
endif

# How to open a page and how to put text on the clipboard, if this machine
# can. Both are best-effort: a headless box has neither, and `make install`
# must not fail over a convenience.
ifeq ($(UNAME),Darwin)
OPEN := open -a "Google Chrome"
COPY := sh -c 'printf "%s" "$$1" | pbcopy' _
else
OPEN := xdg-open
COPY := sh -c 'printf "%s" "$$1" | { command -v wl-copy >/dev/null && wl-copy || command -v xclip >/dev/null && xclip -selection clipboard; }' _
endif

# Colours, but only when something is there to read them.
BOLD := $(shell test -t 1 && tput bold 2>/dev/null)
DIM  := $(shell test -t 1 && tput dim 2>/dev/null)
OFF  := $(shell test -t 1 && tput sgr0 2>/dev/null)

.PHONY: help install client extension register unregister uninstall test doctor app clean

help:
	@echo "$(BOLD)dm$(OFF) — a download manager"
	@echo
	@echo "  $(BOLD)make install$(OFF)      build and install the client, stage the extension"
	@echo "  $(BOLD)make register$(OFF)     let the extension hand downloads to it"
	@echo
	@echo "  make client        just the desktop client"
	@echo "  make extension     just build the extension (Rust to WebAssembly)"
	@echo "  make app           installers for this machine (needs cargo-packager)"
	@echo "  make test          the whole suite"
	@echo "  make doctor        what is installed and what is missing"
	@echo "  make uninstall     remove all of it"
	@echo
	@echo "$(DIM)  PREFIX=$(PREFIX)   (override to install elsewhere)$(OFF)"

# --------------------------------------------------------------- checking

# Named tools, checked once and reported together, because being told about
# one missing thing at a time is how a five-minute install takes an hour.
doctor:
	@echo "$(BOLD)what dm needs$(OFF)"
	@missing=0; \
	for tool in cargo rustc rustup; do \
	  if command -v $$tool >/dev/null 2>&1; then \
	    printf '  %-8s %s\n' "$$tool" "$$(command -v $$tool)"; \
	  else \
	    printf '  %-8s $(BOLD)missing$(OFF)\n' "$$tool"; missing=1; \
	  fi; \
	done; \
	if [ "$(UNAME)" = "Linux" ]; then \
	  if pkg-config --exists gtk+-3.0 2>/dev/null; then \
	    printf '  %-8s %s\n' gtk3 "$$(pkg-config --modversion gtk+-3.0)"; \
	  else \
	    printf '  %-8s $(BOLD)missing$(OFF) — the tray will not link without it\n' gtk3; \
	    missing=1; \
	  fi; \
	fi; \
	echo; \
	echo "$(BOLD)what is installed$(OFF)"; \
	if [ -x "$(BINDIR)/dm" ]; then printf '  client   %s\n' "$(BINDIR)/dm"; \
	elif [ -d "$(APPDIR)/dm.app" ]; then printf '  client   %s\n' "$(APPDIR)/dm.app"; \
	else printf '  client   not installed\n'; fi; \
	if [ -f "$(STAGED)/chrome/manifest.json" ]; then printf '  extension built at %s\n' "$(STAGED)/chrome, $(STAGED)/firefox"; \
	else printf '  extension not built (make extension)\n'; fi; \
	if rustup target list --installed 2>/dev/null | grep -qx wasm32-unknown-unknown; then \
	  printf '  %-8s present\n' wasm32; \
	else printf '  %-8s not yet — make extension adds it\n' wasm32; fi; \
	if command -v wasm-bindgen >/dev/null 2>&1; then \
	  printf '  %-8s %s\n' wasm-bindgen "$$(wasm-bindgen --version)"; \
	else printf '  %-8s not yet — make extension installs the version it needs\n' wasm-bindgen; fi; \
	if [ -n "$$(find $(HOST_DIRS) -name com.dm.engine.json 2>/dev/null)" ]; then \
	  find $(HOST_DIRS) -name com.dm.engine.json 2>/dev/null \
	    | sed 's|^|  host     |'; \
	else printf '  host     not registered with any browser\n'; fi; \
	if [ $$missing -ne 0 ]; then \
	  echo; \
	  echo "Install what is missing above, then run make install again."; \
	  if [ "$(UNAME)" = "Linux" ]; then \
	    echo "  Arch:   sudo pacman -S rustup gtk3"; \
	    echo "  Debian: sudo apt install libgtk-3-dev"; \
	    echo "          and rustup from https://rustup.rs"; \
	  else \
	    echo "  rustup: https://rustup.rs"; \
	  fi; \
	  exit 1; \
	fi

# ----------------------------------------------------------------- client

$(BINARY):
	@echo "$(BOLD)building the client$(OFF) — a few minutes the first time"
	@cd desktop && cargo build --release -p dm-desktop

client: doctor $(BINARY)
	@install -d "$(BINDIR)"
	@install -m 755 "$(BINARY)" "$(BINDIR)/dm"
	@echo "installed $(BINDIR)/dm"
ifeq ($(UNAME),Linux)
	@install -d "$(PREFIX)/share/applications" "$(PREFIX)/share/icons/hicolor/128x128/apps"
	@sed 's|^Exec=.*|Exec=$(BINDIR)/dm %u|' desktop/packaging/dm.desktop \
	  > "$(PREFIX)/share/applications/dm.desktop"
	@install -m 644 desktop/crates/dm-desktop/icons/128x128.png \
	  "$(PREFIX)/share/icons/hicolor/128x128/apps/dm.png"
	@echo "installed a menu entry and an icon"
endif
	@case ":$$PATH:" in *":$(BINDIR):"*) ;; \
	  *) echo; echo "$(BOLD)note$(OFF) $(BINDIR) is not on your PATH."; \
	     echo "     add it, or run dm as $(BINDIR)/dm";; esac

# The macOS bundle, which is what people expect to drag to Applications.
# Kept separate because it needs a tool that takes a while to install.
# CI=true skips the Finder AppleScript that lays out the .dmg window, which
# fails from a shell that may not script Finder.
app:
	@command -v cargo-packager >/dev/null 2>&1 || { \
	  echo "needs cargo-packager:  cargo install cargo-packager --locked"; exit 1; }
	@cd desktop/crates/dm-desktop && CI=true cargo packager --release
	@echo
	@echo "built desktop/target/release/bundle/"
ifeq ($(UNAME),Darwin)
	@rm -rf "$(APPDIR)/dm.app"
	@cp -R desktop/target/release/bundle/dm.app "$(APPDIR)/dm.app"
	@echo "installed $(APPDIR)/dm.app"
	@echo "$(DIM)unsigned, so the first launch is right-click → Open$(OFF)"
endif

# -------------------------------------------------------------- extension

# Built rather than copied: the extension is Rust compiled to WebAssembly
# (browser/), with a loader and two manifests (extension/). package.sh adds
# the wasm32 target and the matching wasm-bindgen CLI if they are missing.
#
# Loading it is two clicks no Makefile can make: Chrome and Firefox refuse an
# extension a program put somewhere, deliberately. So the page it is loaded
# from is opened and the folder put on the clipboard. Chrome used to accept
# --load-extension for this and dropped it in 137. One click is possible only
# for an extension on the Web Store, whose id an installer can name in a
# policy; dm's is not there yet.
extension:
	@sh scripts/package.sh
	@$(COPY) "$(CURDIR)/$(STAGED)/chrome" && echo "the folder is on your clipboard" || true
	@$(OPEN) "chrome://extensions" 2>/dev/null \
	  && echo "opened chrome://extensions — turn on Developer mode, Load unpacked, paste" \
	  || true

# ---------------------------------------------------- the bridge between

# The client registers itself: the installed copy if there is one, since a
# browser must be allowed to run the path it is given and macOS will not let
# one run anything under ~/Downloads, ~/Desktop or ~/Documents (the client
# refuses such a path rather than write a manifest that cannot work).
# EXT_ID adds a store build's id; the unpacked Chrome build and Firefox's
# need none.
REGISTRAR = $(firstword $(wildcard $(BINDIR)/dm $(APPDIR)/dm.app/Contents/MacOS/dm-desktop) $(BINARY))

register:
	@test -x "$(REGISTRAR)" || { \
	  echo "no client to register — make client (or make app) first"; exit 1; }
	@"$(REGISTRAR)" --register $(EXT_ID)

unregister:
	@if [ -x "$(REGISTRAR)" ]; then "$(REGISTRAR)" --unregister; \
	else find $(HOST_DIRS) -name com.dm.engine.json -exec rm -v {} + 2>/dev/null || true; fi

# -------------------------------------------------------------- everything

install: client extension
	@echo
	@echo "$(BOLD)the client is installed.$(OFF)"
	@echo
	@echo "$(BOLD)the extension is two clicks$(OFF), because a browser will not let a"
	@echo "program install one — that boundary is why an installer cannot add"
	@echo "something that reads every page you visit."
	@echo
	@echo "  $(BOLD)Chrome, Edge, Brave, Vivaldi$(OFF)"
	@echo "    1. on chrome://extensions (opened for you) turn on Developer mode"
	@echo "    2. Load unpacked → paste the folder (it is on your clipboard):"
	@echo "       $(CURDIR)/$(STAGED)/chrome"
	@echo
	@echo "  $(BOLD)Firefox$(OFF)"
	@echo "    1. open about:debugging#/runtime/this-firefox"
	@echo "    2. Load Temporary Add-on → $(CURDIR)/$(STAGED)/firefox/manifest.json"
	@echo "    $(DIM)temporary is Firefox's rule for unsigned add-ons; it goes on close$(OFF)"
	@echo
	@"$(REGISTRAR)" --register $(EXT_ID) || { \
	  echo; echo "the browsers could not be told where dm is (above); fix that and run make register"; }

uninstall: unregister
	@rm -f "$(BINDIR)/dm"
	@rm -f "$(PREFIX)/share/applications/dm.desktop"
	@rm -f "$(PREFIX)/share/icons/hicolor/128x128/apps/dm.png"
	@rm -rf "$(APPDIR)/dm.app"
	@rm -rf "$(STAGED)" extension/pkg
	@echo "removed the client, the built extension and the host registration"
	@echo "$(DIM)settings and the queue are left in place; delete them yourself if"
	@echo "you mean it:$(OFF)"
ifeq ($(UNAME),Darwin)
	@echo "  rm -rf ~/Library/Application\\ Support/dm-engine"
else
	@echo "  rm -rf ~/.local/share/dm-engine"
endif

test:
	@cd browser && cargo test
	@cd desktop && cargo test --workspace

clean:
	@cd desktop && cargo clean
	@cd browser && cargo clean
	@rm -rf dist extension/pkg
	@echo "removed build output"
