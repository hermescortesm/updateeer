#!/bin/sh
# Builds an .rpm from the release binary and leaves it in dist/.
#   tools/package-rpm.sh              ->  dist/Updateeer-<version>-linux-<arch>.rpm
# Run `cargo build --release --locked` first. Needs rpmbuild.
set -eu
cd "$(dirname "$0")/.."

repo=$PWD
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
top=$(mktemp -d)
trap 'rm -rf "$top"' EXIT
mkdir -p "$top/SPECS"

cat > "$top/SPECS/updateeer.spec" <<SPEC
%global debug_package %{nil}
%define __os_install_post %{nil}
Name: updateeer
Version: $version
Release: 1
Summary: Keeps your installed apps up to date
License: MIT
URL: https://github.com/hermescortesm/updateeer
Requires: mesa-libGL, libxkbcommon, polkit

%description
Detects outdated apps from apt, pacman, dnf, Flatpak and Snap and updates
them from one queue.

%install
install -Dm755 $repo/target/release/updateeer %{buildroot}/usr/bin/updateeer
install -Dm644 $repo/packaging/linux/updateeer.desktop %{buildroot}/usr/share/applications/updateeer.desktop
install -Dm644 $repo/packaging/linux/mx.hermescortes.updateeer.policy %{buildroot}/usr/share/polkit-1/actions/mx.hermescortes.updateeer.policy
install -Dm644 $repo/LICENSE %{buildroot}/usr/share/licenses/updateeer/LICENSE
install -Dm644 $repo/assets/icon.png %{buildroot}/usr/share/icons/hicolor/256x256/apps/updateeer.png

%files
/usr/bin/updateeer
/usr/share/applications/updateeer.desktop
/usr/share/polkit-1/actions/mx.hermescortes.updateeer.policy
/usr/share/licenses/updateeer/LICENSE
/usr/share/icons/hicolor/256x256/apps/updateeer.png
SPEC

rpmbuild -bb --define "_topdir $top" "$top/SPECS/updateeer.spec" >&2
mkdir -p dist
arch=$(uname -m)
cp "$top/RPMS/$arch/updateeer-$version-1.$arch.rpm" "dist/Updateeer-$version-linux-$arch.rpm"
