# Local macro recording

`attach` records keyboard events inside the focused macro text editor when the user starts recording. It captures no global input. `key_usage` translates common GTK key symbols into USB HID usages. Manual editing remains available for keys outside that mapping and mouse events.

`Recording::event` records time between events and clamps delays to the onboard representation. Press handling filters repeats and reserves room for releases within the device's 62-event limit. `release_all` balances any remaining held keys when the user stops. The user can edit and save the result before explicitly uploading it.
