use super::KeyboardRuntime;
use super::{
    Effect, KEY_CELLS, KEY_COLUMNS, KEY_FADE_PALETTE, KeyFade, KeyPhase, KeyboardPreview,
    PreviewCell, RAINBOW_PALETTE,
};

#[derive(Clone, Copy)]
struct Trigger {
    row: usize,
    column: usize,
    distance: usize,
}

pub(super) struct Ripple {
    foreground: [u8; 3],
    rainbow: bool,
    reverse: bool,
    hexagonal: bool,
    last_color: Option<usize>,
    events: Vec<Trigger>,
    states: [KeyFade; KEY_CELLS],
}

impl KeyboardPreview {
    pub fn line_ripple(
        foreground: [u8; 3],
        background: [u8; 3],
        reverse: bool,
        rainbow: bool,
    ) -> Self {
        Self::reactive_ripple(foreground, background, reverse, rainbow, false)
    }

    pub fn hexagonal_ripple(foreground: [u8; 3], reverse: bool, rainbow: bool) -> Self {
        Self::reactive_ripple(foreground, foreground, reverse, rainbow, true)
    }

    fn reactive_ripple(
        foreground: [u8; 3],
        background: [u8; 3],
        reverse: bool,
        rainbow: bool,
        hexagonal: bool,
    ) -> Self {
        Self {
            cells: [PreviewCell {
                color: if rainbow { [0; 3] } else { background },
                level: 0,
            }; KEY_CELLS],
            pressed: false,
            runtime: KeyboardRuntime::default(),
            effect: Effect::LineRipple(Box::new(Ripple {
                foreground,
                rainbow,
                reverse,
                hexagonal,
                last_color: None,
                events: Vec::with_capacity(20),
                states: [KeyFade {
                    phase: KeyPhase::Idle,
                    transitions: 0,
                }; KEY_CELLS],
            })),
        }
    }

    pub fn current_ripple_color(&self) -> [u32; 3] {
        if let Effect::LineRipple(state) = &self.effect {
            state
                .last_color
                .map(|index| RAINBOW_PALETTE[index].map(u32::from))
                .unwrap_or([0; 3])
        } else if let Effect::RingRipple(state) = &self.effect {
            state.current_color()
        } else if self.pressed {
            RAINBOW_PALETTE[0].map(u32::from)
        } else {
            [0; 3]
        }
    }
}

impl Ripple {
    pub(super) fn press(&mut self, row: usize, column: usize, rng: &mut u32) {
        self.last_color = Some(0);
        if self.events.len() == 20 {
            return;
        }
        self.last_color = Some(super::ring::next_rand(rng) % 7);
        let distance = if self.reverse && !self.hexagonal {
            column.max(KEY_COLUMNS - 1 - column)
        } else {
            0
        };
        self.events.insert(
            0,
            Trigger {
                row,
                column,
                distance,
            },
        );
    }

