use super::keymap::{KEY_COLUMNS, KEY_ROWS};

mod config;
mod geometry;
mod rain;
mod ring;
mod ripple;
mod sequential;
mod session;
mod side;
mod wave;
pub use config::PreviewConfigError;
pub use session::LightingPreview;
pub use side::SidePreview;

const MIX_LOOKUP: &[u8; 256] = include_bytes!("mix-lookup.bin");
const KEY_CELLS: usize = KEY_ROWS * KEY_COLUMNS;
const MAX_SIDE_CELLS: usize = 60;
const SPARKLE_TABLES: &[u8; KEY_CELLS * 2] = include_bytes!("sparkle-tables.bin");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreviewCell {
    pub color: [u8; 3],
    pub level: u8,
}

pub struct KeyboardPreview {
    cells: [PreviewCell; KEY_CELLS],
    effect: Effect,
    pressed: bool,
    runtime: KeyboardRuntime,
}

struct KeyboardRuntime {
    rng: u32,
    line_elapsed: u16,
    rain_elapsed: u16,
    ring_ticks: [u8; 2],
    automatic_cursor: [usize; 2],
    automatic_last: Option<[u32; 5]>,
    automatic_pending: bool,
    automatic_frame_flag: bool,
    active_rows: u8,
}

impl Default for KeyboardRuntime {
    fn default() -> Self {
        Self {
            rng: 1,
            line_elapsed: 0,
            rain_elapsed: 0,
            ring_ticks: [0; 2],
            automatic_cursor: [0; 2],
            automatic_last: None,
            automatic_pending: false,
            automatic_frame_flag: false,
            active_rows: 0,
        }
    }
}

enum Effect {
    Steady,
    Breathing(Breathing),
    SpectrumCycle,
    Sparkle(Box<Sparkle>),
    Wave(wave::Wave),
    Sequential(Box<sequential::Sequential>),
    Rain(Box<rain::Rain>),
    LineRipple(Box<ripple::Ripple>),
    RingRipple(Box<ring::Ring>),
    Pulse {
        falling: Box<[bool; KEY_CELLS]>,
        pause: Option<u8>,
    },
    RowFade {
        rows: usize,
        forward: bool,
        rainbow: bool,
    },
    KeyFade {
        foreground: [u8; 3],
        background: [u8; 3],
        rainbow: bool,
        cross: bool,
        progress: Box<[KeyFade; KEY_CELLS]>,
    },
}

#[derive(Clone, Copy)]
enum KeyPhase {
    Idle,
    Rising,
    Falling,
}

#[derive(Clone, Copy)]
struct KeyFade {
    phase: KeyPhase,
    transitions: u8,
}

enum Phase {
    Rising,
    Falling,
    Paused(u8),
}

struct Breathing {
    palette: [[u8; 3]; 7],
    palette_len: usize,
    index: usize,
    phase: Phase,
}

struct Sparkle {
    foreground: [u8; 3],
    rainbow: bool,
    palette: [u8; KEY_CELLS],
    phase: [KeyPhase; KEY_CELLS],
    position: usize,
    wait: bool,
}

impl KeyboardPreview {
    pub fn steady(color: [u8; 3]) -> Self {
        Self {
            cells: [PreviewCell { color, level: 255 }; KEY_CELLS],
            pressed: false,
            runtime: KeyboardRuntime::default(),
            effect: Effect::Steady,
        }
    }

    pub fn breathing(foreground: [u8; 3], background: [u8; 3], rainbow: bool) -> Self {
        let state = Breathing::new(foreground, background, rainbow);
        let mut preview = Self::steady(state.palette[0]);
        preview.effect = Effect::Breathing(state);
        preview
    }
}

impl Breathing {
    fn new(foreground: [u8; 3], background: [u8; 3], rainbow: bool) -> Self {
        let mut palette = [[0; 3]; 7];
        let palette_len;
        if rainbow {
            palette = [
                [255, 0, 0],
                [255, 255, 0],
                [0, 255, 0],
                [0, 255, 255],
                [0, 0, 255],
                [255, 0, 255],
                [255, 255, 255],
            ];
            palette_len = 7;
        } else {
            palette[0] = foreground;
            palette[1] = if background == [0; 3] {
                foreground
            } else {
                background
            };
            palette_len = 2;
        }
        Self {
            palette,
            palette_len,
            index: 0,
            phase: Phase::Falling,
        }
    }
}

