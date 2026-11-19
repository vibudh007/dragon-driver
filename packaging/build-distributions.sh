#!/bin/sh
set -eu
root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
cd -- "$root"
test "$(uname -m)" = x86_64
out="$root/target/dist"
tools="$root/target/package-tools"
test -x "$tools/appimagetool"
test -f "$tools/runtime-x86_64"
install -d -- "$out"
work=$(mktemp -d "$out/.build.XXXXXX")
trap 'rm -rf -- "$work"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
flatpak build-init "$work/flatpak" org.dragondriver.DragonDriver org.gnome.Platform org.gnome.Platform 49
relative=${work#"$root/"}
podman run --rm --pull=never --network=none --userns=keep-id --security-opt label=disable \
    -v "$root:/app:rw" -e CARGO_HOME=/app/target/cargo-home -e CARGO_TARGET_DIR=/app/target/container \
    localhost/dragon-driver-dev:rust-1.90 \
    sh -ec 'cargo build --release --locked --offline --bins; packaging/stage-distribution.sh target/container/release "$1"' sh "$relative"
# The GTK 3 Flatpak extension does not include these GTK 4 stylesheets.
license=/usr/share/licenses/adw-gtk3-theme/LICENSE
if [ -f "$license" ]; then
    for theme in adw-gtk3 adw-gtk3-dark; do
        source=/usr/share/themes/$theme/gtk-4.0
        if [ -f "$source/gtk.css" ]; then
            install -d "$work/flatpak/files/share/themes/$theme"
            cp -RL -- "$source" "$work/flatpak/files/share/themes/$theme/"
            install -D -m 0644 -- "$license" "$work/flatpak/files/share/licenses/adw-gtk3-theme/LICENSE"
        fi
    done
fi
ARCH=x86_64 "$tools/appimagetool" --appimage-extract-and-run --no-appstream \
    --runtime-file "$tools/runtime-x86_64" "$work/AppDir" "$work/Dragon-Driver-x86_64.AppImage"
packaging/flatpak/build-bundle.sh "$work/flatpak" "$work"
tar -C "$work" -czf "$work/dragon-driver-host-x86_64.tar.gz" host
for name in Dragon-Driver-x86_64.AppImage dragon-driver-x86_64.flatpak dragon-driver-host-x86_64.tar.gz; do
    mv -f -- "$work/$name" "$out/$name"
done
( cd -- "$out"; sha256sum Dragon-Driver-x86_64.AppImage dragon-driver-x86_64.flatpak dragon-driver-host-x86_64.tar.gz > SHA256SUMS )
