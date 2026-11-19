# Lighting configuration

`KeyboardPreview::from_config(item, key_count)` dispatches the 14 keyboard mode IDs in the vendor DLL. Modes 2, 4, and 5 use the extracted [layout tables](geometry.md). Mode 2 accepts direction values 0 through 5 after translation. Mode 4 accepts 0 and 1. `normalized_direction` preserves the DLL's 6 to 0 and 7 to 1 mapping. `replace_config` rebuilds the frame after validating the settings. It preserves shared clocks and state while resetting the values that the vendor resets at mode initialization.

`SidePreview::from_config(item, cell_count)` creates a preview with default geometry. `from_model_config` uses the layout name and a list of participating slots, including the model-specific wave and collision branches. `SidePreview::replace_config` preserves applicable timers and the most recent collision result when leaving collision mode.

`byte_color` checks that signed profile values fit the RGB byte range. `PreviewConfigError` reports unsupported IDs and directions, excessive slot lists, and counts larger than the vendor's 60-cell array. The raw enum follows the DLL values. The native layer rejects inputs for which the vendor reads beyond a layout table or relies on uninitialized values.

`KeyboardPreview::mixed_color(row, column, background)` selects the even palette slot during the applicable rainbow transitions, using the configured background for other modes and endpoints. It returns the three channels from the vendor's integer blend.

The [preview session](session.md) owns the state in place of the global initialization and timestamp gate in `CWitmodLight_Init`.
