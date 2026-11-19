use std::env;
use std::fs::File;
use std::io::{self, Read, Write};
use std::num::NonZeroU32;
use std::path::Path;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use dragon_driver::keyboard::bindings::{encode_layer, encode_profile};
use dragon_driver::keyboard::hall::{self, Control, Thresholds, Tuning};
use dragon_driver::keyboard::lighting::{Effect, Region};
use dragon_driver::keyboard::models::{self, Layout, Position};
use dragon_driver::keyboard::performance::WinLock;
use dragon_driver::keyboard::session::Keyboard;
use dragon_driver::witmod_sdk::discovery::ConfigInterface;
use dragon_driver::witmod_sdk::hid::{ReportId, frame_write_report};
use dragon_driver::witmod_sdk::keymap::KeyMap;
use dragon_driver::witmod_sdk::profile::Profiles;
use dragon_driver::witmod_sdk::session::SdkSession;

const COMMANDS: &[(&str, &str, &str)] = &[
    (
        "list",
        "",
        "List supported USB interfaces. TSV columns are USB ID, node and interface kind.",
    ),
    (
        "identify",
        "",
        "Read the model, controller, firmware version and optional build stamp.",
    ),
    (
        "link-status",
        "",
        "Read transport, battery percentage, charging state and receiver state.",
    ),
    (
        "models",
        "",
        "List supported layout families without opening a keyboard.",
    ),
    (
        "layout",
        "FAMILY",
        "Print the zero-based row:column and vendor code for each key. FAMILY comes from models or identify.",
    ),
    (
        "travel",
        "ROW:COLUMN",
        "Read one key's configured actuation and Rapid Trigger distances.",
    ),
    (
        "monitor-travel",
        "ROW:COLUMN SECONDS",
        "Print measured key travel in tenths of a millimeter until the duration ends or you press Ctrl+C.",
    ),
    ("trigger-limit", "", "Read the maximum actuation depth."),
    (
        "disable-rapid-trigger",
        "ROW:COLUMN",
        "Disable Rapid Trigger and preserve the key's actuation threshold.",
    ),
    (
        "tuning",
        "",
        "Read the current anti-shake level and return-rate code.",
    ),
    (
        "set-control",
        "anti-shake|return-rate VALUE",
        "Set a hardware tuning value. Use the numeric codes reported by tuning.",
    ),
    (
        "lighting",
        "[keyboard|side]",
        "Read the active effect parameters. The default region is keyboard.",
    ),
    (
        "light-modes",
        "[keyboard|side]",
        "List the effect IDs and settings the selected region advertises.",
    ),
    (
        "set-effect",
        "keyboard|side MODE [NAME=VALUE ...]",
        "Set an advertised effect ID. Fields: brightness=1..256, speed=1..256, foreground=RRGGBB, background=RRGGBB, direction=0..255, rainbow=on|off, enabled=on|off. Omitted fields keep their current values.",
    ),
    (
        "set-static-color",
        "RRGGBB",
        "Switch to Static mode and verify the chosen RGB color. Accepts six hex digits with an optional #.",
    ),
    (
        "palette",
        "SLOT",
        "Print the app's saved per-key RGB by position. SLOT is 1 or 2. This reads the daemon's RON state.",
    ),
    (
        "fill-colors",
        "SLOT RRGGBB",
        "Replace every key color in the custom slot and save the palette. SLOT is 1 or 2.",
    ),
    (
        "set-key-colors",
        "SLOT ROW:COLUMN=RRGGBB [...]",
        "Paint one or more keys while preserving the rest of a saved palette. SLOT is 1 or 2. Use fill-colors to initialize an empty slot.",
    ),
    (
        "sleep",
        "",
        "Read lighting and keyboard idle timers in seconds.",
    ),
    (
        "set-sleep",
        "LIGHT_SECONDS KEYBOARD_SECONDS",
        "Set the two idle timers and preserve the device's other sleep flags.",
    ),
    (
        "shortcut-lock",
        "WINDOWS ALT_TAB ALT_F4 SHIFT_TAB",
        "Set shortcut locks. Each argument is on or off.",
    ),
    (
        "gamepad",
        "MODE",
        "Set the mode code for a keyboard with gamepad support. This can reconnect the device.",
    ),
    (
        "reset",
        "settings|actuation",
        "Restore device or actuation defaults. A device reset clears the app's stored palettes.",
    ),
    (
        "calibrate",
        "ROW:COLUMN|all SECONDS",
        "Calibrate keys while you press them. Stop early with Ctrl+C.",
    ),
    (
        "live-receiver",
        "SECONDS",
        "Stream the current effect over a supported 2.4 GHz receiver.",
    ),
    (
        "audio-reactive",
        "[BACKGROUND LOW MEDIUM HIGH]",
        "Follow PipeWire playback until Ctrl+C. Omit the four RRGGBB colors to use the defaults. Wireless receivers use their built-in colors.",
    ),
    (
        "audio",
        "FILE|-|desktop RRGGBB",
        "Stream music lighting from signed 16-bit little-endian 44.1 kHz mono PCM. Use - for stdin or desktop for the daemon monitor stream.",
    ),
    (
        "profiles",
        "FILE",
        "List profile indices and names in a vendor XML file.",
    ),
    (
        "export-profile",
        "FILE INDEX",
        "Write the selected profile as XML to stdout.",
    ),
    (
        "import-profile",
        "DESTINATION SOURCE INDEX",
        "Write a merged vendor XML profile file to stdout.",
    ),
    (
        "preview-layer",
        "FILE INDEX FAMILY BANK",
        "Validate a layer and print its report counts offline. BANK is an onboard layer index.",
    ),
    (
        "preview-profile",
        "FAMILY MAIN_FILE INDEX FN_FILE INDEX [ADVANCED_FILE]",
        "Validate a two-layer upload and print the report count without accessing a keyboard.",
    ),
    (
        "apply-profile",
        "FAMILY MAIN_FILE INDEX FN_FILE INDEX [ADVANCED_FILE]",
        "Upload both layers and optional advanced keys. FAMILY must match the connected keyboard.",
    ),
    (
        "preview-trigger",
        "FAMILY POSITIONS PRESS[:RELEASE] [RAPID_PRESS[:RAPID_RELEASE]]",
        "Print the SDK report without opening a keyboard. Use comma-separated zero-based ROW:COLUMN positions and integer tenths of a millimeter.",
    ),
    (
        "apply-trigger",
        "POSITIONS PRESS[:RELEASE] [RAPID_PRESS[:RAPID_RELEASE]]",
        "Write actuation settings to comma-separated zero-based ROW:COLUMN positions. Distances are integer tenths of a millimeter. Supplying Rapid distances enables Rapid Trigger.",
    ),
];

