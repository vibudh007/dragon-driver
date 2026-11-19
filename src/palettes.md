# Saved keyboard lighting

`Palettes::open` reads `dragon-driver/palettes.ron` in the user's XDG data directory. The file stays at format 1. Optional fields record full keyboard and side effects and one active reactive preference. Older files still supply their saved custom frames and keyboard mode. Records match the model family and firmware version.

`write_keyboard_palette` writes the requested matrix and checks Custom mode and brightness. `confirm_slot` commits the confirmed frame, last used slot and returned effect together. Until confirmation succeeds, the previous committed matrix remains available for recovery after a restart. The firmware supplies no verified per-key RGB readback.

`set_lighting` records the observed effect's mode, brightness, speed, colors, direction and flags. `set_reactive` saves the selected mode and audio palette, including the wallpaper choice. `update` replaces the bounded RON file through GIO and synchronizes the file and directory before acknowledging a write. `clear_slots` discards stored lighting before an explicit full device reset.

`restore` runs with an exclusive broker descriptor on first attachment in each daemon session. It checks the firmware's advertised effect modes and replays both known custom pages with the last used page last. It restores and checks the saved effect after the page writes. A mode-only record reuses the parameters read from the keyboard until full results are saved. The daemon records later effect changes from the keyboard and avoids writing custom pages during ordinary polling. [Daemon lifecycle](bin/dragon-driver-daemon.md)
