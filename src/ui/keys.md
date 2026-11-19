# Model keybed

`KeyGrid::new` places native GTK toggle buttons using each model's vendor key geometry, scaled to a bounded width. The horizontal viewport scrolls on narrow windows. `geometry` selects a family and parses the position and rectangle for each visible key. Each button carries its original matrix coordinates for HID operations. `caption` gives common long key names a compact keycap label and keeps the full name in the tooltip.

The geometry test checks all 126 layout records against the backend matrix. [Generated geometry](../keyboard/data/key-geometry.md)

Actuation, Lighting and Profiles share one instance of the keybed per connected device. Only Lighting draws the staged per-key RGB indicators. Selection persists across view changes. Profiles accepts exactly one selected key before enabling assignment editing.

The shared keybed puts Select all and Clear selection beside the keys. In Lighting, that same area shows Color and Brightness. Dragging uses the starting key choice for the gesture, samples each motion segment to catch intermediate keys, and preserves normal single-key activation and keyboard navigation. Device hides the shared figure and its actions.

The keybed has equal side space, with the compact selection actions on the right. The keyboard is centered across the settings area. Lighting controls use a separate right-aligned row below the figure, keeping them outside the keyboard viewport.
