# Actuation and device controls

`actuation` uses the shared keybed. Selecting one key automatically loads its stored thresholds. The sliders use tenths of a millimeter and the reported device limit. Apply sends the displayed thresholds to the selected positions. The separate press and release setting remains independent of the Rapid Trigger toggle. One-key edits receive immediate readback. A batch edit invalidates the affected positions so the next single selection can verify the stored settings. `tuning_from_readback` interprets the fixed and dynamic threshold bits.

`wire_travel` changes Track travel to Stop travel in the same button position. The worker owns travel monitoring until cancellation or a transport failure. GTK shows only the most recent sample. Changing page, closing the window or removing the device cancels tracking.

`device` shows connection status, idle timers, shortcut locks, explicit defaults, and a gamepad selector for compatible vendor USB IDs. Hall-effect models also show performance and calibration. The [automatic readback](readbacks.md) initializes supported settings. `sleep_panel` preserves existing flags and confirms timer writes. `tuning_panel` and `control_row` apply and confirm a single performance field. Return rate uses vendor values 1, 2, 4 and 8 kHz. `wire_reset` confirms first and schedules fresh reads after sending a reset.

`calibration_panel` accepts one physical key or all keys. It confirms before taking exclusive ownership of the device for calibration, observes the firmware status and supports stopping from the status bar. The worker leaves calibration mode on completion, failure, timeout, or cancellation.

The calibration picker is a dropdown popover with an **All keys** option and individual key glyphs. One scroller contains the responsive grid. The menu closes on selection and the button shows that selection. Its column count follows the current monitor and window widths, and screen readers can distinguish left and right modifiers.
