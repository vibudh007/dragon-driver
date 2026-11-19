use std::collections::HashMap;
use std::io;

use super::advanced;
use super::desktop::DesktopBinding;
use super::models::{Layout, Position};
use super::report::Packet;
use super::transfer::{advanced_slot, clear_macro_slot, macro_slot, onboard_chunk};
use crate::witmod_sdk::macros::{encode_onboard_events, event_code};
use crate::witmod_sdk::profile::{AdvancedAction, AdvancedKind, KeyAction, Profiles};

pub struct LayerPackets {
    pub keys: Vec<Packet>,
    pub macros: Vec<[Packet; 5]>,
}

#[derive(Clone, Copy)]
pub(crate) enum UploadStep {
    Data(Packet),
    Macro(Packet),
    Pause,
}

pub struct ProfilePlan {
    pub(crate) family: &'static str,
    pub(crate) steps: Vec<UploadStep>,
    pub(crate) desktop: HashMap<u8, DesktopBinding>,
}

impl ProfilePlan {
    pub fn has_desktop_bindings(&self) -> bool {
        !self.desktop.is_empty()
    }

    pub fn packet_count(&self) -> usize {
        self.steps
            .iter()
            .filter(|step| !matches!(step, UploadStep::Pause))
            .count()
    }
}

#[derive(Default)]
struct Planner<'a> {
    steps: Vec<UploadStep>,
    macro_count: u8,
    advanced_count: u8,
    advanced: HashMap<i32, AdvancedAction<'a>>,
    desktop: HashMap<u8, DesktopBinding>,
}

pub fn encode_profile(
    primary: (&Profiles, &str),
    function: (&Profiles, &str),
    advanced_profiles: Option<&Profiles>,
    layout: &Layout,
) -> io::Result<ProfilePlan> {
    let mut planner = Planner::default();
    if let Some(doc) = advanced_profiles {
        for kind in [
            AdvancedKind::Dks,
            AdvancedKind::ModTap,
            AdvancedKind::Toggle,
        ] {
            for item in doc.advanced_items(kind).unwrap_or_default() {
                if planner.advanced.insert(item.index, item.action).is_some() {
                    return Err(invalid("advanced-key indices must be unique"));
                }
            }
        }
    }
    planner.layer(primary.0, primary.1, layout, 0)?;
    planner.layer(function.0, function.1, layout, 2)?;
    planner.steps.push(UploadStep::Pause);
    for slot in planner.macro_count..10 {
        planner.steps.extend(
            clear_macro_slot(slot)
                .expect("valid slot")
                .map(UploadStep::Macro),
        );
    }
    // The EXE's advanced-key branch is enabled by the type-4 model rule.
    if layout.magnetic() {
        planner.steps.push(UploadStep::Pause);
        for slot in planner.advanced_count..40 {
            planner.steps.push(UploadStep::Data(
                advanced_slot(slot, 0, [0; 50]).expect("valid slot"),
            ));
        }
    }
    Ok(ProfilePlan {
        family: layout.family,
        steps: planner.steps,
        desktop: planner.desktop,
    })
}

pub fn encode_layer(
    profiles: &Profiles,
    index: &str,
    layout: &Layout,
    bank: u8,
) -> io::Result<LayerPackets> {
    let mut planner = Planner::default();
    planner.layer(profiles, index, layout, bank)?;
    let mut keys = Vec::new();
    let mut macros = Vec::new();
    for step in planner.steps {
        match step {
            UploadStep::Data(report) => keys.push(report),
            UploadStep::Macro(report) => macros.push(report),
            UploadStep::Pause => unreachable!("layer emits only packets"),
        }
    }
    let (blocks, remainder) = macros.as_chunks();
    debug_assert!(remainder.is_empty());
    Ok(LayerPackets {
        keys,
        macros: blocks.to_vec(),
    })
}

