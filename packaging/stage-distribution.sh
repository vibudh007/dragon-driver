#!/bin/sh
# Run in the project's build container with already-built GTK and daemon binaries.
set -eu
assets=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
bin=$1
out=$2
app_id=org.dragondriver.DragonDriver
install -d -- "$out/host" "$out/AppDir/usr/bin" "$out/AppDir/usr/lib" "$out/flatpak/files/bin"
install -m 0755 -- "$bin/dragon-driver-daemon" "$out/host"
strip --strip-unneeded "$out/host/dragon-driver-daemon"
install -m 0755 -- "$assets/setup-desktop-service.sh" "$out/host"
install -m 0644 -- "$assets/dragon-driver.socket" "$assets/dragon-driver.service" \
    "$assets/dragon-driver-access@.socket" "$assets/dragon-driver-access@.service" "$out/host"
install -m 0755 -- "$assets/appimage/AppRun" "$out/AppDir"
for root in "$out/AppDir/usr" "$out/flatpak/files"; do
    install -m 0755 -- "$bin/dragon-driver" "$root/bin/dragon-driver"
    install -m 0755 -- "$bin/dragonctl" "$root/bin/dragonctl"
    strip --strip-unneeded "$root/bin/dragon-driver"
    strip --strip-unneeded "$root/bin/dragonctl"
    install -D -m 0644 -- "$assets/$app_id.desktop" "$root/share/applications/$app_id.desktop"
    install -D -m 0644 -- "$assets/$app_id.svg" "$root/share/icons/hicolor/scalable/apps/$app_id.svg"
    install -D -m 0644 -- "$assets/$app_id.metainfo.xml" "$root/share/metainfo/$app_id.metainfo.xml"
    install -D -m 0644 -- "$assets/licenses/fast_image_resize.txt" "$root/share/licenses/fast_image_resize/LICENSE-MIT"
done
cp -- "$assets/$app_id.desktop" "$assets/$app_id.png" "$out/AppDir"
ln -sf -- "$app_id.png" "$out/AppDir/.DirIcon"
install -d -- "$out/AppDir/usr/share/dragon-driver/host"
cp -p -- "$out"/host/* "$out/AppDir/usr/share/dragon-driver/host"
install -d -- "$out/flatpak/files/share/dragon-driver/host"
cp -p -- "$out"/host/* "$out/flatpak/files/share/dragon-driver/host"
install -m 0755 -- "$assets/flatpak/dragon-driver-export-host" "$out/flatpak/files/bin/dragon-driver-export-host"
# Keep the system C library and graphics driver stack supplied by the host.
ldd "$bin/dragon-driver" | awk '$2 == "=>" && $3 ~ /^\// { print $1, $3 }' |
while read -r name library; do
    case "$name" in
        libc.so.*|libm.so.*|libpthread.so.*|libdl.so.*|librt.so.*|libresolv.so.*|libutil.so.*|libGL*.so.*|libEGL*.so.*|libvulkan.so.*|libdrm*.so.*|libgbm.so.*) continue ;;
    esac
    install -m 0755 -- "$library" "$out/AppDir/usr/lib/$name"
done
for directory in glib-2.0/schemas icons/Adwaita icons/hicolor; do
    if [ -d "/usr/share/$directory" ]; then
        install -d -- "$out/AppDir/usr/share/$directory"
        cp -a -- "/usr/share/$directory/." "$out/AppDir/usr/share/$directory"
    fi
done
