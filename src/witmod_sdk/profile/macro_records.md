# Macro XML records

- `list_macros`, `rename_macro`, and `remove_macro` work on the root's indexed macro nodes.
- `remove_macro_events` removes all event items sharing a mark. `update_macro_event` selects the first or second item with that mark and edits four values, preserving the mark and explanation. `move_macro_event` moves that selected item one element in the requested direction.
- `append_bound_macro`, `bound_macro_events`, and `remove_bound_macro` manage the `bindMacro` subtrees attached to a profile.
- `Occurrence::index` converts first or second to a zero-based lookup. `macro_mut` finds a named macro index. `marked` and `is_element` filter event nodes and adjacent elements. `events` and `append_events` share the vendor six-attribute record format across standalone and bound macros.

The XML routines at `0x1001d700`, `0x1001fcd0`, and `0x10028080` establish mark-based edit and move behavior. Bound macros are drawn from `0x1001be30`, `0x1001dc30`, and `0x100232b0`. The Rust module preserves the original element ordering for normal profile documents.

`macro_info` returns the matched macro's index and name. A missing index also covers the vendor's Boolean existence check.
