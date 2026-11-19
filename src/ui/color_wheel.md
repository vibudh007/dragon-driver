# Lighting color wheel

`ColorWheel` fills a rounded GTK button with a drawn color swatch. New choices default to red. Readback replaces the default where a saved color is known. Its modal picker contains a hue and saturation wheel, a brightness slider, and a keyboard-editable hex field. The wheel follows the pointer until clicked. A click pins the color under the pointer and another click resumes following. Taps also work on touch screens. Slider changes and valid hex values also apply directly. **Done** closes the picker. The lighting worker coalesces quick updates. The slider changes the RGB color value independently of keyboard-wide brightness.

`wheel_surface` builds one Cairo wheel image for the GTK thread. Draw callbacks reuse that image and paint an outline marker which keeps the wheel visible through its center. The wheel surface stays at full brightness, including when the selected color is black. Pointer motion outside the disc leaves the selection alone. The RGB and HSV conversion preserves exact byte colors through the hex editor. Tests check conversion, invalid input, and a commit through the GTK picker.

`rainbow_button` uses the same shape and size as a solid swatch and exposes its checked state as a toggle. A cached Cairo surface blends eight hue wedges at their edges and twists them counterclockwise as they radiate from the center. Drawing clips the surface to the shared swatch outline.

`swatch_path` insets the solid fill and the rainbow clip relative to the common GTK button border. The 8 px outer radius and 7 px inset path leave the four border corners clear in each theme. The pixel test checks all four corners and the swatch center.