fn help(out: &mut impl Write, command: Option<&str>) -> io::Result<()> {
    if let Some(command) = command {
        let Some(&(name, args, description)) = COMMANDS.iter().find(|entry| entry.0 == command)
        else {
            return Err(invalid(format!(
                "unknown command {command:?}. See dragonctl --help"
            )));
        };
        writeln!(
            out,
            "Usage: dragonctl [--device /dev/hidrawN] [--tsv] {name} {args}\n\n{description}"
        )
    } else {
        writeln!(
            out,
            "Usage: dragonctl [--device /dev/hidrawN] [--tsv] COMMAND [ARG ...]\n       dragonctl help COMMAND\n"
        )?;
        writeln!(
            out,
            "Select an interface with --device when more than one keyboard is connected."
        )?;
        writeln!(
            out,
            "Use --tsv for script-friendly output from device queries. Errors go to stderr.\n"
        )?;
        for (name, args, _) in COMMANDS {
            writeln!(out, "  {name:<22} {args}")?;
        }
        writeln!(
            out,
            "\nEach command accepts --help. Example: dragonctl set-key-colors --help"
        )
    }
}

fn main() -> ExitCode {
    let args = env::args_os()
        .skip(1)
        .map(|arg| arg.into_string())
        .collect::<Result<Vec<_>, _>>();
    let Ok(args) = args else {
        eprintln!("dragonctl: arguments must be UTF-8");
        return ExitCode::FAILURE;
    };
    #[cfg(feature = "daemon")]
    if let [command, node] = args.as_slice()
        && command == "--config-transport"
    {
        return match dragon_driver::witmod_sdk::cli_transport::serve(
            Path::new(node),
            io::stdin().lock(),
            io::stdout().lock(),
        ) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("dragonctl: {error}");
                ExitCode::FAILURE
            }
        };
    }
    let mut output = io::stdout().lock();
    match run(&args, &mut output) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("dragonctl: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String], out: &mut impl Write) -> io::Result<()> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let (mut node, mut tsv, mut args) = (None, false, args.as_slice());
    loop {
        match args {
            ["--device", path, rest @ ..] if !path.starts_with('-') && node.is_none() => {
                node = Some(Path::new(path));
                args = rest;
            }
            ["--device", ..] => return Err(invalid("--device needs one /dev/hidrawN path")),
            ["--tsv", rest @ ..] => {
                tsv = true;
                args = rest;
            }
            _ => break,
        }
    }
    match args {
        [] | ["--help"] | ["-h"] | ["help"] => help(out, None),
        ["--version"] => writeln!(out, "dragonctl {}", env!("CARGO_PKG_VERSION")),
        ["help", command] | [command, "--help" | "-h"] => help(out, Some(command)),
        ["models"] => {
            for layout in models::layouts() {
                writeln!(out, "{}\t{} keys", layout.family, layout.keys().count())?;
            }
            Ok(())
        }
        ["layout", family] => print_layout(out, layout(family)?),
        ["preview-trigger", family, key_list, thresholds @ ..] => {
            let report = hall::apply(layout(family)?, &positions(key_list)?, tuning(thresholds)?)?;
            let report = frame_write_report(report, ReportId::Default)
                .map_err(|frame| invalid(format!("invalid report {frame:?}")))?;
            for chunk in report.chunks(16) {
                for (index, byte) in chunk.iter().enumerate() {
                    if index > 0 {
                        write!(out, " ")?;
                    }
                    write!(out, "{byte:02x}")?;
                }
                writeln!(out)?;
            }
            Ok(())
        }
        ["list"] => {
            let sdk = SdkSession::initialize()?;
            if sdk.candidates().is_empty() {
                return if tsv {
                    Ok(())
                } else {
                    writeln!(out, "No matching USB interfaces found")
                };
            }
            for candidate in sdk.candidates() {
                let usable = sdk
                    .devices()
                    .iter()
                    .any(|device| device.node() == candidate.node);
                writeln!(
                    out,
                    "{:04x}:{:04x}\t{}\t{}",
                    candidate.usb_id.vendor,
                    candidate.usb_id.product,
                    candidate.node.display(),
                    if usable { "configuration" } else { "candidate" }
                )?;
            }
            Ok(())
        }
        ["identify"] => {
            let sdk = configured_sdk()?;
            let identity = sdk.query_identity(select(&sdk, node)?)?;
            if tsv {
                writeln!(
                    out,
                    "controller\t{}\nmodel\t{}\nfirmware\t{}",
                    String::from_utf8_lossy(identity.controller()),
                    String::from_utf8_lossy(identity.layout_code()),
                    String::from_utf8_lossy(identity.firmware_version())
                )?;
                if let Some(stamp) = &identity.build_stamp {
                    writeln!(out, "built\t{stamp}")?;
                }
                return Ok(());
            }
            writeln!(
                out,
                "{} {} firmware {}",
                String::from_utf8_lossy(identity.controller()),
                String::from_utf8_lossy(identity.layout_code()),
                String::from_utf8_lossy(identity.firmware_version())
            )?;
            if let Some(stamp) = &identity.build_stamp {
                writeln!(out, "Built {stamp}")?;
            }
            Ok(())
        }
        ["travel", key] => {
            let key = position(key)?;
            let reply = open(node)?.read_key(key)?;
            if tsv {
                return writeln!(
                    out,
                    "mode\t{}\npress_tenths_mm\t{}\nrelease_tenths_mm\t{}\nrapid_press_tenths_mm\t{}\nrapid_release_tenths_mm\t{}",
                    reply.mode, reply.press, reply.release, reply.rapid_press, reply.rapid_release
                );
            }
            writeln!(
                out,
                "Mode {:#04x}\nPress {} mm\nRelease {} mm\nStored rapid press {} mm\nStored rapid release {} mm",
                reply.mode,
                millimeters(reply.press),
                millimeters(reply.release),
                millimeters(reply.rapid_press),
                millimeters(reply.rapid_release)
            )
        }
        ["trigger-limit"] => {
            let limit = open(node)?.actuation_limit()?;
            if tsv {
                return writeln!(out, "limit_tenths_mm\t{limit}");
            }
            writeln!(out, "Actuation limit {} mm", millimeters(limit))
        }
        ["link-status"] => {
            let status = open(node)?.read_link_status()?;
            if tsv {
                return writeln!(
                    out,
                    "transport\t{}\nbattery\t{}\ncharging\t{}\nreceiver\t{}",
                    status.transport,
                    status.battery_level,
                    status.charging_state,
                    status.dongle_state
                );
            }
            writeln!(
                out,
                "Transport {}\nBattery {}\nCharging state {}\nReceiver state {}",
                status.transport, status.battery_level, status.charging_state, status.dongle_state
            )
        }
        ["disable-rapid-trigger", key] => {
            let key = position(key)?;
            open(node)?.disable_rapid_trigger(key)?;
            writeln!(
                out,
                "Rapid Trigger is off at {}:{}",
                key.row(),
                key.column()
            )
        }
        ["tuning"] => {
            let mut keyboard = open(node)?;
            if tsv {
                return writeln!(
                    out,
                    "anti_shake\t{}\nreturn_rate\t{}",
                    keyboard.read_control(Control::AntiShake)?,
                    keyboard.read_control(Control::ReturnRate)?
                );
            }
            writeln!(
                out,
                "Anti-shake {}",
                keyboard.read_control(Control::AntiShake)?
            )?;
            writeln!(
                out,
                "Return rate {}",
                keyboard.read_control(Control::ReturnRate)?
            )
        }
        ["lighting", args @ ..] if args.len() <= 1 => {
            let effect =
                open(node)?.read_lighting(region(args.first().copied().unwrap_or("keyboard"))?)?;
            if tsv {
                return writeln!(
                    out,
                    "mode\t{}\nbrightness\t{}\nspeed\t{}\nforeground\t{}\nbackground\t{}\ndirection\t{}\nrainbow\t{}\nenabled\t{}",
                    effect.mode,
                    effect.brightness,
                    effect.speed,
                    color_hex(effect.foreground),
                    color_hex(effect.background),
                    effect.direction,
                    effect.effect_type,
                    effect.open
                );
            }
            writeln!(
                out,
                "Mode {}\nBrightness {}\nSpeed {}\nForeground {:02x}{:02x}{:02x}",
                effect.mode,
                effect.brightness,
                effect.speed,
                effect.foreground[0],
                effect.foreground[1],
                effect.foreground[2]
            )
        }
        ["light-modes", args @ ..] if args.len() <= 1 => {
            let modes = open(node)?
                .read_lighting_modes(region(args.first().copied().unwrap_or("keyboard"))?)?;
            if tsv {
                for mode in modes.modes {
                    writeln!(out, "mode\t{mode}")?;
                }
                for setting in modes.settings {
                    writeln!(out, "setting\t{setting}")?;
                }
                return Ok(());
            }
            writeln!(
                out,
                "Keyboard modes {:?}\nMode settings {:?}",
                modes.modes, modes.settings
            )
        }
        ["set-static-color", color] => {
            let color = rgb(color)?;
            let mut keyboard = open(node)?;
            keyboard.set_static_color(color)?;
            #[cfg(feature = "daemon")]
            if dragon_driver::service::require_user_service().is_ok() {
                let effect = keyboard.read_lighting(Region::Keyboard)?;
                record_effect(&keyboard, Region::Keyboard, effect)?;
            }
            let [red, green, blue] = color;
            writeln!(
                out,
                "Static keyboard color is #{red:02x}{green:02x}{blue:02x}"
            )
        }
        ["sleep"] => {
            let sleep = open(node)?.read_sleep()?;
            if tsv {
                return writeln!(
                    out,
                    "lights_seconds\t{}\nkeyboard_seconds\t{}\nflags\t{}",
                    sleep.lights_seconds, sleep.keyboard_seconds, sleep.flags
                );
            }
            writeln!(
                out,
                "Lighting {} s\nKeyboard {} s\nFlags {:#04x}",
                sleep.lights_seconds, sleep.keyboard_seconds, sleep.flags
            )
        }
        ["profiles", filename] => {
            let profiles = read_profiles(filename)?;
            for profile in profiles.list_profiles() {
                writeln!(out, "{}\t{:?}", profile.index, profile.name)?;
            }
            Ok(())
        }
        ["export-profile", filename, index] => {
            let source = read_profiles(filename)?;
            let mut exported = Profiles::default();
            if !source.export_profile(index, &mut exported, false) {
                return Err(invalid("profile index was not found"));
            }
            out.write_all(&exported.to_xml().map_err(io::Error::other)?)
        }
        ["import-profile", destination, source, index] => {
            let mut destination = read_profiles(destination)?;
            let source = read_profiles(source)?;
            if destination.import_profile(&source, index).is_none() {
                return Err(invalid(
                    "profile index was not found or the destination index overflowed",
                ));
            }
            out.write_all(&destination.to_xml().map_err(io::Error::other)?)
        }
        ["preview-layer", filename, index, family, bank] => {
            let profiles = read_profiles(filename)?;
            let bank = bank.parse().map_err(|_| invalid("invalid onboard bank"))?;
            let plan = encode_layer(&profiles, index, layout(family)?, bank)?;
            writeln!(
                out,
                "{} key reports, {} macro slots",
                plan.keys.len(),
                plan.macros.len()
            )
        }
        [
            command @ ("preview-profile" | "apply-profile"),
            family,
            main,
            index,
            function,
            fn_index,
            advanced @ ..,
        ] if advanced.len() <= 1 => {
            let main = read_profiles(main)?;
            let function = read_profiles(function)?;
            let advanced = advanced
                .first()
                .map(|path| read_profiles(path))
                .transpose()?;
            let plan = encode_profile(
                (&main, index),
                (&function, fn_index),
                advanced.as_ref(),
                layout(family)?,
            )?;
            if *command == "apply-profile" {
                let result = open(node)?.upload_profile(&plan)?;
                writeln!(out, "Profile upload: {result:?}")
            } else {
                writeln!(out, "{} reports for {family}", plan.packet_count())
            }
        }
        ["apply-trigger", keys, thresholds @ ..] => {
            let keys = positions(keys)?;
            let tuning = tuning(thresholds)?;
            open(node)?.apply(&keys, tuning)?;
            writeln!(
                out,
                "Report sent. Use travel to read the resulting key settings."
            )
        }
        rest => settings_command(rest, node, out),
    }
}

