use super::{KEY_CELLS, KEY_COLUMNS, KEY_ROWS};

const HORIZONTAL_BYTES: usize = KEY_CELLS * 4;
const VERTICAL_BYTES: usize = KEY_ROWS * 4;
const SLOPED_BYTES: usize = HORIZONTAL_BYTES + KEY_COLUMNS;
const WAVE_BYTES: usize = HORIZONTAL_BYTES + VERTICAL_BYTES + 2 * SLOPED_BYTES;
const WAVES: &[u8; 11 * WAVE_BYTES] = include_bytes!("keyboard-waves.bin");
const PATH_BYTES: usize = 1 + KEY_CELLS;
const PATHS: &[u8; 8 * PATH_BYTES] = include_bytes!("keyboard-paths.bin");

pub(super) struct WaveGeometry {
    pub horizontal: &'static [u8],
    pub vertical: &'static [u8],
    pub slanted: &'static [u8],
    pub diagonal: &'static [u8],
}

fn geometry_index(key_count: u8) -> usize {
    match key_count {
        61 | 62 => 1,
        63 | 65 => 2,
        68 | 69 => 3,
        82 | 84 => 4,
        83 | 87 | 88 | 91 => 5,
        98 => 6,
        108 => 7,
        18 => 8,
        21 => 9,
        34 => 10,
        _ => 0,
    }
}

pub(super) fn waves(key_count: u8) -> WaveGeometry {
    let start = geometry_index(key_count) * WAVE_BYTES;
    let (horizontal, tail) = WAVES[start..start + WAVE_BYTES].split_at(HORIZONTAL_BYTES);
    let (vertical, tail) = tail.split_at(VERTICAL_BYTES);
    let (slanted, diagonal) = tail.split_at(SLOPED_BYTES);
    WaveGeometry {
        horizontal,
        vertical,
        slanted,
        diagonal,
    }
}

pub(super) fn sequential_path(key_count: u8) -> &'static [u8] {
    let layout = geometry_index(key_count);
    let start = match layout {
        1..=7 => layout,
        _ => 0,
    } * PATH_BYTES;
    let count = usize::from(PATHS[start]);
    &PATHS[start + 1..start + 1 + count]
}
