#!/bin/sh
# Package the staged frontend with the installed GNOME platform and local OSTree.
set -eu
stage=$1
out=$2
app_id=org.dragondriver.DragonDriver
# The frontend was built in the Rust container. No writable SDK is needed here.
[ -f "$stage/metadata" ] || flatpak build-init "$stage" "$app_id" org.gnome.Platform org.gnome.Platform 49
flatpak build-finish --command=dragon-driver --share=ipc \
    --socket=wayland --socket=fallback-x11 --device=dri \
    --filesystem=xdg-data/themes:ro --filesystem='~/.themes:ro' \
    --talk-name=org.freedesktop.Flatpak --filesystem=xdg-run/dragon-driver:create "$stage"
flatpak build-export --disable-sandbox --no-update-summary "$out/repo" "$stage" stable
flatpak build-bundle --runtime-repo=https://flathub.org/repo/flathub.flatpakrepo \
    "$out/repo" "$out/dragon-driver-x86_64.flatpak" "$app_id" stable
