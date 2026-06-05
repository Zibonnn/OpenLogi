#!/usr/bin/env bash
#
# Cargo `runner` for macOS — wired in `.cargo/config.toml`.
#
# Cargo hands this script the freshly built binary as $1 for every
# `cargo run` / `cargo test` / `cargo bench` on macOS. For everything except
# the desktop binary it's a transparent passthrough (`exec "$@"`).
#
# For `openlogi-gui` it launches the build from inside a throwaway
# `OpenLogi.app` so macOS shows the real app name (the bold menu-bar title)
# and the Dock icon during development. Both are read from the bundle's
# `Info.plist` / `Resources` — a bare `target/debug/openlogi-gui` has neither,
# so macOS falls back to the executable name and a generic icon.
#
# Set OPENLOGI_DEV_BUNDLE=0 to skip the wrapper and run the raw binary.
set -euo pipefail

bin="$1"
shift

if [ "${bin##*/}" != "openlogi-gui" ] || [ "${OPENLOGI_DEV_BUNDLE:-1}" = "0" ]; then
  exec "$bin" "$@"
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP="$ROOT/target/dev/OpenLogi.app"
MACOS="$APP/Contents/MacOS"
RES="$APP/Contents/Resources"
ICON_SRC="$ROOT/crates/openlogi-gui/icon/AppIcon.icns"
PLIST_SRC="$ROOT/crates/openlogi-gui/dev/Info.plist"

mkdir -p "$MACOS" "$RES"

# Info.plist — minimal, dev-only. A distinct `.dev` identifier keeps this
# target artifact from registering as the production app in LaunchServices.
PLIST="$APP/Contents/Info.plist"
if [ ! -f "$PLIST" ] || [ "$PLIST_SRC" -nt "$PLIST" ]; then
  cp -f "$PLIST_SRC" "$PLIST"
fi

# App icon — generated from the master SVG on demand. Mirror into the bundle
# when missing or changed, then re-register so Dock/LaunchServices pick it up
# (macOS aggressively caches icons by bundle id).
if [ ! -f "$ICON_SRC" ]; then
  cargo run -p xtask --manifest-path "$ROOT/Cargo.toml" -- macos-icns
fi
ICON_STAMP="$RES/.AppIcon.md5"
ICON_MD5="$(md5 -q "$ICON_SRC")"
if [ ! -f "$RES/AppIcon.icns" ] || [ "$(cat "$ICON_STAMP" 2>/dev/null || true)" != "$ICON_MD5" ]; then
  cp -f "$ICON_SRC" "$RES/AppIcon.icns"
  echo "$ICON_MD5" > "$ICON_STAMP"
  # Bust LaunchServices / Dock icon cache for the dev bundle.
  /usr/libexec/PlistBuddy -c "Set :CFBundleVersion $(date +%s)" "$PLIST"
  touch "$APP"
  LSREGISTER="/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"
  if [ -x "$LSREGISTER" ]; then
    "$LSREGISTER" -f -R -trusted "$APP" >/dev/null 2>&1 || true
  fi
fi

# Hardlink the freshly built binary into the bundle — instant, no 95 MB copy.
# A hardlink (not a symlink) is required: both NSBundle.mainBundle and Rust's
# current_exe() realpath() the executable, which would resolve a symlink back
# to target/debug/ and break the bundle association. cargo rewrites the binary
# atomically on rebuild (new inode), so relink every run; `ln -f` repoints a
# stale link. Fall back to a copy if the bundle ever lands on another volume.
ln -f "$bin" "$MACOS/openlogi-gui" 2>/dev/null || cp -f "$bin" "$MACOS/openlogi-gui"

exec "$MACOS/openlogi-gui" "$@"
