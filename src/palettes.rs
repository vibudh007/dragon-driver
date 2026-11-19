use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

use gio::{glib, prelude::*};
use serde::{Deserialize, Serialize};

use crate::keyboard::{
    lighting::{Effect, Region, RgbFrame},
    session::Keyboard,
};
use crate::service::{self, PaletteUpdate, ReactiveMode, ReactiveTarget};

const FORMAT: u8 = 1;
pub(crate) const MAX_BYTES: u64 = 1024 * 1024;

pub fn write_keyboard_palette(
    keyboard: &mut Keyboard,
    slot: u8,
    brightness: u16,
    frame: &RgbFrame,
) -> io::Result<Effect> {
    keyboard.recover_reads()?;
    let model = keyboard.layout().family;
    let firmware = String::from_utf8_lossy(keyboard.identity().firmware_version()).into_owned();
    keyboard.apply_custom_colors(slot, 10, brightness, 1, frame)?;
    let effect = keyboard.read_lighting(Region::Keyboard)?;
    if effect.mode != 10 || effect.brightness != brightness {
        return Err(io::Error::other(
            "The keyboard did not confirm Custom mode and brightness",
        ));
    }
    service::update_palette(
        model,
        &firmware,
        PaletteUpdate::ConfirmedSlot {
            slot,
            frame: Box::new(*frame),
            effect,
        },
    )?;
    Ok(effect)
}

#[derive(Clone, Deserialize, Serialize)]
struct Model {
    firmware: String,
    initialization_attempted: bool,
    effect: Option<u8>,
    #[serde(default)]
    lighting: [Option<Effect>; 2],
    last_slot: Option<u8>,
    slots: [Option<RgbFrame>; 2],
}

impl Model {
    fn custom_pages(&self) -> impl Iterator<Item = (u8, &RgbFrame)> {
        let active = self.last_slot.filter(|slot| *slot < 2).unwrap_or_default();
        [1 - active, active].into_iter().filter_map(|slot| {
            self.slots[usize::from(slot)]
                .as_ref()
                .map(|frame| (slot, frame))
        })
    }

    fn mode_for(&self, region: Region) -> Option<u8> {
        self.lighting[region.index()]
            .map(|effect| effect.mode)
            .or_else(|| {
                if region == Region::Keyboard {
                    self.effect
                } else {
                    None
                }
            })
    }

    fn restore_target(&self, region: Region, advertised: &[u8], current: Effect) -> Effect {
        self.mode_for(region)
            .filter(|mode| advertised.contains(mode))
            .map_or(current, |mode| {
                self.lighting[region.index()].unwrap_or(Effect { mode, ..current })
            })
    }
}

#[derive(Deserialize, Serialize)]
struct Data {
    version: u8,
    #[serde(default)]
    reactive: Option<(ReactiveTarget, ReactiveMode)>,
    models: BTreeMap<String, Model>,
}

pub struct Palettes {
    path: PathBuf,
    data: Data,
}

impl Palettes {
    pub fn open() -> io::Result<Self> {
        Self::read(glib::user_data_dir().join("dragon-driver/palettes.ron"))
    }

    pub fn open_frontend() -> io::Result<Self> {
        let bytes = crate::service::read_saved_palettes()?;
        Ok(Self {
            path: PathBuf::new(),
            data: Self::decode(&bytes)?,
        })
    }