fn settings_command(args: &[&str], node: Option<&Path>, out: &mut impl Write) -> io::Result<()> {
    match args {
        ["set-control", command, value] => {
            let control = match *command {
                "anti-shake" => Control::AntiShake,
                "return-rate" => Control::ReturnRate,
                _ => return Err(invalid("control must be anti-shake or return-rate")),
            };
            let value = decimal(value, "control value")?;
            open(node)?.set_control(control, value)
        }
        ["set-effect", area, mode, options @ ..] => {
            set_effect(node, region(area)?, decimal(mode, "effect ID")?, options)
        }
        ["set-sleep", lights, keyboard] => {
            let lights_seconds = decimal(lights, "lighting seconds")?;
            let keyboard_seconds = decimal(keyboard, "keyboard seconds")?;
            let mut keyboard = open(node)?;
            let mut state = keyboard.read_sleep()?;
            state.lights_seconds = lights_seconds;
            state.keyboard_seconds = keyboard_seconds;
            keyboard.set_sleep(state)
        }
        ["shortcut-lock", windows, alt_tab, alt_f4, shift_tab] => {
            let lock = WinLock {
                windows_key: flag(windows)?,
                alt_tab: flag(alt_tab)?,
                alt_f4: flag(alt_f4)?,
                shift_tab: flag(shift_tab)?,
            };
            open(node)?.set_shortcut_lock(lock)
        }
        ["gamepad", mode] => {
            let mode = decimal(mode, "gamepad mode")?;
            open(node)?.set_gamepad_mode(mode)
        }
        ["reset", "settings" | "actuation"] => {
            let mut keyboard = open(node)?;
            if args[1] == "actuation" {
                return keyboard.reset_magnetic_settings();
            }
            #[cfg(feature = "daemon")]
            if dragon_driver::service::require_user_service().is_ok() {
                let (model, firmware) = palette_identity(&keyboard);
                dragon_driver::service::update_palette(
                    model,
                    &firmware,
                    dragon_driver::service::PaletteUpdate::Reset,
                )?;
            }
            keyboard.reset_settings()
        }
        ["monitor-travel", key, seconds] => {
            let (key, duration) = (position(key)?, duration(seconds)?);
            let _interrupt = Interrupt::install()?;
            let mut keyboard = open(node)?;
            let mut monitor = keyboard.monitor_travel(key)?;
            let deadline = Instant::now() + duration;
            let result = (|| {
                while let Some(wait) = remaining(deadline) {
                    if let Some(travel) = monitor.next_reading(wait)? {
                        writeln!(out, "{travel}")?;
                        out.flush()?;
                    }
                }
                Ok(())
            })();
            result.and(monitor.stop())
        }
        ["calibrate", key, seconds] => {
            let (key, duration) = (
                (*key != "all").then(|| position(key)).transpose()?,
                duration(seconds)?,
            );
            let _interrupt = Interrupt::install()?;
            let mut keyboard = open(node)?;
            let mut calibration = keyboard.calibrate(key)?;
            let deadline = Instant::now() + duration;
            let result = (|| {
                while let Some(wait) = remaining(deadline) {
                    if let Some(status) = calibration.progress(wait)? {
                        writeln!(out, "{status:?}")?;
                        out.flush()?;
                    }
                }
                Ok(())
            })();
            result.and(calibration.stop())
        }
        ["live-receiver", seconds] => {
            let duration = duration(seconds)?;
            let _interrupt = Interrupt::install()?;
            let mut keyboard = open(node)?;
            let deadline = Instant::now() + duration;
            keyboard.stream_wireless_lighting(|| remaining(deadline).is_none())
        }
        ["audio", path, color] => audio(node, path, rgb(color)?, out),
        ["audio-reactive"] => audio_reactive(
            node,
            dragon_driver::keyboard::lighting::DEFAULT_AUDIO_PALETTE,
        ),
        ["audio-reactive", background, low, medium, high] => audio_reactive(
            node,
            [rgb(background)?, rgb(low)?, rgb(medium)?, rgb(high)?],
        ),
        ["palette", slot] => palette(node, slot_index(slot)?, PaletteCommand::Read, out),
        ["fill-colors", slot, color] => palette(
            node,
            slot_index(slot)?,
            PaletteCommand::Fill(rgb(color)?),
            out,
        ),
        ["set-key-colors", slot, assignments @ ..] if !assignments.is_empty() => {
            let positions = assignments
                .iter()
                .map(|assignment| {
                    let (key, color) = assignment
                        .split_once('=')
                        .ok_or_else(|| invalid("expected ROW:COLUMN=RRGGBB"))?;
                    Ok((position(key)?, rgb(color)?))
                })
                .collect::<io::Result<Vec<_>>>()?;
            palette(
                node,
                slot_index(slot)?,
                PaletteCommand::Keys(positions),
                out,
            )
        }
        [command, ..] if COMMANDS.iter().any(|entry| entry.0 == *command) => Err(invalid(format!(
            "invalid arguments. See dragonctl {command} --help"
        ))),
        _ => Err(invalid("unknown command. See dragonctl --help")),
    }
}