    pub(super) fn tick(
        &mut self,
        cells: &mut [PreviewCell; KEY_CELLS],
        runtime: &mut KeyboardRuntime,
    ) {
        let (increment, threshold) = match (self.hexagonal, self.rainbow) {
            (true, true) => (45, 255),
            (true, false) => (7, 40),
            (false, true) => (8, 255),
            (false, false) => (1, 40),
        };
        let elapsed = if self.hexagonal {
            &mut runtime.rain_elapsed
        } else {
            &mut runtime.line_elapsed
        };
        for row in 0..super::KEY_ROWS {
            let offset = row * KEY_COLUMNS;
            for (cell, state) in cells[offset..offset + KEY_COLUMNS]
                .iter_mut()
                .zip(&mut self.states[offset..offset + KEY_COLUMNS])
            {
                if self.rainbow {
                    state.tick_palette(cell, 40, None);
                } else {
                    match state.phase {
                        KeyPhase::Idle => {}
                        KeyPhase::Rising => {
                            if cell.level > 248 {
                                state.phase = KeyPhase::Falling;
                            }
                            cell.level = cell.level.saturating_add(7);
                        }
                        KeyPhase::Falling => {
                            if cell.level < 7 {
                                state.phase = KeyPhase::Idle;
                            }
                            cell.level = cell.level.saturating_sub(7);
                        }
                    }
                }
            }

            if self.hexagonal && row + 1 != super::KEY_ROWS {
                continue;
            }
            *elapsed += increment;
            if *elapsed < threshold {
                continue;
            }
            *elapsed = 0;
            self.events.retain_mut(|event| {
                let mut place = |row: usize, column: usize| {
                    if row >= super::KEY_ROWS || column >= KEY_COLUMNS {
                        return;
                    }
                    let index = row * KEY_COLUMNS + column;
                    let (cell, state) = if self.rainbow {
                        (
                            PreviewCell {
                                color: KEY_FADE_PALETTE[9],
                                level: 0,
                            },
                            KeyFade {
                                phase: KeyPhase::Rising,
                                transitions: 9,
                            },
                        )
                    } else if self.hexagonal {
                        (
                            PreviewCell {
                                color: cells[index].color,
                                level: [0, 80, 160][event.distance],
                            },
                            KeyFade {
                                phase: KeyPhase::Rising,
                                transitions: 0,
                            },
                        )
                    } else {
                        (
                            PreviewCell {
                                color: self.foreground,
                                level: 255,
                            },
                            KeyFade {
                                phase: KeyPhase::Falling,
                                transitions: 1,
                            },
                        )
                    };
                    cells[index] = cell;
                    self.states[index] = state;
                };
                let touched = if self.hexagonal {
                    hexagon(*event, &mut place)
                } else {
                    let left = event.column.checked_sub(event.distance);
                    let right = (event.column + event.distance < KEY_COLUMNS)
                        .then_some(event.column + event.distance);
                    for column in left.into_iter().chain(right) {
                        place(event.row, column);
                    }
                    left.is_some() || right.is_some()
                };
                if self.reverse {
                    if event.distance == 0 {
                        return false;
                    }
                    event.distance -= 1;
                } else {
                    if !touched {
                        return false;
                    }
                    event.distance += 1;
                }
                true
            });
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

fn hexagon(event: Trigger, place: &mut impl FnMut(usize, usize)) -> bool {
    let Trigger {
        row,
        column,
        distance,
    } = event;
    let right_edge = KEY_COLUMNS - 1;
    match distance {
        0 => place(row, column),
        1 => {
            if row > 0 {
                let up = column.saturating_sub(usize::from(row == 3));
                place(row - 1, up);
                if column < right_edge {
                    place(row - 1, up + 1);
                }
            }
            if row + 1 < super::KEY_ROWS {
                let down = column + usize::from(row == 2 && column < right_edge);
                place(row + 1, down);
                if down > 0 {
                    place(row + 1, down - 1);
                }
            }
            if column > 0 {
                place(row, column - 1);
            }
            place(row, column + 1);
        }
        2 => {
            let up = column.saturating_sub(usize::from(row == 3));
            if row > 0 {
                if up > 0 {
                    place(row - 1, up - 1);
                }
                place(row - 1, up + 2);
            }
            if row > 1 {
                let up_two = if row == 4 {
                    column.saturating_sub(1)
                } else {
                    up
                };
                for column in up_two..=up_two + 2 {
                    place(row - 2, column);
                }
            }
            let down = column + usize::from(row == 2 && column < right_edge);
            if row + 1 < super::KEY_ROWS {
                if row != 2 || column < right_edge {
                    place(row + 1, down + 1);
                }
                if down >= 2 {
                    place(row + 1, down - 2);
                }
            }
            if row + 2 < super::KEY_ROWS {
                let down_two = if row == 1 && column < right_edge {
                    column + 1
                } else {
                    down
                };
                for column in down_two.saturating_sub(2)..=down_two {
                    place(row + 2, column);
                }
            }
            if column >= 2 {
                place(row, column - 2);
            }
            place(row, column + 2);
        }
        _ => return false,
    }
    true
}
