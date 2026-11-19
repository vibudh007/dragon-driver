# Onboard macro encoding

`encode_onboard_events(events)` ports the record transformation at `0x10021010`. It returns the DLL's unsigned word list. Each recognized event contributes its key or mouse code, a `down` flag in bit 4, the mouse flag in bit 0, and two words for the delay of the next event. The final delay is zero. The three pairs of recognized mouse codes map to `1`, `2`, and `4`. `event_code` identifies supported events. Unsupported events are skipped as complete records to preserve alignment. Lookahead uses the next event that will be emitted. The in-memory result gets zero-padding to at least 248 words. The function uses lookahead instead of allocating a second linked list of delays.

This module constructs data only. USB upload framing and the device's rejection behavior are separate.
