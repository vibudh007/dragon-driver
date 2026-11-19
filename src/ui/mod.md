# GTK settings window

`install_style` registers the scoped layout [CSS](theme.md) at startup. `open` builds the window and starts discovery. `build_window` binds the header and settings area to the task runner so a running job cannot be followed by a conflicting operation. The status bar remains interactive. Travel tracking uses an inline Stop action and cancels on navigation away from Actuation. Closing during a job requests cancellation where supported and waits for worker cleanup through the GTK event loop.

`build_window` uses the application ID as the window icon name and displays the approved dragon at 24 pixels next to the app name. The title bar embeds the PNG derived from the master SVG, so it can render from local builds as well as either package. [Packaged icons](../../packaging/stage-distribution.md)

`View::scan` rebuilds the confirmed HID interface list. `connect` opens the selected interface. `show_keyboard` creates Actuation for model type 4 and Profiles for layouts with onboard support. Lighting and Device panels have additional gates. A single keybed sits above Actuation, Lighting and Profiles and keeps position and selection when switching views. Device has no keybed or key actions and uses the full settings area. Per-session [readbacks](readbacks.md) initialize on connection and refresh on focus return. Profile drafts are stored in memory by model family for the life of the window. `watch_devices` uses GIO's Linux device-directory monitor and a short debounce to discover a newly connected keyboard or clear a removed one. Permission changes on a disconnected HID node can trigger discovery again.

`page`, `panel`, `field`, `label`, `primary`, and `add_page` provide consistent grouping and spacing. `empty_screen` describes the next step before a keyboard opens. `confirm` presents an explicit device-write confirmation and checks the connection revision before invoking its callback. `pick_file` uses a native chooser for local files and hands the selected path to the caller.

[Worker](worker.md) · [Desktop integration](desktop.md) · [Controls](controls.md) · [Lighting](controls/lights.md) · [Profiles](profiles.md)

## Scope and evidence

The GTK smoke test exercises opening both reference and representative vendor layouts, staging a remap, and validating its packet plan. The HID backend already has live K617M evidence for identity, static RGB, lighting modes, and actuation. The graphical application was exercised offline with fixture identities. Calibration, live travel, onboard uploads and audio lighting still need graphical testing with a physical keyboard.

Desktop bindings, foreground profiles, and receiver lighting are integrated. The graphical test uses a disposable X11 display to verify foreground identification, Unicode paste, and a fixture app launch. Native Wayland uses the Copy text action. Foreground profile automation uses X11 window-manager properties. Firmware updating is outside the first release. Backend evidence

The read-only graphical test also passed on the physical wired K617M. It exercised the actual GTK discovery, model gates, and settings readbacks. Writes and physical key behavior were outside this test. [Live test results](smoke.md#physical-wired-k617m)

The disconnected screen offers [desktop authorization](access.md) if normal access to the wired K617M is denied. A successful grant retries the original candidate through the ordinary worker.
