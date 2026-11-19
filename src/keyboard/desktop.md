# Vendor desktop bindings

`DesktopBinding::from_assignment` represents operation 6 as UTF-8 text, 7 as a profile index, and 8 as an application. `Planner::desktop_binding` puts `[0x16, 0, 0, physical_key_code]` in the onboard layer and stores the payload separately. Two layers can share a bound key when its desktop action is identical. The input event contains the key code and press state without a layer identifier.

`pressed_key` requires the open collection's report ID, command `0x20`, a nonzero value at offset 10, and a nonzero key code at offset 9. The session looks up that code in the last device-accepted profile snapshot. Vendor dispatcher · Onboard writer
