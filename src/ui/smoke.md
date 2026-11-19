# Graphical smoke test

`window_gates_controls_and_remains_responsive` builds the normal widgets with fixture identities. It checks offline selection, model-specific pages, the assignment dialog, and the delayed spinner with fast and slow worker jobs. It uses no HID device.

Run `xvfb-run -a dbus-run-session -- cargo test --locked --offline --bin dragon-driver -- --ignored` in the project development container. Set `DRAGON_DRIVER_TEST_SCREENSHOTS` to an existing output directory for PNGs of the three visual states. `pump` drives the test GTK context. `find` and its typed wrappers locate widgets without reading system UI state. `screenshot` renders the mapped window into a temporary GSK texture.

The graphical test stages a Unicode desktop binding, exercises the manual clipboard fallback, and confirms that wireless controls appear only for the receiver fixture. On its isolated X11 display, it creates a foreground fixture window and checks actual Unicode paste through XTEST. Set `DRAGON_DRIVER_TEST_DESKTOP_ENTRY` with an isolated `XDG_DATA_HOME` containing `applications/org.dragondriver.OfflineFixture.desktop` to exercise GIO launch with a fixture entry.

The lighting fixture samples the rendered key face for the default red and a saved green key in Custom 2. A simulated reconnect verifies that painting uses the restored slot without an initial fill. A wired fixture with per-key RGB shows a wallpaper toggle that starts off, and the receiver fixture has no wallpaper control. The live read-only fixture skips automatic first-connection writes.

## Physical wired K617M

`wired_k617m_read_only` is ignored by default and returns without touching hardware unless `DRAGON_DRIVER_LIVE_K617M` names the configuration node. Once the process has permission to open that node, use the project's display container, expose only that node, and run:

```sh
DRAGON_DRIVER_LIVE_K617M=/dev/hidraw4 xvfb-run -a dbus-run-session -- \
  cargo test --locked --offline --bin dragon-driver -- wired_k617m_read_only --ignored --nocapture
```

The test drives the production discovery and settings widgets. It clicks the read controls for W actuation and the travel limit, keyboard lighting, USB status, sleep timers, anti-shake, and return rate. It leaves the device settings untouched and verifies the side, receiver, and gamepad controls remain hidden. `wait_live` pumps GTK's main context with a deadline, `read_live` checks the GUI's error state, and `live_row` retrieves a field by its displayed label.

The wired K617M with firmware `V1_11_27` passed. GTK displayed W at 0.1 mm with Rapid Trigger off, a 4.0 mm limit, static violet `#ee82ee` with brightness 4 and 15 available modes, idle timers of 60 and 1,800 seconds, anti-shake 3, and a return rate of 8 kHz.


## GTK roadmap midpoint

The isolated smoke fixture checks automatic read controls, the 0.1–4.0 mm slider, inline travel cancellation, multiple-selection protection in Profiles, stable keyboard position and selection across views, and discarding a previous session's background response. It passed under Adwaita light, Adwaita dark and HighContrast. The offline Rust suite passed with and without the GUI feature, and Clippy accepted all targets.

The updated physical read-only test opened the wired K617M with existing normal-user access. It switched from W to A and confirmed automatic actuation readback. W reported 0.1 mm, Rapid Trigger off and a 4.0 mm maximum. The current lighting was Custom per-key at brightness 4 and the firmware advertised 15 effects. In that mode the effect response's foreground field is not a per-key RGB frame. The app also read the USB link, 60 s and 1800 s idle timers, anti-shake level 3 and the 8 kHz return rate without manual read actions. The live test did not write settings or start physical travel tracking.

## Roadmap and local packages

The graphical fixture exercised the centered disconnected heading, one scan action, drag selection and deselection using real X11 pointer input, unchanged keybed bounds across the keyboard pages and a hidden keybed on Device, and one through three calibration columns without replacing the selected widget. It passed in light, dark, high-contrast, and a GTK 4.8-compatible alternate theme fixture. The alternate fixture asserts that the selected theme's own text color reaches the heading.

The live read-only GTK test passed through a temporary unprivileged daemon while the container lacked the device node. A separate test under the Flatpak GNOME runtime also read the K617M identity through the private socket, without a HIDRAW mount. Proxy audio relay, local peer authentication, descriptor transfer and owned-file cleanup were verified offline. Live color writes, physical calibration, and real desktop audio recording were outside these checks.

The built AppImage launched on the isolated display. The Flatpak bundle imported into a disposable repository and provided the expected x86-64 app ref. The rebuilt native preview was also opened against the wired K617M using existing normal-user access. The installation and replug of the administrative udev rule were not performed.
