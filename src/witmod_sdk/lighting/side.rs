use super::{
    Breathing, MAX_SIDE_CELLS, Phase, PreviewCell, cycle_spectrum, mix_color, reset_side_levels,
};

mod collision;
pub(super) mod geometry;

pub struct SidePreview {
    cells: Vec<PreviewCell>,
    effect: SideEffect,
    collision_elapsed: u16,
    collision_report: [u8; 5],
    pulse_pause: Option<u8>,
}

enum SideEffect {
    Steady { clear_color: bool },
    Breathing(Breathing),
    Pulse { falling: Vec<bool> },
    Spectrum { forward: bool, step: u8 },
    Wave { falling: Vec<bool> },
    Collision(collision::Collision),
}

impl SidePreview {
    pub fn steady(cell_count: usize, color: [u8; 3], clear_color: bool) -> Self {
        let cell_count = cell_count.min(MAX_SIDE_CELLS);
        let color = if clear_color { [0; 3] } else { color };
        Self {
            cells: vec![PreviewCell { color, level: 255 }; cell_count],
            effect: SideEffect::Steady { clear_color },
            collision_elapsed: 0,
            collision_report: [0; 5],
            pulse_pause: None,
        }
    }

    pub fn breathing(
        cell_count: usize,
        foreground: [u8; 3],
        background: [u8; 3],
        rainbow: bool,
    ) -> Self {
        let cell_count = cell_count.min(MAX_SIDE_CELLS);
        let state = Breathing::new(foreground, background, rainbow);
        Self {
            cells: vec![
                PreviewCell {
                    color: state.palette[0],
                    level: 255
                };
                cell_count
            ],
            effect: SideEffect::Breathing(state),
            collision_elapsed: 0,
            collision_report: [0; 5],
            pulse_pause: None,
        }
    }

    pub fn cells(&self) -> &[PreviewCell] {
        &self.cells
    }

    pub fn pulse(cell_count: usize, color: [u8; 3], rainbow: bool) -> Self {
        let cell_count = cell_count.min(MAX_SIDE_CELLS);
        let (cell, effect) = if rainbow {
            (
                PreviewCell {
                    color: [255, 0, 0],
                    level: 255,
                },
                SideEffect::Spectrum {
                    forward: true,
                    step: 6,
                },
            )
        } else {
            (
                PreviewCell { color, level: 0 },
                SideEffect::Pulse {
                    falling: vec![false; cell_count],
                },
            )
        };
        Self {
            cells: vec![cell; cell_count],
            effect,
            collision_elapsed: 0,
            collision_report: [0; 5],
            pulse_pause: None,
        }
    }

    pub fn wave_default(
        cell_count: usize,
        foreground: [u8; 3],
        background: [u8; 3],
        rainbow: bool,
    ) -> Self {
        let cell_count = cell_count.min(MAX_SIDE_CELLS);
        let (cells, effect) = if rainbow {
            (
                vec![
                    PreviewCell {
                        color: [0; 3],
                        level: 255
                    };
                    cell_count
                ],
                SideEffect::Spectrum {
                    forward: true,
                    step: 6,
                },
            )
        } else {
            let cell = PreviewCell {
                color: mix_color(foreground, background, 0, false),
                level: 0,
            };
            (
                vec![cell; cell_count],
                SideEffect::Wave {
                    falling: vec![false; cell_count],
                },
            )
        };
        Self {
            cells,
            effect,
            collision_elapsed: 0,
            collision_report: [0; 5],
            pulse_pause: None,
        }
    }

