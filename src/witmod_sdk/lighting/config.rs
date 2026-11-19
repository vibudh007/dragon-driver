use super::{Effect, KeyboardPreview, MAX_SIDE_CELLS, SidePreview, mix_color};
use crate::witmod_sdk::profile::LightItem;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreviewConfigError {
    ColorOutOfRange,
    TooManySideCells(usize),
    SideLayoutTooLong { maximum_active: usize },
    UnsupportedSideDirection(i32),
    UnsupportedMode(i32),
    UnsupportedWaveLayout { key_count: u8, direction: i32 },
}

impl KeyboardPreview {
    pub fn replace_config(
        &mut self,
        item: LightItem,
        key_count: u8,
    ) -> Result<(), PreviewConfigError> {
        let next = Self::from_config(item, key_count)?;
        self.cells = next.cells;
        self.effect = next.effect;
        self.pressed = false;
        self.runtime.active_rows = 0;
        self.runtime.automatic_pending = false;
        self.runtime.automatic_frame_flag = false;
        match item.mode {
            14 => self.runtime.rng = next.runtime.rng,
            15 => self.runtime.rain_elapsed = 0,
            _ => {}
        }
        Ok(())
    }

    pub fn from_config(item: LightItem, key_count: u8) -> Result<Self, PreviewConfigError> {
        let foreground = byte_color(item.foreground)?;
        let background = byte_color(item.background)?;
        let rainbow = item.mode_type != 0;
        let direction = normalized_direction(item.direction);
        let preview = match item.mode {
            0 => Self::steady(foreground),
            1 => Self::breathing(foreground, background, rainbow),
            2 if matches!(direction, 0..=5) => Self::wave_for_layout(
                key_count,
                foreground,
                background,
                direction as u8,
                rainbow,
                false,
            ),
            3 => Self::pulse(foreground, rainbow),
            4 if matches!(direction, 0 | 1) => Self::wave_for_layout(
                key_count,
                foreground,
                background,
                direction as u8,
                rainbow,
                true,
            ),
            2 | 4 => {
                return Err(PreviewConfigError::UnsupportedWaveLayout {
                    key_count,
                    direction,
                });
            }
            5 => Self::sequential_for_layout(
                key_count,
                foreground,
                background,
                direction & 1 != 0,
                rainbow,
            ),
            6 => Self::key_fade(foreground, background, rainbow),
            7 => Self::line_ripple(foreground, background, direction & 1 != 0, rainbow),
            8 => Self::ring_ripple(foreground, background, direction & 1 != 0, rainbow),
            14 => Self::automatic_ripple(foreground, background, direction & 1 != 0, rainbow),
            9 => Self::sparkle(foreground, background, rainbow),
            11 => Self::cross_fade(foreground, background, rainbow),
            16 => Self::hexagonal_ripple(foreground, direction & 1 != 0, rainbow),
            15 => Self::rain(foreground, direction & 1 != 0, rainbow),
            12 => Self::row_fade(foreground, direction & 1 == 1, rainbow),
            mode => return Err(PreviewConfigError::UnsupportedMode(mode)),
        };
        Ok(preview)
    }

    pub fn mixed_color(&self, row: usize, column: usize, background: [u8; 3]) -> Option<[u8; 3]> {
        let cell = self.cell(row, column)?;
        let index = row * super::KEY_COLUMNS + column;
        let background = match &self.effect {
            Effect::Sequential(state) => state.background_for(index),
            Effect::Rain(state) => state.background_for(index),
            Effect::LineRipple(state) => state.background_for(index),
            Effect::KeyFade {
                progress,
                rainbow: true,
                ..
            } => progress[index].palette_background(),
            _ => None,
        }
        .unwrap_or(background);
        Some(mix_color(cell.color, background, cell.level, false))
    }
}

impl SidePreview {
    pub fn from_config(item: LightItem, cell_count: usize) -> Result<Self, PreviewConfigError> {
        let slots = [true; MAX_SIDE_CELLS];
        let slots = slots
            .get(..cell_count)
            .ok_or(PreviewConfigError::TooManySideCells(cell_count))?;
        Self::from_model_config(item, "", slots)
    }

    pub fn from_model_config(
        item: LightItem,
        model: &str,
        slots: &[bool],
    ) -> Result<Self, PreviewConfigError> {
        let cell_count = slots.len();
        if cell_count > MAX_SIDE_CELLS {
            return Err(PreviewConfigError::TooManySideCells(cell_count));
        }
        let foreground = byte_color(item.foreground)?;
        let background = byte_color(item.background)?;
        let rainbow = item.mode_type != 0;
        let preview = match item.mode {
            0 => Self::steady(cell_count, foreground, false),
            1 => Self::breathing(cell_count, foreground, background, rainbow),
            2 => {
                let direction = normalized_direction(item.direction);
                if !matches!(direction, 0 | 1) {
                    return Err(PreviewConfigError::UnsupportedSideDirection(direction));
                }
                Self::wave_for_model(
                    model,
                    slots,
                    foreground,
                    background,
                    direction == 1,
                    rainbow,
                )?
            }
            3 => Self::collision(
                cell_count,
                super::side::geometry::collision_span(model),
                foreground,
                background,
                rainbow,
            ),
            4 => Self::pulse(cell_count, foreground, rainbow),
            5 => Self::steady(cell_count, foreground, true),
            mode => return Err(PreviewConfigError::UnsupportedMode(mode)),
        };
        Ok(preview)
    }
}

fn normalized_direction(direction: i32) -> i32 {
    match direction {
        6 => 0,
        7 => 1,
        _ => direction,
    }
}

fn byte_color(channels: [i32; 3]) -> Result<[u8; 3], PreviewConfigError> {
    let [Ok(red), Ok(green), Ok(blue)] = channels.map(u8::try_from) else {
        return Err(PreviewConfigError::ColorOutOfRange);
    };
    Ok([red, green, blue])
}
