use super::{KeyboardPreview, MAX_SIDE_CELLS, PreviewConfigError, SidePreview};
use crate::witmod_sdk::{catalog::Model, profile::LightItem};

pub struct LightingPreview {
    key_count: u8,
    model_name: &'static str,
    keyboard_config: LightItem,
    keyboard_background: [u8; 3],
    keyboard: KeyboardPreview,
    side: Option<SidePreview>,
    side_slots: Vec<bool>,
    side_enabled: bool,
}

impl LightingPreview {
    pub fn new(
        model: &Model,
        keyboard: LightItem,
        side_enabled: bool,
    ) -> Result<Self, PreviewConfigError> {
        Ok(Self {
            key_count: model.key_count,
            model_name: model.hid_name,
            keyboard_config: keyboard,
            keyboard: KeyboardPreview::from_config(keyboard, model.key_count)?,
            keyboard_background: keyboard.background.map(|channel| channel as u8),
            side: None,
            side_slots: Vec::new(),
            side_enabled,
        })
    }

    pub fn set_keyboard_config(
        &mut self,
        item: LightItem,
        side_enabled: bool,
    ) -> Result<(), PreviewConfigError> {
        self.update_keyboard_config(item)?;
        self.side_enabled = side_enabled;
        Ok(())
    }

    pub fn update_keyboard_config(&mut self, item: LightItem) -> Result<(), PreviewConfigError> {
        self.keyboard.replace_config(item, self.key_count)?;
        self.keyboard_config = item;
        self.keyboard_background = item.background.map(|channel| channel as u8);
        Ok(())
    }

    pub fn keyboard_config(&self) -> LightItem {
        self.keyboard_config
    }

    pub fn set_side_config(
        &mut self,
        item: LightItem,
        cell_count: usize,
    ) -> Result<(), PreviewConfigError> {
        let slots = [true; MAX_SIDE_CELLS];
        let slots = slots
            .get(..cell_count)
            .ok_or(PreviewConfigError::TooManySideCells(cell_count))?;
        self.set_side_layout(item, slots)
    }

    pub fn set_side_layout(
        &mut self,
        item: LightItem,
        slots: &[bool],
    ) -> Result<(), PreviewConfigError> {
        let next = SidePreview::from_model_config(item, self.model_name, slots)?;
        self.replace_side(next);
        self.side_slots.clear();
        self.side_slots.extend_from_slice(slots);
        Ok(())
    }

    fn replace_side(&mut self, next: SidePreview) {
        if let Some(current) = &mut self.side {
            current.replace_config(next);
        } else {
            self.side = Some(next);
        }
    }

    pub fn update_side_config(&mut self, item: LightItem) -> Result<(), PreviewConfigError> {
        let next = SidePreview::from_model_config(item, self.model_name, &self.side_slots)?;
        self.replace_side(next);
        Ok(())
    }

    pub fn tick_keyboard(&mut self) -> &KeyboardPreview {
        self.keyboard.tick();
        &self.keyboard
    }

    pub fn tick_side(&mut self) -> Option<&SidePreview> {
        let side = self.side.as_mut()?;
        if self.side_enabled {
            side.tick();
        }
        Some(side)
    }

    pub fn press(&mut self, row: i32, column: i32) -> i32 {
        if row >= 0 && column >= 0 && self.keyboard.press(row as usize, column as usize) {
            0
        } else {
            -1
        }
    }

    pub fn mixed_color(&self, row: usize, column: usize) -> Option<[u8; 3]> {
        self.keyboard
            .mixed_color(row, column, self.keyboard_background)
    }

    pub fn current_ripple_color(&self) -> [u32; 3] {
        self.keyboard.current_ripple_color()
    }

    pub fn current_automatic_ripple_config(&mut self) -> [u32; 5] {
        self.keyboard.current_automatic_ripple_config()
    }

