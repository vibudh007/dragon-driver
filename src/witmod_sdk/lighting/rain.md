# Keyboard rain preview

`KeyboardPreview::rain` creates mode 15 with row counters `[21, 31, 41, 51, 61, 71]` from `0x1003fb10`. `Rain::tick` steps the six counters on the vendor interval. Counters above 21 suppress activation and each counter wraps as its row reaches the last position. The direction selects which edge activates first. Monochrome dims by seven and rainbow advances through the shared palette at steps of 40. `background_for` exposes the intermediate palette background for blending.

The DLL routines are `0x1000c160`, `0x1000c250`, and `0x10008f50`. The offline trace compares both mono and rainbow at six positions.