impl Planner<'_> {
    fn layer(
        &mut self,
        profiles: &Profiles,
        index: &str,
        layout: &Layout,
        bank: u8,
    ) -> io::Result<()> {
        if !layout.onboard_profiles {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "model has no onboard profiles",
            ));
        }
        let mappings = profiles
            .key_actions(index)
            .ok_or_else(|| invalid("profile has no key map"))?;
        let mut assignments = HashMap::new();
        for action in mappings {
            let code = number::<i16>(action.key_code)?;
            assignments.entry(code).or_insert(action);
        }
        let mut payload = [0; 56];
        let mut used = 0;
        let mut packet_index = 0;
        for row in 0..6 {
            for column in 0..22 {
                let code = Position::new(row, column)
                    .and_then(|position| layout.key(position))
                    .unwrap_or(0);
                let record = match assignments.get(&code).filter(|_| code != 0) {
                    Some(action) if action.operation_type == "1" => {
                        self.add_macro(profiles, index, *action)?
                    }
                    Some(action) if action.operation_type == "40" => {
                        if !layout.magnetic() {
                            return Err(io::Error::new(
                                io::ErrorKind::Unsupported,
                                "advanced keys require a type-4 layout",
                            ));
                        }
                        self.add_advanced(number(action.value)?)?
                    }
                    Some(action) if matches!(action.operation_type, "6" | "7" | "8") => {
                        self.desktop_binding(code, *action)?
                    }
                    Some(action) => binding(*action)?,
                    None => default_record(code),
                };
                payload[used..used + 4].copy_from_slice(&record);
                used += 4;
                if used == payload.len() {
                    self.steps.push(UploadStep::Data(
                        onboard_chunk(bank, packet_index, &payload).expect("valid key chunk"),
                    ));
                    used = 0;
                    packet_index += 1;
                }
            }
        }
        if used != 0 {
            self.steps.push(UploadStep::Data(
                onboard_chunk(bank, packet_index, &payload[..used]).expect("valid key chunk"),
            ));
        }
        Ok(())
    }

    fn desktop_binding(&mut self, code: i16, action: KeyAction<'_>) -> io::Result<[u8; 4]> {
        let code =
            u8::try_from(code).map_err(|_| invalid("This key cannot trigger desktop actions"))?;
        let binding = DesktopBinding::from_assignment(action)?.expect("desktop assignment branch");
        if self
            .desktop
            .get(&code)
            .is_some_and(|previous| previous != &binding)
        {
            return Err(invalid(
                "A key must have the same desktop action on both layers when assigned on both",
            ));
        }
        self.desktop.insert(code, binding);
        Ok([0x16, 0, 0, code])
    }

    fn add_macro(
        &mut self,
        profiles: &Profiles,
        index: &str,
        action: KeyAction<'_>,
    ) -> io::Result<[u8; 4]> {
        let events = profiles
            .bound_macro_events(index, action.value)
            .ok_or_else(|| invalid("missing bound macro"))?;
        if events.len() > 62 {
            return Err(invalid("onboard macros support up to 62 events"));
        }
        for event in &events {
            if event_code(event).is_none()
                || (event.event_type == "key" && event.key_code.trim().parse::<u8>().is_err())
                || !matches!(event.explanation, "up" | "down")
                || event.delay.trim().parse::<u16>().is_err()
            {
                return Err(invalid(
                    "macro event cannot be represented in onboard storage",
                ));
            }
        }
        let words = encode_onboard_events(&events).map_err(io::Error::other)?;
        let bytes = words
            .into_iter()
            .take(events.len() * 4)
            .map(|word| {
                u8::try_from(word).map_err(|_| invalid("macro code or delay exceeds its field"))
            })
            .collect::<io::Result<Vec<_>>>()?;
        let slot = self.macro_count;
        let packets = macro_slot(
            slot,
            number(action.mode)?,
            number(action.repeat_count)?,
            &bytes,
        )
        .ok_or_else(|| invalid("too many onboard macros or invalid macro settings"))?;
        self.steps.extend(packets.map(UploadStep::Macro));
        self.macro_count += 1;
        Ok([0x10, slot, 0, 0])
    }

    fn add_advanced(&mut self, id: i32) -> io::Result<[u8; 4]> {
        let slot = self.advanced_count;
        let action = *self
            .advanced
            .get(&id)
            .ok_or_else(|| invalid("missing advanced-key definition"))?;
        self.steps
            .push(UploadStep::Data(advanced::encode(slot, action)?));
        self.advanced_count += 1;
        Ok([0x17, slot, 0, 0])
    }
}

