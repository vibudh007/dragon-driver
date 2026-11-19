# Device lighting reports

`Region::query` reads the preset effect and `modes` builds the mode-list queries for the keyboard and side regions. `reply` expects their independently mapped command bytes. `effect` encodes the preset fields, checks one-based brightness and speed, clears effect type for mode zero, and appends the supplied millisecond stamp's low word in big-endian order. `decode` reverses the one-based conversion.

`synchronize` builds the EXE's host synchronization command. `color_chunk` encodes one bounded payload for USB streaming or custom effects. These are packet codecs. `keyboard_frame` uses page 0 for USB streaming and page 2 for a custom preview. It preserves all 132 matrix positions, emits seven chunks of 18 RGB triples, then six triples in chunk 7. `audio_frame` maps spectrum levels onto the six keyboard rows. `wireless_frame` builds the separate 2.4 GHz command and accepts the vendor’s keyboard and side telemetry variants. `decode_modes` returns the ordered device modes and the two settings bytes after the marker. EXE lighting paths · [SDK lighting previews](../witmod_sdk/lighting.md)

`rgb_frame` frames a caller-selected custom color page. `activate_custom` builds the vendor's separate preset command with mode, brightness, and speed after the color transfer. Vendor custom color apply

`AudioKeybed` maps the daemon's 22 spectrum bands across the populated matrix columns. Bars grow from the bottom through the Low, Medium, and High palette entries. Keys above each bar use Background. Gaps stay black in the USB frame.
