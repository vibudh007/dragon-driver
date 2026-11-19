# Onboard data packets

`six_key_mapping`, `full_key_mapping`, `multimedia_mapping`, and `mouse_mapping` create the corresponding application command and copy fixed-size input fields. The declared bytes for six-key and full-key modes exceed the copied bytes in the EXE. Rust preserves the zero padding. `onboard_chunk` encodes an arbitrary bounded bank segment. `key_chunks` groups at most 14 four-byte records per report with consecutive indices from zero.

`macro_slot` encodes one eight-byte header and 248-byte event field into five reports with body sizes of 58, 58, 58, 58, and 24 bytes. It validates slot limits and event alignment. `clear_macro_slot` emits the zero-filled slot. `advanced_slot` encodes a checked slot and 50-byte payload. The advanced-key payload comes from the [typed encoder](advanced.md).

These functions return packets for the offline planner or the [session uploader](session.md). Vendor onboard trace

`DirectMapping::packet` dispatches the caller-selected fixed remap operation to the matching packet builder.
