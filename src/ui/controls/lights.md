# Lighting controls

`lighting` opens to per-key color on supported keyboards. The controls below the keybed contain a solid color swatch and an unlabeled keyboard brightness slider with an accessible label and tooltip. An always-open Lighting effects panel exposes the mode catalog at the same level as Key colors and Music-reactive lighting. `KeyboardSwatches` switches the toolbar between the per-key brush and the active effect colors. When effect colors occupy the toolbar, the same brush moves into Key colors and retains its color. Changes commit directly from GTK and are coalesced by the [lighting queue](changes.md).

`ColorEditor` starts with the worker's saved palettes for each custom slot. A verified Static effect can supply a uniform starting frame when the selected slot has no remembered colors. Selecting a key paints it with the current swatch when the base frame is known. Dragging batches newly selected keys. Deselecting a painted key restores the RGB it had when selected. Changing the swatch only affects future selections. The brightness slider remains independent. Switching effects keeps saved custom palettes. Switching slots clears selection restore data and uses the destination slot's own palette.

**Apply color to all keys** replaces a complete palette without changing selection. Individual edits require a known base frame. `send` stages a complete frame so rapid edits build on it. `draw` tints the full visible face of each key using that frame or the saved palette and leaves theme text and selection feedback visible. Unavailable slots show a red preview. The worker records a frame after sending all packets and reading back Custom mode and brightness. A failed transfer invalidates the affected slot unless a newer complete frame for that slot is queued in GTK. Custom mode readback contains no individual RGB data.

The reconnect trace showed a successful palette send at 22:39:50, a disconnect, then `palette-known=false` on the rebuilt editor and `brush skipped: unknown palette` for every click. The user verified that this K617M preserved its active custom colors across a USB power cycle. The [RON store](../../palettes.md) preserves the app's matching palette across editor recreation, reconnects, and app restarts. On a new supported model the worker writes a solid red palette to Custom 1 and makes it the active effect.

## Vendor readback limits

The wired K617M's full effect and mode-list replies were checked in Custom mode. The attached firmware returned effect settings and supported modes. The related 7186 firmware stores the custom palette in flash and exposes the preset index in its HID reply. Wire capture and firmware analysis

The vendor keyboard lighting query at `00464830` returns effect parameters. The custom receive slot at `004813c0` forwards three scalar arguments to `004b0760`. That function stores a selector and updates the controls. The vendor editor fills its RGB matrix from local XML via `CWitmodXML_ProFileCustomLightInfoSelectAll` at `0047de90`. The custom writer at `004b6500` sends command 8 in chunks. Neither the verified firmware response nor these paths establish a complete device RGB read or an independent single-key write. The GTK app treats an existing firmware custom frame as unknown when it connects. Vendor pseudocode · Focused HID paths

## Audio and receiver

The Audio-reactive toggle follows PipeWire playback. The desktop daemon chooses wired RGB or wireless spectrum transport and keeps both reactive effects running when the window closes. `wireless_panel` only appears for a confirmed receiver link. The wallpaper panel uses wired live lighting and previews its active colors on the same keybed. [Reactive controls](reactive.md) · [Receiver renderer](../../keyboard/wireless.md)


## Effect colors

The Windows Full Color toggle at `004bb490` stores a binary mode type. The effect packet writer sends that value in report byte 16. The vendor preview config uses zero for a chosen color and nonzero for rainbow. The toolbar shows supported controls from left to right as Rainbow, background, and foreground. `EffectEditor::update_controls` decides separately whether Rainbow and background are available. Side lighting uses the same controls in its own section. Choosing either color turns rainbow off in the same queued update. Readback checks the palette and, in single-color mode, the requested foreground and changed supported background. Vendor toggle · [Preview config](../../witmod_sdk/lighting/config.rs)

The main, background, per-key, and music spectrum picks share a [color wheel](../color_wheel.md). Opening a wheel leaves the device color alone. Picking a new color applies it directly. A partial edit requires a known color frame to preserve the other keys.

## Direction

The panel displays Direction when the matching vendor effect exposes it. `direction_options` follows the Windows keyboard mode visibility rules. Wave shows four travel arrows and Inward or Outward. Radar and Snake show rotation arrows. Striation displays left for the firmware’s counterclockwise value 7 and right for its clockwise value 6. Aurora shows Inward or Outward, and Speed response shows vertical arrows. The Windows side-lighting panel hides directions for its known modes. Vendor mode visibility · Direction selection

The Windows styles at EXE addresses `004c8c21` through `004c9978` map direction IDs 0–7 to right, left, up, down, outward, inward, clockwise, and counterclockwise. `update_direction` keeps the returned direction byte while rendering the corresponding choice. It selects a valid default if changing effects would otherwise carry an unsupported direction into the new effect. `display_direction` treats 0 and 1 as aliases for 6 and 7 for the rotation choices, consistent with vendor preview normalization. `effect_matches` compares the known direction on readback. Effect writes preserve the separate firmware `open` byte.
