use super::{
    CustomLightInfo, CustomLightKey, KeyAction, LightBank, LightItem, MacroEvent, MacroInfo,
    Profiles,
};
use crate::witmod_sdk::keymap::{KEY_COLUMNS, KEY_ROWS, KeyMap};
use std::io;

const RGB_BANK_BYTES: usize = KEY_ROWS * KEY_COLUMNS * 3;
const RGB_BANK_START: [usize; 2] = [0x103c, 0x1200];
const LIGHT_BLOCK_BYTES: usize = 0x200;
const LIGHT_DATA: usize = 10 * LIGHT_BLOCK_BYTES;
const LIGHT_BLOCKS: usize = 14;
const MACRO_BASE: usize = 0x600;
const MACRO_SLOT: usize = 0x100;
const MACRO_SLOTS: usize = 10;

impl Profiles {
    pub fn import_key_actions(
        &mut self,
        profile: &str,
        onboard: &[u8],
        key_map: &KeyMap,
    ) -> io::Result<bool> {
        let metadata_end = MACRO_BASE + (MACRO_SLOTS - 1) * MACRO_SLOT + 8;
        if onboard.len() < metadata_end {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "incomplete onboard key actions",
            ));
        }
        if self
            .config(profile)
            .and_then(|node| node.child("keyEvent"))
            .is_none()
        {
            return Ok(false);
        }

        let mut macro_index = 0;
        let mut edits = Vec::new();
        for (cell, record) in onboard[..KEY_ROWS * KEY_COLUMNS * 4]
            .chunks_exact(4)
            .enumerate()
        {
            let [opcode, first, second, third] =
                <[u8; 4]>::try_from(record).expect("four-byte key entry");
            let code = key_map
                .code(cell / KEY_COLUMNS, cell % KEY_COLUMNS)
                .expect("grid position");
            if code == 0 || (opcode == 0 && first == 0) {
                continue;
            }
            let Some(key_character) = key_map.label(code) else {
                break;
            };
            let key_code = code.to_string();
            let (operation_type, value) = match opcode {
                0 => ("0", first.to_string()),
                1 => ("0", "-10".to_owned()),
                0x10 if macro_index < MACRO_SLOTS => ("1", macro_index.to_string()),
                0x10 => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "too many onboard macro bindings",
                    ));
                }
                0x13 => {
                    let modifiers = modifier(first) + if third == 0 { 0 } else { modifier(second) };
                    let code = if third == 0 { second } else { third };
                    ("2", format!("{modifiers},{code}"))
                }
                0x07 => ("3", media_action(first).to_string()),
                0x12 => ("4", secondary_action(first, second).to_string()),
                _ => continue,
            };
            let (repeat_count, mode) = if operation_type == "1" {
                let metadata = MACRO_BASE + MACRO_SLOT * macro_index;
                macro_index += 1;
                (
                    onboard[metadata + 5].to_string(),
                    (u16::from(onboard[metadata + 1]) + 1).to_string(),
                )
            } else {
                ("1".to_owned(), "1".to_owned())
            };
            edits.push((
                key_code,
                key_character,
                operation_type,
                repeat_count,
                mode,
                value,
            ));
        }
        for (key_code, key_character, operation_type, repeat_count, mode, value) in edits {
            self.remove_key_action(profile, &key_code);
            self.append_key_action(
                profile,
                KeyAction {
                    key_code: &key_code,
                    key_character,
                    operation_type,
                    repeat_count: &repeat_count,
                    mode: &mode,
                    value: &value,
                },
            );
        }
        Ok(true)
    }

    pub fn import_bound_macros(
        &mut self,
        profile: &str,
        onboard: &[u8],
        key_map: &KeyMap,
    ) -> io::Result<bool> {
        let slots = onboard
            .get(MACRO_BASE..MACRO_BASE + MACRO_SLOTS * MACRO_SLOT)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "incomplete onboard macro slots",
                )
            })?;
        if self
            .config(profile)
            .and_then(|node| node.child("bindMacro"))
            .is_none()
        {
            return Ok(false);
        }
        for (index, slot) in slots.chunks_exact(MACRO_SLOT).enumerate() {
            if slot[15] == 0xff {
                break;
            }
            let events: Vec<_> = slot[8..]
                .chunks_exact(4)
                .filter(|event| event[0] != 0)
                .map(|event| decode_macro_event(event, key_map))
                .collect();
            if events.is_empty() {
                continue;
            }
            let records: Vec<_> = events
                .iter()
                .map(|event| MacroEvent {
                    mark: "mark-0123456789",
                    delay: &event.delay,
                    event_type: event.input,
                    key_code: &event.code,
                    code_name: event.label,
                    explanation: event.action,
                })
                .collect();
            let index_text = index.to_string();
            self.remove_bound_macro(profile, &index_text);
            self.append_bound_macro(
                profile,
                MacroInfo {
                    index: &index_text,
                    name: &format!("Macro{index}"),
                },
                &records,
            );
        }
        Ok(true)
    }

    pub fn import_light_config(&mut self, profile: &str, onboard: &[u8]) -> io::Result<bool> {
        let data = onboard
            .get(LIGHT_DATA..LIGHT_DATA + LIGHT_BLOCKS * LIGHT_BLOCK_BYTES)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "incomplete onboard light configuration",
                )
            })?;
        let mut blocks = data.chunks_exact(LIGHT_BLOCK_BYTES);
        let header = blocks.next().expect("fixed-length light data");
        let offset = light_slot(header) * 8 + 4;
        let using_index = i32::from(header[offset]);

        let mut items = Vec::with_capacity(20);
        for (block_index, block) in blocks.take(10).enumerate() {
            let start = light_slot(block) * 20 + 4;
            let entries = block.get(start..start + 18).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "light slot exceeds its block")
            })?;
            for (slot_index, entry) in entries.chunks_exact(9).enumerate() {
                let [brightness, speed, red, green, blue, bg_r, bg_g, bg_b, flags] =
                    <[u8; 9]>::try_from(entry)
                        .expect("nine-byte entry")
                        .map(i32::from);
                items.push(LightItem {
                    region: 0,
                    mode: (2 * block_index + slot_index) as i32,
                    brightness,
                    speed,
                    foreground: [red, green, blue],
                    background: [bg_r, bg_g, bg_b],
                    direction: (flags >> 1) & 7,
                    mode_type: flags & 1,
                    open: 0,
                    update: 0,
                });
            }
        }
        let bank = LightBank::Keyboard;
        if !self.set_light_using_index(profile, bank, using_index) {
            return Ok(false);
        }
        for item in items {
            if self.contains_light_mode(profile, bank, item.mode) {
                self.modify_light(profile, bank, item.mode, item);
            } else {
                self.add_light(profile, bank, item);
            }
        }
        Ok(true)
    }

    pub fn import_custom_rgb(
        &mut self,
        profile: &str,
        onboard: &[u8],
        key_map: &KeyMap,
    ) -> io::Result<bool> {
        if onboard.len() < RGB_BANK_START[1] + RGB_BANK_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "incomplete onboard RGB banks",
            ));
        }
        if self
            .config(profile)
            .and_then(|node| node.child("customLight"))
            .is_none()
        {
            return Ok(false);
        }

        for (bank, offset) in RGB_BANK_START.into_iter().enumerate() {
            let index = bank.to_string();
            if !self.contains_custom_light(profile, &index) {
                self.append_custom_light(
                    profile,
                    CustomLightInfo {
                        index: &index,
                        name: &format!("Custom {bank}"),
                        is_using: "UnUsing",
                        flag_index: -1,
                    },
                );
            }
            self.clear_custom_light(profile, &index);
            for (cell, channels) in onboard[offset..offset + RGB_BANK_BYTES]
                .chunks_exact(3)
                .enumerate()
            {
                if channels == [0; 3] {
                    continue;
                }
                let (row, column) = (cell / KEY_COLUMNS, cell % KEY_COLUMNS);
                let Some(code) = key_map.code(row, column).filter(|&code| code != 0) else {
                    break;
                };
                let Some(label) = key_map.label(code) else {
                    break;
                };
                self.append_custom_light_key(
                    profile,
                    &index,
                    CustomLightKey {
                        key_code: &code.to_string(),
                        code_name: label,
                        index_x: row as i32,
                        index_y: column as i32,
                        color: [channels[0], channels[1], channels[2]].map(i32::from),
                        transparency: 128,
                    },
                );
            }
        }
        Ok(true)
    }
}

