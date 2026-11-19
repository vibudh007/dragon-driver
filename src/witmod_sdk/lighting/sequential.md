# Sequential keyboard preview

`KeyboardPreview::sequential_61` creates mode 5 for the vendor layout group covering key-count values 61 and 62. `sequential_for_layout` also selects the other vendor paths by key count. The 75 bytes beginning at `0x1003f5f8` specify the ordered position list. A byte packs its row in the upper three bits and column in the lower five.

`Sequential::activate` starts the current cell at the appropriate endpoint for mono or rainbow. `tick` advances cells in row-major order. It starts the next cell as soon as the mono level reaches 234 or lower, or the rainbow cell first reaches maximum brightness. A cell scheduled later in the scan can begin fading in the same frame. `background_for` returns the palette background used for intermediate rainbow blends.

The vendor initializer is `0x1000b850` and its update is `0x1000baa0`. Forward monochrome and reverse rainbow fixtures come from the DLL.
