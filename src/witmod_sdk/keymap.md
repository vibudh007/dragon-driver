# Vendor default key map

`KeyMap::default()` initializes 112 code labels from `FUN_100194c0` and the 132 signed matrix cells constructed by `FUN_1001a810`. The matrix layout matches the DLL's six rows and 22 columns. Values are signed, including the original negative Fn code. These are SDK defaults. A physical device layout may supply different values.

`label(code)` borrows the label for a code. `code(row, column)` returns the matrix cell or no value outside the grid. `replace_labels(labels)` and `replace_matrix(matrix)` are the owned Rust equivalents of the full replacement operations in the two XML key-map initialization exports.

Default labels use the static sorted vendor table. Custom callers can replace the full map, and missing custom labels remain absent.