fn set_effect(node: Option<&Path>, region: Region, mode: u8, options: &[&str]) -> io::Result<()> {
    fn value<T>(result: &mut Option<T>, value: T) -> io::Result<()> {
        if result.replace(value).is_some() {
            Err(invalid("each effect field can be specified once"))
        } else {
            Ok(())
        }
    }
    let (
        mut brightness,
        mut speed,
        mut foreground,
        mut background,
        mut direction,
        mut rainbow,
        mut enabled,
    ) = (None, None, None, None, None, None, None);
    for option in options {
        let (field, input) = option
            .split_once('=')
            .ok_or_else(|| invalid("expected NAME=VALUE"))?;
        match field {
            "brightness" => value(&mut brightness, level(input)?)?,
            "speed" => value(&mut speed, level(input)?)?,
            "foreground" => value(&mut foreground, rgb(input)?)?,
            "background" => value(&mut background, rgb(input)?)?,
            "direction" => value(&mut direction, decimal(input, "direction")?)?,
            "rainbow" => value(&mut rainbow, flag(input)?)?,
            "enabled" => value(&mut enabled, flag(input)?)?,
            _ => return Err(invalid(format!("unknown effect field {field:?}"))),
        }
    }
    let mut keyboard = open(node)?;
    if !keyboard.read_lighting_modes(region)?.modes.contains(&mode) {
        return Err(invalid("the keyboard did not advertise that effect"));
    }
    let previous = keyboard.read_lighting(region)?;
    let effect = Effect {
        mode,
        brightness: brightness.unwrap_or(previous.brightness),
        speed: speed.unwrap_or(previous.speed),
        foreground: foreground.unwrap_or(previous.foreground),
        background: background.unwrap_or(previous.background),
        direction: direction.unwrap_or(previous.direction),
        effect_type: rainbow.unwrap_or(previous.effect_type),
        open: enabled.unwrap_or(previous.open),
    };
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?;
    keyboard.set_lighting(region, effect, timestamp.as_millis() as u16)?;
    let actual = keyboard.read_lighting(region)?;
    if actual.mode != effect.mode {
        return Err(io::Error::other(
            "the keyboard did not confirm the effect ID",
        ));
    }
    record_effect(&keyboard, region, actual)?;
    Ok(())
}

