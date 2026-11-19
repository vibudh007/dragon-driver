# Keyboard wave effects

`KeyboardPreview::wave_for_layout` implements mode 2 horizontal, vertical and slanted initialization, and mode 4 diagonal initialization. It selects the [vendor geometry](geometry.md) by key count. The `horizontal_wave_61`, `directional_wave_61`, and `diagonal_wave_61` functions apply that logic to the confirmed 61-key layout.

`Wave::tick` advances a mono cell by four brightness levels, reverses at overflow, and begins rising on the first frame below ten. The rainbow path advances each cell along the RGB edges in the selected direction. The step is six for mode 2 and nine for mode 4. `tick_wave_levels` is shared with side mode 2.

Initializer addresses are `0x1000c6d0` and `0x1000aaa0`. The update routines are `0x1000d6e0` and `0x1000b0e0`. The offline tests compare color and brightness for the observed 61-key layout and additional catalog layouts. [Fixture formats](../../../tests/fixtures/vendor-sdk/README.md)
