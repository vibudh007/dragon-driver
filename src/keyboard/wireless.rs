use std::io;

use super::lighting::{Effect, WirelessEffect, WirelessSide, wireless_frame};
use super::models::Layout;
use super::report::Packet;
use crate::witmod_sdk::{
    lighting::{KeyboardPreview, SidePreview, mix_color},
    profile::LightItem,
};

pub(super) struct Live {
    mode: u8,
    keyboard: Option<KeyboardPreview>,
    foreground: [u8; 3],
    background: [u8; 3],
    side: Option<(u8, [u8; 3], Option<SidePreview>)>,
    last_press: Option<[u8; 5]>,
}

impl Live {
    pub fn new(layout: &Layout, keyboard: Effect, side: Option<Effect>) -> io::Result<Self> {
        let render = match keyboard.mode {
            2 | 4 | 5 | 9 | 15 | 100 => None,
            0 | 1 | 3 | 6 | 7 | 8 | 11 | 12 | 14 | 16 => Some(
                KeyboardPreview::from_config(item(0, keyboard), layout.keys().count() as u8)
                    .map_err(|error| {
                        io::Error::other(format!(
                            "Unsupported keyboard lighting options ({error:?})"
                        ))
                    })?,
            ),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "Select a supported preset or start Audio-reactive lighting",
                ));
            }
        };
        let side = match side.filter(|_| layout.side_lighting) {
            Some(effect) if matches!(effect.mode, 0..=5) => {
                let preview = if effect.mode == 2 {
                    None
                } else {
                    Some(
                        SidePreview::from_model_config(
                            item(1, effect),
                            layout.family,
                            &vec![true; usize::from(layout.side_lights)],
                        )
                        .map_err(|error| {
                            io::Error::other(format!(
                                "Unsupported side lighting options ({error:?})"
                            ))
                        })?,
                    )
                };
                Some((effect.mode, effect.background, preview))
            }
            Some(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "Unsupported live side lighting effect",
                ));
            }
            None => None,
        };
        Ok(Self {
            mode: keyboard.mode,
            keyboard: render,
            foreground: keyboard.foreground,
            background: keyboard.background,
            side,
            last_press: None,
        })
    }

    pub fn takes_input(&self) -> bool {
        matches!(self.mode, 6 | 7 | 8 | 11 | 12 | 16)
    }

    pub fn press(&mut self, layout: &Layout, code: u8) {
        if !self.takes_input() {
            return;
        }
        let Some((position, _)) = layout.keys().find(|(_, key)| *key == i16::from(code)) else {
            return;
        };
        let Some(preview) = &mut self.keyboard else {
            return;
        };
        if preview.press(usize::from(position.row()), usize::from(position.column())) {
            let color = if matches!(self.mode, 7 | 8 | 16) {
                preview.current_ripple_color().map(|value| value as u8)
            } else {
                self.foreground
            };
            self.last_press = Some([
                position.row(),
                position.column(),
                color[0],
                color[1],
                color[2],
            ]);
        }
    }

    pub fn packet(&mut self, stamp: u16) -> Packet {
        let keyboard = if let Some(preview) = &mut self.keyboard {
            preview.tick();
            match self.mode {
                0 | 1 | 3 => WirelessEffect::Color {
                    mode: self.mode,
                    color: preview
                        .mixed_color(0, 0, self.background)
                        .unwrap_or(self.foreground),
                },
                12 => {
                    let level = preview.cells().chunks_exact(22).rev().find_map(|row| {
                        let level = row[0].level;
                        (level > 0).then_some(if self.background == [0; 3] {
                            level.min(200)
                        } else {
                            level
                        })
                    });
                    self.last_press = None;
                    WirelessEffect::RowEvent(level.map(|value| [preview.active_rows(), value]))
                }
                14 => {
                    let event = preview.automatic_ripple_pending().then(|| {
                        preview
                            .current_automatic_ripple_config()
                            .map(|value| value as u8)
                    });
                    WirelessEffect::AutoRipple(event)
                }
                _ => WirelessEffect::KeyEvent {
                    mode: self.mode,
                    event: self.last_press.take(),
                },
            }
        } else {
            WirelessEffect::Automatic { mode: self.mode }
        };
        wireless_frame(stamp, keyboard, self.side_frame()).expect("validated wireless presets")
    }

    pub fn audio_packet(&mut self, stamp: u16, levels: [u8; 11], peaks: [u8; 11]) -> Packet {
        wireless_frame(
            stamp,
            WirelessEffect::Audio { levels, peaks },
            self.side_frame(),
        )
        .expect("validated wireless presets")
    }

    fn side_frame(&mut self) -> Option<WirelessSide> {
        self.side.as_mut().map(|(mode, background, preview)| {
            if let Some(preview) = preview {
                preview.tick();
                if *mode == 3 {
                    WirelessSide::Collision(preview.collision_config())
                } else {
                    let color = preview.cells().first().map_or([0; 3], |cell| {
                        mix_color(cell.color, *background, cell.level, false)
                    });
                    WirelessSide::Color {
                        mode: *mode,
                        color: if *mode == 5 { [0; 3] } else { color },
                    }
                }
            } else {
                WirelessSide::Wave
            }
        })
    }
}

fn item(region: i32, effect: Effect) -> LightItem {
    LightItem {
        region,
        mode: i32::from(effect.mode),
        brightness: i32::from(effect.brightness),
        speed: i32::from(effect.speed),
        foreground: effect.foreground.map(i32::from),
        background: effect.background.map(i32::from),
        direction: i32::from(effect.direction),
        mode_type: i32::from(effect.effect_type),
        open: i32::from(effect.open),
        update: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keyboard::models::find;

    #[test]
    fn live_wireless_frames_route_all_supported_modes_and_one_shot_events() {
        let layout = find("7153_AUSHERGB").unwrap();
        for mode in [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 11, 12, 14, 15, 16] {
            let mut stream = Live::new(
                layout,
                Effect {
                    mode,
                    brightness: 3,
                    speed: 2,
                    foreground: [40, 60, 90],
                    background: [0; 3],
                    direction: 0,
                    effect_type: 0,
                    open: 1,
                },
                None,
            )
            .unwrap();
            stream.press(layout, 26);
            let wire = stream.packet(0x1234);
            assert_eq!(&wire[..5], &[1, 0x28, 0x12, 0x34, mode]);
            if mode == 6 {
                assert_eq!(&wire[5..10], &[2, 2, 40, 60, 90]);
                assert_eq!(stream.packet(0x1235)[5..10], [0; 5]);
            }
        }
    }
}
