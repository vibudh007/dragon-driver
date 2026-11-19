# Performance setting reports

`Sleep::request` encodes the light and keyboard idle times as two big-endian 16-bit integers in bytes 9–12. This replaces the EXE's integer-to-hex-text conversion and byte splitting. `sleep_query` sends selector 2 at byte 2, and `sleep_reply` matches the `0x14` reply with that selector. `Sleep::decode` reads both timers and the flag byte.

`WinLock::request` preserves the vendor ordering of the Windows-key, Alt-Tab, Shift-Tab, and Alt-F4 fields. `reset` and `gamepad_mode` build the separate command variants. `sleep_write_reply` requires the vendor’s `0x14` acknowledgement with byte 2 equal to zero. The terminal exposes read-only sleep inspection. Backend reset and gamepad operations require explicit calls, and a gamepad mode change forces session reopen.

`status_query` builds the normal EXE handshake poll. `status_reply` matches wire command `0x01`. `LinkStatus::decode` checks the size and payload marker, then parses the mode flag and four values passed to the vendor charging-status callback. Transport value 0 represents USB. Vendor poll · Status dispatcher

Vendor sleep encoder · Vendor sleep decoder
