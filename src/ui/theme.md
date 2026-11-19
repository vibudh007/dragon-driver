# GTK theme

The application inherits the desktop or user-selected GTK 4 theme. The [settings portal](desktop_theme.md) passes the desktop theme name and appearance preference to older or sandboxed GTK runtimes. GTK draws the native window, header bar, navigation, focus, selection, sliders and suggested-action buttons. Application CSS defines spacing, key geometry, and theme-derived separators and group outlines. Lighting adds a per-key color indicator within the shared keybed while the Lighting page is open. GTK stack transitions follow the toolkit's animation setting.

The light, dark and high-contrast GTK variants can be checked in an isolated graphical test by setting `GTK_THEME` for that test process. [GTK settings](https://docs.gtk.org/gtk4/class.Settings.html) · [GNOME navigation guidance](https://developer.gnome.org/hig/patterns/nav/view-switchers.html)

Page headers use a symbolic icon and a divider. Related settings use outlined groups with explicit headings, and adjacent settings rows have light separators. Group contrast is derived from the active GTK foreground color. The display drops redundant page introductions and keeps operating hints beside the controls they explain. This follows GTK conventions for grouping and scanning. [GNOME boxed lists](https://developer.gnome.org/hig/patterns/containers/boxed-lists.html)
