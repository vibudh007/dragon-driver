# Line and staggered keyboard ripples

`KeyboardPreview::line_ripple` creates mode 7. Each accepted press schedules a line moving toward or away from its originating column. `hexagonal_ripple` creates mode 16. It places up to three rings around a press with the vendor's staggered row coordinates. `reactive_ripple` initializes their shared per-cell storage. `Ripple::press` mirrors the 20-event limit. The release of a finished wave makes its slot available again.

`Ripple::tick` applies the mode's brightness and advances scheduled effects. Line timing runs per row. The mode 16 trigger clock runs after the entire grid. `hexagon` computes the row offsets and each candidate cell while the placement callback bounds coordinates. `background_for` selects the blend palette for intervening rainbow phases. `current_ripple_color` returns three LED channel words, with black before the first press. Each preview session owns one Windows-compatible pseudorandom sequence across configuration changes.

The DLL uses `0x100099b0`, `0x10009ac0`, `0x100089b0`, and `0x100093d0` for mode 7. Mode 16 uses `0x1000a340`, `0x1000a450`, `0x10008ea0`, `0x10009040`, and `0x1000dc70`. Mono and rainbow samples compare both effects with vendor frames. The fixture resets the shared mode 15 and 16 timer before each mode 16 run.
