# POS event record format

`append_pos_event_group` creates an `event` group. `append_pos_layer` creates a `layer` group. `append_named_group` writes their index and name attributes.

`append_pos_event` adds four signed numeric attributes to the selected event group. `pos_events` reads all records in a group. `pos_events_for_function` filters the first event group and preserves one-based document positions. `remove_pos_events` removes records with the selected mark. `event_group` matches groups by numeric index and `read_event` converts the ordered attributes to a typed record.

The source addresses are `0x100217c0` through `0x10022080` for creation and selection, and `0x1001d970` for deletion.

`CWitmodXML_POS_QueryLastMarkNum_128Bit` assembles and sorts a temporary list and then releases it. The x86 routine supplies no return value, output buffer, or XML update. The native POS record API has no duplicate call for this export.
