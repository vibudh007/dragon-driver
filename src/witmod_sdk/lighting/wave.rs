use super::KeyboardRuntime;
use super::{Effect, KEY_CELLS, KeyboardPreview, PreviewCell, cycle_spectrum, mix_color};

use crate::witmod_sdk::keymap::KEY_COLUMNS;

pub(super) enum Wave {
    SingleColor(Box<[bool; KEY_CELLS]>),
    Rainbow { forward: bool, step: u8 },
}

impl KeyboardPreview {
    pub fn horizontal_wave_61(
        foreground: [u8; 3],
        background: [u8; 3],
        reverse: bool,
        rainbow: bool,
    ) -> Self {
        Self::directional_wave_61(foreground, background, u8::from(reverse), rainbow)
    }

    pub fn diagonal_wave_61(
        foreground: [u8; 3],
        background: [u8; 3],
        reverse: bool,
        rainbow: bool,
    ) -> Self {
        Self::wave_for_layout(61, foreground, background, u8::from(reverse), rainbow, true)
    }

    pub(super) fn directional_wave_61(
        foreground: [u8; 3],
        background: [u8; 3],
        direction: u8,
        rainbow: bool,
    ) -> Self {
        Self::wave_for_layout(61, foreground, background, direction, rainbow, false)
    }

    pub(super) fn wave_for_layout(
        key_count: u8,
        foreground: [u8; 3],
        background: [u8; 3],
        direction: u8,
        rainbow: bool,
        diagonal: bool,
    ) -> Self {
        let reverse = direction & 1 != 0;
        let vertical = direction >> 1 == 1;
        let slanted = direction >> 1 == 2;
        let geometry = super::geometry::waves(key_count);
        let initial = if diagonal {
            geometry.diagonal
        } else if slanted {
            geometry.slanted
        } else {
            geometry.horizontal
        };
        if rainbow {
            let cells = std::array::from_fn(|index| {
                let channels = if vertical {
                    let offset = index / KEY_COLUMNS * 4 + 1;
                    &geometry.vertical[offset..offset + 3]
                } else {
                    let offset = KEY_CELLS + index * 3;
                    &initial[offset..offset + 3]
                };
                PreviewCell {
                    color: channels.try_into().expect("three color channels"),
                    level: 255,
                }
            });
            return Self {
                cells,
                pressed: false,
                runtime: KeyboardRuntime::default(),
                effect: Effect::Wave(Wave::Rainbow {
                    forward: reverse,
                    step: if diagonal { 9 } else { 6 },
                }),
            };
        }
        let cells = std::array::from_fn(|index| {
            let mut level = if vertical {
                geometry.vertical[index / KEY_COLUMNS * 4]
            } else {
                initial[index]
            };
            if reverse {
                level = 255 - level;
            }
            PreviewCell {
                color: mix_color(foreground, background, level, false),
                level,
            }
        });
        Self {
            cells,
            pressed: false,
            runtime: KeyboardRuntime::default(),
            effect: Effect::Wave(Wave::SingleColor(Box::new(std::array::from_fn(|index| {
                if diagonal || slanted {
                    let phase_mask = initial[KEY_CELLS * 4 + index % KEY_COLUMNS];
                    let bit = phase_mask & (1 << (index / KEY_COLUMNS)) != 0;
                    bit != slanted
                } else {
                    !vertical && index % KEY_COLUMNS < 10
                }
            })))),
        }
    }
}

impl Wave {
    pub(super) fn tick(&mut self, cells: &mut [PreviewCell; KEY_CELLS]) {
        match self {
            Self::SingleColor(falling) => {
                tick_wave_levels(cells, falling.as_mut());
            }
            Self::Rainbow { forward, step } => {
                for cell in cells {
                    cell.color = cycle_spectrum(cell.color, *step, *forward);
                }
            }
        }
    }
}

pub(super) fn tick_wave_levels(cells: &mut [PreviewCell], falling: &mut [bool]) {
    for (cell, falling) in cells.iter_mut().zip(falling) {
        if *falling && cell.level < 10 {
            *falling = false;
        }
        if *falling {
            cell.level -= 4;
        } else {
            *falling = cell.level > 251;
            cell.level = cell.level.saturating_add(4);
        }
    }
}
