# Side-light collision

`SidePreview::collision(cell_count, span, foreground, background, rainbow)` creates mode 3. The visible cell count and the model's animation span are independent. `Collision::tick` updates each visible LED and then advances the paired highlights from the edges toward the middle or back out. It leaves the seven-event pause between passes. `step` chooses a brightness increment of seven for mono or 35 for rainbow.

`collision_config` returns the five byte fields exposed by the DLL. They are elapsed time, current pass count, position, return flag, and brightness increment. `replace_config` retains the elapsed collision timer across side modes. The DLL reads those fields at `0x100108a0`. The update calls `0x1000e1f0` and `0x1000db60`, and initialization begins at `0x1000e0f0`.

Differential tests use a six-cell fixture layout with the 33-position default geometry. The mode-specific geometry is selected by model name. `collision_transition` returns the cell palette index and `collision_background` supplies the RGB channels used by the renderer. `report` packs the five telemetry bytes after each tick. Tests compare the default frame and telemetry sequence against the DLL.