enum PaletteCommand {
    Read,
    Fill([u8; 3]),
    Keys(Vec<(Position, [u8; 3])>),
}

fn palette(
    node: Option<&Path>,
    slot: u8,
    command: PaletteCommand,
    out: &mut impl Write,
) -> io::Result<()> {
    #[cfg(feature = "daemon")]
    {
        use dragon_driver::palettes::{Palettes, write_keyboard_palette};
        use dragon_driver::witmod_sdk::keymap::{KEY_COLUMNS, KEY_ROWS};
        let mut keyboard = open(node)?;
        let (model, firmware) = palette_identity(&keyboard);
        let stored = Palettes::open_frontend()?;
        let known = stored.slots(model, &firmware)[usize::from(slot)];
        let mut frame = match command {
            PaletteCommand::Fill(color) => [[color; KEY_COLUMNS]; KEY_ROWS],
            _ => known.ok_or_else(|| invalid("palette is unknown. Run fill-colors first"))?,
        };
        match command {
            PaletteCommand::Read => {
                for (key, _) in keyboard.layout().keys() {
                    let color = frame[usize::from(key.row())][usize::from(key.column())];
                    writeln!(out, "{}:{}\t{}", key.row(), key.column(), color_hex(color))?;
                }
                return Ok(());
            }
            PaletteCommand::Keys(keys) => {
                for (key, color) in keys {
                    if keyboard.layout().key(key).is_none() {
                        return Err(invalid(format!("no key at {}:{}", key.row(), key.column())));
                    }
                    frame[usize::from(key.row())][usize::from(key.column())] = color;
                }
            }
            PaletteCommand::Fill(_) => {}
        }
        if !keyboard
            .read_lighting_modes(Region::Keyboard)?
            .modes
            .contains(&10)
        {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "this keyboard does not advertise per-key lighting",
            ));
        }
        let brightness = keyboard.read_lighting(Region::Keyboard)?.brightness;
        write_keyboard_palette(&mut keyboard, slot, brightness, &frame)?;
        Ok(())
    }
    #[cfg(not(feature = "daemon"))]
    {
        let _ = (node, slot, out);
        match command {
            PaletteCommand::Read => {}
            PaletteCommand::Fill(color) => drop(color_hex(color)),
            PaletteCommand::Keys(keys) => drop(keys),
        }
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "palette commands need a build with the daemon feature",
        ))
    }
}

