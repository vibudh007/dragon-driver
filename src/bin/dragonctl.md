# dragonctl

The public CLI and GUI use the same Rust device operations. The GUI sends transport commands to a private `dragonctl` process for every HID exchange. The public CLI uses the installed daemon on a configured systemd desktop. A session holds exclusive access, so close the GTK app before running standalone commands that need the same keyboard.

```sh
dragonctl --help
dragonctl help set-key-colors
dragonctl --device /dev/hidraw4 --tsv lighting
dragonctl --device /dev/hidraw4 set-effect keyboard 4 rainbow=on
dragonctl --device /dev/hidraw4 set-key-colors 1 2:2=ff0000 3:1=0000ff
```

`run` parses global options, dispatches read and offline operations and gives every command its own usage and argument help. `settings_command` validates write arguments before opening the device where possible. Device queries with `--tsv` emit tab-separated labels and values. Discovery and layout listings use documented positional columns. Error text goes to stderr and invalid or failed commands return a failure status.

`palette` reads the daemon's saved slot and checks each requested matrix position against the model layout. `fill-colors` initializes a slot and `set-key-colors` preserves the others. An empty slot needs a full write. Successful palette writes and explicit effect changes update daemon state.

`set_effect` checks the advertised mode and keeps omitted settings. Device controls include actuation and Rapid Trigger, idle timers, shortcut locks, calibration, and gamepad mode on supported models. `apply-profile` uploads validated XML layers and optional advanced keys. Profile import and export write XML to stdout. `monitor-travel`, `calibrate`, `audio` and `live-receiver` use shared device sessions and handle Ctrl+C for cleanup. Desktop automation actions such as launching applications remain desktop-session features.

The offline fixture checks every help entry, input validation and report previews. Without the daemon feature the CLI supports direct validated HID access and offline tools. Palette persistence and desktop audio require the daemon feature.

`dragonctl audio-reactive [BACKGROUND LOW MEDIUM HIGH]` follows PipeWire playback with default or four supplied hex colors on USB. Receivers use their own colors. Ctrl+C ends the stream and releases the keyboard. The legacy `audio` command still accepts PCM.
