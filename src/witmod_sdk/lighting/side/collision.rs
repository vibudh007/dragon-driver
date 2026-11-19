use super::{MAX_SIDE_CELLS, PreviewCell, SideEffect, SidePreview};
use crate::witmod_sdk::lighting::{KeyFade, KeyPhase};

const PALETTE: [[u8; 3]; 8] = [
    [255, 0, 255],
    [127, 0, 255],
    [0, 127, 255],
    [0, 255, 191],
    [0, 255, 63],
    [255, 255, 0],
    [255, 127, 0],
    [255, 63, 0],
];

pub(super) struct Collision {
    foreground: [u8; 3],
    background: [u8; 3],
    rainbow: bool,
    states: Vec<KeyFade>,
    span: u8,
    position: u8,
    pass: u16,
    returning: bool,
}

impl SidePreview {
    pub fn collision(
        cell_count: usize,
        span: u8,
        foreground: [u8; 3],
        background: [u8; 3],
        rainbow: bool,
    ) -> Self {
        let cell_count = cell_count.min(MAX_SIDE_CELLS);
        let span = span.min(MAX_SIDE_CELLS as u8);
        let color = if rainbow { background } else { foreground };
        Self {
            cells: vec![PreviewCell { color, level: 0 }; cell_count],
            collision_elapsed: 0,
            collision_report: [0; 5],
            pulse_pause: None,
            effect: SideEffect::Collision(Collision {
                foreground,
                background,
                rainbow,
                states: vec![
                    KeyFade {
                        phase: KeyPhase::Idle,
                        transitions: 0
                    };
                    cell_count
                ],
                span,
                position: 0,
                pass: 0,
                returning: false,
            }),
        }
    }

    pub fn replace_config(&mut self, mut next: Self) {
        next.collision_elapsed = self.collision_elapsed;
        next.pulse_pause = self.pulse_pause;
        if !matches!(next.effect, SideEffect::Collision(_)) {
            next.collision_report = self.collision_report;
        }
        *self = next;
    }

    pub fn collision_config(&self) -> [u8; 5] {
        self.collision_report
    }

    pub fn collision_transition(&self, cell: usize) -> Option<u8> {
        let SideEffect::Collision(state) = &self.effect else {
            return None;
        };
        Some(state.states.get(cell)?.transitions)
    }

    pub fn collision_background(&self, cell: usize) -> Option<[u8; 3]> {
        let SideEffect::Collision(state) = &self.effect else {
            return None;
        };
        let remaining = usize::from(state.states.get(cell)?.transitions);
        Some(if state.rainbow && (1..7).contains(&remaining) {
            PALETTE[(remaining + 1) & !1]
        } else {
            state.background
        })
    }
}

impl Collision {
    pub(super) fn report(&self, elapsed: u16) -> [u8; 5] {
        [
            elapsed as u8,
            self.pass as u8,
            self.position,
            u8::from(self.returning),
            self.step(),
        ]
    }

    fn step(&self) -> u8 {
        if self.rainbow { 35 } else { 7 }
    }

    pub(super) fn tick(&mut self, cells: &mut [PreviewCell], elapsed: &mut u16) {
        let step = self.step();
        for (cell, state) in cells.iter_mut().zip(&mut self.states) {
            match state.phase {
                KeyPhase::Idle => continue,
                KeyPhase::Rising if cell.level <= 255 - step => {
                    cell.level += step;
                    continue;
                }
                KeyPhase::Rising => cell.level = 255,
                KeyPhase::Falling if cell.level >= step => {
                    cell.level -= step;
                    continue;
                }
                KeyPhase::Falling => cell.level = 0,
            }
            if state.transitions == 0 {
                state.phase = KeyPhase::Idle;
                continue;
            }
            state.transitions -= 1;
            if state.transitions & 1 != 0 {
                cell.color = PALETTE[usize::from(state.transitions)];
            }
            state.phase = if state.transitions == 0 || matches!(state.phase, KeyPhase::Rising) {
                KeyPhase::Falling
            } else {
                KeyPhase::Rising
            };
        }

        *elapsed += u16::from(step);
        if *elapsed <= if self.rainbow { 255 } else { 40 } || self.span == 0 {
            if self.span == 0 {
                *elapsed = 0;
            }
            return;
        }
        *elapsed = 0;
        if self.pass < u16::from(self.span) {
            self.pass += 1;
            let far_end = self.span - 1;
            if self.position <= far_end {
                for position in [self.position, far_end - self.position] {
                    let index = usize::from(position);
                    let Some(cell) = cells.get_mut(index) else {
                        continue;
                    };
                    *cell = PreviewCell {
                        color: if self.rainbow {
                            PALETTE[7]
                        } else {
                            self.foreground
                        },
                        level: 255,
                    };
                    self.states[index] = if self.rainbow {
                        KeyFade {
                            phase: KeyPhase::Rising,
                            transitions: 7,
                        }
                    } else {
                        KeyFade {
                            phase: KeyPhase::Falling,
                            transitions: 0,
                        }
                    };
                }
                if !self.returning {
                    self.position += 1;
                }
            }
            if self.returning {
                self.position = self.position.saturating_sub(1);
            }
        } else if self.pass < u16::from(self.span) + 7 {
            self.pass += 1;
        } else {
            self.returning = !self.returning;
            self.pass = 0;
            self.position = if self.returning { self.span - 1 } else { 0 };
        }
    }
}
