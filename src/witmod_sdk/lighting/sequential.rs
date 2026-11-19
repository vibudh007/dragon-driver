use super::KeyboardRuntime;
use super::{
    Effect, KEY_CELLS, KEY_COLUMNS, KEY_FADE_PALETTE, KeyFade, KeyPhase, KeyboardPreview,
    PreviewCell,
};

pub(super) struct Sequential {
    foreground: [u8; 3],
    background: [u8; 3],
    rainbow: bool,
    reverse: bool,
    position: usize,
    path: &'static [u8],
    states: [KeyFade; KEY_CELLS],
}

impl KeyboardPreview {
    pub fn sequential_61(
        foreground: [u8; 3],
        background: [u8; 3],
        reverse: bool,
        rainbow: bool,
    ) -> Self {
        Self::sequential_for_layout(61, foreground, background, reverse, rainbow)
    }

    pub(super) fn sequential_for_layout(
        key_count: u8,
        foreground: [u8; 3],
        background: [u8; 3],
        reverse: bool,
        rainbow: bool,
    ) -> Self {
        let mut cells = [PreviewCell {
            color: if rainbow { background } else { foreground },
            level: 0,
        }; KEY_CELLS];
        let mut state = Sequential {
            foreground,
            background,
            rainbow,
            reverse,
            position: 0,
            path: super::geometry::sequential_path(key_count),
            states: [KeyFade {
                phase: KeyPhase::Idle,
                transitions: 0,
            }; KEY_CELLS],
        };
        state.activate(&mut cells);
        Self {
            cells,
            pressed: false,
            runtime: KeyboardRuntime::default(),
            effect: Effect::Sequential(Box::new(state)),
        }
    }
}

impl Sequential {
    fn activate(&mut self, cells: &mut [PreviewCell; KEY_CELLS]) {
        let index = if self.reverse {
            self.path.len() - 1 - self.position
        } else {
            self.position
        };
        let packed = usize::from(self.path[index]);
        let cell = (packed >> 5) * KEY_COLUMNS + (packed & 31);
        let (color, level, phase, transitions) = if self.rainbow {
            (KEY_FADE_PALETTE[9], 0, KeyPhase::Rising, 9)
        } else {
            (self.foreground, 255, KeyPhase::Falling, 1)
        };
        cells[cell] = PreviewCell { color, level };
        self.states[cell] = KeyFade { phase, transitions };
    }

    pub(super) fn tick(&mut self, cells: &mut [PreviewCell; KEY_CELLS]) {
        for index in 0..KEY_CELLS {
            let mut cell = cells[index];
            let mut state = self.states[index];
            let next = if self.rainbow {
                state.tick_palette(&mut cell, 60, Some(self.background))
            } else {
                if matches!(state.phase, KeyPhase::Falling) {
                    if cell.level < 5 {
                        state.phase = KeyPhase::Idle;
                    }
                    cell.level = cell.level.saturating_sub(5);
                }
                let next = state.transitions == 1 && cell.level <= 234;
                if next {
                    state.transitions = 0;
                }
                next
            };
            if next {
                self.position = (self.position + 1) % self.path.len();
                self.activate(cells);
            }
            cells[index] = cell;
            self.states[index] = state;
        }
    }

    pub(super) fn palette_transition(&self, index: usize) -> u8 {
        self.states[index].transitions
    }

    pub(super) fn background_for(&self, index: usize) -> Option<[u8; 3]> {
        self.rainbow
            .then(|| self.states[index].palette_background())
            .flatten()
    }
}
