# Witmod HID report framing

`frame_write_report(report, report_id)` implements the pure 64-byte transformation from the DLL's write entry point. It takes a report by value and changes only bytes 0 and 1. A typed report ID represents the collection choice. Rejections return a typed error for the caller. The function does not take a device handle or perform I/O.

The [Linux device adapter](hidraw.md) takes verified collection context, applies output-length padding, and writes the report. SDK session behavior is still pending. See the vendor function notes.
