# GUI worker and loading feedback

`Tasks::new` creates one worker thread and a command channel. The worker's `Device` owns the only live `Keyboard` descriptor. `scan` drops the previous session and lists SDK-validated interfaces. `connect` checks that the selected interface still exists before opening it. `keyboard` supplies an exclusive borrow to a submitted operation.

`Tasks::start` serializes device and file work and polls the result channel from GTK every 20 ms while a task is pending. It starts the spinner only if the task is still pending after 500 ms. The main context owns all widgets and retains navigation, window, and cancellation event handling. `finish` stops the spinner, clears the busy state, and re-enables device controls. Stop uses an atomic flag. Calibration and streaming expose Stop in the status bar. `track` runs the travel job with an inline Stop action and cancels on departure from Actuation or loss of the device. Ordinary report writes run to completion. `keyboard` wraps a typed session operation and routes errors to the status bar.

Reactive requests share the worker for short daemon calls. The desktop daemon owns the long-lived renderer. Device jobs use the configuration lock so request and reply reports stay grouped while the daemon sends lighting frames. [Reactive controls](controls/reactive.md)

`background_device` accepts one background operation at a time on the same queue and keeps GTK sensitive. `background_keyboard` delegates to it with an exclusive keyboard borrow. A foreground command can queue behind the current read. Background work shows the spinner after 500 ms. Jobs check the connection revision before touching the device, and GTK discards results for superseded connections.

`Device::apply_palette` invalidates the saved slot before sending the full frame and activation report. It saves the new frame after confirming Custom mode and brightness. `connect` returns both saved slots by model and firmware, so the app assumes one device per model. `initialize_palette` checks the advertised mode list and applies red to the first slot on a new supported model, preserving the reported brightness. `observe_effect` stores the latest keyboard mode returned by readback. The [RON store](../palettes.md) owns the on-disk format.

Native builds and the AppImage check for a desktop companion with wallpaper support during `scan` and reuse the existing setup flow when it needs an update. Flatpak keeps ordinary keyboard controls available with the older companion and checks wallpaper support when the feature opens its subscription. [Host setup](../../packaging/setup-desktop-service.md)

`message` displays a status or error without blocking the window. `error` presents a specific access error when the process lacks permission to open the HID interface. No host permission rules are installed by the application. A connection revision invalidates confirmations for a previous device selection.

When the device has acknowledged desktop bindings or foreground monitoring is active, `Device::tick` polls the worker-owned sources and publishes events over a bounded channel. `Tasks::next_event` drops notices from previous device revisions. `desktop_action` launches installed apps or sends X11 paste on the worker. GTK owns the clipboard and the one-value fallback used by Copy text. `clear_pending_text` clears that value when the connection changes.

`Device::tick` checks for active desktop bindings before taking a hardware configuration lock. A session that only watches foreground profiles does not open or lock a device file on each tick.