fn binding(action: KeyAction<'_>) -> io::Result<[u8; 4]> {
    const MEDIA: [u8; 16] = [
        0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x30, 0x32, 0x34, 0x23, 0x20, 0x38, 0x39,
        0x21,
    ];
    match action.operation_type {
        "0" => match number::<i16>(action.value)? {
            -20 => Ok([0x14, 0, 0, 0]),
            -10 => Ok([1, 0, 0, 0]),
            code => Ok([
                0,
                u8::try_from(code).map_err(|_| invalid("invalid remap key code"))?,
                0,
                0,
            ]),
        },
        "2" => combination(action.value),
        "3" => MEDIA
            .get(number::<usize>(action.value)?)
            .map(|code| [7, *code, 0, 0])
            .ok_or_else(|| invalid("invalid multimedia index")),
        "4" => {
            let index = number::<usize>(action.value)?;
            let flags = [1, 4, 2, 1, 0xff, 0x10, 8]
                .get(index)
                .ok_or_else(|| invalid("invalid secondary action index"))?;
            Ok([0x12, u8::from(index == 3), *flags, 0])
        }
        "5" => Ok([0; 4]),
        "9" => {
            let shortcuts = [
                [0xe2, 0x3d],
                [0xe2, 0x2b],
                [0xe3, 7],
                [0xe3, 8],
                [0xe3, 0x0f],
                [0xe0, 6],
                [0xe0, 0x1b],
                [0xe0, 0x19],
                [0xe0, 9],
                [0xe0, 0x11],
                [0xe0, 0x13],
                [0xe0, 0x16],
            ];
            let &[modifier, code] = shortcuts
                .get(number::<usize>(action.value)?)
                .ok_or_else(|| invalid("invalid shortcut index"))?;
            Ok([0x13, modifier, code, 0])
        }
        "27" => Ok([9, 0, 0, 0]),
        operation => {
            let opcode = match operation {
                "20" => 2,
                "21" => 3,
                "22" => 4,
                "23" => 5,
                "24" => 6,
                "25" => 8,
                "30" => 10,
                "31" => 11,
                "32" => 12,
                "33" => 13,
                "45" => 0x18,
                "46" => 0x19,
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "unsupported profile action",
                    ));
                }
            };
            Ok([opcode, number(action.value)?, 0, 0])
        }
    }
}

pub(super) fn combination(value: &str) -> io::Result<[u8; 4]> {
    let (mask, key) = value
        .split_once(',')
        .ok_or_else(|| invalid("invalid modifier binding"))?;
    let mask = number::<u8>(mask)?;
    let key = number::<u8>(key)?;
    let mut modifiers = (0..8)
        .filter(|&bit| mask & (1 << bit) != 0)
        .map(|bit| 0xe0 + bit);
    match (modifiers.next(), modifiers.next(), modifiers.next()) {
        (Some(first), None, None) => Ok([0x13, first, key, 0]),
        (Some(first), Some(second), None) => Ok([0x13, first, second, key]),
        _ => Err(invalid("a combination requires one or two modifiers")),
    }
}

fn default_record(code: i16) -> [u8; 4] {
    match code {
        -10 => [1, 0, 0, 0],
        0xd7 => [7, 0x32, 0, 0],
        0x7f => [7, 0x12, 0, 0],
        0x81 => [7, 0x11, 0, 0],
        0x80 => [7, 0x10, 0, 0],
        value => [0, value.clamp(0, 255) as u8, 0, 0],
    }
}

