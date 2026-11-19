# Lighting preview session

The preview is in-memory SDK state. It uses the catalog model's key count for keyboard geometry and its layout code for the side geometry. The two preview engines can run independently.

| Rust operation | Vendor equivalent |
| --- | --- |
| `LightingPreview::new` | Creates a keyboard preview with the initial 14-field setting and the side-tick flag from `CWitmodHid_SetKeyboardColorInfoDataStu`. |
| `set_keyboard_config`, `update_keyboard_config` | Replace the effect and preserve the vendor's shared clocks and PRNG. The full setter also changes the side-tick flag. Validation precedes both changes. `keyboard_config` returns the original settings, including fields the tick functions leave unused. |
| `set_side_config`, `set_side_layout`, `update_side_config` | Set the side effect and either supply a cell count or a list of participating cells. The update uses the stored list. `replace_side` preserves the current side timers through a change. |
| `tick_keyboard`, `tick_side` | Advance one frame and lend the result to the caller. A disabled side clock keeps its most recent frame. This replaces the two Windows callback exports with borrows. |
| `press` | Accept signed grid coordinates, return 0 for a valid press and −1 for a coordinate outside the grid. |
| `mixed_color` | Blend foreground and background using the SDK arithmetic and the current palette transition. |
| `current_ripple_color`, `current_automatic_ripple_config`, `side_collision_config` | Read the SDK's ripple and collision results. Reading automatic ripple configuration acknowledges the pending trigger. |

The [keyboard](../lighting.md) and [side](side.md) previews expose the fields consumed by the EXE. The EXE keyboard renderer uses RGB, intensity, palette progress, the row count, and the automatic trigger flag. The side renderer also reads the collision background channels. The native interface uses explicit values for these fields. The DLL also copies temporary words that its x86 code sometimes leaves uninitialized, including the initial collision blend channels. Rust uses the configured background until a palette step supplies another color.

The offline fixtures include an actual vendor reconfiguration trace, row count, automatic trigger and readback, five-byte side telemetry, and named geometry. The fixtures use no USB access. [Fixture formats](../../../tests/fixtures/vendor-sdk/README.md)
