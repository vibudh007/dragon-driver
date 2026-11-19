# First-launch authorization

The native app, AppImage and Flatpak check the desktop daemon before scanning. When setup is needed, the app launches the bundled installer asynchronously. The host installer uses polkit for administrator authentication. systemd keeps the access socket enabled across boots. The app never handles the password.

`native_installer` registers the actual AppImage file or native executable. `flatpak_installer` reads Flatpak's application ID and deployed files path, then resolves the installer in that deployment. `setup_command` invokes the helper directly on native builds or through Flatpak's host-command interface. `View::authorize` keeps one request in flight, retries discovery after success, and leaves the access button available after cancellation or failure.

The Flatpak creates the dedicated runtime directory before mounting it so the new daemon socket becomes visible while the app is already open. The bundle requests `org.freedesktop.Flatpak`, which permits host commands as the desktop user. Administrator operations still require authentication. [Host setup](../../packaging/setup-desktop-service.md)