fn number<T: std::str::FromStr>(value: &str) -> io::Result<T> {
    value
        .parse()
        .map_err(|_| invalid("invalid numeric profile value"))
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_pack_uses_physical_key_and_keeps_payload_off_the_wire() {
        use crate::keyboard::desktop::DesktopBinding;
        let layout = crate::keyboard::models::find("7153_AUSHERGB").unwrap();
        let mut profiles = Profiles::default();
        profiles.append_profile("1", "Main", false);
        profiles.append_profile("2", "Fn", false);
        let make = |value| KeyAction {
            key_code: "26",
            key_character: "W",
            operation_type: "6",
            mode: "1",
            repeat_count: "1",
            value,
        };
        assert!(profiles.append_key_action(
            "1",
            make(
                "λ
Hello"
            )
        ));
        let plan = encode_profile((&profiles, "1"), (&profiles, "2"), None, layout).unwrap();
        assert_eq!(
            plan.desktop.get(&26),
            Some(&DesktopBinding::Text(
                "λ
Hello"
                    .into()
            ))
        );
        let UploadStep::Data(report) = plan.steps[3] else {
            panic!("ordinary key chunk")
        };
        assert_eq!(&report[22..26], &[0x16, 0, 0, 26]);
        assert!(profiles.append_key_action("2", make("different")));
        assert!(encode_profile((&profiles, "1"), (&profiles, "2"), None, layout).is_err());
    }

    #[test]
    fn whole_profile_keeps_banks_and_advanced_slots_in_vendor_order() {
        use crate::witmod_sdk::profile::AdvancedItem;

        let mut profiles = Profiles::default();
        profiles.append_profile("1", "Main", false);
        profiles.append_profile("2", "Fn", false);
        assert!(profiles.append_key_action(
            "1",
            KeyAction {
                key_code: "26",
                key_character: "W",
                operation_type: "40",
                repeat_count: "1",
                mode: "1",
                value: "7",
            }
        ));
        let mut definitions = Profiles::advanced_document();
        assert!(definitions.append_advanced_item(AdvancedItem {
            index: 7,
            name: "W toggle",
            action: AdvancedAction::Toggle {
                key_code: "26",
                status: 1
            },
        }));
        let model = crate::keyboard::models::find("7153_AUSHERGB").unwrap();
        assert!(encode_profile((&profiles, "1"), (&profiles, "2"), None, model).is_err());
        let plan = encode_profile(
            (&profiles, "1"),
            (&profiles, "2"),
            Some(&definitions),
            model,
        )
        .unwrap();
        assert_eq!(plan.packet_count(), 110);
        let data: Vec<_> = plan
            .steps
            .iter()
            .filter_map(|step| match step {
                UploadStep::Data(packet) => Some(packet),
                _ => None,
            })
            .collect();
        assert_eq!(data[3][1], 0x2a);
        assert_eq!(&data[3][6..11], &[3, 0, 26, 0, 0]);
        assert_eq!(&data[4][22..26], &[0x17, 0, 0, 0]);
        assert_eq!(data[11][2], 2);
        assert_eq!(data.last().unwrap()[2], 39);
    }

    #[test]
    fn all_vendor_layouts_use_their_profile_capabilities() {
        use crate::keyboard::models::layouts;

        let mut profiles = Profiles::default();
        profiles.append_profile("1", "Default", false);
        for model in layouts() {
            let result = encode_profile((&profiles, "1"), (&profiles, "1"), None, model);
            if model.onboard_profiles {
                let plan = result.unwrap_or_else(|error| panic!("{}: {error}", model.family));
                assert_eq!(plan.packet_count(), if model.magnetic() { 110 } else { 70 });
            } else {
                assert!(matches!(result, Err(error) if error.kind() == io::ErrorKind::Unsupported));
            }
        }
    }

    #[test]
    fn default_layer_retains_matrix_holes_and_key_order() {
        let mut profiles = Profiles::default();
        profiles.append_profile("1", "Default", false);
        let model = crate::keyboard::models::find("7153_AUSHERGB").unwrap();
        let plan = encode_layer(&profiles, "1", model, 0).unwrap();
        assert_eq!(plan.keys.len(), 10);
        assert_eq!(&plan.keys[1][38..42], &[0, 41, 0, 0]);
        assert!(plan.macros.is_empty());
        assert_eq!(combination("3,41").unwrap(), [0x13, 0xe0, 0xe1, 41]);
        assert!(combination("7,41").is_err());
    }
}
