# Side preview geometry

`wave_layout(model)` selects the DLL's mode 2 table by ASCII case-insensitive layout code and supplies the cutoff for its starting phase. `matches` handles aliases. `SidePreview::wave_for_model` uses the vendor’s slot order. A participating slot advances through the table. An inactive slot uses the default color or intensity and inherits the current phase. The source initializer and tick routines are `0x1000eb80` and `0x100103b0`. Some model names have the default wave shape, including the K617M layout.

`collision_span(model)` selects the distance used by the two opposing highlights independently of the number of displayed slots. The source is `0x1000e1f0`. The default is 33.

The table extracts 20 named side geometries from the DLL resource data. Rust rejects slot lists that exceed the available table. The vendor K665 fixtures verify both one-color and rainbow waves with inactive slots interleaved. They are software previews from Wine. Table extractor
