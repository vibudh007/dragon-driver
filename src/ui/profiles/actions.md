# Key assignment editor

`dialog` displays controls for the selected action and loads the current mapping when available. Regular choices include key, modifiers, media, mouse buttons, shortcuts, macros, lighting controls, and applicable gamepad mappings. DKS, Mod-Tap, and Toggle are offered on type-4 models. `key_picker`, `numbered`, `select`, and `operation_name` translate between the editor and vendor operation values.

`DksEditor` edits four output keys, four travel points, and the per-point action matrix. `MacroEditor` reads and writes explicit key and mouse events, delays and playback mode. `store_advanced` validates the definition using the existing wire encoder and assigns a persistent XML identifier. The onboard compiler assigns its own slot during upload.

[Local macro recording](recording.md)

The editor stages text, installed application launch, and profile switch actions as vendor XML operations 6, 8, and 7 respectively. Their payloads remain in the host workspace and the onboard map holds the physical key code. [Protocol](../../keyboard/desktop.md)
