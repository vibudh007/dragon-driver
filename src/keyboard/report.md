# Application report format

`Packet` is the SDK's fixed 64-byte buffer. `packet` puts the EXE command in byte 1 and its four application header bytes at 2–5. It initializes the report ID placeholder to 1 and zero-pads the remaining bytes. The SDK maps outgoing commands and sets the actual report ID separately.

`Reply::matches` requires the minimum byte count and exact collection ID and command. If provided, it also compares a byte at the expected selector offset. Decoders still check their own fields and lengths for callers who use them outside the session. [SDK framing](../witmod_sdk/hid.md)
