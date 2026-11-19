# Reactive lighting in the desktop daemon

`Reactive::handle` starts one effect for the selected keyboard in the logged-in user's daemon. The target contains the HID node, USB ID, model and firmware. The initial worker validates the node and keyboard identity and checks that wired Custom lighting or compatible receiver audio is available. Further clients can query the current mode, update audio colors, or request a stop. The effect belongs to the daemon and survives the GUI and its command connection. The daemon saves the enabled mode, audio palette, and target identity in RON before sending lighting. A new daemon resumes the saved preference after desktop login. The worker matches USB ID, model and firmware across node changes, and retries while the keyboard or desktop source is unavailable. Turning the switch off clears the preference before the worker restores normal lighting.

`Run::wallpaper` watches existing desktop notifications and decodes updates on a separate worker. The inbox coalesces new colors, and identical frames skip the RGB write. Synchronization reports keep the last frame on the keyboard every 500 ms. The last usable colors stay visible when the current wallpaper cannot be read. Turning the effect off releases synchronization and restores the previous built-in effect when the device is available.

`Run::audio` consumes the daemon's existing PipeWire bar stream. The USB renderer uses the selected colors or a palette extracted from wallpaper updates, with the saved selection as the fallback for the current run. The receiver renderer continues to use firmware colors. The render path holds the configuration lock only while working with the keyboard, and releases it while waiting for more audio.

`Watch` owns and joins the image watcher. `Run::finish` publishes the terminal state after cleanup. `Reactive::shutdown` cancels the running effect and allows it to restore base lighting while retaining the saved preference for the next session. Neither effect writes a custom palette slot. [GUI controls](ui/controls/reactive.md) · [Service protocol](service.md) · [RGB transfer and restoration](keyboard/session.md)

The lifecycle fixture closes the start socket before publishing a frame, then queries and stops the same active effect through a fresh client. A read-only check on the wired K617M verified two configuration clients could share the real broker lease and that the daemon answered status with GTK closed.

`Run::wallpaper` releases the inbox lock after taking the pending frame and before waiting for HID access. `Reactive::shutdown` closes admission under the state mutex before cancelling the active worker.

The wired K617M resumed the same audio preference after restarting the installed desktop daemon with the GUI closed. Offline fixtures cover mode-only RON migration, restoring both custom pages in order, and clearing the active preference when Stop is saved.
