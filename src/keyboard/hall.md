# Hall-effect protocol

`apply` encodes a populated key selection, validates positive active thresholds and consistent shared or separate mode, then places the four thresholds after the 22 column masks. `set_masks` sets the bit corresponding to the row for each selected column. Duplicate positions naturally combine. Thresholds use the vendor's integer distance units. Preview defaults unused rapid thresholds to zero when rapid mode is off. `values` duplicates the shared threshold into the two report fields. `disable_rapid_report` uses mode 0, matching the vendor’s Quick Trigger switch-off handler. It copies the original thresholds if press and release are equal, rejects unequal values before writing, and skips the write if the keyboard already reports mode 0.

`Control::request` creates anti-shake and return-rate reads or writes. The selector is 10 or 9. A write sets byte 7 to 1. `Control::reply` expects the matching selector and eight bytes. `key_request` sends the row then column, and `key_reply` matches the selector-5 response. `decode_key` reads bytes 7–11 into a mode and four unsigned values. The reply omits the requested coordinates, so the session permits one request at a time.

`sensitivity_request` and `defaults_request` generate selectors 4 and 6. `sensitivity_reply` matches the limit byte stored by the EXE at `004674fe`. `calibration_selection` makes the selector-2 mask, `calibration_start` sends a single position, and `calibration_all` keeps the vendor's inverted enable byte. `decode_calibration` accepts the single-key status or an all-keys status and optional progress masks. These functions supply the session’s start, progress, and stop operations. The wire transitions still need a physical calibration test.

`command` builds the common `0x29` EXE header, and `reply` matches the corresponding `0x21` wire command. EXE Apply and query trace

`decode_travel` reads the two matrix coordinates and the raw live sample from wire command `0x21` selector 1. `stop_travel` reproduces the EXE's selector-3 packet used when leaving the trigger page. Live dispatcher · Page exit
