#!/bin/sh
# Builds a .deb from the release binary and leaves it in dist/.
#   tools/package-deb.sh              ->  dist/Updateeer-<version>-linux-<arch>.deb
# Run `cargo build --release --locked` first. Needs dpkg-deb.
set -eu
cd "$(dirname "$0")/.."

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
arch=$(dpkg --print-architecture)
root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT

install -Dm755 target/release/updateeer "$root/usr/bin/updateeer"
install -Dm644 packaging/linux/updateeer.desktop "$root/usr/share/applications/updateeer.desktop"
install -Dm644 assets/icon.png "$root/usr/share/icons/hicolor/256x256/apps/updateeer.png"
mkdir -p "$root/DEBIAN"
cat > "$root/DEBIAN/control" <<CONTROL
Package: updateeer
Version: $version
Section: utils
Priority: optional
Architecture: $arch
Maintainer: Hermes Cortes <admin@hermescortes.com>
Depends: libc6, libgl1 | libegl1, libxkbcommon0, policykit-1 | pkexec
Recommends: libwayland-client0, libx11-6
Description: Keeps your installed apps up to date
 Detects outdated apps from apt, pacman, dnf, Flatpak and Snap and updates
 them from one queue.
CONTROL

mkdir -p dist
dpkg-deb --root-owner-group --build "$root" "dist/Updateeer-$version-linux-$arch.deb"