    pub fn tick(&mut self) {
        match &mut self.effect {
            SideEffect::Steady { clear_color } => reset_side_levels(&mut self.cells, *clear_color),
            SideEffect::Pulse { falling } => {
                super::tick_pulse(&mut self.cells, falling, &mut self.pulse_pause);
            }
            SideEffect::Spectrum { forward, step } => {
                for cell in &mut self.cells {
                    cell.color = cycle_spectrum(cell.color, *step, *forward);
                }
            }
            SideEffect::Wave { falling } => super::wave::tick_wave_levels(&mut self.cells, falling),
            SideEffect::Collision(state) => {
                state.tick(&mut self.cells, &mut self.collision_elapsed);
                self.collision_report = state.report(self.collision_elapsed);
            }
            SideEffect::Breathing(state) => {
                let Some(cell) = self.cells.first() else {
                    return;
                };
                match state.phase {
                    Phase::Rising => {
                        let level = cell.level.saturating_add(2);
                        self.cells.iter_mut().for_each(|cell| cell.level = level);
                        if level == 255 {
                            state.phase = Phase::Falling;
                        }
                    }
                    Phase::Falling => {
                        if cell.level < 2 {
                            state.phase = Phase::Paused(0);
                        }
                        self.cells
                            .iter_mut()
                            .for_each(|cell| cell.level = cell.level.saturating_sub(2));
                    }
                    Phase::Paused(ticks) if ticks < 6 => state.phase = Phase::Paused(ticks + 1),
                    Phase::Paused(_) => {
                        state.index = (state.index + 1) % state.palette_len;
                        self.cells.fill(PreviewCell {
                            color: state.palette[state.index],
                            level: 0,
                        });
                        state.phase = Phase::Rising;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_constructors_bound_their_storage_to_the_sdk_grid() {
        for preview in [
            SidePreview::steady(usize::MAX, [0; 3], false),
            SidePreview::breathing(usize::MAX, [1; 3], [2; 3], false),
            SidePreview::wave_default(usize::MAX, [1; 3], [2; 3], false),
            SidePreview::collision(usize::MAX, u8::MAX, [1; 3], [2; 3], false),
            SidePreview::pulse(usize::MAX, [1; 3], false),
        ] {
            assert_eq!(preview.cells().len(), MAX_SIDE_CELLS);
        }
    }

    #[test]
    fn matches_vendor_side_previews() {
        let foreground = [20, 40, 60];
        let background = [2, 4, 6];
        let mut preview = SidePreview::steady(6, foreground, false);
        let telemetry =
            include_bytes!("../../../tests/fixtures/vendor-sdk/side-collision-config.bin");
        assert_eq!(telemetry.len(), 2 * 530 * 5);
        let mut collision_reports = telemetry.chunks_exact(5);
        for (name, next, fixture) in [
            (
                "side-breathing",
                SidePreview::breathing(6, foreground, background, false),
                &include_bytes!("../../../tests/fixtures/vendor-sdk/side-breathing.bin")[..],
            ),
            (
                "side-wave",
                SidePreview::wave_default(6, foreground, background, false),
                &include_bytes!("../../../tests/fixtures/vendor-sdk/side-wave.bin")[..],
            ),
            (
                "side-wave-rainbow",
                SidePreview::wave_default(6, foreground, background, true),
                &include_bytes!("../../../tests/fixtures/vendor-sdk/side-wave-rainbow.bin")[..],
            ),
            (
                "side-collision",
                SidePreview::collision(6, 33, foreground, background, false),
                &include_bytes!("../../../tests/fixtures/vendor-sdk/side-collision.bin")[..],
            ),
            (
                "side-collision-rainbow",
                SidePreview::collision(6, 33, foreground, background, true),
                &include_bytes!("../../../tests/fixtures/vendor-sdk/side-collision-rainbow.bin")[..],
            ),
            (
                "side-pulse",
                SidePreview::pulse(6, foreground, false),
                &include_bytes!("../../../tests/fixtures/vendor-sdk/side-pulse.bin")[..],
            ),
            (
                "side-spectrum",
                SidePreview::pulse(6, foreground, true),
                &include_bytes!("../../../tests/fixtures/vendor-sdk/side-spectrum.bin")[..],
            ),
        ] {
            preview.replace_config(next);
            assert_eq!(fixture.len(), 530 * 24);
            for (step, frame) in fixture.chunks_exact(24).enumerate() {
                preview.tick();
                if name.starts_with("side-collision") {
                    assert_eq!(
                        preview.collision_config(),
                        collision_reports.next().unwrap(),
                        "{name}, tick {step}"
                    );
                }
                for (index, (cell, expected)) in preview
                    .cells()
                    .iter()
                    .zip(frame.chunks_exact(4))
                    .enumerate()
                {
                    let [r, g, b] = cell.color;
                    assert_eq!(
                        [r, g, b, cell.level],
                        expected,
                        "{name}, tick {step}, cell {index}"
                    );
                }
            }
        }
        assert!(collision_reports.next().is_none());
    }

    #[test]
    fn side_breathing_updates_every_cell_at_the_end_of_the_fade() {
        let mut preview = SidePreview::breathing(4, [7; 3], [8; 3], false);
        for _ in 0..128 {
            preview.tick();
        }
        assert!(preview.cells().iter().all(|cell| cell.level == 0));
        for _ in 0..7 {
            preview.tick();
        }
        assert_eq!(
            preview.cells()[0],
            PreviewCell {
                color: [8; 3],
                level: 0
            }
        );
    }
}
