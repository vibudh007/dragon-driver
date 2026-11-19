# Keyboard models

`layouts` parses the bundled vendor matrix data once. `find` searches the sorted collection by firmware-family name. `from_identity` uses the SDK's fifth identity field. The model count is a count of layout records. A live connection requires the vendor USB and interface rules, a recognized settings descriptor, and an exact layout identity. The reference family has live evidence. Other layouts use the same code when those checks pass.

`Position::new` accepts zero-based rows 0–5 and columns 0–21. `row` and `column` expose those coordinates. `Layout::key` returns the signed vendor key code at a populated position. `keys` iterates populated positions in row-major order. `magnetic` recognizes vendor device type 4. Photos, Qt widget geometry, and localized keycap artwork are separate from the protocol matrix.

`parse` requires complete model fields and six complete rows, including empty cells. `flag` decodes the vendor's binary fields. Column masks retain holes. The EXE's model constructor passes the resource row minus one and the key entry index to the widget position setter at `00423fb0`. The Apply handler's `00477518` instruction strides by 24 bytes in the column index, then indexes by four in the row index. The reference test checks an empty row and the Escape, backslash, and Fn positions. Vendor model trace

The source table provides onboard support, onboard selection availability, and the side-light flag and count. Five families have a nonzero side-light count with the side-light flag cleared. Side commands follow the flag. Hall-effect operations and advanced storage follow the EXE’s device type 4 branch. Other device types retain their applicable lighting and profile capabilities.

`Position` supports equality and hashing by its validated row and column so per-session readback can cache confirmed settings.

`geometry` reads the model’s vendor key rectangles for both the GUI keybed and daemon wallpaper sampling. [Geometry data](data/key-geometry.md)
