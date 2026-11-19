# Export Flatpak host components

`dragon-driver-export-host` copies the bundled daemon, setup script and systemd units to a directory the user explicitly shares with the one export command. It requires an existing directory. It performs no system installation. The extracted host script registers the real system service with the user's administrator authorization. The regular Flatpak retains its private service socket access. [Setup](../setup-desktop-service.md)
