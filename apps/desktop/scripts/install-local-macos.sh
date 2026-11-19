#!/usr/bin/env bash
# Build Timbre in release mode and install it to ~/Applications, where
# Spotlight and Launchpad find it. Run from anywhere:
#
#   pnpm -F @anlg/desktop install:local
#
# Skip the build and reinstall the last one with SKIP_BUILD=1.
set -euo pipefail

APP_NAME="Timbre"
DESKTOP_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/anarlog-target}"
BUILT_APP="$TARGET_DIR/release/bundle/macos/$APP_NAME.app"
INSTALL_DIR="$HOME/Applications"
INSTALLED_APP="$INSTALL_DIR/$APP_NAME.app"

if [[ "$(uname)" != "Darwin" ]]; then
  echo "This script installs the macOS app only." >&2
  exit 1
fi

if pgrep -f "$INSTALLED_APP/Contents/MacOS/" >/dev/null; then
  echo "$APP_NAME is running. Quit it (Cmd+Q) and run this again." >&2
  exit 1
fi

if [[ "${SKIP_BUILD:-0}" != "1" ]]; then
  [[ -f "$HOME/.cargo/env" ]] && source "$HOME/.cargo/env"
  export CARGO_TARGET_DIR="$TARGET_DIR"
  # macOS 27 dyld rejects proc-macro dylibs stripped by Rust < 1.98
  # (rust-lang/rust#157750); release builds strip them unless told not to.
  export CARGO_PROFILE_RELEASE_BUILD_OVERRIDE_STRIP=false
  # Release builds need an API base URL at compile time. This fork has no
  # hosted backend, so use the same local default as debug builds.
  export VITE_API_URL="${VITE_API_URL:-http://localhost:3001}"

  cd "$DESKTOP_DIR"
  # The bundle step ends with an updater-signing error when no
  # TAURI_SIGNING_PRIVATE_KEY is set. The app itself is already built and
  # signed by then, so judge success by the bundle, not the exit code.
  pnpm tauri build --features dev --bundles app || true
fi

if [[ ! -d "$BUILT_APP" ]]; then
  echo "No build found at $BUILT_APP" >&2
  exit 1
fi
codesign --verify --deep --strict "$BUILT_APP"

mkdir -p "$INSTALL_DIR"
rm -rf "$INSTALLED_APP"
ditto "$BUILT_APP" "$INSTALLED_APP"

# Tell Launch Services and Spotlight about the new copy right away.
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$INSTALLED_APP"
mdimport "$INSTALLED_APP" 2>/dev/null || true

echo "Installed $INSTALLED_APP"
echo "Open it from Spotlight (Cmd+Space, type \"$APP_NAME\") or Launchpad."
