# COSMIC active displays

`Outputs::open` connects to the user's Wayland compositor and binds the read-only events of the wlroots and COSMIC output-management interfaces. COSMIC exposes its configured primary through the XWayland-primary property. `primary` excludes disabled and mirrored outputs, prefers the configured primary, and uses the display at the layout origin as the fallback. Connector name breaks any remaining tie.

`OutputState` commits the selection at the compositor's Done event. `Outputs::refresh` drains Wayland events when its socket is ready, reports changes to the wallpaper subscriber, and destroys its GLib source on drop. No monitor settings are written. [COSMIC output protocol](https://github.com/pop-os/cosmic-protocols/blob/main/unstable/cosmic-output-management-unstable-v1.xml)
