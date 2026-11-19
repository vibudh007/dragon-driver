use super::KeyboardRuntime;
use super::{
    Effect, KEY_CELLS, KEY_COLUMNS, KeyPhase, KeyboardPreview, PreviewCell, RAINBOW_PALETTE,
};

const LEVELS: [u8; 10] = [255, 255, 255, 255, 255, 255, 205, 155, 105, 55];
const AUTO_CELLS: [u8; 15] = [
    34, 162, 100, 6, 39, 103, 115, 109, 171, 16, 178, 172, 81, 105, 144,
];

#[derive(Clone, Copy)]
struct Wave {
    row: usize,
    column: usize,
    radius: usize,
    color: usize,
}

impl Wave {
    fn new(row: usize, column: usize, color: usize, reverse: bool) -> Self {
        let radius = if reverse {
            column.max(KEY_COLUMNS - 1 - column)
        } else {
            0
        };
        Self {
            row,
            column,
            radius,
            color,
        }
    }
}

struct Auto {
    choices: [(usize, usize); 100],
    wave: Option<Wave>,
}

pub(super) struct Ring {
    foreground: [u8; 3],
    rainbow: bool,
    reverse: bool,
    last_color: Option<usize>,
    waves: Vec<Wave>,
    auto: Option<Auto>,
    phases: [KeyPhase; KEY_CELLS],
}

impl KeyboardPreview {
    pub fn ring_ripple(
        foreground: [u8; 3],
        background: [u8; 3],
        reverse: bool,
        rainbow: bool,
    ) -> Self {
        Self::ring(foreground, background, reverse, rainbow, false)
    }

    pub fn automatic_ripple(
        foreground: [u8; 3],
        background: [u8; 3],
        reverse: bool,
        rainbow: bool,
    ) -> Self {
        Self::ring(foreground, background, reverse, rainbow, true)
    }

    fn ring(
        foreground: [u8; 3],
        background: [u8; 3],
        reverse: bool,
        rainbow: bool,
        automatic: bool,
    ) -> Self {
        let mut runtime = KeyboardRuntime::default();
        let auto = automatic.then(|| Auto::new(&mut runtime.rng));
        Self {
            cells: [PreviewCell {
                color: if rainbow { [0; 3] } else { background },
                level: 0,
            }; KEY_CELLS],
            pressed: false,
            runtime,
            effect: Effect::RingRipple(Box::new(Ring {
                foreground,
                rainbow,
                reverse,
                last_color: None,
                waves: Vec::with_capacity(if automatic { 0 } else { 20 }),
                auto,
                phases: [KeyPhase::Idle; KEY_CELLS],
            })),
        }
    }

    pub fn current_automatic_ripple(&mut self) -> Option<[u32; 5]> {
        self.runtime.automatic_pending = false;
        self.runtime.automatic_last
    }

    pub fn current_automatic_ripple_config(&mut self) -> [u32; 5] {
        self.current_automatic_ripple().unwrap_or_default()
    }

    pub fn automatic_ripple_pending(&self) -> bool {
        self.runtime.automatic_pending
    }
}

impl Auto {
    fn new(rng: &mut u32) -> Self {
        *rng = 3;
        Self {
            choices: std::array::from_fn(|_| (next_rand(rng) & 7, next_rand(rng) & 15)),
            wave: None,
        }
    }

    fn schedule(&mut self, reverse: bool, runtime: &mut KeyboardRuntime) {
        let [position, color] = &mut runtime.automatic_cursor;
        *color = (*color + 1) % self.choices.len();
        *position = (*position + 1) % self.choices.len();
        while self.choices[*position].1 >= AUTO_CELLS.len() {
            *position = (*position + 1) % self.choices.len();
        }
        let packed = usize::from(AUTO_CELLS[self.choices[*position].1]);
        let (row, column) = (packed >> 5, packed & 31);
        let color = self.choices[*color].0.min(6);
        let [r, g, b] = RAINBOW_PALETTE[color].map(u32::from);
        runtime.automatic_last = Some([row as u32, column as u32, r, g, b]);
        runtime.automatic_pending = true;
        runtime.automatic_frame_flag = true;
        self.wave = Some(Wave::new(row, column, color, reverse));
    }
}

impl Ring {
    pub(super) fn press(&mut self, row: usize, column: usize, rng: &mut u32) {
        self.last_color = Some(0);
        if self.auto.is_none() && self.waves.len() < 20 {
            let color = next_rand(rng) % 7;
            self.last_color = Some(color);
            self.waves.push(Wave::new(row, column, color, self.reverse));
        }
    }

    pub(super) fn current_color(&self) -> [u32; 3] {
        self.last_color
            .map(|index| RAINBOW_PALETTE[index].map(u32::from))
            .unwrap_or([0; 3])
    }

    pub(super) fn tick(
        &mut self,
        cells: &mut [PreviewCell; KEY_CELLS],
        runtime: &mut KeyboardRuntime,
    ) {
        for (cell, phase) in cells.iter_mut().zip(&mut self.phases) {
            if !matches!(phase, KeyPhase::Idle) {
                if cell.level < 10 {
                    *phase = KeyPhase::Idle;
                }
                cell.level = cell.level.saturating_sub(10);
            }
        }
        if self.auto.is_some() && !runtime.automatic_pending {
            runtime.automatic_frame_flag = false;
        }
        let ticks = &mut runtime.ring_ticks[usize::from(self.auto.is_some())];
        *ticks = (*ticks + 1) % 4;
        if *ticks != 0 {
            return;
        }

        let color = |wave: &Wave| {
            if self.rainbow {
                RAINBOW_PALETTE[wave.color]
            } else {
                self.foreground
            }
        };
        if let Some(auto) = &mut self.auto {
            if let Some(wave) = &mut auto.wave {
                if !advance(wave, color(wave), self.reverse, cells, &mut self.phases) {
                    auto.wave = None;
                }
            } else {
                auto.schedule(self.reverse, runtime);
            }
        } else {
            self.waves.retain_mut(|wave| {
                advance(wave, color(wave), self.reverse, cells, &mut self.phases)
            });
        }
    }
}

fn advance(
    wave: &mut Wave,
    color: [u8; 3],
    reverse: bool,
    cells: &mut [PreviewCell; KEY_CELLS],
    phases: &mut [KeyPhase; KEY_CELLS],
) -> bool {
    let mut touches_grid = false;
    for (index, cell) in cells.iter_mut().enumerate() {
        let distance =
            wave.row.abs_diff(index / KEY_COLUMNS) + wave.column.abs_diff(index % KEY_COLUMNS);
        let Some(layer) = wave
            .radius
            .checked_sub(distance)
            .filter(|&layer| layer < LEVELS.len())
        else {
            continue;
        };
        touches_grid = true;
        let level = LEVELS[layer];
        if cell.level == 0 || level > 204 {
            *cell = PreviewCell { color, level };
            phases[index] = if layer < 5 {
                KeyPhase::Idle
            } else {
                KeyPhase::Falling
            };
        }
    }
    if reverse {
        if wave.radius == 0 {
            return false;
        }
        wave.radius -= 1;
        true
    } else {
        wave.radius += 1;
        touches_grid
    }
}

pub(super) fn next_rand(state: &mut u32) -> usize {
    *state = state.wrapping_mul(214013).wrapping_add(2531011);
    ((*state >> 16) & 0x7fff) as usize
}