    fn decode(bytes: &[u8]) -> io::Result<Data> {
        let data: Data = ron::de::from_bytes(bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if data.version != FORMAT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Unsupported palette file version",
            ));
        }
        Ok(data)
    }

    fn read(path: PathBuf) -> io::Result<Self> {
        let data: Data = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&path)
        {
            Ok(file) => {
                if !file.metadata()?.is_file() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Invalid palette file",
                    ));
                }
                let mut data = Vec::new();
                file.take(MAX_BYTES + 1).read_to_end(&mut data)?;
                if data.len() as u64 > MAX_BYTES {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Palette file is too large",
                    ));
                }
                Self::decode(&data)?
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Data {
                version: FORMAT,
                reactive: None,
                models: BTreeMap::new(),
            },
            Err(error) => return Err(error),
        };
        Ok(Self { path, data })
    }

    fn model(&self, model: &str, firmware: &str) -> Option<&Model> {
        self.data
            .models
            .get(model)
            .filter(|entry| entry.firmware == firmware)
    }

    pub fn slots(&self, model: &str, firmware: &str) -> [Option<RgbFrame>; 2] {
        self.model(model, firmware)
            .map_or([None; 2], |entry| entry.slots)
    }

    pub fn last_slot(&self, model: &str, firmware: &str) -> u8 {
        self.model(model, firmware)
            .and_then(|entry| entry.last_slot)
            .filter(|slot| *slot < 2)
            .unwrap_or_default()
    }

    pub fn needs_initialization(&self, model: &str) -> bool {
        !self
            .data
            .models
            .get(model)
            .is_some_and(|entry| entry.initialization_attempted)
    }

    pub fn set_slot(
        &mut self,
        model: &str,
        firmware: &str,
        slot: u8,
        frame: Option<RgbFrame>,
    ) -> io::Result<()> {
        let slot = usize::from(slot);
        if slot >= 2 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid custom palette slot",
            ));
        }
        if self.model(model, firmware).is_some_and(|entry| {
            entry.initialization_attempted
                && entry.slots[slot] == frame
                && (frame.is_none()
                    || entry.last_slot == Some(slot as u8) && entry.effect == Some(10))
        }) {
            return Ok(());
        }
        self.update(model, firmware, |entry| {
            entry.slots[slot] = frame;
            entry.initialization_attempted = true;
            if frame.is_some() {
                if entry.effect != Some(10) {
                    entry.lighting[Region::Keyboard.index()] = None;
                }
                entry.effect = Some(10);
                entry.last_slot = Some(slot as u8);
            }
        })
    }

    pub fn set_effect(&mut self, model: &str, firmware: &str, mode: u8) -> io::Result<()> {
        if self.model(model, firmware).and_then(|entry| entry.effect) == Some(mode) {
            return Ok(());
        }
        self.update(model, firmware, |entry| {
            entry.effect = Some(mode);
            entry.lighting[Region::Keyboard.index()] = None;
        })
    }

    pub fn set_lighting(
        &mut self,
        model: &str,
        firmware: &str,
        region: Region,
        effect: Effect,
    ) -> io::Result<()> {
        if self
            .model(model, firmware)
            .is_some_and(|entry| entry.lighting[region.index()] == Some(effect))
        {
            return Ok(());
        }
        self.update(model, firmware, |entry| {
            entry.lighting[region.index()] = Some(effect);
            if region == Region::Keyboard {
                entry.effect = Some(effect.mode);
            }
        })
    }

    pub fn confirm_slot(
        &mut self,
        model: &str,
        firmware: &str,
        slot: u8,
        frame: RgbFrame,
        effect: Effect,
    ) -> io::Result<()> {
        if slot >= 2 || effect.mode != 10 {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
        self.update(model, firmware, |entry| {
            entry.slots[usize::from(slot)] = Some(frame);
            entry.last_slot = Some(slot);
            entry.initialization_attempted = true;
            entry.effect = Some(effect.mode);
            entry.lighting[Region::Keyboard.index()] = Some(effect);
        })
    }

    pub fn saved_reactive(&self) -> Option<&(ReactiveTarget, ReactiveMode)> {
        self.data.reactive.as_ref()
    }

    pub fn set_reactive(
        &mut self,
        preference: Option<(ReactiveTarget, ReactiveMode)>,
    ) -> io::Result<()> {
        if self.data.reactive == preference {
            return Ok(());
        }
        let previous = std::mem::replace(&mut self.data.reactive, preference);
        if let Err(error) = self.save() {
            self.data.reactive = previous;
            return Err(error);
        }
        Ok(())
    }

    pub fn restore(&self, keyboard: &mut Keyboard) -> io::Result<()> {
        let firmware = String::from_utf8_lossy(keyboard.identity().firmware_version()).into_owned();
        let Some(entry) = self.model(keyboard.layout().family, &firmware) else {
            return Ok(());
        };
        for region in [Region::Keyboard, Region::Side] {
            let palettes = region == Region::Keyboard && entry.slots.iter().any(Option::is_some);
            if (entry.mode_for(region).is_none() && !palettes)
                || (region == Region::Side && !keyboard.layout().side_lighting)
            {
                continue;
            }
            let modes = keyboard.read_lighting_modes(region)?;
            let current = keyboard.read_lighting(region)?;
            let wanted = entry.restore_target(region, &modes.modes, current);
            let palettes = palettes && modes.modes.contains(&10);
            if palettes {
                for (slot, frame) in entry.custom_pages() {
                    keyboard.write_custom_page(slot, frame)?;
                }
            }
            // The last uploaded custom page determines the active slot.
            if wanted != current || palettes {
                keyboard.restore_effect(region, wanted)?;
            }
        }
        Ok(())
    }

    pub fn clear_slots(&mut self, model: &str, firmware: &str) -> io::Result<()> {
        if self.model(model, firmware).is_none() {
            return Ok(());
        }
        self.update(model, firmware, |entry| {
            entry.effect = None;
            entry.lighting = [None; 2];
            entry.last_slot = None;
            entry.slots = [None; 2];
        })
    }

    fn update(
        &mut self,
        model: &str,
        firmware: &str,
        edit: impl FnOnce(&mut Model),
    ) -> io::Result<()> {
        let mut entry = self
            .model(model, firmware)
            .cloned()
            .unwrap_or_else(|| Model {
                firmware: firmware.into(),
                initialization_attempted: !self.needs_initialization(model),
                effect: None,
                lighting: [None; 2],
                last_slot: None,
                slots: [None; 2],
            });
        edit(&mut entry);
        let old = self.data.models.insert(model.into(), entry);
        if let Err(error) = self.save() {
            if let Some(old) = old {
                self.data.models.insert(model.into(), old);
            } else {
                self.data.models.remove(model);
            }
            return Err(error);
        }
        Ok(())
    }

    pub fn snapshot(&self) -> io::Result<String> {
        let settings = ron::ser::PrettyConfig::new().depth_limit(3);
        let text = ron::ser::to_string_pretty(&self.data, settings).map_err(io::Error::other)?;
        if text.len() as u64 > MAX_BYTES {
            return Err(io::Error::other("Palette file is too large"));
        }
        Ok(text)
    }

    fn save(&self) -> io::Result<()> {
        let text = self.snapshot()?;
        let directory = self.path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Read-only palette snapshot",
            )
        })?;
        fs::create_dir_all(directory)?;
        gio::File::for_path(&self.path)
            .replace_contents(
                text.as_bytes(),
                None,
                false,
                gio::FileCreateFlags::PRIVATE | gio::FileCreateFlags::REPLACE_DESTINATION,
                gio::Cancellable::NONE,
            )
            .map_err(io::Error::other)?;
        fs::File::open(&self.path)?.sync_all()?;
        fs::File::open(directory)?.sync_all()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            Self(PathBuf::from("target").join(format!(
                "palette-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            )))
        }

        fn path(&self) -> PathBuf {
            self.0.join("palettes.ron")
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn saved_lighting_and_reactive_preferences_survive_reopening_the_store() {
        let fixture = Fixture::new();
        let mut store = Palettes::read(fixture.path()).unwrap();
        let colors = crate::keyboard::lighting::DEFAULT_AUDIO_PALETTE;
        let target = ReactiveTarget {
            node: 4,
            model: "7153_AUSHERGB".into(),
            firmware: "V1_11_27".into(),
            vendor: 0x0416,
            product: 0x7372,
        };
        let audio = ReactiveMode::Audio {
            colors,
            follow_wallpaper: true,
        };
        let custom = Effect {
            mode: 10,
            brightness: 3,
            speed: 1,
            foreground: [0; 3],
            background: [0; 3],
            direction: 0,
            effect_type: 0,
            open: 0,
        };
        let radar = Effect {
            mode: 6,
            foreground: [30, 20, 40],
            background: [0, 5, 1],
            effect_type: 1,
            direction: 2,
            speed: 4,
            ..custom
        };
        let side = Effect { mode: 2, ..custom };
        store
            .confirm_slot(
                &target.model,
                &target.firmware,
                1,
                [[[1, 2, 3]; 22]; 6],
                custom,
            )
            .unwrap();
        store
            .confirm_slot(
                &target.model,
                &target.firmware,
                0,
                [[[4, 5, 6]; 22]; 6],
                custom,
            )
            .unwrap();
        store
            .set_lighting(&target.model, &target.firmware, Region::Keyboard, radar)
            .unwrap();
        store
            .set_lighting(&target.model, &target.firmware, Region::Side, side)
            .unwrap();
        store.set_reactive(Some((target.clone(), audio))).unwrap();
        let mut restored = Palettes::read(fixture.path()).unwrap();
        assert_eq!(restored.saved_reactive(), Some(&(target.clone(), audio)));
        let entry = restored.model(&target.model, &target.firmware).unwrap();
        let pages = entry
            .custom_pages()
            .map(|(slot, frame)| (slot, frame[0][0]))
            .collect::<Vec<_>>();
        assert_eq!(pages, [(1, [1, 2, 3]), (0, [4, 5, 6])]);
        assert_eq!(
            entry.restore_target(Region::Keyboard, &[0, 6, 10], custom),
            radar
        );
        assert_eq!(
            entry.restore_target(Region::Keyboard, &[0, 10], custom),
            custom
        );
        assert_eq!(entry.restore_target(Region::Side, &[0, 2], custom), side);
        restored.set_reactive(None).unwrap();
        assert_eq!(
            Palettes::read(fixture.path()).unwrap().saved_reactive(),
            None
        );
    }

    #[test]
    fn older_mode_only_store_loads_without_inventing_reactive_preferences() {
        let old = br#"(version:1,models:{"K617":(firmware:"1",initialization_attempted:true,effect:Some(6),last_slot:None,slots:(None,None))})"#;
        let model = &Palettes::decode(old).unwrap().models["K617"];
        assert_eq!(model.mode_for(Region::Keyboard), Some(6));
        assert_eq!(model.mode_for(Region::Side), None);
        assert!(Palettes::decode(old).unwrap().reactive.is_none());
    }

    #[test]
    fn confirming_existing_colors_updates_the_active_slot_and_mode() {
        let fixture = Fixture::new();
        let red = [[[255, 0, 0]; 22]; 6];
        let mut store = Palettes::read(fixture.path()).unwrap();
        store.set_slot("K617", "1", 0, Some(red)).unwrap();
        store.set_slot("K617", "1", 1, Some(red)).unwrap();
        store.set_effect("K617", "1", 4).unwrap();
        store.set_slot("K617", "1", 0, Some(red)).unwrap();
        let reopened = Palettes::read(fixture.path()).unwrap();
        assert_eq!(reopened.last_slot("K617", "1"), 0);
        assert_eq!(reopened.model("K617", "1").unwrap().effect, Some(10));
    }

    #[test]
    fn reopening_preserves_both_slots_and_the_last_observed_effect_by_model() {
        let fixture = Fixture::new();
        let red: RgbFrame = [[[255, 0, 0]; 22]; 6];
        let green: RgbFrame = [[[0, 255, 0]; 22]; 6];
        let mut store = Palettes::read(fixture.path()).unwrap();
        assert!(store.needs_initialization("K617"));
        store.set_effect("K617", "1", 4).unwrap();
        assert!(store.needs_initialization("K617"));
        store.set_slot("K617", "1", 0, None).unwrap();
        assert!(
            !Palettes::read(fixture.path())
                .unwrap()
                .needs_initialization("K617")
        );
        store.set_slot("K617", "1", 0, Some(red)).unwrap();
        store.set_slot("K617", "1", 1, Some(green)).unwrap();
        store.set_effect("K617", "1", 4).unwrap();
        store.set_slot("K683", "1", 0, Some(green)).unwrap();

        let mut reopened = Palettes::read(fixture.path()).unwrap();
        assert_eq!(reopened.slots("K617", "1"), [Some(red), Some(green)]);
        assert_eq!(reopened.model("K617", "1").unwrap().effect, Some(4));
        assert_eq!(reopened.last_slot("K617", "1"), 1);
        assert_eq!(reopened.slots("K683", "1"), [Some(green), None]);
        assert_eq!(reopened.slots("K617", "2"), [None; 2]);
        reopened.set_effect("K617", "2", 4).unwrap();
        assert!(!reopened.needs_initialization("K617"));
        assert_eq!(reopened.slots("K617", "2"), [None; 2]);
    }

    #[test]
    fn interrupted_transfer_never_reloads_the_previous_frame() {
        let fixture = Fixture::new();
        let frame = RgbFrame::default();
        let mut store = Palettes::read(fixture.path()).unwrap();
        store.set_slot("K617", "1", 0, Some(frame)).unwrap();
        store.set_slot("K617", "1", 1, Some(frame)).unwrap();
        store.set_slot("K617", "1", 0, None).unwrap();

        let mut reopened = Palettes::read(fixture.path()).unwrap();
        assert_eq!(reopened.slots("K617", "1"), [None, Some(frame)]);
        assert!(!reopened.needs_initialization("K617"));
        reopened.path = fixture.path().join("missing-parent.ron");
        assert!(reopened.set_slot("K617", "1", 1, None).is_err());
        assert_eq!(reopened.slots("K617", "1"), [None, Some(frame)]);
        reopened.path = fixture.path();
        reopened.clear_slots("K617", "1").unwrap();
        let reset = Palettes::read(fixture.path()).unwrap();
        assert_eq!(reset.slots("K617", "1"), [None; 2]);
        assert!(!reset.needs_initialization("K617"));
    }
}