impl KeyboardPreview {
    pub fn row_fade(color: [u8; 3], forward: bool, rainbow: bool) -> Self {
        let color = if rainbow { [0; 3] } else { color };
        Self {
            cells: [PreviewCell { color, level: 0 }; KEY_CELLS],
            pressed: false,
            runtime: KeyboardRuntime::default(),
            effect: Effect::RowFade {
                rows: 0,
                forward,
                rainbow,
            },
        }
    }

    pub fn pulse(color: [u8; 3], rainbow: bool) -> Self {
        if rainbow {
            let mut preview = Self::steady([255, 0, 0]);
            preview.effect = Effect::SpectrumCycle;
            return preview;
        }
        Self {
            cells: [PreviewCell { color, level: 0 }; KEY_CELLS],
            pressed: false,
            runtime: KeyboardRuntime::default(),
            effect: Effect::Pulse {
                falling: Box::new([false; KEY_CELLS]),
                pause: None,
            },
        }
    }

    pub fn sparkle(foreground: [u8; 3], background: [u8; 3], rainbow: bool) -> Self {
        let mut state = Sparkle {
            foreground,
            rainbow,
            palette: [0; KEY_CELLS],
            phase: [KeyPhase::Idle; KEY_CELLS],
            position: 0,
            wait: false,
        };
        for (column_major, &palette) in SPARKLE_TABLES[KEY_CELLS..].iter().enumerate() {
            state.palette[row_major(column_major)] = palette;
        }
        Self {
            cells: [PreviewCell {
                color: background,
                level: 0,
            }; KEY_CELLS],
            pressed: false,
            runtime: KeyboardRuntime::default(),
            effect: Effect::Sparkle(Box::new(state)),
        }
    }

    pub fn key_fade(foreground: [u8; 3], background: [u8; 3], rainbow: bool) -> Self {
        Self::reactive(foreground, background, rainbow, false)
    }

    pub fn cross_fade(foreground: [u8; 3], background: [u8; 3], rainbow: bool) -> Self {
        Self::reactive(foreground, background, rainbow, true)
    }

    fn reactive(foreground: [u8; 3], background: [u8; 3], rainbow: bool, cross: bool) -> Self {
        Self {
            cells: [PreviewCell {
                color: background,
                level: 0,
            }; KEY_CELLS],
            pressed: false,
            runtime: KeyboardRuntime::default(),
            effect: Effect::KeyFade {
                foreground,
                background,
                rainbow,
                cross,
                progress: Box::new(
                    [KeyFade {
                        phase: KeyPhase::Idle,
                        transitions: 0,
                    }; KEY_CELLS],
                ),
            },
        }
    }

    pub fn cell(&self, row: usize, column: usize) -> Option<PreviewCell> {
        if row >= KEY_ROWS || column >= KEY_COLUMNS {
            return None;
        }
        Some(self.cells[row * KEY_COLUMNS + column])
    }

    pub fn cells(&self) -> &[PreviewCell] {
        &self.cells
    }

    pub fn palette_transition(&self, row: usize, column: usize) -> Option<u8> {
        self.cell(row, column)?;
        let index = row * KEY_COLUMNS + column;
        Some(match &self.effect {
            Effect::Sequential(state) => state.palette_transition(index),
            Effect::Rain(state) => state.palette_transition(index),
            Effect::LineRipple(state) => state.palette_transition(index),
            Effect::KeyFade { progress, .. } => progress[index].transitions,
            _ => 0,
        })
    }

    pub fn active_rows(&self) -> u8 {
        self.runtime.active_rows
    }

    pub fn automatic_frame_flag(&self) -> bool {
        self.runtime.automatic_frame_flag
    }

