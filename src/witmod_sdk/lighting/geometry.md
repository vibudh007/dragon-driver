# Keyboard preview geometry

`geometry_index(key_count)` maps the DLL's key-count selectors to its table groups. It includes the original default branch. The key count comes from the identified model. A matching preview table establishes visual compatibility for that function.

`waves(key_count)` returns slices for horizontal, vertical, slanted and diagonal effects. Horizontal and vertical data are separate because the original routines use different strides. A slanted or diagonal record also carries per-column bit masks for the initial brightness phase. `sequential_path(key_count)` reads the count and packed row/column bytes from the sequential pattern.

The offline extractor records source addresses and file format. There are differential fixtures for the observed 61-key layout and for the 68-, 82-, and 98-key table groups. [Fixture formats](../../../tests/fixtures/vendor-sdk/README.md)
