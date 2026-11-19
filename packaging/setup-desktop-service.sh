#!/bin/sh
# Install or remove the host companion for one app location.
set -eu

assets=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
daemon=$assets/dragon-driver-daemon
if [ ! -x "$daemon" ]; then daemon=$assets/../target/container/debug/dragon-driver-daemon; fi
if [ ! -x "$daemon" ]; then daemon=$assets/../target/debug/dragon-driver-daemon; fi

if [ "$(id -u)" = 0 ]; then
    PATH=/usr/bin:/usr/sbin:/bin:/sbin
    export PATH
    account=${PKEXEC_UID:-${SUDO_UID:-}}
    case "$account" in ''|0|*[!0-9]*) exit 2 ;; esac
    [ "$#" -eq 1 ]
    socket=dragon-driver-access@$account.socket
    service=dragon-driver-access@$account.service
    case "$1" in
        --install-system)
            test -x "$daemon"
            for file in dragon-driver-access@.socket dragon-driver-access@.service; do
                if [ -e "/etc/systemd/system/$file" ] &&
                    ! grep -qx '# Managed by Dragon Driver' "/etc/systemd/system/$file"; then
                    echo "An existing system unit uses $file." >&2
                    exit 1
                fi
            done
            install -d -m 0755 -- /usr/local/libexec/dragon-driver
            stage=$(mktemp /usr/local/libexec/dragon-driver/.dragon-driver-daemon.XXXXXX)
            trap 'rm -f -- "$stage"' EXIT
            install -m 0755 -- "$daemon" "$stage"
            mv -f -- "$stage" /usr/local/libexec/dragon-driver/dragon-driver-daemon
            install -m 0644 -- "$assets/dragon-driver-access@.socket" "$assets/dragon-driver-access@.service" /etc/systemd/system
            systemctl daemon-reload
            systemctl enable --now "$socket"
            ;;
        --remove-system)
            systemctl stop "$service"
            systemctl disable --now "$socket"
            ;;
        *) echo "Run setup as your desktop user." >&2; exit 2 ;;
    esac
    exit 0
fi

install_dir=${HOME:?}/.local/lib/dragon-driver
unit_dir=${XDG_CONFIG_HOME:-"$HOME/.config"}/systemd/user
socket=dragon-driver-access@$(id -u).socket
administrator() (
    stage=$(mktemp -d "${XDG_RUNTIME_DIR:?}/dragon-driver-setup.XXXXXX")
    trap 'rm -rf -- "$stage"' EXIT
    install -m 0755 -- "$assets/setup-desktop-service.sh" "$stage"
    if [ "$1" = --install-system ]; then
        install -m 0755 -- "$daemon" "$stage/dragon-driver-daemon"
        install -m 0644 -- "$assets/dragon-driver-access@.socket" "$assets/dragon-driver-access@.service" "$stage"
    fi
    if [ -t 0 ]; then
        sudo -- "$stage/setup-desktop-service.sh" "$1"
    else
        /usr/bin/pkexec --disable-internal-agent "$stage/setup-desktop-service.sh" "$1"
    fi
)

for file in dragon-driver.socket dragon-driver.service; do
    if [ -e "$unit_dir/$file" ] && ! grep -qx '# Managed by Dragon Driver' "$unit_dir/$file"; then
        echo "An existing desktop service uses $file." >&2
        exit 1
    fi
done

if [ "${1:-}" = --uninstall ] && [ "$#" -eq 1 ]; then
    if [ -f "$unit_dir/dragon-driver.service" ] || [ -f "$unit_dir/dragon-driver.socket" ]; then
        systemctl --user disable --now dragon-driver.service dragon-driver.socket
    fi
    rm -f -- "$unit_dir/dragon-driver.service" "$unit_dir/dragon-driver.socket"
    rm -f -- "$install_dir/frontend.app" "$install_dir/frontend.flatpak" "$install_dir/dragon-driver-daemon"
    rmdir -- "$install_dir" 2>/dev/null || :
    systemctl --user daemon-reload
    if systemctl is-enabled --quiet "$socket"; then
        administrator --remove-system
    fi
    exit 0
fi

case "${1:-}" in
    --flatpak)
        [ "$#" -eq 1 ]
        registration=frontend.flatpak
        app_id=org.dragondriver.DragonDriver
        if flatpak --user info "$app_id" >/dev/null 2>&1; then
            frontend=${XDG_DATA_HOME:-"$HOME/.local/share"}/flatpak/exports/share/applications/$app_id.desktop
        elif flatpak --system info "$app_id" >/dev/null 2>&1; then
            frontend=/var/lib/flatpak/exports/share/applications/$app_id.desktop
        else
            echo "Install the Dragon Driver Flatpak before running setup." >&2
            exit 1
        fi
        ;;
    --app)
        [ "$#" -eq 2 ]
        registration=frontend.app
        frontend=$(realpath -ms -- "$2")
        ;;
    *)
        echo "Usage: $0 --app /path/to/app | --flatpak | --uninstall" >&2
        exit 2
        ;;
esac
case "$frontend" in
    *'
'*) echo "The app path cannot include a newline." >&2; exit 2 ;;
esac
test -f "$frontend"
test -x "$daemon"
if ! systemctl is-enabled --quiet "$socket"; then
    administrator --install-system
fi
install -d -m 0700 -- "$install_dir"
install -d -- "$unit_dir"
stage=$(mktemp -d "$install_dir/.setup.XXXXXX")
trap 'rm -rf -- "$stage"' EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
install -m 0755 -- "$daemon" "$stage/dragon-driver-daemon"
printf '%s\n' "$frontend" > "$stage/frontend"
mv -f -- "$stage/frontend" "$install_dir/$registration"
mv -f -- "$stage/dragon-driver-daemon" "$install_dir/dragon-driver-daemon"
install -m 0644 -- "$assets/dragon-driver.socket" "$assets/dragon-driver.service" "$unit_dir"
systemctl --user daemon-reload
systemctl --user reenable dragon-driver.socket dragon-driver.service
systemctl --user start dragon-driver.socket
systemctl --user restart dragon-driver.service
echo "Setup complete. Open Dragon Driver."
