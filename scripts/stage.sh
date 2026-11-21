#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
case "${1:-armhf}" in
    armhf|armv7-unknown-linux-musleabihf) target=armv7-unknown-linux-musleabihf ;;
    *) echo "Unsupported package architecture: $1" >&2; exit 1 ;;
esac

stage=${2:-build/stage}
target_dir=${CARGO_TARGET_DIR:-target}
binary=$target_dir/$target/release/viewer-kindle
[ -x "$binary" ] || { echo "Build $binary before staging." >&2; exit 1; }
file "$binary" | grep -q 'ELF.*ARM' || { echo "Expected an ARM binary." >&2; exit 1; }
[ -r assets/viewer-icon.png ] || { echo "Add the approved Viewer icon before staging." >&2; exit 1; }
[ -r docs/licenses/THIRD-PARTY.txt ] || { echo "Missing Viewer third-party notices." >&2; exit 1; }
icon="# Icon: data:image/png;base64,$(base64 < assets/viewer-icon.png | tr -d '\n')"
icon_dark="# IconDark: data:image/png;base64,$(base64 < assets/viewer-icon-dark.png | tr -d '\n')"

install -d "$stage/viewer/bin" "$stage/viewer/licenses" "$stage/documents"
install -m 755 "$binary" "$stage/viewer/bin/kl-viewer-app"
cp -R docs/licenses/. "$stage/viewer/licenses/"
awk -v icon="$icon" -v icon_dark="$icon_dark" '{ print } /^# Author:/ { print icon; print icon_dark }' deploy/Viewer.sh > "$stage/documents/Viewer.sh"
chmod 755 "$stage/documents/Viewer.sh"