    pub fn tick(&mut self) {
        match &mut self.effect {
            Effect::Steady => {}
            Effect::Breathing(breathing) => breathing.tick(&mut self.cells),
            Effect::SpectrumCycle => {
                let color = advance_spectrum(self.cells[0].color);
                self.cells.fill(PreviewCell { color, level: 255 });
            }
            Effect::Sparkle(sparkle) => sparkle.tick(&mut self.cells),
            Effect::Wave(wave) => wave.tick(&mut self.cells),
            Effect::Sequential(state) => state.tick(&mut self.cells),
            Effect::Rain(state) => state.tick(&mut self.cells, &mut self.runtime.rain_elapsed),
            Effect::LineRipple(state) => state.tick(&mut self.cells, &mut self.runtime),
            Effect::RingRipple(state) => state.tick(&mut self.cells, &mut self.runtime),
            Effect::Pulse { falling, pause } => {
                tick_pulse(&mut self.cells, falling.as_mut(), pause);
            }
            Effect::RowFade { rows, forward, .. } => {
                if *rows != 0 {
                    self.runtime.active_rows = *rows as u8;
                    let mut cleared = false;
                    for cell in row_cells(&mut self.cells, *rows - 1, *forward) {
                        cleared |= cell.level < 25;
                        cell.level = cell.level.saturating_sub(25);
                    }
                    if cleared {
                        *rows -= 1;
                    }
                }
            }
            Effect::KeyFade {
                background,
                rainbow,
                cross,
                progress,
                ..
            } => {
                for (state, cell) in progress.iter_mut().zip(&mut self.cells) {
                    state.tick(cell, *background, *rainbow, *cross);
                }
            }
        }
    }

    pub fn press(&mut self, row: usize, column: usize) -> bool {
        if row >= KEY_ROWS || column >= KEY_COLUMNS {
            return false;
        }
        self.pressed = true;
        if let Effect::RingRipple(state) = &mut self.effect {
            state.press(row, column, &mut self.runtime.rng);
            return true;
        }
        if let Effect::LineRipple(state) = &mut self.effect {
            state.press(row, column, &mut self.runtime.rng);
            return true;
        }
        if let Effect::KeyFade {
            foreground,
            rainbow,
            cross,
            progress,
            ..
        } = &mut self.effect
        {
            let row_start = row * KEY_COLUMNS;
            let mut trigger = |index| {
                self.cells[index] = PreviewCell {
                    color: if *rainbow { [255, 127, 0] } else { *foreground },
                    level: 255,
                };
                let state: &mut KeyFade = &mut progress[index];
                state.phase = if *rainbow {
                    KeyPhase::Rising
                } else {
                    KeyPhase::Falling
                };
                if *rainbow || !*cross {
                    state.transitions = 9;
                }
            };
            if *cross {
                for index in row_start..row_start + KEY_COLUMNS {
                    trigger(index);
                }
                for other in (0..KEY_ROWS).filter(|&other| other != row) {
                    trigger(other * KEY_COLUMNS + column);
                }
            } else {
                trigger(row_start + column);
            }
            return true;
        }
        let Effect::RowFade {
            rows,
            forward,
            rainbow,
        } = &mut self.effect
        else {
            return true;
        };
        if *rows == 0
            || (*rows < KEY_ROWS && row_cells(&mut self.cells, *rows - 1, *forward)[0].level > 120)
        {
            *rows += 1;
        }
        let palette = [
            [255, 255, 0],
            [0, 255, 0],
            [0, 255, 255],
            [0, 0, 255],
            [255, 0, 255],
            [255, 0, 0],
        ];
        for (rank, color) in palette.into_iter().take(*rows).enumerate() {
            for cell in row_cells(&mut self.cells, rank, *forward) {
                cell.level = 255 - (rank as u8 * 20);
                if *rainbow {
                    cell.color = color;
                }
            }
        }
        true
    }
}

const RAINBOW_PALETTE: [[u8; 3]; 8] = [
    [255, 0, 0],
    [255, 128, 0],
    [255, 255, 0],
    [0, 255, 0],
    [0, 255, 255],
    [0, 0, 255],
    [255, 0, 255],
    [255, 255, 255],
];

fn row_major(column_major: usize) -> usize {
    (column_major % KEY_ROWS) * KEY_COLUMNS + column_major / KEY_ROWS
}

