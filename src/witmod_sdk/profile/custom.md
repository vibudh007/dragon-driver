# Custom lighting and key actions

- `custom_lights` lists the indexed custom LED records for a profile. `append_custom_light`, `rename_custom_light`, `set_custom_light_usage`, and `remove_custom_light` manage the record name, usage text, and numeric flag index.
- `custom_light_keys` reads the recorded coordinates, RGB values, and transparency. `append_custom_light_key` appends one vendor item, and `clear_custom_light` clears the child list without removing the metadata node. `custom_light_mut` resolves a matching profile, then a custom LED index.
- `key_actions` decodes the six text attributes on a profile's `keyEvent` items. `append_key_action` and `remove_key_action` edit that list by key code. `count_key_action_values` counts operations matching both type and value.
- `attr` reads a text attribute and supplies empty text for a missing value. `number` parses signed decimal attributes and supplies zero when missing.

The main evidence is the vendor calls around `0x1001c0f0` (custom key rows), `0x1001c460` (custom LED metadata), `0x1001cba0` (key actions), and `0x1001ece0` (value count).

`assigned_key_codes` reads `keyCode` text from the `keyEvent` children in document order and preserves duplicates. The vendor inserts these into a linked list at `0x1001bb80`. `standalone_custom_light_keys` finds a direct child by `index` and reads the same RGB record fields as the nested profile selector. `light_key` centralizes those field conversions.

`contains_custom_light` checks for a custom light with the requested index. `first_reserved_custom_light_index` follows attribute order until a slot flag of 0 or 1 and returns -1 when no slot matches. `custom_light_keys_for_flag` selects by exact flag text, and `has_key_action` checks for a key code in the profile. `append_key_action` also covers the DLL wrapper that forwards a packed C record to the same operation.
