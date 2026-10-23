#!/bin/sh
# Installs Calibro for the current user only (no administrator password needed).
# Works on any Linux, including immutable systems such as Bazzite or Fedora Silverblue.
# Usage:  ./install.sh            install
#         ./install.sh --remove   uninstall
set -e
HERE="$(cd "$(dirname "$0")" && pwd)"
BIN="$HOME/.local/bin"
SHARE="${XDG_DATA_HOME:-$HOME/.local/share}"
ID=io.github.giovaceciliato.Calibro

if [ "$1" = "--remove" ]; then
  rm -f "$BIN/calibro" "$SHARE/applications/$ID.desktop" "$SHARE/mime/packages/$ID.xml"
  for s in 64 128 256; do rm -f "$SHARE/icons/hicolor/${s}x${s}/apps/$ID.png"; done
  update-mime-database "$SHARE/mime" 2>/dev/null || true
  update-desktop-database "$SHARE/applications" 2>/dev/null || true
  echo "Calibro removed."
  exit 0
fi

mkdir -p "$BIN" "$SHARE/applications" "$SHARE/mime/packages"
install -m 0755 "$HERE/calibro" "$BIN/calibro"
sed "s|^Exec=calibro|Exec=$BIN/calibro|" "$HERE/share/$ID.desktop" > "$SHARE/applications/$ID.desktop"
install -m 0644 "$HERE/share/$ID.xml" "$SHARE/mime/packages/$ID.xml"
for s in 64 128 256; do
  mkdir -p "$SHARE/icons/hicolor/${s}x${s}/apps"
  install -m 0644 "$HERE/share/calibro-$s.png" "$SHARE/icons/hicolor/${s}x${s}/apps/$ID.png"
done
update-mime-database "$SHARE/mime" 2>/dev/null || true
update-desktop-database "$SHARE/applications" 2>/dev/null || true
gtk-update-icon-cache -q "$SHARE/icons/hicolor" 2>/dev/null || true
echo "Calibro installed. Find it in your app menu, or run: $BIN/calibro"
