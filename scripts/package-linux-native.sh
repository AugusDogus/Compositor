#!/usr/bin/env bash
# Repackage the validated AppImage payload so codecs and offline models stay identical.
set -euo pipefail
project_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$project_root"
for command in dpkg-deb rpmbuild jq cargo; do
    command -v "$command" >/dev/null || { printf 'Install %s and retry.\n' "$command" >&2; exit 1; }
done
version="$(cargo metadata --locked --no-deps --format-version 1 | jq -r '.packages[] | select(.name == "compositor") | .version')"
# Build metadata is not a package-manager version. Rust release tags are checked separately in CI.
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || { printf 'Unsupported package version: %s\n' "$version" >&2; exit 1; }
package_version="${version//-/~}"
appimage="${1:-dist/Compositor-$version-x86_64.AppImage}"
appimage="$(realpath -- "$appimage")"
[[ -x "$appimage" && "$(basename "$appimage")" == "Compositor-$version-x86_64.AppImage" ]] || { printf 'Build the matching x86_64 AppImage first.\n' >&2; exit 1; }
stage_dir="$(mktemp -d)"
trap 'rm -r -- "$stage_dir"' EXIT
(cd "$stage_dir" && "$appimage" --appimage-extract >/dev/null)
bundle="$stage_dir/root/opt/compositor"
mkdir -p "$(dirname "$bundle")" dist
mv "$stage_dir/squashfs-root" "$bundle"
test -x "$bundle/usr/bin/compositor"
test -f "$bundle/usr/share/compositor/inference/object-selection/model-manifest.json"
touch "$bundle/usr/share/compositor/system-package"
install -Dm755 packaging/linux/native-launcher "$stage_dir/root/usr/bin/compositor"
for file in applications/compositor.desktop icons/hicolor/256x256/apps/compositor.png metainfo/io.github.AugusDogus.Compositor.metainfo.xml; do
    install -Dm644 "$bundle/usr/share/$file" "$stage_dir/root/usr/share/$file"
done
install -Dm644 LICENSE "$stage_dir/root/usr/share/doc/compositor/copyright"

mkdir -p "$stage_dir/root/DEBIAN"
cat > "$stage_dir/root/DEBIAN/control" <<EOF
Package: compositor
Version: $package_version
Architecture: amd64
Maintainer: AugusDogus <AugusDogus@users.noreply.github.com>
Section: graphics
Priority: optional
Homepage: https://github.com/AugusDogus/Compositor
Depends: libc6 (>= 2.39), libstdc++6, libgcc-s1, libwayland-client0, xdg-desktop-portal
Recommends: xdg-desktop-portal-gtk | xdg-desktop-portal-kde | xdg-desktop-portal-gnome
Installed-Size: $(du -sk "$stage_dir/root" | cut -f1)
Description: Layered image editor for Linux
 Includes offline background removal and object-selection models.
 Requires working host Vulkan or OpenGL graphics drivers.
EOF
deb="$project_root/dist/compositor_${package_version}_amd64.deb"
dpkg-deb --root-owner-group --threads-max=2 -Zzstd -z10 --build "$stage_dir/root" "$deb"
rm -r -- "$stage_dir/root/DEBIAN"

mkdir -p "$stage_dir/rpm"
cat > "$stage_dir/rpm/compositor.spec" <<EOF
Name: compositor
Version: $package_version
Release: 1
Summary: Layered image editor for Linux
License: MIT AND LicenseRef-Bundled-Dependencies
URL: https://github.com/AugusDogus/Compositor
BuildArch: x86_64
AutoReqProv: no
Requires: glibc >= 2.39
Requires: libstdc++.so.6()(64bit), libgcc_s.so.1()(64bit), libwayland-client.so.0()(64bit), xdg-desktop-portal

%description
Layered image editor with bundled offline background removal and object-selection
models. Requires working host Vulkan or OpenGL graphics drivers.

%install
mkdir -p %{buildroot}
cp -al "$stage_dir/root/." %{buildroot}/

%files
/opt/compositor
/usr/bin/compositor
/usr/share/applications/compositor.desktop
/usr/share/icons/hicolor/256x256/apps/compositor.png
/usr/share/metainfo/io.github.AugusDogus.Compositor.metainfo.xml
%license /usr/share/doc/compositor/copyright
EOF
rpmbuild --define "_topdir $stage_dir/rpm" --define '_build_id_links none' \
    --define '__os_install_post %{nil}' --define '_binary_payload w10T2.zstdio' \
    -bb "$stage_dir/rpm/compositor.spec"
rpm="$project_root/dist/compositor-$package_version-1.x86_64.rpm"
install -m644 "$stage_dir/rpm/RPMS/x86_64/compositor-$package_version-1.x86_64.rpm" "$rpm"
for artifact in "$deb" "$rpm"; do
    if (( $(stat -c %s "$artifact") >= 2147483648 )); then
        printf '%s exceeds the GitHub 2 GiB asset limit.\n' "$artifact" >&2
        exit 1
    fi
    (cd dist && sha256sum "$(basename "$artifact")" > "$(basename "$artifact").sha256")
done
dpkg-deb --info "$deb" >/dev/null
rpm --checksig "$rpm"