#[cfg(feature = "daemon")]
fn palette_identity(keyboard: &Keyboard) -> (&'static str, String) {
    (
        keyboard.layout().family,
        String::from_utf8_lossy(keyboard.identity().firmware_version()).into_owned(),
    )
}

fn record_effect(keyboard: &Keyboard, region: Region, effect: Effect) -> io::Result<()> {
    #[cfg(feature = "daemon")]
    if dragon_driver::service::require_user_service().is_ok() {
        let (model, firmware) = palette_identity(keyboard);
        return dragon_driver::service::update_palette(
            model,
            &firmware,
            dragon_driver::service::PaletteUpdate::Lighting { region, effect },
        );
    }
    let _ = (keyboard, region, effect);
    Ok(())
}

fn audio(node: Option<&Path>, path: &str, color: [u8; 3], out: &mut impl Write) -> io::Result<()> {
    let _interrupt = Interrupt::install()?;
    let source: Box<dyn Read> = match path {
        "-" => Box::new(io::stdin().lock()),
        #[cfg(feature = "daemon")]
        "desktop" => Box::new(dragon_driver::service::DesktopAudio::open(&INTERRUPTED)?),
        #[cfg(not(feature = "daemon"))]
        "desktop" => {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "desktop audio needs the daemon feature",
            ));
        }
        path => Box::new(File::open(path)?),
    };
    struct Reader<'a>(Box<dyn Read + 'a>);
    impl Read for Reader<'_> {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            if INTERRUPTED.load(Ordering::Relaxed) {
                Ok(0)
            } else {
                self.0.read(bytes)
            }
        }
    }
    let frames = open(node)?.stream_audio(
        &mut Reader(source),
        NonZeroU32::new(44100).unwrap(),
        55.0..7500.0,
        color,
    )?;
    writeln!(out, "{frames}")
}

