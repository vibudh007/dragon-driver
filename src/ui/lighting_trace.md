# Lighting diagnostics

Set `DRAGON_DRIVER_LIGHT_TRACE=1` to write lighting diagnostics to standard error. Launch the app from a terminal and filter its output for `lighting-trace`. Entries include elapsed time, view changes, virtual key selections, palette availability, the full-palette button state, outbound lighting operations, and decoded device replies. Opening the app in this mode does not change device settings. Ordinary keyboard input and profile text are outside this trace.