impl Sparkle {
    fn tick(&mut self, cells: &mut [PreviewCell; KEY_CELLS]) {
        for (index, cell) in cells.iter_mut().enumerate() {
            match self.phase[index] {
                KeyPhase::Idle => {}
                KeyPhase::Rising if cell.level > 251 => {
                    cell.level = 255;
                    self.phase[index] = KeyPhase::Falling;
                }
                KeyPhase::Rising => cell.level += 4,
                KeyPhase::Falling if cell.level < 4 => {
                    cell.level = 0;
                    self.phase[index] = KeyPhase::Idle;
                }
                KeyPhase::Falling => cell.level -= 4,
            }
            cell.color = if self.rainbow {
                RAINBOW_PALETTE[usize::from(self.palette[index])]
            } else {
                self.foreground
            };
        }
        self.wait = !self.wait;
        if !self.wait {
            self.position = (self.position + 1) % KEY_CELLS;
            let index = row_major(usize::from(SPARKLE_TABLES[self.position]));
            self.phase[index] = KeyPhase::Rising;
            cells[index].level = u8::from(!self.rainbow);
            if self.rainbow {
                self.palette[index] = self.palette[index].checked_sub(1).unwrap_or(7);
            }
        }
    }
}

const KEY_FADE_PALETTE: [[u8; 3]; 10] = [
    [255, 0, 255],
    [191, 0, 255],
    [150, 0, 255],
    [127, 0, 255],
    [0, 127, 255],
    [0, 255, 191],
    [0, 255, 63],
    [127, 255, 0],
    [255, 255, 0],
    [255, 127, 0],
];

impl KeyFade {
    fn palette_background(self) -> Option<[u8; 3]> {
        let remaining = usize::from(self.transitions);
        (1..9)
            .contains(&remaining)
            .then(|| KEY_FADE_PALETTE[(remaining + 1) & !1])
    }

    fn tick(&mut self, cell: &mut PreviewCell, background: [u8; 3], rainbow: bool, cross: bool) {
        if matches!(self.phase, KeyPhase::Idle) {
            return;
        }
        if !rainbow {
            if cell.level < 2 {
                *cell = PreviewCell {
                    color: background,
                    level: 0,
                };
                self.phase = KeyPhase::Idle;
            } else {
                cell.level -= 2;
            }
            return;
        }

        self.tick_palette(cell, 14, cross.then_some(background));
    }

    fn tick_palette(
        &mut self,
        cell: &mut PreviewCell,
        step: u8,
        background: Option<[u8; 3]>,
    ) -> bool {
        let next = match self.phase {
            KeyPhase::Idle => return false,
            KeyPhase::Rising if cell.level <= 255 - step => {
                cell.level += step;
                return false;
            }
            KeyPhase::Rising => {
                cell.level = 255;
                KeyPhase::Falling
            }
            KeyPhase::Falling if cell.level >= step => {
                cell.level -= step;
                return false;
            }
            KeyPhase::Falling => {
                cell.level = 0;
                KeyPhase::Rising
            }
        };
        if self.transitions == 0 {
            self.phase = KeyPhase::Idle;
            if let Some(background) = background {
                cell.color = background;
            }
            return false;
        }
        self.transitions -= 1;
        self.phase = if self.transitions == 0 {
            KeyPhase::Falling
        } else {
            next
        };
        if self.transitions & 1 != 0 {
            cell.color = KEY_FADE_PALETTE[usize::from(self.transitions)];
        }
        self.transitions == 8
    }
}

fn tick_pulse(cells: &mut [PreviewCell], falling: &mut [bool], pause: &mut Option<u8>) {
    for (cell, falling) in cells.iter_mut().zip(falling) {
        if pause.is_none() {
            if *falling {
                if cell.level == 0 {
                    *falling = false;
                    *pause = Some(0);
                } else {
                    cell.level -= 1;
                }
            } else if cell.level == 255 {
                *falling = true;
            } else {
                cell.level += 1;
            }
        }
        if let Some(step) = pause.as_mut() {
            *step += 1;
            if *step > 6 {
                *pause = None;
            }
        }
    }
}

fn advance_spectrum(color: [u8; 3]) -> [u8; 3] {
    advance_spectrum_by(color, 6)
}

fn cycle_spectrum(color: [u8; 3], step: u8, forward: bool) -> [u8; 3] {
    if forward {
        advance_spectrum_by(color, step)
    } else {
        let [red, green, blue] = color;
        let [red, blue, green] = advance_spectrum_by([red, blue, green], step);
        [red, green, blue]
    }
}

fn advance_spectrum_by(color: [u8; 3], step: u8) -> [u8; 3] {
    match color {
        [255, green, 0] if green < 255 => [255, green.saturating_add(step), 0],
        [red, 255, 0] if red > 0 => [red.saturating_sub(step), 255, 0],
        [0, 255, blue] if blue < 255 => [0, 255, blue.saturating_add(step)],
        [0, green, 255] if green > 0 => [0, green.saturating_sub(step), 255],
        [red, 0, 255] if red < 255 => [red.saturating_add(step), 0, 255],
        [255, 0, blue] => [255, 0, blue.saturating_sub(step)],
        _ => color,
    }
}