fn modifier(value: u8) -> u16 {
    match value {
        0xe0..=0xe6 => 1 << (value - 0xe0),
        0xdd => 128,
        _ => 0,
    }
}

fn media_action(value: u8) -> u8 {
    match value {
        0x11..=0x16 => value - 0x10,
        0x20 => 12,
        0x23 => 11,
        0x30 => 8,
        0x32 => 9,
        0x34 => 10,
        _ => 0,
    }
}

fn secondary_action(first: u8, second: u8) -> u8 {
    if first == 1 {
        return 3;
    }
    match second {
        2 => 2,
        4 => 1,
        8 => 5,
        0x10 => 6,
        0xff => 4,
        _ => 0,
    }
}

struct DecodedMacro<'a> {
    delay: String,
    input: &'static str,
    code: String,
    label: &'a str,
    action: &'static str,
}

fn decode_macro_event<'a>(event: &[u8], key_map: &'a KeyMap) -> DecodedMacro<'a> {
    let (code, flags) = (u32::from(event[0]), event[1]);
    let key_label = key_map.label(code as i32).unwrap_or("keyMouse");
    let (input, code, label, action) = match (flags, code) {
        (0x11, 1) => ("mouse", 513, "Left Click Down", "down"),
        (1, 1) => ("mouse", 514, "Left Click Up", "up"),
        (0x11, 2) => ("mouse", 516, "Right Click Down", "down"),
        (1, 2) => ("mouse", 517, "Right Click Up", "up"),
        (0x11, 4) => ("mouse", 519, "Center Click Down", "down"),
        (1, 4) => ("mouse", 520, "Center Click Up", "up"),
        (0x11, _) => ("mouse", code, key_label, "down"),
        (1, _) => ("mouse", code, key_label, "up"),
        (0, _) => ("key", code, key_label, "up"),
        _ => ("key", code, key_label, "down"),
    };
    DecodedMacro {
        delay: u16::from_be_bytes([event[2], event[3]]).to_string(),
        input,
        code: code.to_string(),
        label,
        action,
    }
}

