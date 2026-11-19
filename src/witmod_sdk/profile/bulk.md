# Onboard profile imports

The four import methods accept a byte slice returned by a caller. These functions only update a parsed profile in memory. A short slice returns an error before mutation. A missing requested profile or container returns false.

| Rust function | DLL export and format |
| --- | --- |
| `import_key_actions` | `0x10023730`. Four bytes per cell in the 6 × 22 matrix map to remaps, Fn, macro bindings, modifier combinations, media actions, and secondary controls. `modifier`, `media_action`, and `secondary_action` translate the vendor jump tables. Macro repeat and mode read each slot's metadata at `0x600 + slot * 0x100`. Edits are validated before the profile changes. |
| `import_bound_macros` | `0x10023bb0`. Ten 256-byte slots have an eight-byte prefix. Byte 7 in the 248-byte event payload is the stop marker. `decode_macro_event` turns four-byte records into the original delay, code, action and label, including standard Windows mouse codes in XML. |
| `import_custom_rgb` | `0x10023810`. Two 396-byte banks begin at `0x103c` and `0x1200`. Each bank assigns RGB triples to valid matrix cells. Zero triples are skipped, and a nonzero color at an unmapped cell ends the bank as in the SDK. The mark `transparentNum` is 128. |
| `import_light_config` | `0x100238a0`. The first 512-byte block selects the keyboard light index and the next ten hold two selected modes each. `light_slot` reads the least significant set bit of the little-endian mask. Each mode loads eight data bytes and four flag bits. An invalid offset fails before mutation. |

Reference XML files for all four functions were generated from synthetic onboard buffers passed to the original DLL in Wine.