fn row_cells(
    cells: &mut [PreviewCell; KEY_CELLS],
    rank: usize,
    forward: bool,
) -> &mut [PreviewCell] {
    let row = if forward { rank } else { KEY_ROWS - 1 - rank };
    &mut cells[row * KEY_COLUMNS..(row + 1) * KEY_COLUMNS]
}

impl Breathing {
    fn tick(&mut self, cells: &mut [PreviewCell; KEY_CELLS]) {
        let level = cells[0].level;
        match self.phase {
            Phase::Rising => {
                if level > 253 {
                    self.phase = Phase::Falling;
                }
                let level = level.saturating_add(2);
                cells.iter_mut().for_each(|cell| cell.level = level);
            }
            Phase::Falling if level < 2 => {
                cells[0].level = 0;
                self.phase = Phase::Paused(0);
            }
            Phase::Falling => cells.iter_mut().for_each(|cell| cell.level -= 2),
            Phase::Paused(ticks) if ticks < 6 => self.phase = Phase::Paused(ticks + 1),
            Phase::Paused(_) => {
                self.index = (self.index + 1) % self.palette_len;
                cells.fill(PreviewCell {
                    color: self.palette[self.index],
                    level: 0,
                });
                self.phase = Phase::Rising;
            }
        }
    }
}

pub fn reset_side_levels(cells: &mut [PreviewCell], clear_color: bool) {
    for cell in cells {
        cell.level = 255;
        if clear_color {
            cell.color = [0; 3];
        }
    }
}

