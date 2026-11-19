# Settings portal and GTK themes

`follow` reads the desktop’s GTK theme name and light or dark preference asynchronously from the settings portal. The proxy listens for changes for the application lifetime. Native GTK settings are used if the portal is unavailable or if a caller explicitly specifies `GTK_THEME`.

`apply` selects an available GTK 4 stylesheet and uses the portal appearance preference. `apply_appearance` uses the modern interface color scheme where available and falls back to the older GTK preference. Theme names must be a single path component. When GTK 4 assets are unavailable inside a sandbox, Adwaita follows the requested appearance. The Flatpak can read user theme directories, and local packages include the installed adw-gtk3 GTK 4 assets when available. [Settings portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Settings.html)
