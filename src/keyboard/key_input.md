# Linux key input for live lighting

`Input::open` selects event nodes below the already-identified USB device. It checks keyboard capability, opens read-only and nonblocking, and compares the open character device against its sysfs device number. It never grabs the input device. The reactive modes require permission to read one of those nodes.

`newest_press` reads native Linux `input_event` records in bounded batches. `sample` pairs keyboard-page `MSC_SCAN` with a press, discards repeats and releases, and clears stale scan state at sync events. No key history is kept. The test supplies synthetic event fields without touching real devices.
