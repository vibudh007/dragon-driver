# Reactive lighting controls

`reactive_panels` adds both toggles on wired devices that advertise per-key lighting. Compatible 2.4 GHz devices get audio only. The panel sends start, stop and palette changes on the keyboard worker. Status fetches run while the panel is mapped and reflect the effect already running when the app reopens. Closing the window simply closes the client.

While a reactive effect is active, the panel locks manual lighting and the second reactive toggle. For wallpaper lighting, the daemon's last applied frame also supplies the keyboard preview. Audio accepts live custom color changes and restores the active palette choices when the app reopens. Startup shows a spinner after 500 ms. [Daemon renderer](../../reactive.md)

The GTK switch state-set signal submits a command with the requested value. The daemon reply synchronizes both GTK switch properties. An older status reply leaves a queued Start or Stop alone. The switch completes the transition when the command is accepted, and other lighting controls remain disabled until the renderer finishes restoring saved lighting.

With `DRAGON_DRIVER_LIGHT_TRACE=1`, the app logs each switch's active and backend state, sensitivity, queued requests, and daemon replies. The trace records whether stop or startup is in progress and omits wallpaper frame bytes.

An unchanged status reply must leave the switch untouched. GTK cancels the slider animation in `set_active` even when the requested value matches its active value. If that happens before the animation emits `state-set`, no command reaches the daemon. The graphical regression starts a switch animation and delivers the previous status before allowing the frame clock to finish.

On the wired K617M, the original trace showed Audio remaining on at the daemon when the GTK animation stalled before a Stop request. The graphical regression reproduced the loss on the old code. After installing the fix, the same user confirmed both switches completed while repeating the clicks in the Flatpak.