fn audio_reactive(
    node: Option<&Path>,
    palette: dragon_driver::keyboard::lighting::AudioPalette,
) -> io::Result<()> {
    #[cfg(feature = "daemon")]
    {
        let _interrupt = Interrupt::install()?;
        let mut keyboard = open(node)?;
        let mut bars = dragon_driver::service::DesktopAudio::open_bars(&INTERRUPTED)?;
        keyboard.stream_audio_bars(&mut bars, || palette, || {})
    }
    #[cfg(not(feature = "daemon"))]
    {
        let _ = (node, palette);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "audio lighting needs the desktop daemon",
        ))
    }
}

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

extern "C" fn stop_on_interrupt(_: libc::c_int) {
    INTERRUPTED.store(true, Ordering::Relaxed);
}

struct Interrupt(libc::sigaction);

impl Interrupt {
    fn install() -> io::Result<Self> {
        // SAFETY: zero is valid for these signal action fields. Only SIGINT is changed.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            let mut previous: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = stop_on_interrupt as libc::sighandler_t;
            libc::sigemptyset(&raw mut action.sa_mask);
            if libc::sigaction(libc::SIGINT, &raw const action, &raw mut previous) == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(Self(previous))
        }
    }
}

impl Drop for Interrupt {
    fn drop(&mut self) {
        // SAFETY: restore the initialized action returned by sigaction.
        unsafe { libc::sigaction(libc::SIGINT, &raw const self.0, std::ptr::null_mut()) };
    }
}

fn remaining(deadline: Instant) -> Option<Duration> {
    let wait = deadline.saturating_duration_since(Instant::now());
    (!INTERRUPTED.load(Ordering::Relaxed) && !wait.is_zero())
        .then(|| wait.min(Duration::from_millis(100)))
}

fn duration(seconds: &str) -> io::Result<Duration> {
    let seconds = decimal(seconds, "duration in seconds")?;
    (1..=86_400)
        .contains(&seconds)
        .then(|| Duration::from_secs(seconds))
        .ok_or_else(|| invalid("duration must be from 1 to 86400 seconds"))
}

fn decimal<T: std::str::FromStr>(value: &str, label: &str) -> io::Result<T> {
    value
        .parse()
        .map_err(|_| invalid(format!("invalid {label}: {value:?}")))
}

fn level(value: &str) -> io::Result<u16> {
    let value = decimal(value, "brightness or speed")?;
    (1..=256)
        .contains(&value)
        .then_some(value)
        .ok_or_else(|| invalid("brightness and speed must be from 1 to 256"))
}

fn region(value: &str) -> io::Result<Region> {
    match value {
        "keyboard" => Ok(Region::Keyboard),
        "side" => Ok(Region::Side),
        _ => Err(invalid("region must be keyboard or side")),
    }
}

fn flag(value: &str) -> io::Result<u8> {
    match value {
        "off" => Ok(0),
        "on" => Ok(1),
        _ => Err(invalid("expected on or off")),
    }
}

fn slot_index(value: &str) -> io::Result<u8> {
    match value {
        "1" => Ok(0),
        "2" => Ok(1),
        _ => Err(invalid("custom slot must be 1 or 2")),
    }
}

fn color_hex([red, green, blue]: [u8; 3]) -> String {
    format!("{red:02x}{green:02x}{blue:02x}")
}

