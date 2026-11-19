# Onboard profile plans

`encode_profile` plans the vendor primary bank (0) and Fn bank (2). It assigns macro and advanced slots across both banks in traversal order. Records that refer to a slot are preceded by its data in the plan. It inserts the vendor timing boundaries and clears unused macro slots and, on type-4 models, unused advanced slots. It validates all referenced profiles and assignments before any I/O. `ProfilePlan::packet_count` counts wire packets, excluding delays.

`Planner::layer` resolves XML assignments by HID key code and visits all 132 matrix cells in row-major order, including holes. It groups up to 14 four-byte records per report. `add_macro` encodes up to 62 events with the SDK and emits five reports for the slot. It rejects events the onboard codec would skip and values that overflow the key or delay fields. `add_advanced` resolves a host definition, emits its packet, and puts its allocated slot in the key record. The standalone `encode_layer` exposes the per-bank packet preview for simpler fixtures.

`binding` packs the numbered vendor mappings, including direct keys, modifiers, media, secondary actions, and preset shortcuts. `combination` validates one or two modifiers. `default_record` handles Fn and special media defaults. `number` and `invalid` reject malformed numeric data. The planner rejects unsupported actions before returning packets.

The plan can be inspected with `dragonctl preview-profile`. [Session execution](session.md) · Vendor onboard function
