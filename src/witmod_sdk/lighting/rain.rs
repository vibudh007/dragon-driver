use super::KeyboardRuntime;
use super::{
    Effect, KEY_CELLS, KEY_COLUMNS, KEY_FADE_PALETTE, KEY_ROWS, KeyFade, KeyPhase, KeyboardPreview,
    PreviewCell,
};

pub(super) struct Rain {
    rainbow: bool,
    reverse: bool,
    countdown: [u8; KEY_ROWS],
    states: [KeyFade; KEY_CELLS],
}

impl KeyboardPreview {
    pub fn rain(color: [u8; 3], reverse: bool, rainbow: bool) -> Self {
        Self {
            cells: [PreviewCell {
                color: if rainbow { [0; 3] } else { color },
                level: 0,
            }; KEY_CELLS],
            pressed: false,
            runtime: KeyboardRuntime::default(),
            effect: Effect::Rain(Box::new(Rain {
                rainbow,
                reverse,
                countdown: [21, 31, 41, 51, 61, 71],
                states: [KeyFade {
                    phase: KeyPhase::Idle,
                    transitions: 0,
                }; KEY_CELLS],
            })),
        }
    }
}

impl Rain {
    pub(super) fn tick(&mut self, cells: &mut [PreviewCell; KEY_CELLS], elapsed: &mut u16) {
        for (cell, state) in cells.iter_mut().zip(&mut self.states) {
            if self.rainbow {
                state.tick_palette(cell, 40, None);
            } else if matches!(state.phase, KeyPhase::Falling) {
                if cell.level < 7 {
                    state.phase = KeyPhase::Idle;
                }
                cell.level = cell.level.saturating_sub(7);
            }
        }

        let (increment, threshold) = if self.rainbow { (45, 255) } else { (7, 40) };
        *elapsed += increment;
        if *elapsed < threshold {
            return;
        }
        *elapsed = 0;
        for (row, countdown) in self.countdown.iter_mut().enumerate() {
            *countdown -= 1;
            if usize::from(*countdown) >= KEY_COLUMNS {
                continue;
            }
            let column = usize::from(*countdown);
            let column = if self.reverse { column } else { 21 - column };
            let index = row * KEY_COLUMNS + column;
            if self.rainbow {
                cells[index] = PreviewCell {
                    color: KEY_FADE_PALETTE[9],
                    level: 0,
                };
                self.states[index] = KeyFade {
                    phase: KeyPhase::Rising,
                    transitions: 9,
                };
            } else {
                cells[index].level = 255;
                self.states[index].phase = KeyPhase::Falling;
            }
            if *countdown == 0 {
                *countdown = KEY_COLUMNS as u8;
            }
        }
    }

    pub(super) fn palette_transition(&self, index: usize) -> u8 {
        self.states[index].transitions
    }

    pub(super) fn background_for(&self, index: usize) -> Option<[u8; 3]> {
        if self.rainbow {
            self.states[index].palette_background()
        } else {
            None
        }
    }
}