fn select<'a>(sdk: &'a SdkSession, node: Option<&Path>) -> io::Result<&'a ConfigInterface> {
    if let Some(path) = node {
        return sdk
            .devices()
            .iter()
            .find(|device| device.node() == path)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "no verified interface at that path",
                )
            });
    }
    match sdk.devices() {
        [device] => Ok(device),
        [] => Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no verified keyboard found",
        )),
        _ => Err(invalid("use --device to select a keyboard from list")),
    }
}

fn open(node: Option<&Path>) -> io::Result<Keyboard> {
    let sdk = configured_sdk()?;
    Keyboard::open(&sdk, select(&sdk, node)?)
}

fn configured_sdk() -> io::Result<SdkSession> {
    #[cfg(feature = "daemon")]
    if dragon_driver::service::uid() != 0 && dragon_driver::service::require_user_service().is_ok()
    {
        return SdkSession::initialize_broker();
    }
    SdkSession::initialize()
}

fn position(value: &str) -> io::Result<Position> {
    let parse = || {
        let (row, column) = value.split_once(':')?;
        Position::new(row.parse().ok()?, column.parse().ok()?)
    };
    parse().ok_or_else(|| invalid(format!("invalid key position {value}")))
}

fn positions(list: &str) -> io::Result<Vec<Position>> {
    let list: Vec<_> = list
        .splitn(133, ',')
        .map(position)
        .collect::<Result<_, _>>()?;
    if list.len() > 132 {
        return Err(invalid("at most 132 key positions are allowed"));
    }
    Ok(list)
}

fn tuning(values: &[&str]) -> io::Result<Tuning> {
    let parse = |value: &str| {
        if let Some((press, release)) = value.split_once(':') {
            Some(Thresholds::Separate {
                press: press.parse().ok()?,
                release: release.parse().ok()?,
            })
        } else {
            Some(Thresholds::Shared(value.parse().ok()?))
        }
    };
    let parse =
        |value: &str| parse(value).ok_or_else(|| invalid(format!("invalid threshold {value}")));
    match values {
        [actuation] => Ok(Tuning {
            actuation: parse(actuation)?,
            rapid: None,
        }),
        [actuation, rapid] => Ok(Tuning {
            actuation: parse(actuation)?,
            rapid: Some(parse(rapid)?),
        }),
        _ => Err(invalid("expected actuation and optional rapid thresholds")),
    }
}

fn layout(family: &str) -> io::Result<&'static Layout> {
    models::find(family).ok_or_else(|| invalid(format!("unknown layout {family}")))
}

fn read_profiles(path: &str) -> io::Result<Profiles> {
    const MAX_BYTES: u64 = 16 * 1024 * 1024;
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)?;
    Profiles::from_xml(&bytes)
}

fn print_layout(out: &mut impl Write, layout: &Layout) -> io::Result<()> {
    let labels = KeyMap::default();
    writeln!(out, "{}\t{} keys", layout.family, layout.keys().count())?;
    for (position, code) in layout.keys() {
        writeln!(
            out,
            "{}:{}\t{}\t{}",
            position.row(),
            position.column(),
            code,
            labels.label(i32::from(code)).unwrap_or("")
        )?;
    }
    Ok(())
}

fn millimeters(value: u8) -> String {
    format!("{}.{}", value / 10, value % 10)
}

fn rgb(value: &str) -> io::Result<[u8; 3]> {
    let value = value.strip_prefix('#').unwrap_or(value);
    if value.len() != 6 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid("expected six hexadecimal RGB digits"));
    }
    let [_, red, green, blue] = u32::from_str_radix(value, 16)
        .map_err(io::Error::other)?
        .to_be_bytes();
    Ok([red, green, blue])
}

fn invalid(message: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_runs_without_discovery() {
        let args = ["preview-trigger", "7153_AUSHERGB", "1:0", "15", "2"].map(str::to_owned);
        let mut out = Vec::new();
        run(&args, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("01 21 00 00 00 18 0c 02"));
        assert_eq!(text.split_whitespace().count(), 64);
    }

    #[test]
    fn every_command_has_help_without_device_access() {
        for &(command, _, description) in COMMANDS {
            for args in [[command, "--help"], ["help", command]] {
                let mut output = Vec::new();
                run(&args.map(str::to_owned), &mut output).unwrap();
                assert!(String::from_utf8(output).unwrap().contains(description));
            }
        }
    }

    #[test]
    fn rejects_invalid_values_before_opening_a_keyboard() {
        for (args, reason) in [
            (
                vec!["set-effect", "keyboard", "4", "brightness=0"],
                "from 1 to 256",
            ),
            (
                vec!["set-effect", "keyboard", "4", "rainbow=on", "rainbow=off"],
                "specified once",
            ),
            (
                vec!["set-key-colors", "3", "2:2=ff0000"],
                "slot must be 1 or 2",
            ),
            (vec!["calibrate", "all", "0"], "from 1 to 86400"),
        ] {
            let args: Vec<_> = args.into_iter().map(str::to_owned).collect();
            let error = run(&args, &mut Vec::new()).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
            assert!(error.to_string().contains(reason));
        }
    }
}
