# Wallpaper colors for audio lighting

`extract` uses the bounded wallpaper decoder and resizes the image to 96 × 64. `prominent` groups pixels in a 4-bit-per-channel histogram, weights candidate centers by represented population and distance, and refines the four colors against the weighted samples. A solid image gives four identical colors. Colors are sorted by HSV saturation with brightness as the tie breaker for Background, Low, Medium, and High. A worker recomputes the palette when a wallpaper event arrives.
