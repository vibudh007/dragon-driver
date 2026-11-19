use std::collections::BTreeMap;

pub struct KeyMap {
    labels: Option<BTreeMap<i32, String>>,
    matrix: [[i32; KEY_COLUMNS]; KEY_ROWS],
}

impl Default for KeyMap {
    fn default() -> Self {
        Self {
            labels: None,
            matrix: MATRIX,
        }
    }
}

impl KeyMap {
    pub fn label(&self, code: i32) -> Option<&str> {
        match &self.labels {
            Some(labels) => labels.get(&code).map(String::as_str),
            None => {
                let index = LABELS.binary_search_by_key(&code, |&(key, _)| key).ok()?;
                Some(LABELS[index].1)
            }
        }
    }

    pub fn code(&self, row: usize, column: usize) -> Option<i32> {
        self.matrix.get(row)?.get(column).copied()
    }

    pub fn replace_labels(&mut self, labels: BTreeMap<i32, String>) {
        self.labels = Some(labels);
    }

    pub fn replace_matrix(&mut self, matrix: [[i32; KEY_COLUMNS]; KEY_ROWS]) {
        self.matrix = matrix;
    }
}

pub const KEY_ROWS: usize = 6;
pub const KEY_COLUMNS: usize = 22;
const LABELS: &[(i32, &str)] = &[
    (-10, "Fn"),
    (4, "A"),
    (5, "B"),
    (6, "C"),
    (7, "D"),
    (8, "E"),
    (9, "F"),
    (10, "G"),
    (11, "H"),
    (12, "I"),
    (13, "J"),
    (14, "K"),
    (15, "L"),
    (16, "M"),
    (17, "N"),
    (18, "O"),
    (19, "P"),
    (20, "Q"),
    (21, "R"),
    (22, "S"),
    (23, "T"),
    (24, "U"),
    (25, "V"),
    (26, "W"),
    (27, "X"),
    (28, "Y"),
    (29, "Z"),
    (30, "1!"),
    (31, "2@"),
    (32, "3#"),
    (33, "4$"),
    (34, "5%"),
    (35, "6^"),
    (36, "7&"),
    (37, "8*"),
    (38, "9("),
    (39, "0)"),
    (40, "Enter"),
    (41, "Esc"),
    (42, "Backspace"),
    (43, "Tab"),
    (44, "Space"),
    (45, "-_"),
    (46, "=+"),
    (47, "[{"),
    (48, "]}"),
    (49, "\\|"),
    (50, "K42"),
    (51, ";:"),
    (52, "'\""),
    (53, "`~"),
    (54, ",<"),
    (55, ".>"),
    (56, "/?"),
    (57, "Caps"),
    (58, "F1"),
    (59, "F2"),
    (60, "F3"),
    (61, "F4"),
    (62, "F5"),
    (63, "F6"),
    (64, "F7"),
    (65, "F8"),
    (66, "F9"),
    (67, "F10"),
    (68, "F11"),
    (69, "F12"),
    (70, "PrtSc"),
    (71, "ScrLk"),
    (72, "Pause"),
    (73, "Insert"),
    (74, "Home"),
    (75, "PgUp"),
    (76, "Del"),
    (77, "End"),
    (78, "PgDn"),
    (79, "Right"),
    (80, "Left"),
    (81, "Down"),
    (82, "Up"),
    (83, "Num Lock"),
    (84, "Num /"),
    (85, "Num *"),
    (86, "Num -"),
    (87, "Num +"),
    (88, "Num Enter"),
    (89, "Num 1"),
    (90, "Num 2"),
    (91, "Num 3"),
    (92, "Num 4"),
    (93, "Num 5"),
    (94, "Num 6"),
    (95, "Num 7"),
    (96, "Num 8"),
    (97, "Num 9"),
    (98, "Num 0"),
    (99, "Num .Del"),
    (100, "K45"),
    (101, "Context Menu"),
    (133, "K107"),
    (135, "K56"),
    (136, "K133"),
    (137, "K14"),
    (138, "K132"),
    (139, "K131"),
    (224, "Left Ctrl"),
    (225, "Left Shift"),
    (226, "Left Alt"),
    (227, "Left Win"),
    (228, "Right Ctrl"),
    (229, "Right Shift"),
    (230, "Right Alt"),
];

const MATRIX: [[i32; KEY_COLUMNS]; KEY_ROWS] = [
    [
        41, 0, 58, 59, 60, 61, 0, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 0, 0, 0, 0,
    ],
    [
        53, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 45, 46, 0, 42, 73, 74, 75, 83, 84, 85, 86,
    ],
    [
        43, 20, 26, 8, 21, 23, 28, 24, 12, 18, 19, 47, 48, 0, 49, 76, 77, 78, 95, 96, 97, 87,
    ],
    [
        57, 0, 4, 22, 7, 9, 10, 11, 13, 14, 15, 51, 52, 0, 40, 0, 0, 0, 92, 93, 94, 0,
    ],
    [
        225, 0, 29, 27, 6, 25, 5, 17, 16, 54, 55, 56, 0, 0, 229, 0, 82, 0, 89, 90, 91, 88,
    ],
    [
        224, 227, 226, 0, 0, 0, 44, 0, 0, 0, 230, -10, 101, 228, 0, 80, 81, 79, 98, 0, 99, 0,
    ],
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retains_signed_codes_and_allows_full_replacement() {
        let mut keys = KeyMap::default();
        assert_eq!(keys.label(0x29), Some("Esc"));
        for &(code, label) in LABELS {
            assert_eq!(keys.label(code), Some(label));
        }
        assert_eq!(keys.code(6, 0), None);
        assert_eq!(keys.code(0, 0), Some(0x29));
        keys.replace_labels(BTreeMap::from([(0x29, "Escape".into())]));
        assert_eq!(keys.label(0x29), Some("Escape"));
        assert_eq!(keys.label(-10), None);
        assert_eq!(keys.label(-10), None);
        keys.replace_matrix([[0; KEY_COLUMNS]; KEY_ROWS]);
        assert_eq!(keys.code(0, 0), Some(0));
    }
}
