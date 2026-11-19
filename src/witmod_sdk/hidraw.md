# Linux HID transport

`ReportLayout` contains the native Linux input length, the outgoing padding length and the verified output report ID. Discovery must supply these from the matching keyboard and collection. Opening a path by itself does not verify the keyboard model.

- `HidrawDevice::open(path, layout)` resolves the HIDRAW device number through sysfs. `open_checked` validates the node metadata both before and after opening. Its initial open uses nonblocking, no-follow and no-controlling-terminal flags. `node_name` validates the path and `verify_node` checks the device type and number. The owned file closes when the device drops. The function allocates input and optional zero-padding storage once per device.
- `set_nonblocking(enabled)` changes the descriptor's nonblocking flag. Devices open in blocking mode. The flag is applied to the Linux file description.
- `read_report(output)` uses `read_report_from` to read one report into reusable storage and copy up to 64 bytes. It returns the copied count or zero when a nonblocking read has no report ready. Bytes after that count stay unchanged.
- `write_report(report)` checks and frames the source report, then uses `write_report_to` to pad it when required. It returns the actual write count. A short result is returned to the caller since a retry of the suffix would send a separate HID report. The helper's `write_once` retries interrupted system calls.
- `wait_for_report(timeout)` polls a borrowed descriptor until the kernel signals I/O or the timeout expires. `wait_readable` keeps one deadline across interrupted syscalls.
- `write_framed_report(report)` is the internal path for commands constructed directly as wire reports, like the identity request.

Linux errors are returned with their cause. Windows uses the fixed failure value and a separate error path. The adapter preserves the distinctions among rejection, no data, I/O error, and successful transfer. Windows' leading zero for unnumbered input reports is absent from the Linux HIDRAW representation. Vendor read and write evidence

Tests use in-memory readers and writers and a local socket pair for readiness. The SDK session verifies the configuration descriptor before it opens the transport.

`layout` exposes the verified I/O lengths within the crate. `wait_for_report` reports disconnection when poll signals an error without readable data.
