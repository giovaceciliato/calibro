#!/bin/sh
# Builds the Linux packages from a release binary:
#   dist-linux/calibro-<ver>-1.x86_64.rpm      (Fedora 40+, openSUSE)
#   dist-linux/calibro-<ver>-linux-x86_64.tar.gz (portable, user install via install.sh)
# Usage: packaging/build-linux-packages.sh [path/to/calibro-binary]
# Needs: rpmbuild (Fedora: `sudo dnf install rpm-build`; Ubuntu: `sudo apt install rpm`).
set -e
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="${1:-$ROOT/target/release/calibro}"
VER="$(grep -m1 '^version' "$ROOT/Cargo.toml" | cut -d'"' -f2)"
OUT="$ROOT/dist-linux"
L="$ROOT/packaging/linux"
[ -x "$BIN" ] || { echo "binary not found: $BIN (run: cargo build --release -p calibro)"; exit 1; }
rm -rf "$OUT" && mkdir -p "$OUT"

# --- RPM ---
TOP="$(mktemp -d)"
mkdir -p "$TOP/SOURCES"
cp "$BIN" "$TOP/SOURCES/calibro"
cp "$L"/io.github.giovaceciliato.Calibro.* "$L"/calibro-*.png "$ROOT/LICENSE" "$ROOT/README.md" "$TOP/SOURCES/"
rpmbuild -bb --quiet \
  --define "_topdir $TOP" --define "calibro_version $VER" \
  --define "dist %{nil}" --define "_build_id_links none" \
  "$ROOT/packaging/rpm/calibro.spec"
cp "$TOP"/RPMS/x86_64/*.rpm "$OUT/"
rm -rf "$TOP"

# --- portable tarball ---
P="calibro-$VER-linux-x86_64"
mkdir -p "$OUT/$P/share"
cp "$BIN" "$OUT/$P/calibro"
cp "$L/install.sh" "$OUT/$P/"
cp "$L"/io.github.giovaceciliato.Calibro.desktop "$L"/io.github.giovaceciliato.Calibro.xml "$L"/calibro-*.png "$OUT/$P/share/"
cp "$ROOT/LICENSE" "$ROOT/README.md" "$OUT/$P/"
tar -C "$OUT" -czf "$OUT/$P.tar.gz" "$P"
rm -rf "$OUT/$P"
ls -la "$OUT"
