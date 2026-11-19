# SDK identity packet

`request(report_id)` builds the original zero-filled 64-byte wire request with report ID 1 or 6 and command `0x0d`.

`first_reply(report)` checks the first response's header and parses only its available bytes. The payload starts at byte 6 and ends at the first NUL or the buffer boundary. A colon in the payload requests a retry. Otherwise the parser returns raw byte 2 and the first six nonempty comma-separated fields, matching the vendor's tokenization. An empty payload is invalid. Unavailable fields stay empty. The first response supplies a Boolean feature flag in the Windows result. The live K617M result confirms the controller, layout code, and firmware version in tokens one, five, and six. The meanings of tokens two through four are still being traced.

- `payload(report)` provides a bounded view of the meaningful bytes.
- `second_reply(report)` reproduces the vendor's English month conversion and emits `year/month/day time`. A missing, malformed, or unknown date yields no build stamp.
- `identify(device, report_id)` owns a fresh handle opened nonblocking by the session. `exchange(io, report_id, delay)` sends the report and waits the vendor's 250 ms before reading the first reply. Both replies must match the requested report ID. If the optional second reply has yet to arrive, `IdentityIo::wait_for_optional_reply` polls for up to 250 ms. The retry loop has a fixed bound because the original can repeat indefinitely on colon replies. `IdentityIo::send` and `receive` are one-report operations, using HIDRAW in production and scripted replies in tests.

`Identity::controller()`, `layout_code()`, and `firmware_version()` borrow the corresponding fields without decoding or allocation. `model()` finds the exact layout code in the vendor catalog and returns no entry for an unknown code. The user verified the exchange on the attached K617M. The regression test reconstructs both replies from the decoded output. The identity result mapping shows how the Linux session supplies the USB metadata and path stored in the Windows enumeration record. Discovery notes

`identify` borrows the caller’s descriptor so SDK identity and later settings share the same device handle.
