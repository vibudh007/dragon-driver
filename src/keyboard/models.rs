use std::sync::OnceLock;

use crate::witmod_sdk::identity::Identity;
use crate::witmod_sdk::keymap::{KEY_COLUMNS, KEY_ROWS};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Position {
    row: u8,
    column: u8,
}

impl Position {
    pub fn new(row: u8, column: u8) -> Option<Self> {
        (usize::from(row) < KEY_ROWS && usize::from(column) < KEY_COLUMNS)
            .then_some(Self { row, column })
    }

    pub fn row(self) -> u8 {
        self.row
    }

    pub fn column(self) -> u8 {
        self.column
    }
}

#[derive(Debug)]
pub struct Layout {
    pub family: &'static str,
    pub device_type: u8,
    pub onboard_profiles: bool,
    pub side_lighting: bool,
    pub side_lights: u8,
    pub onboard_selection: bool,
    codes: [[i16; KEY_COLUMNS]; KEY_ROWS],
}

impl Layout {
    pub fn key(&self, position: Position) -> Option<i16> {
        let code = self.codes[usize::from(position.row)][usize::from(position.column)];
        (code != 0).then_some(code)
    }

    pub fn keys(&self) -> impl Iterator<Item = (Position, i16)> + '_ {
        (0..KEY_ROWS as u8).flat_map(move |row| {
            (0..KEY_COLUMNS as u8).filter_map(move |column| {
                let position = Position { row, column };
                self.key(position).map(|code| (position, code))
            })
        })
    }

    pub fn magnetic(&self) -> bool {
        self.device_type == 4
    }
}

pub fn layouts() -> &'static [Layout] {
    static LAYOUTS: OnceLock<Vec<Layout>> = OnceLock::new();
    LAYOUTS.get_or_init(|| {
        include_str!("data/models.tsv")
            .lines()
            .map(|line| parse(line).expect("invalid bundled keyboard layout"))
            .collect()
    })
}

pub fn find(family: &str) -> Option<&'static Layout> {
    let models = layouts();
    models
        .binary_search_by_key(&family, |model| model.family)
        .ok()
        .map(|index| &models[index])
}

pub fn from_identity(identity: &Identity) -> Option<&'static Layout> {
    find(std::str::from_utf8(identity.layout_code()).ok()?)
}

fn parse(line: &'static str) -> Option<Layout> {
    let mut fields = line.split('\t');
    let family = fields.next()?;
    let device_type = fields.next()?.parse().ok()?;
    let onboard_profiles = flag(fields.next()?)?;
    let side_lighting = flag(fields.next()?)?;
    let side_lights = fields.next()?.parse().ok()?;
    let onboard_selection = flag(fields.next()?)?;
    let mut codes = [[0; KEY_COLUMNS]; KEY_ROWS];
    for row in &mut codes {
        let mut cells = fields.next()?.split(',');
        for code in row {
            *code = cells.next()?.parse().ok()?;
        }
        if cells.next().is_some() {
            return None;
        }
    }
    fields.next().is_none().then_some(Layout {
        family,
        device_type,
        onboard_profiles,
        side_lighting,
        side_lights,
        onboard_selection,
        codes,
    })
}

fn flag(value: &str) -> Option<bool> {
    match value {
        "0" => Some(false),
        "1" => Some(true),
        _ => None,
    }
}

pub fn geometry(layout: &Layout) -> Vec<(Position, [i32; 4])> {
    include_str!("data/key-geometry.tsv")
        .lines()
        .find_map(|line| {
            let (name, values) = line.split_once('\t')?;
            (name == layout.family).then(|| {
                values
                    .split('\t')
                    .filter_map(|value| {
                        let mut fields = value.split(':');
                        let pos = Position::new(
                            fields.next()?.parse().ok()?,
                            fields.next()?.parse().ok()?,
                        )?;
                        let bounds = [
                            fields.next()?.parse().ok()?,
                            fields.next()?.parse().ok()?,
                            fields.next()?.parse().ok()?,
                            fields.next()?.parse().ok()?,
                        ];
                        Some((pos, bounds))
                    })
                    .collect()
            })
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_data_matches_the_reference_layout() {
        assert_eq!(layouts().len(), 126);
        assert!(
            layouts()
                .windows(2)
                .all(|pair| pair[0].family < pair[1].family)
        );
        let reference = find("7153_AUSHERGB").unwrap();
        assert!(reference.magnetic());
        assert!(reference.onboard_profiles);
        assert!(!reference.side_lighting);
        assert_eq!(reference.side_lights, 0);
        let disabled_side = find("VK68HE_RGB").unwrap();
        assert!(!disabled_side.side_lighting);
        assert_eq!(disabled_side.side_lights, 21);
        assert_eq!(reference.keys().count(), 61);
        assert_eq!(reference.key(Position::new(1, 0).unwrap()), Some(41));
        assert_eq!(reference.key(Position::new(2, 14).unwrap()), Some(49));
        assert_eq!(reference.key(Position::new(5, 10).unwrap()), Some(-10));
        assert_eq!(reference.key(Position::new(0, 0).unwrap()), None);
        assert!(Position::new(6, 0).is_none());
        assert!(Position::new(0, 22).is_none());
    }
}