fn light_slot(block: &[u8]) -> usize {
    let mask = u32::from_le_bytes(block[..4].try_into().expect("four-byte slot mask"));
    mask.trailing_zeros().min(31) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_vendor_onboard_actions_and_macro_events() {
        let reference = Profiles::from_xml(include_bytes!(
            "../../../tests/fixtures/vendor-sdk/onboard-actions.xml"
        ))
        .unwrap();
        let mut profiles = Profiles::default();
        profiles.append_profile("3", "Events", false);
        let mut onboard = [0u8; 0x1000];
        for (index, action) in [
            [0, 4, 0, 0],
            [0, 0, 0, 0],
            [1, 0, 0, 0],
            [0x10, 0, 0, 0],
            [0x13, 0xe1, 4, 0],
            [0x13, 0xe0, 0xe2, 1],
            [0, 0, 0, 0],
            [7, 0x10, 0, 0],
            [7, 0x12, 0, 0],
            [0x12, 0, 4, 0],
            [0x12, 0, 0xff, 0],
        ]
        .into_iter()
        .enumerate()
        {
            onboard[index * 4..index * 4 + 4].copy_from_slice(&action);
        }
        onboard[0x601] = 2;
        onboard[0x605] = 7;
        let key_map = KeyMap::default();
        assert!(
            profiles
                .import_key_actions("3", &onboard, &key_map)
                .unwrap()
        );
        assert!(
            profiles
                .key_actions("3")
                .unwrap()
                .eq(reference.key_actions("3").unwrap())
        );

        for (index, event) in [
            [4, 0x10, 0, 20],
            [4, 0, 0, 200],
            [1, 0x11, 1, 16],
            [1, 1, 0, 1],
            [13, 0x10, 1, 0],
        ]
        .into_iter()
        .enumerate()
        {
            let start = 0x608 + index * 4;
            onboard[start..start + 4].copy_from_slice(&event);
        }
        onboard[0x70f] = 0xff;
        assert!(
            profiles
                .import_bound_macros("3", &onboard, &key_map)
                .unwrap()
        );
        assert_eq!(
            profiles.bound_macro_events("3", "0"),
            reference.bound_macro_events("3", "0")
        );
    }

    #[test]
    fn imports_selected_light_records() {
        let reference = Profiles::from_xml(include_bytes!(
            "../../../tests/fixtures/vendor-sdk/light-config.xml"
        ))
        .unwrap();
        let mut profiles = Profiles::default();
        profiles.append_profile("3", "Colors", false);
        let mut onboard = [0u8; 0x3000];
        for start in (LIGHT_DATA..onboard.len()).step_by(LIGHT_BLOCK_BYTES) {
            onboard[start] = 1;
        }
        onboard[0x1404] = 4;
        onboard[0x1600] = 2;
        onboard[0x1618..0x162a].copy_from_slice(&[
            52, 5, 20, 40, 60, 2, 4, 6, 7, 31, 2, 45, 23, 12, 2, 4, 8, 10,
        ]);
        assert!(profiles.import_light_config("3", &onboard).unwrap());
        assert!(profiles.import_light_config("3", &onboard).unwrap());
        assert_eq!(
            profiles.lights("3", LightBank::Keyboard),
            reference.lights("3", LightBank::Keyboard)
        );
        assert_eq!(
            profiles.light_using_index("3", LightBank::Keyboard),
            Some(4)
        );
    }

    #[test]
    fn imports_the_original_rgb_dump() {
        let reference = Profiles::from_xml(include_bytes!(
            "../../../tests/fixtures/vendor-sdk/custom-rgb.xml"
        ))
        .unwrap();
        let mut profiles = Profiles::default();
        profiles.append_profile("3", "Colors", false);
        let mut onboard = [0; RGB_BANK_START[1] + RGB_BANK_BYTES];
        onboard[0x103c..0x103f].copy_from_slice(b"ABC");
        onboard[0x1206..0x1209].copy_from_slice(b"DEF");
        assert!(
            profiles
                .import_custom_rgb("3", &onboard, &KeyMap::default())
                .unwrap()
        );
        assert_eq!(profiles.custom_lights("3"), reference.custom_lights("3"));
        for bank in ["0", "1"] {
            assert_eq!(
                profiles.custom_light_keys("3", bank),
                reference.custom_light_keys("3", bank)
            );
        }
    }
}
