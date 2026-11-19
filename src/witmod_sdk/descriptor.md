# Configuration report detection

`configuration_report_id` parses the USB HID report descriptor for the vendor Application collection on usage page `0xff1b`, usage `0x91`. It accepts report ID 1 or 6 when it can uniquely identify a 63-byte Data/Variable input and output report. It counts bits within each ID and rejects any use of the same ID in other collections, truncated items, unbalanced scopes, and ambiguous configurations. Unrelated report IDs can coexist in a composite descriptor.

`Global` carries the current report page, ID, size, and count through the HID global stack. `Report` accounts for input and output widths and rejects conflicting fields. The parser uses no OS access. The discovery caller uses the vendor's USB and interface catalog before opening any node.

[Discovery](discovery.md) · Reference descriptor evidence