    pub fn side_collision_config(&self) -> [u8; 5] {
        self.side
            .as_ref()
            .map_or([0; 5], SidePreview::collision_config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::witmod_sdk::catalog::MODELS;

    fn light(mode: i32, direction: i32, rainbow: bool) -> LightItem {
        LightItem {
            region: 0,
            mode,
            brightness: 1,
            speed: 0,
            foreground: [20, 40, 60],
            background: [2, 4, 6],
            direction,
            mode_type: i32::from(rainbow),
            open: 0,
            update: 0,
        }
    }

    #[test]
    fn reconfiguration_preserves_the_shared_rain_clock_and_callback_transition() {
        let fixture = include_bytes!("../../../tests/fixtures/vendor-sdk/keyboard-reconfigure.bin");
        assert_eq!(fixture.len(), 53 * 30);
        let mut preview = LightingPreview::new(&MODELS[0], light(15, 0, true), false).unwrap();
        for (step, expected) in fixture.chunks_exact(30).enumerate() {
            if step == 5 {
                preview.update_keyboard_config(light(16, 0, true)).unwrap();
                assert_eq!(preview.press(-1, 1), -1);
                assert_eq!(preview.press(0, 1), 0);
            }
            let frame = preview.tick_keyboard();
            for (cell, expected) in [0, 1, 22, 88, 110, 131]
                .into_iter()
                .zip(expected.chunks_exact(5))
            {
                let [r, g, b] = frame.cells()[cell].color;
                let transition = frame.palette_transition(cell / 22, cell % 22).unwrap();
                assert_eq!(
                    [r, g, b, frame.cells()[cell].level, transition],
                    expected,
                    "tick {step}, cell {cell}"
                );
            }
        }
    }

    #[test]
    fn invalid_configuration_keeps_the_current_preview_and_side_gate() {
        let mut preview = LightingPreview::new(&MODELS[0], light(0, 0, false), false).unwrap();
        preview.set_side_config(light(4, 0, false), 3).unwrap();
        assert!(
            preview
                .set_keyboard_config(light(200, 0, false), true)
                .is_err()
        );
        assert_eq!(preview.keyboard_config().mode, 0);
        assert_eq!(preview.tick_side().unwrap().cells()[0].level, 0);
        preview
            .set_keyboard_config(light(0, 0, false), true)
            .unwrap();
        assert_eq!(preview.tick_side().unwrap().cells()[0].level, 1);
    }

    #[test]
    fn side_wave_uses_named_model_tables_and_skips_inactive_slots() {
        for (item, fixture) in [
            (
                light(2, 1, false),
                &include_bytes!("../../../tests/fixtures/vendor-sdk/side-k665.bin")[..],
            ),
            (
                light(2, 0, true),
                &include_bytes!("../../../tests/fixtures/vendor-sdk/side-k665-rainbow.bin")[..],
            ),
        ] {
            let slots = [true, false, true, true, false, true];
            let mut preview = SidePreview::from_model_config(item, "k665rgb", &slots).unwrap();
            assert_eq!(fixture.len(), 100 * 24);
            for (step, expected) in fixture.chunks_exact(24).enumerate() {
                preview.tick();
                for (index, (cell, expected)) in preview
                    .cells()
                    .iter()
                    .zip(expected.chunks_exact(4))
                    .enumerate()
                {
                    let [r, g, b] = cell.color;
                    assert_eq!(
                        [r, g, b, cell.level],
                        expected,
                        "tick {step}, cell {index}, mode {}",
                        item.mode_type
                    );
                }
            }
        }
    }

    #[test]
    fn exposes_the_vendor_row_count_and_automatic_ripple_latch() {
        let rows = include_bytes!("../../../tests/fixtures/vendor-sdk/active-rows.bin");
        assert_eq!(rows.len(), 280);
        let mut preview = LightingPreview::new(&MODELS[0], light(12, 1, true), false).unwrap();
        for (step, &expected) in rows.iter().enumerate() {
            if step < 2 {
                assert_eq!(preview.press(0, 1), 0);
            }
            assert_eq!(
                preview.tick_keyboard().active_rows(),
                expected,
                "tick {step}"
            );
        }

        let events = include_bytes!("../../../tests/fixtures/vendor-sdk/automatic-config.bin");
        assert_eq!(events.len(), 280 * 21);
        preview.update_keyboard_config(light(14, 0, true)).unwrap();
        for (step, expected) in events.chunks_exact(21).enumerate() {
            let frame = preview.tick_keyboard();
            let flag = expected[0] != 0;
            assert_eq!(frame.automatic_frame_flag(), flag, "tick {step}");
            assert_eq!(frame.automatic_ripple_pending(), flag, "tick {step}");
            if flag {
                let expected = std::array::from_fn(|index| {
                    let start = 1 + index * 4;
                    u32::from_le_bytes(expected[start..start + 4].try_into().unwrap())
                });
                assert_eq!(
                    preview.current_automatic_ripple_config(),
                    expected,
                    "tick {step}"
                );
            }
        }
    }

    #[test]
    fn uses_vendor_geometry_for_additional_known_layouts() {
        for (name, key_count, settings, trace) in [
            (
                "wave 82",
                82,
                light(2, 2, true),
                &include_bytes!("../../../tests/fixtures/vendor-sdk/wave-82.bin")[..],
            ),
            (
                "wave 98",
                98,
                light(2, 0, true),
                &include_bytes!("../../../tests/fixtures/vendor-sdk/wave-98.bin")[..],
            ),
            (
                "diagonal 68",
                68,
                light(4, 1, false),
                &include_bytes!("../../../tests/fixtures/vendor-sdk/diagonal-68.bin")[..],
            ),
            (
                "sequential 68",
                68,
                light(5, 1, true),
                &include_bytes!("../../../tests/fixtures/vendor-sdk/sequential-68.bin")[..],
            ),
        ] {
            let model = MODELS
                .iter()
                .find(|model| model.key_count == key_count)
                .unwrap();
            let mut preview = LightingPreview::new(model, settings, false).unwrap();
            assert_eq!(trace.len(), 280 * 24);
            for (step, expected) in trace.chunks_exact(24).enumerate() {
                let frame = preview.tick_keyboard();
                for (index, expected) in [0, 1, 22, 88, 110, 131]
                    .into_iter()
                    .zip(expected.chunks_exact(4))
                {
                    let cell = frame.cells()[index];
                    let [r, g, b] = cell.color;
                    assert_eq!(
                        [r, g, b, cell.level],
                        expected,
                        "{name}, tick {step}, cell {index}"
                    );
                }
            }
        }
    }
}