pub fn mix_color(foreground: [u8; 3], background: [u8; 3], weight: u8, lookup: bool) -> [u8; 3] {
    let weight = u16::from(weight);
    std::array::from_fn(|channel| {
        let value = (weight * u16::from(foreground[channel])
            + (255 - weight) * u16::from(background[channel]))
            >> 8;
        if lookup {
            MIX_LOOKUP[usize::from(value)]
        } else {
            value as u8
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_original_divisor_and_lookup() {
        assert_eq!(mix_color([255; 3], [0; 3], 255, false), [254; 3]);
        assert_eq!(mix_color([255; 3], [0; 3], 128, true), [79; 3]);
        assert_eq!(
            mix_color([255, 0, 128], [0, 255, 128], 0, false),
            [0, 254, 127]
        );
    }

    #[test]
    fn breathing_preserves_the_original_pause_and_palette_rotation() {
        let mut preview = KeyboardPreview::breathing([20, 40, 60], [2, 4, 6], false);
        for _ in 0..127 {
            preview.tick();
        }
        assert_eq!(preview.cell(0, 0).unwrap().level, 1);
        preview.tick();
        assert_eq!(preview.cell(0, 0).unwrap().level, 0);
        assert_eq!(preview.cell(0, 1).unwrap().level, 1);
        for _ in 0..6 {
            preview.tick();
        }
        assert_eq!(preview.cell(0, 1).unwrap().color, [20, 40, 60]);
        preview.tick();
        assert_eq!(
            preview.cell(5, 21),
            Some(PreviewCell {
                color: [2, 4, 6],
                level: 0
            })
        );
        preview.tick();
        assert_eq!(preview.cell(5, 21).unwrap().level, 2);
        assert!(preview.cell(6, 0).is_none());
    }

    #[test]
    fn row_fade_restarts_active_rows_in_reverse_order_and_decays_the_last_one() {
        let mut preview = KeyboardPreview::row_fade([1, 2, 3], false, true);
        assert!(preview.press(0, 0));
        assert!(preview.press(1, 0));
        assert_eq!(
            preview.cell(5, 0),
            Some(PreviewCell {
                color: [255, 255, 0],
                level: 255
            })
        );
        assert_eq!(
            preview.cell(4, 0),
            Some(PreviewCell {
                color: [0, 255, 0],
                level: 235
            })
        );
        preview.tick();
        assert_eq!(preview.cell(5, 0).unwrap().level, 255);
        assert_eq!(preview.cell(4, 0).unwrap().level, 210);
    }

    #[test]
    fn key_fade_keeps_the_effect_local_to_the_pressed_key() {
        let mut preview = KeyboardPreview::key_fade([3, 5, 7], [2, 4, 6], false);
        assert!(preview.press(0, 1));
        preview.tick();
        assert_eq!(
            preview.cell(0, 1),
            Some(PreviewCell {
                color: [3, 5, 7],
                level: 253
            })
        );
        assert_eq!(
            preview.cell(0, 0),
            Some(PreviewCell {
                color: [2, 4, 6],
                level: 0
            })
        );
        for _ in 0..127 {
            preview.tick();
        }
        assert_eq!(preview.cell(0, 1), preview.cell(0, 0));
    }

    #[test]
    fn matches_original_dll_preview_traces() {
        for (name, mut preview, fixture, presses) in [
            (
                "hexagonal-ripple",
                KeyboardPreview::hexagonal_ripple([20, 40, 60], false, false),
                &include_bytes!("../../tests/fixtures/vendor-sdk/hexagonal-ripple.bin")[..],
                1,
            ),
            (
                "hexagonal-rainbow",
                KeyboardPreview::hexagonal_ripple([20, 40, 60], false, true),
                &include_bytes!("../../tests/fixtures/vendor-sdk/hexagonal-rainbow.bin")[..],
                1,
            ),
            (
                "ring-ripple",
                KeyboardPreview::ring_ripple([20, 40, 60], [2, 4, 6], false, false),
                &include_bytes!("../../tests/fixtures/vendor-sdk/ring-ripple.bin")[..],
                1,
            ),
            (
                "auto-ripple",
                KeyboardPreview::automatic_ripple([20, 40, 60], [2, 4, 6], false, true),
                &include_bytes!("../../tests/fixtures/vendor-sdk/auto-ripple.bin")[..],
                0,
            ),
            (
                "line-ripple",
                KeyboardPreview::line_ripple([20, 40, 60], [2, 4, 6], false, false),
                &include_bytes!("../../tests/fixtures/vendor-sdk/line-ripple.bin")[..],
                1,
            ),
            (
                "line-ripple-rainbow",
                KeyboardPreview::line_ripple([20, 40, 60], [2, 4, 6], true, true),
                &include_bytes!("../../tests/fixtures/vendor-sdk/line-ripple-rainbow.bin")[..],
                1,
            ),
            (
                "rain",
                KeyboardPreview::rain([20, 40, 60], false, false),
                &include_bytes!("../../tests/fixtures/vendor-sdk/rain.bin")[..],
                0,
            ),
            (
                "rain-rainbow",
                KeyboardPreview::rain([20, 40, 60], true, true),
                &include_bytes!("../../tests/fixtures/vendor-sdk/rain-rainbow.bin")[..],
                0,
            ),
            (
                "sequential",
                KeyboardPreview::sequential_61([20, 40, 60], [2, 4, 6], false, false),
                &include_bytes!("../../tests/fixtures/vendor-sdk/sequential.bin")[..],
                0,
            ),
            (
                "sequential-rainbow",
                KeyboardPreview::sequential_61([20, 40, 60], [2, 4, 6], true, true),
                &include_bytes!("../../tests/fixtures/vendor-sdk/sequential-rainbow.bin")[..],
                0,
            ),
            (
                "diagonal",
                KeyboardPreview::diagonal_wave_61([20, 40, 60], [2, 4, 6], false, false),
                &include_bytes!("../../tests/fixtures/vendor-sdk/diagonal.bin")[..],
                0,
            ),
            (
                "diagonal-rainbow",
                KeyboardPreview::diagonal_wave_61([20, 40, 60], [2, 4, 6], false, true),
                &include_bytes!("../../tests/fixtures/vendor-sdk/diagonal-rainbow.bin")[..],
                0,
            ),
            (
                "diagonal-reverse",
                KeyboardPreview::diagonal_wave_61([20, 40, 60], [2, 4, 6], true, true),
                &include_bytes!("../../tests/fixtures/vendor-sdk/diagonal-reverse.bin")[..],
                0,
            ),
            (
                "vertical",
                KeyboardPreview::directional_wave_61([20, 40, 60], [2, 4, 6], 2, false),
                &include_bytes!("../../tests/fixtures/vendor-sdk/vertical.bin")[..],
                0,
            ),
            (
                "vertical-rainbow",
                KeyboardPreview::directional_wave_61([20, 40, 60], [2, 4, 6], 3, true),
                &include_bytes!("../../tests/fixtures/vendor-sdk/vertical-rainbow.bin")[..],
                0,
            ),
            (
                "slanted",
                KeyboardPreview::directional_wave_61([20, 40, 60], [2, 4, 6], 4, false),
                &include_bytes!("../../tests/fixtures/vendor-sdk/slanted.bin")[..],
                0,
            ),
            (
                "slanted-rainbow",
                KeyboardPreview::directional_wave_61([20, 40, 60], [2, 4, 6], 5, true),
                &include_bytes!("../../tests/fixtures/vendor-sdk/slanted-rainbow.bin")[..],
                0,
            ),
            (
                "wave",
                KeyboardPreview::horizontal_wave_61([20, 40, 60], [2, 4, 6], false, false),
                &include_bytes!("../../tests/fixtures/vendor-sdk/wave.bin")[..],
                0,
            ),
            (
                "wave rainbow",
                KeyboardPreview::horizontal_wave_61([20, 40, 60], [2, 4, 6], false, true),
                &include_bytes!("../../tests/fixtures/vendor-sdk/wave-rainbow.bin")[..],
                0,
            ),
            (
                "wave reverse",
                KeyboardPreview::horizontal_wave_61([20, 40, 60], [2, 4, 6], true, true),
                &include_bytes!("../../tests/fixtures/vendor-sdk/wave-reverse.bin")[..],
                0,
            ),
            (
                "sparkle",
                KeyboardPreview::sparkle([20, 40, 60], [2, 4, 6], true),
                &include_bytes!("../../tests/fixtures/vendor-sdk/sparkle.bin")[..],
                0,
            ),
            (
                "pulse",
                KeyboardPreview::pulse([20, 40, 60], false),
                &include_bytes!("../../tests/fixtures/vendor-sdk/pulse.bin")[..],
                0,
            ),
            (
                "spectrum",
                KeyboardPreview::pulse([20, 40, 60], true),
                &include_bytes!("../../tests/fixtures/vendor-sdk/spectrum.bin")[..],
                0,
            ),
            (
                "breathing",
                KeyboardPreview::breathing([20, 40, 60], [2, 4, 6], false),
                &include_bytes!("../../tests/fixtures/vendor-sdk/breathing.bin")[..],
                0,
            ),
            (
                "key fade",
                KeyboardPreview::key_fade([20, 40, 60], [2, 4, 6], false),
                &include_bytes!("../../tests/fixtures/vendor-sdk/key-fade.bin")[..],
                1,
            ),
            (
                "key rainbow",
                KeyboardPreview::key_fade([20, 40, 60], [2, 4, 6], true),
                &include_bytes!("../../tests/fixtures/vendor-sdk/key-rainbow.bin")[..],
                1,
            ),
            (
                "cross fade",
                KeyboardPreview::cross_fade([20, 40, 60], [2, 4, 6], false),
                &include_bytes!("../../tests/fixtures/vendor-sdk/cross-fade.bin")[..],
                1,
            ),
            (
                "cross rainbow",
                KeyboardPreview::cross_fade([20, 40, 60], [2, 4, 6], true),
                &include_bytes!("../../tests/fixtures/vendor-sdk/cross-rainbow.bin")[..],
                1,
            ),
            (
                "row fade",
                KeyboardPreview::row_fade([20, 40, 60], true, true),
                &include_bytes!("../../tests/fixtures/vendor-sdk/row-fade.bin")[..],
                2,
            ),
        ] {
            let steps = if name == "pulse" || name == "spectrum" {
                530
            } else {
                280
            };
            assert_eq!(fixture.len(), steps * 24);
            for (step, expected) in fixture.chunks_exact(24).enumerate() {
                if step < presses {
                    preview.press(0, 1);
                }
                preview.tick();
                for (index, expected) in [0, 1, 22, 88, 110, 131]
                    .into_iter()
                    .zip(expected.chunks_exact(4))
                {
                    let cell = preview
                        .cell(index / KEY_COLUMNS, index % KEY_COLUMNS)
                        .unwrap();
                    let [r, g, b] = cell.color;
                    assert_eq!(
                        [r, g, b, cell.level],
                        expected,
                        "{name}, step {step}, cell {index}"
                    );
                }
            }
        }
    }
}
