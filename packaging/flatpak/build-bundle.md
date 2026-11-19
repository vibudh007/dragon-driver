# Flatpak export

`build-bundle.sh` takes an initialized staging directory and an output directory. It completes the Flatpak using the GNOME 49 platform, exports a temporary OSTree ref, and builds the local single-file bundle. The app receives display, graphics and read-only theme directory access. Flatpak creates and mounts the Dragon Driver runtime directory, so the running app can see the socket installed on first launch.

The bundle requests `org.freedesktop.Flatpak` to run its own deployed setup helper on the host. This interface allows host commands as the desktop user. Polkit handles the one-time authorization for systemd installation. `dragon-driver-export-host` still exports the same daemon and setup files for manual recovery.

[Flatpak sandbox permissions](https://docs.flatpak.org/en/latest/sandbox-permissions.html)
