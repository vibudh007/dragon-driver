# One-time service setup

The native app, AppImage and Flatpak request administrator authentication on first launch. A terminal setup uses sudo and a graphical setup uses polkit. The installer copies the broker to a root-owned directory, enables a systemd socket for the desktop UID and registers the unprivileged desktop daemon. The socket resumes on reboot. The broker validates and opens the current configuration interface on each connection, including after a replug.

The Flatpak contains the GUI, CLI, daemon and host setup files. The first launch runs the bundled setup helper on the host and prompts for administrator approval. GTK reconnects to the daemon after setup, in the same window. The Flatpak uses the host-command interface for setup and the private desktop socket for keyboard configuration.

If automatic setup is unavailable, install the host components from the companion archive or export them directly from the Flatpak.

```sh
mkdir -p dragon-driver-host
flatpak run --command=dragon-driver-export-host --filesystem="$PWD/dragon-driver-host" \
  org.dragondriver.DragonDriver "$PWD/dragon-driver-host"
./dragon-driver-host/setup-desktop-service.sh --flatpak
```

`--app /absolute/path/to/dragon-driver` registers a native build and the AppImage also supports `--setup`. The setup script stages its privileged inputs under a private directory before authentication and installs fixed validated unit names. The daemon keeps the per-user RON file across application, service and OS restarts. The uninstaller stops the system socket and removes user units without deleting palettes.

When all registered frontends disappear for 30 seconds, the desktop daemon removes its user units and retires the account's system access socket. Use `setup-desktop-service.sh --uninstall` or the AppImage with `--uninstall-service` for explicit removal.

Setup re-enables the desktop units from their current definitions. An upgrade migrates the desktop daemon from the default user target to the graphical session target, so wallpaper and audio can be restored after login.
